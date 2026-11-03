//! Shared resources.

use std::collections::VecDeque;

use bevy::prelude::*;
use valence_chem::{AtomId, Lab, MolGraph, Namer, Names, Reject, Vec3 as CVec3};

use crate::crash::guard;

/// Bevy and the chem crate both use glam 0.33, so these are the same type.
const _: fn(CVec3) -> Vec3 = |v| v;

const UNDO_LIMIT: usize = 64;

pub struct Molecule {
    pub atoms: Vec<AtomId>,
    pub names: Names,
}

/// The chemistry world plus cached per-molecule names and undo history.
#[derive(Resource)]
pub struct Chem {
    pub lab: Lab,
    pub namer: Namer,
    pub molecules: Vec<Molecule>,
    /// atom id → index into `molecules`.
    pub atom_mol: Vec<usize>,
    named_rev: u64,
    undo: VecDeque<Lab>,
    redo: Vec<Lab>,
    /// Molecules were moved by hand since the last save (layout is saved,
    /// but moving isn't a structural edit).
    pub user_moved: bool,
}

impl Default for Chem {
    fn default() -> Self {
        Self {
            lab: Lab::new(),
            namer: Namer::new(),
            molecules: Vec::new(),
            atom_mol: Vec::new(),
            named_rev: u64::MAX,
            undo: VecDeque::new(),
            redo: Vec::new(),
            user_moved: false,
        }
    }
}

/// Outcome of a guarded edit.
pub enum Edit<T> {
    Done(T),
    Rejected(Reject),
    /// A bug panicked; the lab was rolled back.
    Crashed,
}

impl Chem {
    /// Apply a structural edit transactionally: on success it becomes an undo
    /// step; on rejection nothing changes; on a panic the lab is restored.
    pub fn edit<T>(
        &mut self,
        what: &str,
        f: impl FnOnce(&mut Lab) -> Result<T, Reject>,
    ) -> Edit<T> {
        let before = self.lab.clone();
        match guard(what, || f(&mut self.lab)) {
            Some(Ok(v)) => {
                self.undo.push_back(before);
                if self.undo.len() > UNDO_LIMIT {
                    self.undo.pop_front();
                }
                self.redo.clear();
                Edit::Done(v)
            }
            Some(Err(e)) => Edit::Rejected(e),
            None => {
                self.lab.restore(before);
                Edit::Crashed
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop_back() else {
            return false;
        };
        let current = self.lab.clone();
        self.lab.restore(prev);
        self.redo.push(current);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        let current = self.lab.clone();
        self.lab.restore(next);
        self.undo.push_back(current);
        true
    }

    /// Run the layout physics; a panic here just stops the motion.
    pub fn relax(&mut self, iterations: usize, pinned: Option<AtomId>) {
        if guard("layout", || self.lab.relax(iterations, pinned)).is_none() {
            self.lab.sleep();
        }
    }

    /// Re-derive molecules and names after a structural change. O(atoms), and
    /// only runs when the lab's revision moved.
    pub fn refresh(&mut self) {
        if self.named_rev == self.lab.revision() {
            return;
        }
        self.named_rev = self.lab.revision();
        self.molecules.clear();
        self.atom_mol = vec![usize::MAX; self.lab.atom_slots()];
        for atoms in self.lab.components() {
            let graph = self.lab.graph(&atoms);
            let names = guard("naming", || self.namer.name(&graph))
                .unwrap_or_else(|| fallback_names(&graph));
            for &a in &atoms {
                if let Some(slot) = self.atom_mol.get_mut(a as usize) {
                    *slot = self.molecules.len();
                }
            }
            self.molecules.push(Molecule { atoms, names });
        }
    }

    pub fn molecule_of(&self, atom: AtomId) -> Option<&Molecule> {
        self.atom_mol
            .get(atom as usize)
            .and_then(|&i| self.molecules.get(i))
    }
}

fn fallback_names(g: &MolGraph) -> Names {
    Names {
        title: "Unnamed compound".into(),
        formula: guard("formula", || g.formula()).unwrap_or_default(),
        open_sites: g.total_free(),
        complete: g.is_complete(),
        ..Default::default()
    }
}

#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub enum Drag {
    #[default]
    None,
    /// Dragging an element out of the periodic table.
    Palette(u8),
    /// Mouse is down on an atom but hasn't moved far enough to count as a drag.
    Pressed { atom: AtomId, at: Vec2 },
    /// Dragging by one atom. `rigid`: move the whole molecule; otherwise
    /// (Shift held) move just this atom so it can close a ring.
    Atom {
        atom: AtomId,
        depth: f32,
        rigid: bool,
    },
}

/// What the mouse is over, refreshed every frame.
#[derive(Resource, Default)]
pub struct Pointer {
    pub pos: Option<Vec2>,
    /// Pointer is over the 3D view (not over the UI).
    pub in_scene: bool,
    pub ray: Option<Ray3d>,
    pub hovered_atom: Option<AtomId>,
    /// Result of checking the current drag against the hovered atom:
    /// Ok(description) or Err(reason).
    pub drop_check: Option<Result<String, String>>,
}

#[derive(Resource)]
pub struct Settings {
    pub show_lone_pairs: bool,
    pub show_symbols: bool,
    pub show_names: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_lone_pairs: true,
            show_symbols: true,
            show_names: true,
        }
    }
}

/// Last molecule the player clicked (shown in the info panel).
#[derive(Resource, Default)]
pub struct Selection {
    pub atom: Option<AtomId>,
    /// Element hovered in the periodic table.
    pub element: Option<u8>,
}

/// Logical-pixel rectangle of the 3D viewport (right of the panel).
#[derive(Resource, Default, Clone, Copy)]
pub struct ViewRect(pub Rect);

/// Short-lived visual feedback: a ring + floating text at a world position.
pub struct Effect {
    pub at: Vec3,
    pub text: String,
    pub ok: bool,
    pub born: f64,
}

#[derive(Resource, Default)]
pub struct Effects(pub Vec<Effect>);

impl Effects {
    pub fn push(&mut self, at: Vec3, text: impl Into<String>, ok: bool, now: f64) {
        self.0.push(Effect {
            at,
            text: text.into(),
            ok,
            born: now,
        });
        if self.0.len() > 12 {
            self.0.remove(0);
        }
    }

    /// Report the outcome of an edit at `at`.
    pub fn report<T>(&mut self, edit: &Edit<T>, at: Vec3, ok_text: &str, now: f64) {
        match edit {
            Edit::Done(_) => self.push(at, ok_text, true, now),
            Edit::Rejected(e) => self.push(at, e.0.clone(), false, now),
            Edit::Crashed => self.push(
                at,
                "Something went wrong, so that change was undone.",
                false,
                now,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_undo_and_redo() {
        let mut chem = Chem::default();
        let Edit::Done(o) = chem.edit("add", |lab| lab.add_atom(8, Vec3::ZERO)) else {
            panic!("add failed")
        };
        assert!(matches!(
            chem.edit("attach", |lab| lab.attach(o, 1, Vec3::X)),
            Edit::Done(_)
        ));
        assert_eq!(chem.lab.atom_count(), 2);
        // A rejected edit is not an undo step.
        assert!(matches!(
            chem.edit("bad", |lab| lab.attach(o, 10, Vec3::X)),
            Edit::Rejected(_)
        ));
        assert!(chem.undo());
        assert_eq!(chem.lab.atom_count(), 1);
        assert!(chem.undo());
        assert!(chem.lab.is_empty());
        assert!(!chem.undo());
        assert!(chem.redo() && chem.redo());
        assert_eq!(chem.lab.atom_count(), 2);
        chem.refresh();
        assert_eq!(chem.molecules.len(), 1);
        assert_eq!(chem.molecules[0].names.title, "Hydroxyl radical");
    }

    #[test]
    fn a_panicking_edit_is_rolled_back() {
        let mut chem = Chem::default();
        let Edit::Done(c) = chem.edit("add", |lab| lab.add_atom(6, Vec3::ZERO)) else {
            panic!("add failed")
        };
        let before = chem.lab.revision();
        let r: Edit<()> = chem.edit("boom", |lab| {
            let _ = lab.attach(c, 1, Vec3::X); // partially mutate...
            panic!("simulated bug"); // ...then blow up
        });
        assert!(matches!(r, Edit::Crashed));
        assert_eq!(chem.lab.atom_count(), 1, "partial edit must be rolled back");
        assert!(chem.lab.revision() > before, "caches must see the restore");
        chem.lab.validate().unwrap();
        // The game carries on normally afterwards.
        assert!(matches!(
            chem.edit("attach", |lab| lab.attach(c, 1, Vec3::X)),
            Edit::Done(_)
        ));
    }
}

/// Sidebar tab.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum Tab {
    #[default]
    Build,
    Export,
    Reactions,
}

/// What an image export captures.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ExportTarget {
    /// Everything in the 3D view.
    #[default]
    Lab,
    /// Just the selected molecule, framed and isolated.
    Molecule,
    /// The molecules in the reaction, with the equation.
    Reaction,
}

/// Window/tab state for the interface.
#[derive(Resource)]
pub struct UiState {
    pub tab: Tab,
    pub settings_open: bool,
    pub help_open: bool,
    pub about_open: bool,
    pub export_target: ExportTarget,
    /// Draw element symbols and name tags in exported images.
    pub export_labels: bool,
    /// Draw the details card (name, formula, mass, energy, …) in exports.
    pub export_details: bool,
    /// Draw the reaction diagram (+, arrow, captions) over the 3D view.
    pub show_reaction_diagram: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            tab: Tab::Build,
            settings_open: false,
            help_open: false,
            about_open: false,
            export_target: ExportTarget::Lab,
            export_labels: true,
            export_details: true,
            show_reaction_diagram: true,
        }
    }
}

/// Which atoms are visible (everything when `None`). Used to isolate one
/// molecule or a reaction while exporting an image.
#[derive(Resource, Default)]
pub struct Isolation(pub Option<std::collections::HashSet<AtomId>>);

impl Isolation {
    pub fn shows(&self, atom: AtomId) -> bool {
        self.0.as_ref().is_none_or(|s| s.contains(&atom))
    }
}
