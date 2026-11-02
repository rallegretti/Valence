//! Shared resources.

use bevy::prelude::*;
use valence_chem::{AtomId, Lab, Namer, Names, Vec3 as CVec3};

/// Bevy and the chem crate both use glam 0.33, so these are the same type.
const _: fn(CVec3) -> Vec3 = |v| v;

pub struct Molecule {
    pub atoms: Vec<AtomId>,
    pub names: Names,
}

/// The chemistry world plus cached per-molecule names.
#[derive(Resource)]
pub struct Chem {
    pub lab: Lab,
    pub namer: Namer,
    pub molecules: Vec<Molecule>,
    /// atom id → index into `molecules`.
    pub atom_mol: Vec<usize>,
    named_rev: u64,
}

impl Default for Chem {
    fn default() -> Self {
        Self {
            lab: Lab::new(),
            namer: Namer::new(),
            molecules: Vec::new(),
            atom_mol: Vec::new(),
            named_rev: u64::MAX,
        }
    }
}

impl Chem {
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
            let names = self.namer.name(&self.lab.graph(&atoms));
            for &a in &atoms {
                self.atom_mol[a as usize] = self.molecules.len();
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
}
