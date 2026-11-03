//! Exporting PNG images of the 3D view: the whole build area, one molecule
//! (isolated and framed, optionally with a details card), or a reaction.
//!
//! Flow: the request sets up the scene (isolation, camera framing, label
//! settings), waits a few frames for it to draw, captures the window, crops
//! to the 3D view and saves the PNG, then restores everything exactly.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    window::PrimaryWindow,
};
use valence_chem::AtomId;

use crate::camera::OrbitCam;
use crate::reactions_ui::ReactionState;
use crate::state::{Chem, ExportTarget, Isolation, Selection, Settings, UiState, ViewRect};

/// Content of the card drawn on exported images.
#[derive(Clone, Default)]
pub struct DetailsCard {
    pub title: String,
    pub subtitle: String,
    pub rows: Vec<(String, String)>,
}

type Slot = Arc<Mutex<Option<Result<PathBuf, String>>>>;

#[derive(Default)]
enum Stage {
    #[default]
    Idle,
    Requested,
    Preparing {
        frames: u32,
    },
    Capturing {
        since: f64,
        slot: Slot,
    },
}

#[derive(Resource, Default)]
pub struct ExportJob {
    stage: Stage,
    saved_rig: Option<OrbitCam>,
    saved_labels: (bool, bool),
    /// Card drawn over the 3D view while capturing (None = no card).
    pub card: Option<DetailsCard>,
    /// Outcome of the last export, for the Export tab.
    pub last: Option<Result<PathBuf, String>>,
}

impl ExportJob {
    pub fn request(&mut self) {
        if matches!(self.stage, Stage::Idle) {
            self.stage = Stage::Requested;
            self.last = None;
        }
    }

    pub fn busy(&self) -> bool {
        !matches!(self.stage, Stage::Idle)
    }

    /// True while the scene is set up for the picture (hide hover tooltips,
    /// selection rings, effects and other transient UI).
    pub fn capturing(&self) -> bool {
        matches!(
            self.stage,
            Stage::Preparing { .. } | Stage::Capturing { .. }
        )
    }
}

/// Where exported images go: `Pictures/Valence`.
pub fn export_dir() -> PathBuf {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if let Some(dir) = env("VALENCE_EXPORT_DIR") {
        return dir;
    }
    let home = env("HOME").or_else(|| env("USERPROFILE"));
    let pictures = env("XDG_PICTURES_DIR")
        .or_else(|| home.clone().map(|h| h.join("Pictures")))
        .unwrap_or_else(std::env::temp_dir);
    pictures.join("Valence")
}

/// Open a folder in the platform's file manager (best effort).
pub fn open_folder(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(dir).spawn();
}

/// `valence-2026-11-03-1412-water.png` style names (UTC, no extra crates).
fn file_name(label: &str) -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let slug: String = label
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("-");
    format!(
        "valence-{y:04}-{m:02}-{d:02}-{:02}{:02}{:02}-{}.png",
        tod / 3600,
        tod / 60 % 60,
        tod % 60,
        if slug.is_empty() { "lab".into() } else { slug }
    )
}

fn frame_points(rig: &mut OrbitCam, pts: &[Vec3]) {
    if pts.is_empty() {
        return;
    }
    let center = pts.iter().copied().sum::<Vec3>() / pts.len() as f32;
    let radius = pts
        .iter()
        .map(|p| p.distance(center))
        .fold(0.0f32, f32::max);
    rig.focus = center;
    rig.dist = (radius * 2.4 + 4.5).clamp(3.5, 200.0);
}

/// Atoms, card and file label for the chosen target. Err = a message for the user.
fn plan(
    target: ExportTarget,
    chem: &Chem,
    selection: &Selection,
    reaction: &ReactionState,
    details: bool,
) -> Result<(Option<Vec<AtomId>>, Option<DetailsCard>, String), String> {
    match target {
        ExportTarget::Lab => {
            if chem.lab.is_empty() {
                return Err("The lab is empty: build something first.".into());
            }
            let card = details.then(|| DetailsCard {
                title: "Valence".into(),
                subtitle: format!(
                    "{} molecule(s), {} atoms",
                    chem.molecules.len(),
                    chem.lab.atom_count()
                ),
                rows: chem
                    .molecules
                    .iter()
                    .take(14)
                    .map(|m| (m.names.title.clone(), m.names.formula.clone()))
                    .collect(),
            });
            Ok((None, card, "lab".into()))
        }
        ExportTarget::Molecule => {
            let m = selection
                .atom
                .and_then(|a| chem.molecule_of(a))
                .ok_or("Select a molecule first (click one of its atoms).")?;
            let card = details.then(|| DetailsCard {
                title: m.names.title.clone(),
                subtitle: m.names.formula.clone(),
                rows: crate::facts::molecule_facts(&m.names, &chem.lab.graph(&m.atoms))
                    .into_iter()
                    .filter(|(k, _)| k != "Formula") // already the subtitle
                    .collect(),
            });
            Ok((Some(m.atoms.clone()), card, m.names.title.clone()))
        }
        ExportTarget::Reaction => {
            let atoms = reaction.atoms(chem);
            if atoms.is_empty() {
                return Err("Set up a reaction first (Reactions tab).".into());
            }
            let card = details.then(|| reaction.card(chem));
            Ok((Some(atoms), card, reaction.title(chem)))
        }
    }
}

pub fn run_export(
    mut commands: Commands,
    mut job: ResMut<ExportJob>,
    ui: Res<UiState>,
    chem: Res<Chem>,
    selection: Res<Selection>,
    reaction: Res<ReactionState>,
    mut settings: ResMut<Settings>,
    mut isolation: ResMut<Isolation>,
    mut rig: Single<&mut OrbitCam>,
    view: Res<ViewRect>,
    window: Single<&Window, With<PrimaryWindow>>,
    time: Res<Time<Real>>,
    mut label: Local<String>,
) {
    let now = time.elapsed_secs_f64();
    match std::mem::take(&mut job.stage) {
        Stage::Idle => {}
        Stage::Requested => match plan(
            ui.export_target,
            &chem,
            &selection,
            &reaction,
            ui.export_details,
        ) {
            Err(msg) => job.last = Some(Err(msg)),
            Ok((atoms, card, name)) => {
                job.saved_rig = Some(**rig);
                job.saved_labels = (settings.show_symbols, settings.show_names);
                settings.show_symbols = ui.export_labels;
                settings.show_names = ui.export_labels;
                if let Some(atoms) = &atoms {
                    let pts: Vec<Vec3> = atoms
                        .iter()
                        .filter_map(|&a| chem.lab.atom(a))
                        .map(|a| a.pos)
                        .collect();
                    frame_points(&mut rig, &pts);
                    isolation.0 = Some(atoms.iter().copied().collect());
                }
                job.card = card;
                *label = name;
                job.stage = Stage::Preparing { frames: 0 };
            }
        },
        Stage::Preparing { frames } => {
            // Give transforms, labels and the camera a few frames to settle.
            if frames < 8 {
                job.stage = Stage::Preparing { frames: frames + 1 };
                return;
            }
            let scale = window.scale_factor();
            let r = view.0;
            let crop = (
                (r.min.x * scale).max(0.0) as u32,
                (r.min.y * scale).max(0.0) as u32,
                (r.width() * scale).max(1.0) as u32,
                (r.height() * scale).max(1.0) as u32,
            );
            let path = export_dir().join(file_name(&label));
            let slot: Slot = Arc::default();
            let out = slot.clone();
            commands.spawn(Screenshot::primary_window()).observe(
                move |captured: On<ScreenshotCaptured>| {
                    let result = save_cropped(&captured, crop, &path);
                    if let Ok(mut s) = out.lock() {
                        *s = Some(result);
                    }
                },
            );
            job.stage = Stage::Capturing { since: now, slot };
        }
        Stage::Capturing { since, slot } => {
            let result = slot.lock().ok().and_then(|mut s| s.take());
            let result = match result {
                Some(r) => r,
                None if now - since > 10.0 => {
                    Err("The screenshot didn't arrive (is the window minimised?)".into())
                }
                None => {
                    job.stage = Stage::Capturing { since, slot };
                    return;
                }
            };
            // Restore the player's view exactly.
            if let Some(saved) = job.saved_rig.take() {
                **rig = saved;
            }
            (settings.show_symbols, settings.show_names) = job.saved_labels;
            isolation.0 = None;
            job.card = None;
            job.last = Some(result);
        }
    }
}

fn save_cropped(
    captured: &ScreenshotCaptured,
    crop: (u32, u32, u32, u32),
    path: &Path,
) -> Result<PathBuf, String> {
    let img = captured
        .image
        .clone()
        .try_into_dynamic()
        .map_err(|e| format!("Couldn't read the screenshot: {e}"))?;
    let (x, y, w, h) = crop;
    let w = w.min(img.width().saturating_sub(x)).max(1);
    let h = h.min(img.height().saturating_sub(y)).max(1);
    let rgb = img.crop_imm(x, y, w, h).to_rgb8();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    }
    rgb.save(path)
        .map_err(|e| format!("Couldn't save {}: {e}", path.display()))?;
    Ok(path.to_path_buf())
}

/// The Export tab in the sidebar.
pub fn export_tab(
    ui: &mut bevy_egui::egui::Ui,
    state: &mut UiState,
    job: &mut ExportJob,
    chem: &Chem,
    selection: &Selection,
    reaction: &ReactionState,
) {
    use bevy_egui::egui::{Color32, RichText};
    let muted = Color32::from_rgb(150, 158, 175);
    ui.label(
        RichText::new("Export an image")
            .size(18.0)
            .strong()
            .color(Color32::WHITE),
    );
    ui.label(
        RichText::new("Saves a PNG of the 3D view (the sidebar isn't included).").color(muted),
    );
    ui.add_space(8.0);

    ui.label(RichText::new("What to export").strong());
    ui.radio_value(
        &mut state.export_target,
        ExportTarget::Lab,
        "Whole build area, as you see it now",
    );
    let selected = selection
        .atom
        .and_then(|a| chem.molecule_of(a))
        .map(|m| m.names.title.clone());
    ui.radio_value(
        &mut state.export_target,
        ExportTarget::Molecule,
        match &selected {
            Some(name) => format!("Selected molecule: {name}"),
            None => "Selected molecule (select one first)".to_string(),
        },
    );
    ui.radio_value(
        &mut state.export_target,
        ExportTarget::Reaction,
        if reaction.is_empty() {
            "Reaction (set one up on the Reactions tab)".to_string()
        } else {
            format!("Reaction: {}", reaction.equation(chem))
        },
    );
    ui.add_space(8.0);

    ui.label(RichText::new("Include").strong());
    ui.checkbox(&mut state.export_labels, "Atom symbols and name tags");
    ui.checkbox(&mut state.export_details, "Details card");
    let card_note = match state.export_target {
        ExportTarget::Lab => "Lists every molecule in the lab with its formula.",
        ExportTarget::Molecule => {
            "Name, IUPAC name, formula, molar mass, atoms, bonds, bond energy, charge, reactivity, radioactivity."
        }
        ExportTarget::Reaction => {
            "Equation, each input/catalyst/output, balance and energy change."
        }
    };
    ui.label(RichText::new(card_note).size(12.0).color(muted));
    ui.add_space(10.0);

    let busy = job.busy();
    if ui
        .add_enabled(
            !busy,
            bevy_egui::egui::Button::new(RichText::new("Export PNG").size(16.0).strong())
                .min_size([160.0, 34.0].into()),
        )
        .clicked()
    {
        job.request();
    }
    if busy {
        ui.label(RichText::new("Capturing…").color(muted));
    }
    match &job.last {
        Some(Ok(path)) => {
            ui.label(RichText::new("✓ Saved").color(Color32::from_rgb(80, 220, 120)));
            ui.label(
                RichText::new(path.display().to_string())
                    .monospace()
                    .size(11.5),
            );
            if ui.button("Open folder").clicked() {
                open_folder(path.parent().unwrap_or(&export_dir()));
            }
        }
        Some(Err(msg)) => {
            ui.label(RichText::new(format!("✗ {msg}")).color(Color32::from_rgb(255, 110, 110)));
        }
        None => {
            ui.label(
                RichText::new(format!("Images are saved to {}", export_dir().display()))
                    .size(11.5)
                    .color(muted),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_names_are_tidy() {
        let n = super::file_name("Ethanol (drinking alcohol)");
        assert!(
            n.starts_with("valence-20") && n.ends_with("-ethanol-drinking-alcohol.png"),
            "{n}"
        );
        assert!(super::file_name("").ends_with("-lab.png"));
    }
}
