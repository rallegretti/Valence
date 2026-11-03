//! The live play-space: atoms, bonds, bonding rules and the bond-order solver.
//!
//! Model (a simplified but chemically grounded valence model):
//! - Each atom has a bonding *capacity* chosen from its element's allowed
//!   valences (smallest that fits). Hypervalent elements (P, S, Cl, Xe, ...)
//!   expand to the next valence when something more is attached.
//! - Bond orders are never chosen by hand: after every change the solver
//!   resets orders to 1 and raises them to fill as many open sites as possible
//!   (a maximum b-matching). This turns O + O into O=O, N + N into N≡N, and
//!   lets "hydrogenation" (H onto C≡C) happen naturally.
//! - Metal + nonmetal pairs with a large electronegativity gap form ionic
//!   bonds; the bond "order" is the number of electrons transferred, which
//!   gives the ion charges (Mg²⁺ O²⁻).
//! - H onto a full N/O/P/S lone pair makes an "-onium" cation (NH₄⁺, H₃O⁺).
//!
//! Robustness: no operation here panics on any input. Every public mutation
//! either succeeds or returns a `Reject` and leaves the lab unchanged, and
//! all searches are polynomial and bounded.

use std::collections::VecDeque;

use glam::Vec3;

use crate::elements::{self, Category, category, element, valence_electrons};
use crate::graph::{BondKind, MolGraph};

pub type AtomId = u32;
pub type BondId = u32;

/// Hard cap on atoms in the lab, so a runaway session can't exhaust
/// memory or make the frame time collapse.
pub const MAX_ATOMS: usize = 2000;

#[derive(Clone, Debug)]
pub struct Atom {
    pub z: u8,
    pub pos: Vec3,
    /// Current bonding capacity (one of the element's valences).
    pub valence: u8,
    /// Carries an extra proton on a lone pair (+1 charge, +1 capacity).
    pub protonated: bool,
    pub bonds: Vec<BondId>,
    /// Directions of non-bonding electron domains (VSEPR): the first
    /// `free` entries are open bonding sites, the rest are lone pairs.
    pub domains: Vec<Vec3>,
    /// Layout velocity (damped dynamics, see layout.rs).
    pub vel: Vec3,
}

#[derive(Clone, Debug)]
pub struct Bond {
    /// For ionic bonds `a` is always the cation (the metal).
    pub a: AtomId,
    pub b: AtomId,
    pub order: u8,
    pub kind: BondKind,
}

impl Bond {
    pub fn other(&self, id: AtomId) -> AtomId {
        if self.a == id { self.b } else { self.a }
    }
}

/// Why two things won't combine, phrased for the player.
#[derive(Clone, Debug, PartialEq)]
pub struct Reject(pub String);

impl std::fmt::Display for Reject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn reject<T>(msg: impl Into<String>) -> Result<T, Reject> {
    Err(Reject(msg.into()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachMode {
    /// Target has an open site (possibly after the solver lowers a multiple bond).
    Direct,
    /// Target moves up to its next allowed valence (e.g. S: 2 → 4).
    Expand(u8),
    /// A proton lands on a lone pair, making an -onium cation.
    Protonate,
}

#[derive(Clone, Debug)]
pub struct AttachPlan {
    pub kind: BondKind,
    pub mode: AttachMode,
    /// Short description for the UI ("Covalent bond", "Ionic: Na gives 1 e⁻ to Cl", ...).
    pub note: String,
}

/// How each side of a new bond gets room: `None` = already has room,
/// `Some(v)` = expand to valence `v`.
#[derive(Clone, Copy, Debug)]
pub struct ConnectPlan {
    pub kind: BondKind,
    pub expand_a: Option<u8>,
    pub expand_b: Option<u8>,
}

/// Serializable snapshot of a lab: atoms and which pairs are bonded. Bond
/// kinds, orders and charges are recomputed on load by the normal rules.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LabData {
    pub atoms: Vec<AtomData>,
    pub bonds: Vec<BondData>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AtomData {
    pub z: u8,
    pub pos: [f32; 3],
    pub valence: u8,
    #[cfg_attr(feature = "serde", serde(default))]
    pub protonated: bool,
}

/// A bond between `atoms[a]` and `atoms[b]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BondData {
    pub a: u32,
    pub b: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Lab {
    atoms: Vec<Option<Atom>>,
    bonds: Vec<Option<Bond>>,
    free_atom_slots: Vec<AtomId>,
    free_bond_slots: Vec<BondId>,
    revision: u64,
    /// Physics keeps running while this is > 0 (see layout.rs).
    pub(crate) awake: u32,
}

// ---------------------------------------------------------------------------
// Pair rules
// ---------------------------------------------------------------------------

/// What kind of bond two elements would form, or why they can't.
pub fn pair_kind(za: u8, zb: u8) -> Result<BondKind, Reject> {
    for z in [za, zb] {
        let e = element(z);
        if e.valences.is_empty() {
            return reject(if category(z) == Category::NobleGas {
                format!(
                    "{} is a noble gas: its electron shell is already full, so it doesn't bond.",
                    e.name
                )
            } else {
                format!(
                    "{} is a synthetic superheavy element. It decays within seconds, far too fast to form compounds.",
                    e.name
                )
            });
        }
    }
    // Heavy noble gases only react with the most electronegative partners.
    for (n, other) in [(za, zb), (zb, za)] {
        let allowed: &[u8] = match n {
            36 | 86 => &[9],
            54 => &[8, 9],
            _ => continue,
        };
        if !allowed.contains(&other) {
            let partners = if allowed.len() == 1 {
                "fluorine"
            } else {
                "fluorine or oxygen"
            };
            return reject(format!(
                "{} only forms compounds with {partners}.",
                element(n).name
            ));
        }
    }
    let (ma, mb) = (category(za).is_metal(), category(zb).is_metal());
    match (ma, mb) {
        (true, true) => reject(
            "Two metals don't form a molecule: they'd share a sea of electrons as a metallic alloy instead.",
        ),
        (false, false) => Ok(BondKind::Covalent),
        _ => {
            let (m, n) = if ma { (za, zb) } else { (zb, za) };
            let strongly_ionic = matches!(category(m), Category::AlkaliMetal)
                || (category(m) == Category::AlkalineEarth && m != 4);
            if strongly_ionic || element(n).en - element(m).en >= 1.7 {
                Ok(BondKind::Ionic)
            } else {
                Ok(BondKind::Covalent)
            }
        }
    }
}

/// World-space bond length for the layout (not to physical scale: atoms are
/// drawn smaller than their radii so bonds stay visible).
pub fn bond_length(za: u8, zb: u8, order: u8, kind: BondKind) -> f32 {
    let sum = (element(za).radius_pm + element(zb).radius_pm) as f32;
    let base = 0.55 + 0.009 * sum;
    match kind {
        BondKind::Ionic => base * 1.08,
        BondKind::Covalent => match order {
            0 | 1 => base,
            2 => base * 0.88,
            _ => base * 0.8,
        },
    }
}

/// Visual sphere radius in world units.
pub fn atom_radius(z: u8) -> f32 {
    0.22 + 0.0026 * element(z).radius_pm as f32
}

fn smallest_valence_at_least(z: u8, need: u8) -> Option<u8> {
    element(z).valences.iter().copied().find(|&v| v >= need)
}

fn next_valence(z: u8, current: u8) -> Option<u8> {
    element(z).valences.iter().copied().find(|&v| v > current)
}

/// Is `z` the electron-accepting (anion) side of an ionic bond?
fn is_anion_side(kind: BondKind, z: u8) -> bool {
    kind == BondKind::Ionic && !category(z).is_metal()
}

fn valid_z(z: u8) -> Result<(), Reject> {
    if (1..=118).contains(&z) {
        Ok(())
    } else {
        reject("Unknown element.")
    }
}

// ---------------------------------------------------------------------------
// Lab: storage & queries
// ---------------------------------------------------------------------------

impl Lab {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bumped on every structural change (atoms/bonds added, removed, re-ordered).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn atom(&self, id: AtomId) -> Option<&Atom> {
        self.atoms.get(id as usize).and_then(|a| a.as_ref())
    }

    pub(crate) fn atom_mut(&mut self, id: AtomId) -> Option<&mut Atom> {
        self.atoms.get_mut(id as usize).and_then(|a| a.as_mut())
    }

    pub fn bond(&self, id: BondId) -> Option<&Bond> {
        self.bonds.get(id as usize).and_then(|b| b.as_ref())
    }

    fn bond_mut(&mut self, id: BondId) -> Option<&mut Bond> {
        self.bonds.get_mut(id as usize).and_then(|b| b.as_mut())
    }

    pub fn atoms(&self) -> impl Iterator<Item = (AtomId, &Atom)> {
        self.atoms
            .iter()
            .enumerate()
            .filter_map(|(i, a)| a.as_ref().map(|a| (i as AtomId, a)))
    }

    pub fn bonds(&self) -> impl Iterator<Item = (BondId, &Bond)> {
        self.bonds
            .iter()
            .enumerate()
            .filter_map(|(i, b)| b.as_ref().map(|b| (i as BondId, b)))
    }

    pub fn atom_count(&self) -> usize {
        self.atoms.len() - self.free_atom_slots.len()
    }

    /// Slot capacity: all live ids are below this.
    pub fn atom_slots(&self) -> usize {
        self.atoms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.atom_count() == 0
    }

    pub fn is_full(&self) -> bool {
        self.atom_count() >= MAX_ATOMS
    }

    pub fn clear(&mut self) {
        *self = Lab {
            revision: self.revision + 1,
            ..Lab::default()
        };
    }

    /// Snapshot for saving. Also returns the atom id at each index, so
    /// callers can translate their own references (e.g. reaction anchors).
    pub fn to_data(&self) -> (LabData, Vec<AtomId>) {
        let ids: Vec<AtomId> = self.atoms().map(|(id, _)| id).collect();
        let mut index = vec![u32::MAX; self.atoms.len()];
        let atoms = ids
            .iter()
            .enumerate()
            .filter_map(|(i, &id)| {
                let a = self.atom(id)?;
                index[id as usize] = i as u32;
                Some(AtomData {
                    z: a.z,
                    pos: a.pos.to_array(),
                    valence: a.valence,
                    protonated: a.protonated,
                })
            })
            .collect();
        let bonds = self
            .bonds()
            .filter_map(|(_, b)| {
                let (ia, ib) = (*index.get(b.a as usize)?, *index.get(b.b as usize)?);
                (ia != u32::MAX && ib != u32::MAX).then_some(BondData { a: ia, b: ib })
            })
            .collect();
        (LabData { atoms, bonds }, ids)
    }

    /// Build a lab from a snapshot, rejecting anything inconsistent. Returns
    /// the new lab and the atom id for each snapshot index. Never touches an
    /// existing lab, so a bad file can't damage what's open.
    pub fn from_data(data: &LabData) -> Result<(Lab, Vec<AtomId>), Reject> {
        let bad = |msg: String| -> Result<(Lab, Vec<AtomId>), Reject> {
            Err(Reject(format!("Invalid lab data: {msg}")))
        };
        if data.atoms.len() > MAX_ATOMS {
            return bad(format!(
                "{} atoms is more than the limit of {MAX_ATOMS}",
                data.atoms.len()
            ));
        }
        let mut lab = Lab::new();
        let mut ids = Vec::with_capacity(data.atoms.len());
        for (i, a) in data.atoms.iter().enumerate() {
            if !(1..=118).contains(&a.z) {
                return bad(format!("atom {i} has unknown element {}", a.z));
            }
            let pos = Vec3::from_array(a.pos);
            if !pos.is_finite() || pos.abs().max_element() > 1.0e6 {
                return bad(format!("atom {i} has an invalid position"));
            }
            let allowed = element(a.z).valences;
            if !allowed.contains(&a.valence) && !(allowed.is_empty() && a.valence == 0) {
                return bad(format!(
                    "atom {i} ({}) can't have valence {}",
                    element(a.z).symbol,
                    a.valence
                ));
            }
            if a.protonated && (!elements::can_protonate(a.z) || category(a.z).is_metal()) {
                return bad(format!(
                    "atom {i} ({}) can't be protonated",
                    element(a.z).symbol
                ));
            }
            ids.push(lab.alloc_atom(Atom {
                z: a.z,
                pos,
                valence: a.valence,
                protonated: a.protonated,
                bonds: Vec::new(),
                domains: Vec::new(),
                vel: Vec3::ZERO,
            }));
        }
        let mut seen = std::collections::HashSet::new();
        for (k, b) in data.bonds.iter().enumerate() {
            let (Some(&ia), Some(&ib)) = (ids.get(b.a as usize), ids.get(b.b as usize)) else {
                return bad(format!("bond {k} refers to a missing atom"));
            };
            if ia == ib {
                return bad(format!("bond {k} joins an atom to itself"));
            }
            if !seen.insert((ia.min(ib), ia.max(ib))) {
                return bad(format!("bond {k} is a duplicate"));
            }
            let (za, zb) = (data.atoms[b.a as usize].z, data.atoms[b.b as usize].z);
            let kind = match pair_kind(za, zb) {
                Ok(k) => k,
                Err(e) => return bad(format!("bond {k}: {e}")),
            };
            if lab.alloc_bond(ia, ib, kind).is_none() {
                return bad(format!("bond {k} couldn't be created"));
            }
        }
        for (i, &id) in ids.iter().enumerate() {
            if lab.sigma(id) > lab.capacity(id) {
                return bad(format!("atom {i} has more bonds than it can hold"));
            }
        }
        lab.after_change(&ids);
        if let Err(e) = lab.validate() {
            return bad(e);
        }
        Ok((lab, ids))
    }

    /// Replace the contents with a snapshot (undo/redo/rollback). The
    /// revision keeps increasing, so anything cached per revision refreshes.
    pub fn restore(&mut self, mut snapshot: Lab) {
        snapshot.revision = self.revision.max(snapshot.revision) + 1;
        snapshot.awake = 240;
        *self = snapshot;
    }

    /// Stop the layout until the next change.
    pub fn sleep(&mut self) {
        self.awake = 0;
    }

    pub fn set_pos(&mut self, id: AtomId, pos: Vec3) {
        if !pos.is_finite() {
            return;
        }
        if let Some(a) = self.atom_mut(id) {
            a.pos = pos;
        }
        self.wake();
    }

    /// Move a set of atoms rigidly (dragging a whole molecule).
    pub fn translate(&mut self, ids: &[AtomId], delta: Vec3) {
        if !delta.is_finite() {
            return;
        }
        for &id in ids {
            if let Some(a) = self.atom_mut(id) {
                a.pos += delta;
            }
        }
        self.wake();
    }

    pub fn neighbors(&self, id: AtomId) -> impl Iterator<Item = (AtomId, &Bond)> + '_ {
        self.atom(id)
            .into_iter()
            .flat_map(|a| a.bonds.iter())
            .filter_map(move |&b| self.bond(b).map(|bd| (bd.other(id), bd)))
    }

    pub fn bonded(&self, a: AtomId, b: AtomId) -> bool {
        self.neighbors(a).any(|(n, _)| n == b)
    }

    /// Number of bonds (σ count), regardless of order.
    pub fn sigma(&self, id: AtomId) -> u8 {
        self.atom(id)
            .map_or(0, |a| a.bonds.len().min(u8::MAX as usize) as u8)
    }

    /// Bonding electrons in use (sum of bond orders).
    pub fn used(&self, id: AtomId) -> u8 {
        self.neighbors(id)
            .fold(0u8, |acc, (_, b)| acc.saturating_add(b.order))
    }

    pub fn capacity(&self, id: AtomId) -> u8 {
        self.atom(id)
            .map_or(0, |a| a.valence.saturating_add(a.protonated as u8))
    }

    /// Open bonding sites (unpaired electrons).
    pub fn free(&self, id: AtomId) -> u8 {
        self.capacity(id).saturating_sub(self.used(id))
    }

    pub fn charge(&self, id: AtomId) -> i8 {
        let Some(a) = self.atom(id) else { return 0 };
        let mut q = a.protonated as i8;
        for (_, b) in self.neighbors(id) {
            if b.kind == BondKind::Ionic {
                let o = b.order.min(i8::MAX as u8) as i8;
                q = if b.a == id {
                    q.saturating_add(o)
                } else {
                    q.saturating_sub(o)
                };
            }
        }
        q
    }

    pub fn covalent_used(&self, id: AtomId) -> u8 {
        self.neighbors(id)
            .filter(|(_, b)| b.kind == BondKind::Covalent)
            .fold(0u8, |acc, (_, b)| acc.saturating_add(b.order))
    }

    pub fn lone_pairs(&self, id: AtomId) -> u8 {
        let Some(a) = self.atom(id) else { return 0 };
        let v = valence_electrons(a.z) as i32;
        if v == 0 {
            return 0;
        }
        let nonbonding = v - self.covalent_used(id) as i32 - self.charge(id) as i32;
        ((nonbonding - self.free(id) as i32) / 2).clamp(0, 8) as u8
    }

    pub fn has_ionic(&self, id: AtomId) -> bool {
        self.neighbors(id).any(|(_, b)| b.kind == BondKind::Ionic)
    }

    // -----------------------------------------------------------------------
    // Components & graphs
    // -----------------------------------------------------------------------

    /// Atoms connected to `start`, sorted ascending (empty if `start` is gone).
    pub fn component(&self, start: AtomId) -> Vec<AtomId> {
        if self.atom(start).is_none() {
            return Vec::new();
        }
        let mut seen = vec![false; self.atoms.len()];
        self.component_with(start, &mut seen)
    }

    fn component_with(&self, start: AtomId, seen: &mut [bool]) -> Vec<AtomId> {
        let mut out = vec![start];
        if let Some(s) = seen.get_mut(start as usize) {
            *s = true;
        }
        let mut i = 0;
        while let Some(&u) = out.get(i) {
            i += 1;
            for (v, _) in self.neighbors(u) {
                if let Some(s) = seen.get_mut(v as usize)
                    && !*s
                {
                    *s = true;
                    out.push(v);
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// All molecules (connected components), each sorted, ordered by lowest id.
    pub fn components(&self) -> Vec<Vec<AtomId>> {
        let mut seen = vec![false; self.atoms.len()];
        let mut out = Vec::new();
        for (id, _) in self.atoms() {
            if !seen.get(id as usize).copied().unwrap_or(true) {
                out.push(self.component_with(id, &mut seen));
            }
        }
        out
    }

    /// Naming graph for a component; node i corresponds to `atoms[i]`.
    pub fn graph(&self, atoms: &[AtomId]) -> MolGraph {
        let mut g = MolGraph::default();
        let mut map = std::collections::HashMap::with_capacity(atoms.len());
        for &a in atoms {
            if let Some(at) = self.atom(a) {
                map.insert(a, g.add_atom(at.z, self.charge(a), self.free(a)));
            }
        }
        for (_, b) in self.bonds() {
            if let (Some(&ia), Some(&ib)) = (map.get(&b.a), map.get(&b.b)) {
                g.add_edge(ia, ib, b.order, b.kind);
            }
        }
        g
    }

    /// Check internal invariants. Used by tests and debug builds; returns a
    /// description of the first problem found.
    pub fn validate(&self) -> Result<(), String> {
        for (bid, b) in self.bonds() {
            for end in [b.a, b.b] {
                let Some(a) = self.atom(end) else {
                    return Err(format!("bond {bid} points at dead atom {end}"));
                };
                if !a.bonds.contains(&bid) {
                    return Err(format!("atom {end} doesn't list bond {bid}"));
                }
            }
            if b.a == b.b {
                return Err(format!("bond {bid} is a self-loop"));
            }
            if !(1..=3).contains(&b.order) {
                return Err(format!("bond {bid} has order {}", b.order));
            }
        }
        for (id, a) in self.atoms() {
            for &bid in &a.bonds {
                match self.bond(bid) {
                    Some(b) if b.a == id || b.b == id => {}
                    _ => return Err(format!("atom {id} lists foreign/dead bond {bid}")),
                }
            }
            let mut nbrs: Vec<AtomId> = self.neighbors(id).map(|(n, _)| n).collect();
            nbrs.sort_unstable();
            if nbrs.windows(2).any(|w| w[0] == w[1]) {
                return Err(format!("atom {id} has a duplicate bond"));
            }
            if self.used(id) > self.capacity(id) {
                return Err(format!(
                    "atom {id} over capacity: {} > {}",
                    self.used(id),
                    self.capacity(id)
                ));
            }
            if !a.pos.is_finite() {
                return Err(format!("atom {id} has a non-finite position"));
            }
            if a.domains.len() != (self.free(id) + self.lone_pairs(id)) as usize {
                return Err(format!(
                    "atom {id} has {} domains, expected {}",
                    a.domains.len(),
                    self.free(id) + self.lone_pairs(id)
                ));
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Mutations
    // -----------------------------------------------------------------------

    fn alloc_atom(&mut self, atom: Atom) -> AtomId {
        if let Some(id) = self.free_atom_slots.pop()
            && let Some(slot) = self.atoms.get_mut(id as usize)
        {
            *slot = Some(atom);
            return id;
        }
        self.atoms.push(Some(atom));
        (self.atoms.len() - 1) as AtomId
    }

    /// Both atoms must exist (callers check first).
    fn alloc_bond(&mut self, a: AtomId, b: AtomId, kind: BondKind) -> Option<BondId> {
        let za = self.atom(a)?.z;
        self.atom(b)?;
        // Ionic bonds store the cation first.
        let (a, b) = if kind == BondKind::Ionic && !category(za).is_metal() {
            (b, a)
        } else {
            (a, b)
        };
        let bond = Bond {
            a,
            b,
            order: 1,
            kind,
        };
        let id = match self.free_bond_slots.pop() {
            Some(id) if (id as usize) < self.bonds.len() => {
                self.bonds[id as usize] = Some(bond);
                id
            }
            _ => {
                self.bonds.push(Some(bond));
                (self.bonds.len() - 1) as BondId
            }
        };
        self.atom_mut(a)?.bonds.push(id);
        self.atom_mut(b)?.bonds.push(id);
        Some(id)
    }

    /// Drop a lone atom into the play space.
    pub fn add_atom(&mut self, z: u8, pos: Vec3) -> Result<AtomId, Reject> {
        valid_z(z)?;
        if self.is_full() {
            return reject(format!(
                "The lab is full ({MAX_ATOMS} atoms). Remove something first."
            ));
        }
        let pos = if pos.is_finite() { pos } else { Vec3::ZERO };
        let valence = element(z).valences.first().copied().unwrap_or(0);
        let id = self.alloc_atom(Atom {
            z,
            pos,
            valence,
            protonated: false,
            bonds: Vec::new(),
            domains: Vec::new(),
            vel: Vec3::ZERO,
        });
        self.after_change(&[id]);
        Ok(id)
    }

    /// Add a whole molecule (e.g. a reaction product from the built-in
    /// library) around `origin`. Bond kinds follow the normal pair rules and
    /// bond orders come from the solver, so the result is exactly what the
    /// player would get building it by hand. All-or-nothing: on any problem
    /// the lab is left unchanged.
    pub fn add_molecule(&mut self, g: &MolGraph, origin: Vec3) -> Result<Vec<AtomId>, Reject> {
        if g.is_empty() {
            return reject("Nothing to add.");
        }
        if self.atom_count() + g.len() > MAX_ATOMS {
            return reject(format!(
                "The lab is full ({MAX_ATOMS} atoms). Remove something first."
            ));
        }
        let mut trial = self.clone();
        let origin = if origin.is_finite() {
            origin
        } else {
            Vec3::ZERO
        };

        // Lay atoms out breadth-first so bonded atoms start near each other;
        // the layout physics tidies up from there.
        let n = g.len();
        let mut pos = vec![None::<Vec3>; n];
        for start in 0..n {
            if pos[start].is_some() {
                continue;
            }
            pos[start] = Some(origin + jitter_dir(start as u64 * 131) * 0.5 * start as f32);
            let mut queue = VecDeque::from([start]);
            while let Some(u) = queue.pop_front() {
                let pu = pos[u].unwrap_or(origin);
                for (k, &(v, _)) in g.neighbors(u).iter().enumerate() {
                    if pos[v].is_none() {
                        let len = bond_length(g.z[u], g.z[v], 1, BondKind::Covalent);
                        pos[v] = Some(pu + jitter_dir((u * 977 + k * 31 + v) as u64) * len);
                        queue.push_back(v);
                    }
                }
            }
        }

        let mut ids = Vec::with_capacity(n);
        for (i, p) in pos.iter().enumerate() {
            let z = g.z[i];
            valid_z(z)?;
            let protonated =
                g.charge[i] > 0 && elements::can_protonate(z) && !category(z).is_metal();
            let id = trial.alloc_atom(Atom {
                z,
                pos: p.unwrap_or(origin),
                valence: element(z).valences.first().copied().unwrap_or(0),
                protonated,
                bonds: Vec::new(),
                domains: Vec::new(),
                vel: Vec3::ZERO,
            });
            ids.push(id);
        }
        for e in &g.edges {
            let (a, b) = (ids[e.a], ids[e.b]);
            let kind = pair_kind(g.z[e.a], g.z[e.b])?;
            trial
                .alloc_bond(a, b, kind)
                .ok_or_else(|| Reject("Couldn't create a bond.".into()))?;
        }
        // Each atom takes the smallest valence that covers the bond orders in
        // the source structure (so S in H2SO4 gets 6, not 4).
        let mut order_sum = vec![0u8; n];
        for e in &g.edges {
            order_sum[e.a] = order_sum[e.a].saturating_add(e.order);
            order_sum[e.b] = order_sum[e.b].saturating_add(e.order);
        }
        for (i, &id) in ids.iter().enumerate() {
            let (z, protonated) = match trial.atom(id) {
                Some(a) => (a.z, a.protonated as u8),
                None => continue,
            };
            let need = order_sum[i].max(trial.sigma(id)).saturating_sub(protonated);
            let Some(v) = smallest_valence_at_least(z, need) else {
                return reject(format!("{} can't form {} bonds.", element(z).name, need));
            };
            if let Some(a) = trial.atom_mut(id) {
                a.valence = v;
            }
        }
        trial.after_change(&ids);
        *self = trial;
        Ok(ids)
    }

    pub fn remove_atom(&mut self, id: AtomId) {
        let Some(atom) = self.atom(id).cloned() else {
            return;
        };
        let mut touched = Vec::new();
        for &bid in &atom.bonds {
            let Some(bond) = self.bonds.get_mut(bid as usize).and_then(Option::take) else {
                continue;
            };
            self.free_bond_slots.push(bid);
            let other = bond.other(id);
            touched.push(other);
            // What the neighbour keeps after losing this bond (the bond is
            // already out of the table, so `used` no longer counts it).
            let remaining = self.used(other);
            let Some(o) = self.atom_mut(other) else {
                continue;
            };
            o.bonds.retain(|&b| b != bid);
            // Shrink back to the smallest valence that still covers what it
            // keeps (SO₃ minus an O → SO₂, NH₄⁺ minus H → NH₃).
            if o.protonated && remaining <= o.valence {
                o.protonated = false;
            }
            let p = o.protonated as u8;
            let sigma = o.bonds.len().min(u8::MAX as usize) as u8;
            let need = remaining.saturating_sub(p).max(sigma.saturating_sub(p));
            if let Some(v) = smallest_valence_at_least(o.z, need) {
                o.valence = o.valence.min(v);
            }
        }
        if let Some(slot) = self.atoms.get_mut(id as usize) {
            *slot = None;
            self.free_atom_slots.push(id);
        }
        self.after_change(&touched);
    }

    /// Can element `z` be attached to atom `target`? Cheap enough to call on hover.
    pub fn check_attach(&self, target: AtomId, z: u8) -> Result<AttachPlan, Reject> {
        valid_z(z)?;
        let Some(t) = self.atom(target) else {
            return reject("That atom no longer exists.");
        };
        if self.is_full() {
            return reject(format!(
                "The lab is full ({MAX_ATOMS} atoms). Remove something first."
            ));
        }
        let kind = pair_kind(t.z, z)?;
        let tname = element(t.z).name;
        if smallest_valence_at_least(z, 1).is_none() {
            return reject(format!("{} can't form a bond here.", element(z).name));
        }

        let anion_target = is_anion_side(kind, t.z);
        if anion_target {
            // Target is the anion side: it can only take electrons into its
            // base valence (Cl⁻, O²⁻), never by expanding.
            let base = smallest_valence_at_least(t.z, 1).unwrap_or(0);
            if t.protonated || self.sigma(target).saturating_add(1) > base {
                return reject(format!(
                    "{tname} can't accept another electron here: it already has a full octet."
                ));
            }
        }

        let mut candidates = Vec::new();
        if self.sigma(target) < self.capacity(target) {
            candidates.push(AttachMode::Direct);
        }
        if z == 1
            && kind == BondKind::Covalent
            && elements::can_protonate(t.z)
            && !t.protonated
            && !self.has_ionic(target)
            && self.lone_pairs(target) > 0
            && self.sigma(target) == self.capacity(target)
        {
            candidates.push(AttachMode::Protonate);
        }
        if !anion_target
            && let Some(next) = next_valence(t.z, t.valence)
            && self.sigma(target) < next.saturating_add(t.protonated as u8)
        {
            candidates.push(AttachMode::Expand(next));
        }
        if candidates.is_empty() {
            let max = element(t.z)
                .valences
                .last()
                .copied()
                .unwrap_or(0)
                .saturating_add(t.protonated as u8);
            return reject(if category(t.z).is_metal() {
                format!("{tname} has no more electrons to give (max {max}).")
            } else {
                format!(
                    "{tname} already has {} bond{}: its maximum.",
                    self.sigma(target),
                    if max == 1 { "" } else { "s" }
                )
            });
        }

        // Pick the option that leaves the fewest open sites; ties favour the
        // earlier (simpler) option.
        let mut mode = candidates[0];
        if candidates.len() > 1 {
            let mut best = u32::MAX;
            for &m in &candidates {
                let mut trial = self.clone();
                if let Some(id) = trial.apply_attach(target, z, m, kind, Vec3::ZERO) {
                    let open: u32 = trial
                        .component(id)
                        .iter()
                        .map(|&a| trial.free(a) as u32)
                        .sum();
                    if open < best {
                        best = open;
                        mode = m;
                    }
                }
            }
        }

        let (n_new, n_t) = (element(z).symbol, element(t.z).symbol);
        let note = match (kind, mode) {
            (BondKind::Ionic, _) => {
                let (m, n) = if category(z).is_metal() {
                    (n_new, n_t)
                } else {
                    (n_t, n_new)
                };
                format!("Ionic bond: {m} gives electrons to {n}")
            }
            (_, AttachMode::Protonate) => {
                format!("Protonation: H⁺ bonds to a lone pair on {n_t} (+1 charge)")
            }
            (_, AttachMode::Expand(v)) => format!("Covalent bond ({n_t} expands to valence {v})"),
            _ => {
                let den = (element(z).en - element(t.z).en).abs();
                if den > 0.4 {
                    "Polar covalent bond".into()
                } else {
                    "Covalent bond".into()
                }
            }
        };
        Ok(AttachPlan { kind, mode, note })
    }

    /// Attach a new atom of element `z` to `target`. `hint` is a world-space
    /// direction used to pick which open site to use.
    pub fn attach(&mut self, target: AtomId, z: u8, hint: Vec3) -> Result<AtomId, Reject> {
        let plan = self.check_attach(target, z)?;
        self.apply_attach(target, z, plan.mode, plan.kind, hint)
            .ok_or_else(|| Reject("Couldn't place the new atom.".into()))
    }

    fn apply_attach(
        &mut self,
        target: AtomId,
        z: u8,
        mode: AttachMode,
        kind: BondKind,
        hint: Vec3,
    ) -> Option<AtomId> {
        let valence = smallest_valence_at_least(z, 1)?;
        let tz = self.atom(target)?.z;
        let (dir, used_domain) = self.placement_dir(target, hint);
        let t = self.atom_mut(target)?;
        match mode {
            AttachMode::Direct => {}
            AttachMode::Expand(v) => t.valence = v,
            AttachMode::Protonate => t.protonated = true,
        }
        if let Some(i) = used_domain
            && i < t.domains.len()
        {
            t.domains.remove(i);
        }
        let pos = t.pos + dir * bond_length(tz, z, 1, kind);
        let id = self.alloc_atom(Atom {
            z,
            pos,
            valence,
            protonated: false,
            bonds: Vec::new(),
            domains: Vec::new(),
            vel: Vec3::ZERO,
        });
        self.alloc_bond(target, id, kind)?;
        self.after_change(&[target, id]);
        Some(id)
    }

    /// Can two existing atoms be bonded (joining molecules or closing a ring)?
    pub fn check_connect(&self, a: AtomId, b: AtomId) -> Result<ConnectPlan, Reject> {
        if a == b {
            return reject("An atom can't bond to itself.");
        }
        let (Some(aa), Some(ab)) = (self.atom(a), self.atom(b)) else {
            return reject("That atom no longer exists.");
        };
        if self.bonded(a, b) {
            return reject("These atoms are already bonded.");
        }
        let kind = pair_kind(aa.z, ab.z)?;
        for (id, at) in [(a, aa), (b, ab)] {
            if is_anion_side(kind, at.z)
                && (at.protonated
                    || self.sigma(id).saturating_add(1)
                        > smallest_valence_at_least(at.z, 1).unwrap_or(0))
            {
                return reject(format!(
                    "{} can't accept another electron: its octet is full.",
                    element(at.z).name
                ));
            }
        }
        let options = |id: AtomId, at: &Atom| -> Vec<Option<u8>> {
            let mut v = Vec::new();
            if self.sigma(id) < self.capacity(id) {
                v.push(None);
            }
            if !is_anion_side(kind, at.z)
                && let Some(next) = next_valence(at.z, at.valence)
            {
                v.push(Some(next));
            }
            v
        };
        let (oa, ob) = (options(a, aa), options(b, ab));
        if oa.is_empty() || ob.is_empty() {
            let full = if oa.is_empty() { aa } else { ab };
            return reject(format!(
                "{} has no open bonding sites left.",
                element(full.z).name
            ));
        }
        let mut best: Option<(u32, ConnectPlan)> = None;
        for &expand_a in &oa {
            for &expand_b in &ob {
                let plan = ConnectPlan {
                    kind,
                    expand_a,
                    expand_b,
                };
                let mut trial = self.clone();
                if trial.apply_connect(a, b, plan).is_none() {
                    continue;
                }
                let open: u32 = trial
                    .component(a)
                    .iter()
                    .map(|&i| trial.free(i) as u32)
                    .sum();
                if best.as_ref().is_none_or(|(o, _)| open < *o) {
                    best = Some((open, plan));
                }
            }
        }
        best.map(|(_, p)| p)
            .ok_or_else(|| Reject("These atoms can't bond here.".into()))
    }

    pub fn connect(&mut self, a: AtomId, b: AtomId) -> Result<(), Reject> {
        let plan = self.check_connect(a, b)?;
        self.apply_connect(a, b, plan)
            .ok_or_else(|| Reject("These atoms can't bond here.".into()))
    }

    fn apply_connect(&mut self, a: AtomId, b: AtomId, plan: ConnectPlan) -> Option<()> {
        if let Some(v) = plan.expand_a {
            self.atom_mut(a)?.valence = v;
        }
        if let Some(v) = plan.expand_b {
            self.atom_mut(b)?.valence = v;
        }
        self.alloc_bond(a, b, plan.kind)?;
        self.after_change(&[a, b]);
        Some(())
    }

    /// Direction for a new neighbour: the open site closest to `hint`, else
    /// the emptiest direction around the atom. Also returns the index of the
    /// consumed domain, if one was used.
    fn placement_dir(&self, id: AtomId, hint: Vec3) -> (Vec3, Option<usize>) {
        let Some(a) = self.atom(id) else {
            return (Vec3::Y, None);
        };
        let free = self.free(id) as usize;
        let hint = if hint.is_finite() {
            hint.normalize_or_zero()
        } else {
            Vec3::ZERO
        };
        let mut best: Option<(f32, usize)> = None;
        for (i, &d) in a
            .domains
            .iter()
            .enumerate()
            .take(free.max(1).min(a.domains.len()))
        {
            let score = d.dot(hint);
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, i));
            }
        }
        if let Some((_, i)) = best
            && let Some(&d) = a.domains.get(i)
            && d.is_finite()
            && d != Vec3::ZERO
        {
            return (d.normalize_or(Vec3::Y), Some(i));
        }
        let sum: Vec3 = self
            .neighbors(id)
            .filter_map(|(n, _)| self.atom(n))
            .map(|n| (n.pos - a.pos).normalize_or_zero())
            .sum();
        let away = -sum;
        let d = if away.length_squared() > 1e-4 {
            away.normalize_or(Vec3::Y)
        } else if hint != Vec3::ZERO {
            hint
        } else {
            jitter_dir(id as u64 * 7919 + a.bonds.len() as u64)
        };
        (d, None)
    }

    /// Re-solve bond orders and refresh domain directions for the molecules
    /// containing `seeds`.
    fn after_change(&mut self, seeds: &[AtomId]) {
        let mut done: Vec<AtomId> = Vec::new();
        for &s in seeds {
            if self.atom(s).is_none() || done.contains(&s) {
                continue;
            }
            let comp = self.component(s);
            self.solve(&comp);
            for &a in &comp {
                self.sync_domains(a);
            }
            done.extend(comp);
        }
        self.revision += 1;
        self.wake();
        debug_assert!(
            self.validate().is_ok(),
            "lab invariant broken: {:?}",
            self.validate()
        );
    }

    // -----------------------------------------------------------------------
    // Bond-order solver
    // -----------------------------------------------------------------------

    /// Reset orders to 1, then raise them to fill as many open sites as
    /// possible: greedy round-robin, then augmenting alternating paths.
    /// Runs in O(F·(V+E)) where F is the number of open sites.
    pub(crate) fn solve(&mut self, comp: &[AtomId]) {
        let mut bond_ids: Vec<BondId> = comp
            .iter()
            .filter_map(|&a| self.atom(a))
            .flat_map(|a| a.bonds.iter().copied())
            .collect();
        bond_ids.sort_unstable();
        bond_ids.dedup();
        for &b in &bond_ids {
            if let Some(bd) = self.bond_mut(b) {
                bd.order = 1;
            }
        }
        // Capacity must cover σ bonds (expand if somehow short).
        for &a in comp {
            let sigma = self.sigma(a);
            if self.capacity(a) < sigma
                && let Some(at) = self.atom_mut(a)
            {
                let need = sigma.saturating_sub(at.protonated as u8);
                if let Some(v) = smallest_valence_at_least(at.z, need) {
                    at.valence = v;
                }
            }
        }
        // Greedy, one increment per bond per pass so multiple bonds spread evenly.
        loop {
            let mut changed = false;
            for &b in &bond_ids {
                let Some(bd) = self.bond(b) else { continue };
                let (x, y, o) = (bd.a, bd.b, bd.order);
                if o < 3
                    && self.free(x) > 0
                    && self.free(y) > 0
                    && let Some(bd) = self.bond_mut(b)
                {
                    bd.order += 1;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        // Each augmentation fills two open sites, so this is bounded.
        let max_rounds = comp.iter().map(|&a| self.free(a) as usize).sum::<usize>() / 2 + 1;
        for _ in 0..max_rounds {
            if !self.augment(comp) {
                break;
            }
        }
    }

    /// One augmenting path: open(s) -inc- x -dec- y -inc- ... -inc- open(t).
    /// Breadth-first over (atom, parity) states, so it's polynomial.
    fn augment(&mut self, comp: &[AtomId]) -> bool {
        let Some(&max_id) = comp.iter().max() else {
            return false;
        };
        let n = max_id as usize + 1;
        for &s in comp {
            if self.free(s) == 0 {
                continue;
            }
            // parent[(atom, parity)] = (previous state, bond used)
            let mut parent: Vec<Option<((AtomId, u8), BondId)>> = vec![None; n * 2];
            let mut visited = vec![false; n * 2];
            let idx = |a: AtomId, p: u8| a as usize * 2 + p as usize;
            visited[idx(s, 0)] = true;
            let mut queue = VecDeque::from([(s, 0u8, None::<BondId>)]);
            let mut found: Option<(AtomId, BondId, (AtomId, u8))> = None;
            'bfs: while let Some((u, parity, came_by)) = queue.pop_front() {
                let Some(au) = self.atom(u) else { continue };
                for &b in &au.bonds {
                    if Some(b) == came_by {
                        continue;
                    }
                    let Some(bd) = self.bond(b) else { continue };
                    let v = bd.other(u);
                    if v as usize >= n {
                        continue;
                    }
                    if parity == 0 {
                        // Increment step.
                        if bd.order >= 3 {
                            continue;
                        }
                        let need = if v == s { 2 } else { 1 };
                        if self.free(v) >= need {
                            found = Some((v, b, (u, parity)));
                            break 'bfs;
                        }
                        if !visited[idx(v, 1)] {
                            visited[idx(v, 1)] = true;
                            parent[idx(v, 1)] = Some(((u, parity), b));
                            queue.push_back((v, 1, Some(b)));
                        }
                    } else {
                        // Decrement step.
                        if bd.order < 2 || visited[idx(v, 0)] {
                            continue;
                        }
                        visited[idx(v, 0)] = true;
                        parent[idx(v, 0)] = Some(((u, parity), b));
                        queue.push_back((v, 0, Some(b)));
                    }
                }
            }
            let Some((_, last_bond, mut state)) = found else {
                continue;
            };
            // Rebuild the path (bond ids, from the end back to s).
            let mut path = vec![last_bond];
            while state != (s, 0) {
                let Some((prev, b)) = parent.get(idx(state.0, state.1)).copied().flatten() else {
                    break;
                };
                path.push(b);
                state = prev;
            }
            if state != (s, 0) {
                continue;
            }
            path.reverse();
            let mut uniq = path.clone();
            uniq.sort_unstable();
            uniq.dedup();
            if uniq.len() != path.len() {
                continue; // a path that reuses a bond isn't a valid augmentation
            }
            // Apply, then verify; roll back if anything went out of range.
            let before: Vec<(BondId, u8)> = path
                .iter()
                .filter_map(|&b| self.bond(b).map(|bd| (b, bd.order)))
                .collect();
            let mut ok = true;
            for (i, &b) in path.iter().enumerate() {
                if let Some(bd) = self.bond_mut(b) {
                    if i % 2 == 0 && bd.order < 3 {
                        bd.order += 1;
                    } else if i % 2 == 1 && bd.order > 1 {
                        bd.order -= 1;
                    } else {
                        ok = false;
                    }
                }
            }
            let ends_ok = path.iter().filter_map(|&b| self.bond(b)).all(|bd| {
                self.used(bd.a) <= self.capacity(bd.a) && self.used(bd.b) <= self.capacity(bd.b)
            });
            if ok && ends_ok {
                return true;
            }
            for (b, o) in before {
                if let Some(bd) = self.bond_mut(b) {
                    bd.order = o;
                }
            }
        }
        false
    }

    /// Keep one direction per open site / lone pair, preserving existing ones.
    fn sync_domains(&mut self, id: AtomId) {
        let want = (self.free(id) + self.lone_pairs(id)) as usize;
        let Some(a) = self.atom(id) else { return };
        let pos = a.pos;
        let mut domains = a.domains.clone();
        domains.truncate(want);
        let nbr_dirs: Vec<Vec3> = self
            .neighbors(id)
            .filter_map(|(n, _)| self.atom(n))
            .map(|n| (n.pos - pos).normalize_or_zero())
            .collect();
        let mut salt = 0u64;
        while domains.len() < want {
            let crowd: Vec3 = nbr_dirs.iter().chain(domains.iter()).copied().sum();
            let j = jitter_dir(id as u64 * 31 + domains.len() as u64 * 977 + salt);
            salt += 1;
            domains.push((-crowd + j * 0.6).normalize_or(j));
        }
        if let Some(a) = self.atom_mut(id) {
            a.domains = domains;
        }
    }

    pub fn wake(&mut self) {
        self.awake = 240;
    }

    pub fn is_awake(&self) -> bool {
        self.awake > 0
    }
}

/// Deterministic pseudo-random unit vector.
pub(crate) fn jitter_dir(seed: u64) -> Vec3 {
    let mut x = seed
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(0xD1B5_4A32_D192_ED03);
    let mut next = || {
        x ^= x >> 33;
        x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        x ^= x >> 29;
        ((x >> 11) as f64 / (1u64 << 53) as f64) as f32
    };
    let zc = next() * 2.0 - 1.0;
    let t = next() * std::f32::consts::TAU;
    let r = (1.0 - zc * zc).max(0.0).sqrt();
    Vec3::new(r * t.cos(), zc, r * t.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order_between(lab: &Lab, a: AtomId, b: AtomId) -> u8 {
        lab.neighbors(a)
            .find(|(n, _)| *n == b)
            .map(|(_, bd)| bd.order)
            .unwrap()
    }

    fn build(lab: &mut Lab, target: AtomId, zs: &[u8]) -> Vec<AtomId> {
        zs.iter()
            .map(|&z| lab.attach(target, z, Vec3::X).unwrap())
            .collect()
    }

    fn open(lab: &Lab, any: AtomId) -> u32 {
        lab.component(any).iter().map(|&a| lab.free(a) as u32).sum()
    }

    #[test]
    fn water() {
        let mut lab = Lab::new();
        let o = lab.add_atom(8, Vec3::ZERO).unwrap();
        build(&mut lab, o, &[1, 1]);
        assert_eq!(open(&lab, o), 0);
        assert_eq!(lab.lone_pairs(o), 2);
        // Third H protonates: hydronium.
        lab.attach(o, 1, Vec3::Y).unwrap();
        assert_eq!(lab.charge(o), 1);
        assert_eq!(open(&lab, o), 0);
        assert_eq!(lab.lone_pairs(o), 1);
        // Fourth is rejected.
        assert!(lab.check_attach(o, 1).is_err());
    }

    #[test]
    fn diatomics_get_multiple_bonds() {
        let mut lab = Lab::new();
        let n = lab.add_atom(7, Vec3::ZERO).unwrap();
        let n2 = lab.attach(n, 7, Vec3::X).unwrap();
        assert_eq!(order_between(&lab, n, n2), 3);
        let o = lab.add_atom(8, Vec3::ZERO).unwrap();
        let o2 = lab.attach(o, 8, Vec3::X).unwrap();
        assert_eq!(order_between(&lab, o, o2), 2);
    }

    #[test]
    fn carbon_dioxide_and_methane() {
        let mut lab = Lab::new();
        let c = lab.add_atom(6, Vec3::ZERO).unwrap();
        let os = build(&mut lab, c, &[8, 8]);
        assert_eq!(order_between(&lab, c, os[0]), 2);
        assert_eq!(order_between(&lab, c, os[1]), 2);
        assert_eq!(open(&lab, c), 0);
        // A third O lowers one C=O: the carbonate skeleton with two open O sites.
        lab.attach(c, 8, Vec3::X).unwrap();
        assert_eq!(open(&lab, c), 2);

        let c = lab.add_atom(6, Vec3::ZERO).unwrap();
        build(&mut lab, c, &[1, 1, 1, 1]);
        assert_eq!(open(&lab, c), 0);
        assert!(lab.check_attach(c, 1).is_err());
    }

    #[test]
    fn hydrogenation_of_acetylene() {
        let mut lab = Lab::new();
        let c1 = lab.add_atom(6, Vec3::ZERO).unwrap();
        let c2 = lab.attach(c1, 6, Vec3::X).unwrap();
        lab.attach(c1, 1, Vec3::X).unwrap();
        lab.attach(c2, 1, Vec3::X).unwrap();
        assert_eq!(order_between(&lab, c1, c2), 3);
        assert_eq!(open(&lab, c1), 0);
        // More H lowers the bond order: ethene, then ethane.
        lab.attach(c1, 1, Vec3::X).unwrap();
        lab.attach(c2, 1, Vec3::X).unwrap();
        assert_eq!(order_between(&lab, c1, c2), 2);
        lab.attach(c1, 1, Vec3::X).unwrap();
        lab.attach(c2, 1, Vec3::X).unwrap();
        assert_eq!(order_between(&lab, c1, c2), 1);
        assert_eq!(open(&lab, c1), 0);
    }

    #[test]
    fn sulfur_expands() {
        let mut lab = Lab::new();
        let s = lab.add_atom(16, Vec3::ZERO).unwrap();
        build(&mut lab, s, &[8, 8]);
        assert_eq!(open(&lab, s), 0, "SO2 should be closed shell");
        lab.attach(s, 8, Vec3::X).unwrap();
        assert_eq!(open(&lab, s), 0, "SO3");
        assert_eq!(lab.atom(s).unwrap().valence, 6);
        let f = lab.add_atom(16, Vec3::ZERO).unwrap();
        build(&mut lab, f, &[9, 9, 9, 9, 9, 9]);
        assert_eq!(open(&lab, f), 0, "SF6");
        assert!(lab.check_attach(f, 9).is_err());
    }

    #[test]
    fn removal_shrinks_valence() {
        let mut lab = Lab::new();
        let s = lab.add_atom(16, Vec3::ZERO).unwrap();
        let os = build(&mut lab, s, &[8, 8, 8]);
        lab.remove_atom(os[2]);
        assert_eq!(lab.atom(s).unwrap().valence, 4);
        assert_eq!(open(&lab, s), 0);
    }

    #[test]
    fn ionic_salts() {
        let mut lab = Lab::new();
        let cl = lab.add_atom(17, Vec3::ZERO).unwrap();
        let na = lab.attach(cl, 11, Vec3::X).unwrap();
        assert_eq!(lab.charge(na), 1);
        assert_eq!(lab.charge(cl), -1);
        assert_eq!(open(&lab, na), 0);
        assert!(lab.check_attach(cl, 11).is_err());

        let mg = lab.add_atom(12, Vec3::ZERO).unwrap();
        let o = lab.attach(mg, 8, Vec3::X).unwrap();
        assert_eq!(lab.charge(mg), 2);
        assert_eq!(lab.charge(o), -2);
        assert_eq!(open(&lab, mg), 0);

        let mg = lab.add_atom(12, Vec3::ZERO).unwrap();
        build(&mut lab, mg, &[17, 17]);
        assert_eq!(open(&lab, mg), 0);
        assert_eq!(lab.charge(mg), 2);
    }

    #[test]
    fn sodium_hydroxide() {
        let mut lab = Lab::new();
        let o = lab.add_atom(8, Vec3::ZERO).unwrap();
        lab.attach(o, 1, Vec3::X).unwrap();
        let na = lab.attach(o, 11, Vec3::X).unwrap();
        assert_eq!(lab.charge(na), 1);
        assert_eq!(lab.charge(o), -1);
        assert_eq!(open(&lab, o), 0);
    }

    #[test]
    fn ammonium_and_rejections() {
        let mut lab = Lab::new();
        let n = lab.add_atom(7, Vec3::ZERO).unwrap();
        build(&mut lab, n, &[1, 1, 1, 1]);
        assert_eq!(lab.charge(n), 1);
        assert_eq!(open(&lab, n), 0);
        let ne = lab.add_atom(10, Vec3::ZERO).unwrap();
        assert!(lab.check_attach(ne, 1).is_err());
        let fe = lab.add_atom(26, Vec3::ZERO).unwrap();
        assert!(lab.check_attach(fe, 11).is_err());
        let xe = lab.add_atom(54, Vec3::ZERO).unwrap();
        assert!(lab.check_attach(xe, 1).is_err());
        build(&mut lab, xe, &[9, 9]);
        assert_eq!(open(&lab, xe), 0);
        assert_eq!(lab.lone_pairs(xe), 3);
    }

    #[test]
    fn benzene_ring_closure() {
        let mut lab = Lab::new();
        let c0 = lab.add_atom(6, Vec3::ZERO).unwrap();
        let mut ring = vec![c0];
        for _ in 0..5 {
            let last = *ring.last().unwrap();
            ring.push(lab.attach(last, 6, Vec3::X).unwrap());
        }
        lab.connect(ring[0], ring[5]).unwrap();
        for &c in &ring {
            lab.attach(c, 1, Vec3::Y).unwrap();
        }
        assert_eq!(open(&lab, c0), 0);
        let doubles = lab.bonds().filter(|(_, b)| b.order == 2).count();
        assert_eq!(doubles, 3);
    }

    #[test]
    fn add_molecule_matches_hand_built() {
        use crate::canon::structure_hash;
        use crate::smiles::parse;
        for smi in [
            "CCO",
            "O=C=O",
            "[Na]Cl",
            "OS(=O)(=O)O",
            "C1=CC=CC=C1",
            "[NH4+]",
            "O=C1O[Ca]O1",
            "O=[Fe]O[Fe]=O",
        ] {
            let mut lab = Lab::new();
            let ids = lab.add_molecule(&parse(smi).unwrap(), Vec3::ZERO).unwrap();
            lab.validate().unwrap();
            let comp = lab.component(ids[0]);
            assert_eq!(comp.len(), ids.len(), "{smi} should be one molecule");
            let g = lab.graph(&comp);
            assert!(
                g.is_complete(),
                "{smi} should be complete, got {} open",
                g.total_free()
            );
            // Round trip: same structure as a covalent parse where applicable.
            if !g.has_ionic() {
                assert_eq!(
                    structure_hash(&g),
                    structure_hash(&parse(smi).unwrap()),
                    "{smi}"
                );
            }
        }
        // Impossible structures are rejected and leave the lab untouched.
        let mut lab = Lab::new();
        let rev = lab.revision();
        assert!(
            lab.add_molecule(&parse("[Ne]C").unwrap(), Vec3::ZERO)
                .is_err()
        );
        assert_eq!(lab.revision(), rev);
        assert!(lab.is_empty());
    }

    #[test]
    fn data_round_trip() {
        use crate::canon::structure_hash;
        let mut lab = Lab::new();
        let c = lab.add_atom(6, Vec3::new(1.0, 2.0, 3.0)).unwrap();
        build(&mut lab, c, &[1, 1, 1]);
        let o = lab.attach(c, 8, Vec3::X).unwrap();
        lab.attach(o, 1, Vec3::X).unwrap();
        let cl = lab.add_atom(17, Vec3::new(9.0, 0.0, 0.0)).unwrap();
        lab.attach(cl, 11, Vec3::X).unwrap();
        let n = lab.add_atom(7, Vec3::new(-9.0, 0.0, 0.0)).unwrap();
        build(&mut lab, n, &[1, 1, 1, 1]); // ammonium (protonated)
        let s = lab.add_atom(16, Vec3::new(0.0, 9.0, 0.0)).unwrap();
        build(&mut lab, s, &[8, 8, 8]); // SO3 (expanded valence)
        lab.remove_atom(c); // leave a hole in the id space

        let (data, ids) = lab.to_data();
        assert_eq!(data.atoms.len(), lab.atom_count());
        let (loaded, new_ids) = Lab::from_data(&data).unwrap();
        loaded.validate().unwrap();
        assert_eq!(new_ids.len(), ids.len());
        let hashes = |l: &Lab| {
            let mut v: Vec<u64> = l
                .components()
                .iter()
                .map(|c| structure_hash(&l.graph(c)))
                .collect();
            v.sort_unstable();
            v
        };
        assert_eq!(hashes(&lab), hashes(&loaded), "same molecules after reload");
        for (&old, &new) in ids.iter().zip(&new_ids) {
            assert_eq!(lab.atom(old).unwrap().pos, loaded.atom(new).unwrap().pos);
            assert_eq!(lab.charge(old), loaded.charge(new));
        }
        assert_eq!(
            loaded.to_data().0,
            data,
            "saving the loaded lab gives identical data"
        );
    }

    #[test]
    fn bad_data_is_rejected() {
        let ok = AtomData {
            z: 6,
            pos: [0.0; 3],
            valence: 4,
            protonated: false,
        };
        let cases = [
            LabData {
                atoms: vec![AtomData { z: 0, ..ok }],
                bonds: vec![],
            },
            LabData {
                atoms: vec![AtomData { z: 200, ..ok }],
                bonds: vec![],
            },
            LabData {
                atoms: vec![AtomData {
                    pos: [f32::NAN, 0.0, 0.0],
                    ..ok
                }],
                bonds: vec![],
            },
            LabData {
                atoms: vec![AtomData { valence: 3, ..ok }],
                bonds: vec![],
            },
            LabData {
                atoms: vec![AtomData {
                    protonated: true,
                    ..ok
                }],
                bonds: vec![],
            },
            LabData {
                atoms: vec![ok],
                bonds: vec![BondData { a: 0, b: 0 }],
            },
            LabData {
                atoms: vec![ok, ok],
                bonds: vec![BondData { a: 0, b: 5 }],
            },
            LabData {
                atoms: vec![ok, ok],
                bonds: vec![BondData { a: 0, b: 1 }, BondData { a: 1, b: 0 }],
            },
            LabData {
                atoms: vec![
                    ok,
                    AtomData {
                        z: 10,
                        valence: 0,
                        ..ok
                    },
                ],
                bonds: vec![BondData { a: 0, b: 1 }],
            },
            LabData {
                atoms: vec![
                    AtomData {
                        z: 1,
                        valence: 1,
                        ..ok
                    };
                    3
                ],
                bonds: vec![BondData { a: 0, b: 1 }, BondData { a: 0, b: 2 }],
            },
        ];
        for (i, d) in cases.iter().enumerate() {
            assert!(Lab::from_data(d).is_err(), "case {i} should be rejected");
        }
        assert!(Lab::from_data(&LabData::default()).unwrap().0.is_empty());
    }

    #[test]
    fn merging_molecules() {
        let mut lab = Lab::new();
        let c1 = lab.add_atom(6, Vec3::ZERO).unwrap();
        build(&mut lab, c1, &[1, 1, 1]);
        let c2 = lab.add_atom(6, Vec3::new(5.0, 0.0, 0.0)).unwrap();
        build(&mut lab, c2, &[1, 1, 1]);
        lab.connect(c1, c2).unwrap();
        assert_eq!(open(&lab, c1), 0);
        assert_eq!(lab.component(c1).len(), 8);
    }
}
