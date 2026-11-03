//! Lab data files (`.valence`, JSON): the lab layout, reactions, the user's
//! own library molecules and the camera view.
//!
//! **Saving is atomic.** The new contents go to a temporary file beside the
//! target, are flushed to disk, read back and re-parsed to verify them, and
//! only then is the temporary file renamed over the target (an atomic
//! replace on the same volume). The previous version is kept as
//! `<name>.bak`. If anything fails, the original file is untouched.
//!
//! **Loading is all-or-nothing.** The whole file is parsed and every part
//! validated into new, separate state; the open lab is only replaced once
//! everything succeeded.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use valence_chem::{BondKind, Lab, LabData, MolGraph};

pub const EXTENSION: &str = "valence";
const FORMAT: &str = "valence-lab";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LabFile {
    pub format: String,
    pub version: u32,
    #[serde(default)]
    pub app_version: String,
    pub lab: LabData,
    #[serde(default)]
    pub reactions: Vec<SavedReactionPart>,
    #[serde(default)]
    pub my_molecules: Vec<SavedMolecule>,
    #[serde(default)]
    pub camera: Option<SavedCamera>,
}

/// A reaction part, referring to a molecule by the index of one of its atoms
/// in `lab.atoms`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SavedReactionPart {
    pub atom: u32,
    pub role: String,
    pub coeff: u32,
}

/// A molecule in "My molecules": a name and its structure.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SavedMolecule {
    pub name: String,
    pub atoms: Vec<SavedAtom>,
    pub bonds: Vec<SavedBond>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct SavedAtom {
    pub z: u8,
    #[serde(default)]
    pub charge: i8,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct SavedBond {
    pub a: u32,
    pub b: u32,
    pub order: u8,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct SavedCamera {
    pub focus: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
}

impl SavedMolecule {
    pub fn from_graph(name: &str, g: &MolGraph) -> Self {
        SavedMolecule {
            name: name.to_string(),
            atoms: g
                .z
                .iter()
                .zip(&g.charge)
                .map(|(&z, &charge)| SavedAtom { z, charge })
                .collect(),
            bonds: g
                .edges
                .iter()
                .map(|e| SavedBond {
                    a: e.a as u32,
                    b: e.b as u32,
                    order: e.order,
                })
                .collect(),
        }
    }

    /// Structure for `Lab::add_molecule` (bond kinds are re-derived there).
    pub fn to_graph(&self) -> Result<MolGraph, String> {
        let mut g = MolGraph::default();
        for a in &self.atoms {
            if !(1..=118).contains(&a.z) {
                return Err(format!("\"{}\" has an unknown element", self.name));
            }
            g.add_atom(a.z, a.charge, 0);
        }
        for b in &self.bonds {
            let (a, c) = (b.a as usize, b.b as usize);
            if a >= g.len() || c >= g.len() || a == c || !(1..=3).contains(&b.order) {
                return Err(format!("\"{}\" has an invalid bond", self.name));
            }
            g.add_edge(a, c, b.order, BondKind::Covalent);
        }
        Ok(g)
    }
}

impl LabFile {
    pub fn new(lab: LabData) -> Self {
        LabFile {
            format: FORMAT.into(),
            version: VERSION,
            app_version: env!("CARGO_PKG_VERSION").into(),
            lab,
            reactions: Vec::new(),
            my_molecules: Vec::new(),
            camera: None,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let mut bytes =
            serde_json::to_vec_pretty(self).map_err(|e| format!("Couldn't encode the lab: {e}"))?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Parse and check the envelope. Content is validated by `validate`.
    pub fn from_bytes(bytes: &[u8]) -> Result<LabFile, String> {
        let file: LabFile = serde_json::from_slice(bytes)
            .map_err(|e| format!("This isn't a readable Valence lab file ({e})."))?;
        if file.format != FORMAT {
            return Err("This isn't a Valence lab file.".into());
        }
        if file.version > VERSION {
            return Err(format!(
                "This file was saved by a newer version of Valence (format {}). Please update Valence to open it.",
                file.version
            ));
        }
        Ok(file)
    }

    /// Build everything the file describes without touching the open lab.
    /// Every part must be valid, or the whole load is rejected.
    pub fn validate(&self) -> Result<Loaded, String> {
        let (lab, ids) = Lab::from_data(&self.lab).map_err(|e| e.0)?;
        let mut reactions = Vec::new();
        for (i, r) in self.reactions.iter().enumerate() {
            let anchor = ids
                .get(r.atom as usize)
                .copied()
                .ok_or(format!("Reaction part {i} refers to a missing atom."))?;
            let role = match r.role.as_str() {
                "input" => crate::reactions_ui::Role::Input,
                "catalyst" => crate::reactions_ui::Role::Catalyst,
                "output" => crate::reactions_ui::Role::Output,
                other => {
                    return Err(format!(
                        "Reaction part {i} has an unknown role \"{other}\"."
                    ));
                }
            };
            if r.coeff == 0 || r.coeff > 1000 {
                return Err(format!("Reaction part {i} has an invalid coefficient."));
            }
            reactions.push((anchor, role, r.coeff));
        }
        let mut molecules = Vec::new();
        for m in &self.my_molecules {
            let g = m.to_graph()?;
            // Must actually be buildable, not just well-formed.
            Lab::new()
                .add_molecule(&g, valence_chem::Vec3::ZERO)
                .map_err(|e| format!("\"{}\": {}", m.name, e.0))?;
            molecules.push(m.clone());
        }
        if let Some(c) = &self.camera {
            let ok = c
                .focus
                .iter()
                .chain([&c.yaw, &c.pitch, &c.dist])
                .all(|v| v.is_finite());
            if !ok {
                return Err("The saved camera view is invalid.".into());
            }
        }
        Ok(Loaded {
            lab,
            reactions,
            molecules,
            camera: self.camera,
        })
    }
}

/// Fully validated contents of a file, ready to swap in.
pub struct Loaded {
    pub lab: Lab,
    pub reactions: Vec<(valence_chem::AtomId, crate::reactions_ui::Role, u32)>,
    pub molecules: Vec<SavedMolecule>,
    pub camera: Option<SavedCamera>,
}

pub fn read(path: &Path) -> Result<LabFile, String> {
    let bytes = fs::read(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    LabFile::from_bytes(&bytes)
}

/// Add the `.valence` extension if the user didn't type one.
pub fn with_extension(path: PathBuf) -> PathBuf {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
    {
        path
    } else {
        path.with_extension(EXTENSION)
    }
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "lab".into());
    path.with_file_name(format!("{name}{suffix}"))
}

/// Write `bytes` to `path` atomically, keeping the previous version as
/// `<path>.bak`. On any error the original file is left as it was.
pub fn save_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = sibling(path, &format!(".saving-{}", std::process::id()));
    let result = write_verify_replace(path, &tmp, bytes);
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn write_verify_replace(path: &Path, tmp: &Path, bytes: &[u8]) -> Result<(), String> {
    let fail = |what: &str, e: std::io::Error| format!("Couldn't save ({what}): {e}");
    // 1. Write the new contents to a temporary file and flush to disk.
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(tmp)
            .map_err(|e| fail("create", e))?;
        f.write_all(bytes).map_err(|e| fail("write", e))?;
        f.sync_all().map_err(|e| fail("flush", e))?;
    }
    // 2. Read it back and make sure it's exactly what we meant to write and
    //    still parses as a lab file.
    let back = fs::read(tmp).map_err(|e| fail("verify", e))?;
    if back != bytes {
        return Err("Couldn't save: the written file didn't match (disk problem?).".into());
    }
    LabFile::from_bytes(&back).map_err(|e| format!("Couldn't save: verification failed ({e})"))?;
    // 3. Keep the previous version as a backup (copied, so the original stays
    //    in place until the very last step).
    if path.exists() {
        let bak = sibling(path, ".bak");
        let bak_tmp = sibling(path, ".bak.saving");
        fs::copy(path, &bak_tmp).map_err(|e| fail("backup", e))?;
        if let Ok(f) = File::open(&bak_tmp) {
            let _ = f.sync_all();
        }
        fs::rename(&bak_tmp, &bak).map_err(|e| fail("backup", e))?;
    }
    // 4. Atomically replace the original with the verified new file.
    fs::rename(tmp, path).map_err(|e| fail("replace", e))?;
    // 5. Make the rename itself durable (directory entry), where supported.
    #[cfg(unix)]
    if let Some(dir) = path.parent()
        && let Ok(d) = File::open(if dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            dir
        })
    {
        let _ = d.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use valence_chem::Vec3;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("valence-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn sample() -> LabFile {
        let mut lab = Lab::new();
        let o = lab.add_atom(8, Vec3::ZERO).unwrap();
        lab.attach(o, 1, Vec3::X).unwrap();
        lab.attach(o, 1, Vec3::Y).unwrap();
        let mut f = LabFile::new(lab.to_data().0);
        f.reactions.push(SavedReactionPart {
            atom: 0,
            role: "input".into(),
            coeff: 2,
        });
        let g = valence_chem::smiles::parse("CCO").unwrap();
        f.my_molecules
            .push(SavedMolecule::from_graph("My ethanol", &g));
        f.camera = Some(SavedCamera {
            focus: [1.0, 2.0, 3.0],
            yaw: 0.5,
            pitch: -0.3,
            dist: 9.0,
        });
        f
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("lab.valence");
        let file = sample();
        save_atomic(&path, &file.to_bytes().unwrap()).unwrap();
        let back = read(&path).unwrap();
        assert_eq!(back, file);
        let loaded = back.validate().unwrap();
        assert_eq!(loaded.lab.atom_count(), 3);
        assert_eq!(loaded.reactions.len(), 1);
        assert_eq!(loaded.molecules[0].name, "My ethanol");
        // No temporary files left behind.
        let names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into())
            .collect();
        assert_eq!(names, vec!["lab.valence".to_string()]);
    }

    #[test]
    fn overwrite_keeps_backup_of_previous_version() {
        let dir = temp_dir("backup");
        let path = dir.join("lab.valence");
        let first = sample();
        save_atomic(&path, &first.to_bytes().unwrap()).unwrap();
        let mut second = sample();
        second.my_molecules.clear();
        save_atomic(&path, &second.to_bytes().unwrap()).unwrap();
        assert_eq!(read(&path).unwrap(), second);
        assert_eq!(read(&dir.join("lab.valence.bak")).unwrap(), first);
    }

    #[test]
    fn failed_save_leaves_original_untouched() {
        let dir = temp_dir("failsave");
        let path = dir.join("lab.valence");
        let good = sample();
        save_atomic(&path, &good.to_bytes().unwrap()).unwrap();
        // Bytes that don't verify as a lab file must not replace the original.
        assert!(save_atomic(&path, b"{ not a lab file").is_err());
        assert_eq!(read(&path).unwrap(), good);
        let leftovers = fs::read_dir(&dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("saving")
            })
            .count();
        assert_eq!(leftovers, 0, "temporary file cleaned up");
    }

    #[test]
    fn bad_files_are_rejected_whole() {
        let mut f = sample();
        f.reactions.push(SavedReactionPart {
            atom: 99,
            role: "input".into(),
            coeff: 1,
        });
        assert!(f.validate().is_err(), "dangling reaction reference");
        let mut f = sample();
        f.my_molecules.push(SavedMolecule {
            name: "bad".into(),
            atoms: vec![
                SavedAtom { z: 10, charge: 0 },
                SavedAtom { z: 1, charge: 0 },
            ],
            bonds: vec![SavedBond {
                a: 0,
                b: 1,
                order: 1,
            }],
        });
        assert!(f.validate().is_err(), "unbuildable library molecule");
        assert!(LabFile::from_bytes(b"{}").is_err());
        assert!(
            LabFile::from_bytes(br#"{"format":"other","version":1,"lab":{"atoms":[],"bonds":[]}}"#)
                .is_err()
        );
        assert!(
            LabFile::from_bytes(
                br#"{"format":"valence-lab","version":99,"lab":{"atoms":[],"bonds":[]}}"#
            )
            .is_err()
        );
    }
}
