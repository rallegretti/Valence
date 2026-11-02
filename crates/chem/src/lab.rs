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

use glam::Vec3;

use crate::elements::{self, Category, category, element, valence_electrons};
use crate::graph::{BondKind, MolGraph};

pub type AtomId = u32;
pub type BondId = u32;

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
            return Err(Reject(if category(z) == Category::NobleGas {
                format!(
                    "{} is a noble gas: its electron shell is already full, so it doesn't bond.",
                    e.name
                )
            } else {
                format!(
                    "{} is a synthetic superheavy element. It decays within seconds, far too fast to form compounds.",
                    e.name
                )
            }));
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
            return Err(Reject(format!(
                "{} only forms compounds with {partners}.",
                element(n).name
            )));
        }
    }
    let (ma, mb) = (category(za).is_metal(), category(zb).is_metal());
    match (ma, mb) {
        (true, true) => Err(Reject(
            "Two metals don't form a molecule: they'd share a sea of electrons as a metallic alloy instead.".into(),
        )),
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
            1 => base,
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

    pub fn clear(&mut self) {
        *self = Lab {
            revision: self.revision + 1,
            ..Lab::default()
        };
    }

    pub fn set_pos(&mut self, id: AtomId, pos: Vec3) {
        if let Some(a) = self.atom_mut(id) {
            a.pos = pos;
        }
        self.wake();
    }

    /// Move a set of atoms rigidly (dragging a whole molecule).
    pub fn translate(&mut self, ids: &[AtomId], delta: Vec3) {
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
        self.atom(id).map_or(0, |a| a.bonds.len() as u8)
    }

    /// Bonding electrons in use (sum of bond orders).
    pub fn used(&self, id: AtomId) -> u8 {
        self.neighbors(id).map(|(_, b)| b.order).sum()
    }

    pub fn capacity(&self, id: AtomId) -> u8 {
        self.atom(id).map_or(0, |a| a.valence + a.protonated as u8)
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
                q += if b.a == id {
                    b.order as i8
                } else {
                    -(b.order as i8)
                };
            }
        }
        q
    }

    pub fn covalent_used(&self, id: AtomId) -> u8 {
        self.neighbors(id)
            .filter(|(_, b)| b.kind == BondKind::Covalent)
            .map(|(_, b)| b.order)
            .sum()
    }

    pub fn lone_pairs(&self, id: AtomId) -> u8 {
        let Some(a) = self.atom(id) else { return 0 };
        let v = valence_electrons(a.z) as i32;
        if v == 0 {
            return 0;
        }
        let nonbonding = v - self.covalent_used(id) as i32 - self.charge(id) as i32;
        ((nonbonding - self.free(id) as i32) / 2).max(0) as u8
    }

    pub fn has_ionic(&self, id: AtomId) -> bool {
        self.neighbors(id).any(|(_, b)| b.kind == BondKind::Ionic)
    }

    // -----------------------------------------------------------------------
    // Components & graphs
    // -----------------------------------------------------------------------

    /// Atoms connected to `start`, sorted ascending.
    pub fn component(&self, start: AtomId) -> Vec<AtomId> {
        let mut out = vec![start];
        let mut seen = vec![false; self.atoms.len()];
        seen[start as usize] = true;
        let mut i = 0;
        while i < out.len() {
            let u = out[i];
            i += 1;
            for (v, _) in self.neighbors(u) {
                if !seen[v as usize] {
                    seen[v as usize] = true;
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
            if !seen[id as usize] {
                let c = self.component(id);
                for &a in &c {
                    seen[a as usize] = true;
                }
                out.push(c);
            }
        }
        out
    }

    /// Naming graph for a component; node i corresponds to `atoms[i]`.
    pub fn graph(&self, atoms: &[AtomId]) -> MolGraph {
        let mut g = MolGraph::default();
        let mut map = std::collections::HashMap::with_capacity(atoms.len());
        for &a in atoms {
            let at = self.atom(a).expect("live atom");
            map.insert(a, g.add_atom(at.z, self.charge(a), self.free(a)));
        }
        for (_, b) in self.bonds() {
            if let (Some(&ia), Some(&ib)) = (map.get(&b.a), map.get(&b.b)) {
                g.add_edge(ia, ib, b.order, b.kind);
            }
        }
        g
    }

    // -----------------------------------------------------------------------
    // Mutations
    // -----------------------------------------------------------------------

    fn alloc_atom(&mut self, atom: Atom) -> AtomId {
        if let Some(id) = self.free_atom_slots.pop() {
            self.atoms[id as usize] = Some(atom);
            id
        } else {
            self.atoms.push(Some(atom));
            (self.atoms.len() - 1) as AtomId
        }
    }

    fn alloc_bond(&mut self, a: AtomId, b: AtomId, kind: BondKind) -> BondId {
        // Ionic bonds store the cation first.
        let (a, b) = if kind == BondKind::Ionic
            && !category(self.atoms[a as usize].as_ref().unwrap().z).is_metal()
        {
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
        let id = if let Some(id) = self.free_bond_slots.pop() {
            self.bonds[id as usize] = Some(bond);
            id
        } else {
            self.bonds.push(Some(bond));
            (self.bonds.len() - 1) as BondId
        };
        self.atom_mut(a).unwrap().bonds.push(id);
        self.atom_mut(b).unwrap().bonds.push(id);
        id
    }

    /// Drop a lone atom into the play space. Always succeeds (noble gases are
    /// happy on their own).
    pub fn add_atom(&mut self, z: u8, pos: Vec3) -> AtomId {
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
        id
    }

    pub fn remove_atom(&mut self, id: AtomId) {
        let Some(atom) = self.atom(id).cloned() else {
            return;
        };
        let mut touched = Vec::new();
        for &bid in &atom.bonds {
            let bond = self.bonds[bid as usize].take().unwrap();
            self.free_bond_slots.push(bid);
            let other = bond.other(id);
            touched.push(other);
            // Shrink the neighbour back to the smallest valence that still
            // covers what it keeps (SO₃ minus an O → SO₂, NH₄⁺ minus H → NH₃).
            // `used` still counts the bond being removed (it's looked up by
            // id and we've only taken it out of the bond table), so subtract.
            let remaining = self
                .atom(other)
                .unwrap()
                .bonds
                .iter()
                .filter(|&&b| b != bid)
                .filter_map(|&b| self.bond(b))
                .map(|b| b.order)
                .sum::<u8>();
            let o = self.atom_mut(other).unwrap();
            o.bonds.retain(|&b| b != bid);
            if o.protonated && remaining <= o.valence {
                o.protonated = false;
            }
            let p = o.protonated as u8;
            let sigma = o.bonds.len() as u8;
            let need = remaining.saturating_sub(p).max(sigma.saturating_sub(p));
            if let Some(v) = smallest_valence_at_least(o.z, need) {
                o.valence = o.valence.min(v);
            }
        }
        self.atoms[id as usize] = None;
        self.free_atom_slots.push(id);
        self.after_change(&touched);
    }

    /// Can element `z` be attached to atom `target`? Cheap enough to call on hover.
    pub fn check_attach(&self, target: AtomId, z: u8) -> Result<AttachPlan, Reject> {
        let t = self
            .atom(target)
            .ok_or_else(|| Reject("That atom no longer exists.".into()))?;
        let kind = pair_kind(t.z, z)?;
        let tname = element(t.z).name;
        smallest_valence_at_least(z, 1)
            .ok_or_else(|| Reject(format!("{} can't form a bond here.", element(z).name)))?;

        if kind == BondKind::Ionic && !category(t.z).is_metal() {
            // Target is the anion side: it can only take electrons into its
            // base valence (Cl⁻, O²⁻), never by expanding.
            let base = smallest_valence_at_least(t.z, 1).unwrap_or(0);
            if t.protonated || self.sigma(target) + 1 > base {
                return Err(Reject(format!(
                    "{tname} can't accept another electron here: it already has a full octet."
                )));
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
        if let Some(&next) = element(t.z).valences.iter().find(|&&v| v > t.valence)
            && self.sigma(target) < next + t.protonated as u8
            && !(kind == BondKind::Ionic && !category(t.z).is_metal())
        {
            candidates.push(AttachMode::Expand(next));
        }
        if candidates.is_empty() {
            let max = element(t.z).valences.last().copied().unwrap_or(0) + t.protonated as u8;
            return Err(Reject(if category(t.z).is_metal() {
                format!("{tname} has no more electrons to give (max {max}).")
            } else {
                format!(
                    "{tname} already has {} bond{}: its maximum.",
                    self.sigma(target),
                    if max == 1 { "" } else { "s" }
                )
            }));
        }

        // Pick the option that leaves the fewest open sites; ties favour the
        // earlier (simpler) option.
        let mode = if candidates.len() == 1 {
            candidates[0]
        } else {
            let mut best = (u32::MAX, candidates[0]);
            for &m in &candidates {
                let mut trial = self.clone();
                let id = trial.apply_attach(target, z, m, kind, Vec3::ZERO);
                let open: u32 = trial
                    .component(id)
                    .iter()
                    .map(|&a| trial.free(a) as u32)
                    .sum();
                if open < best.0 {
                    best = (open, m);
                }
            }
            best.1
        };

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
        Ok(self.apply_attach(target, z, plan.mode, plan.kind, hint))
    }

    fn apply_attach(
        &mut self,
        target: AtomId,
        z: u8,
        mode: AttachMode,
        kind: BondKind,
        hint: Vec3,
    ) -> AtomId {
        let t = self.atom(target).unwrap().clone();
        match mode {
            AttachMode::Direct => {}
            AttachMode::Expand(v) => self.atom_mut(target).unwrap().valence = v,
            AttachMode::Protonate => self.atom_mut(target).unwrap().protonated = true,
        }
        let (dir, used_domain) = self.placement_dir(target, hint);
        if let Some(i) = used_domain {
            self.atom_mut(target).unwrap().domains.remove(i);
        }
        let len = bond_length(t.z, z, 1, kind);
        let valence = smallest_valence_at_least(z, 1).unwrap();
        let id = self.alloc_atom(Atom {
            z,
            pos: t.pos + dir * len,
            valence,
            protonated: false,
            bonds: Vec::new(),
            domains: Vec::new(),
            vel: Vec3::ZERO,
        });
        self.alloc_bond(target, id, kind);
        self.after_change(&[target, id]);
        id
    }

    /// Can two existing atoms be bonded (joining molecules or closing a ring)?
    pub fn check_connect(
        &self,
        a: AtomId,
        b: AtomId,
    ) -> Result<(BondKind, Option<u8>, Option<u8>), Reject> {
        if a == b {
            return Err(Reject("An atom can't bond to itself.".into()));
        }
        let (aa, ab) = match (self.atom(a), self.atom(b)) {
            (Some(x), Some(y)) => (x, y),
            _ => return Err(Reject("That atom no longer exists.".into())),
        };
        if self.bonded(a, b) {
            return Err(Reject("These atoms are already bonded.".into()));
        }
        let kind = pair_kind(aa.z, ab.z)?;
        let options = |id: AtomId, at: &Atom, anion: bool| -> Vec<Option<u8>> {
            let mut v = Vec::new();
            if self.sigma(id) < self.capacity(id) {
                v.push(None);
            }
            if !anion && let Some(&next) = element(at.z).valences.iter().find(|&&x| x > at.valence)
            {
                v.push(Some(next));
            }
            v
        };
        let anion = |at: &Atom| kind == BondKind::Ionic && !category(at.z).is_metal();
        for (id, at) in [(a, aa), (b, ab)] {
            if anion(at)
                && (at.protonated
                    || self.sigma(id) + 1 > smallest_valence_at_least(at.z, 1).unwrap_or(0))
            {
                return Err(Reject(format!(
                    "{} can't accept another electron: its octet is full.",
                    element(at.z).name
                )));
            }
        }
        let (oa, ob) = (options(a, aa, anion(aa)), options(b, ab, anion(ab)));
        if oa.is_empty() || ob.is_empty() {
            let full = if oa.is_empty() { aa } else { ab };
            return Err(Reject(format!(
                "{} has no open bonding sites left.",
                element(full.z).name
            )));
        }
        let mut best: Option<(u32, Option<u8>, Option<u8>)> = None;
        for &xa in &oa {
            for &xb in &ob {
                let mut trial = self.clone();
                trial.apply_connect(a, b, kind, xa, xb);
                let open: u32 = trial
                    .component(a)
                    .iter()
                    .map(|&i| trial.free(i) as u32)
                    .sum();
                if best.is_none_or(|(o, _, _)| open < o) {
                    best = Some((open, xa, xb));
                }
            }
        }
        let (_, xa, xb) = best.unwrap();
        Ok((kind, xa, xb))
    }

    pub fn connect(&mut self, a: AtomId, b: AtomId) -> Result<(), Reject> {
        let (kind, xa, xb) = self.check_connect(a, b)?;
        self.apply_connect(a, b, kind, xa, xb);
        Ok(())
    }

    fn apply_connect(
        &mut self,
        a: AtomId,
        b: AtomId,
        kind: BondKind,
        xa: Option<u8>,
        xb: Option<u8>,
    ) {
        if let Some(v) = xa {
            self.atom_mut(a).unwrap().valence = v;
        }
        if let Some(v) = xb {
            self.atom_mut(b).unwrap().valence = v;
        }
        self.alloc_bond(a, b, kind);
        self.after_change(&[a, b]);
    }

    /// Direction for a new neighbour: the open site closest to `hint`, else
    /// the emptiest direction around the atom.
    /// Returns the direction and, if one was used, the index of the consumed domain.
    fn placement_dir(&self, id: AtomId, hint: Vec3) -> (Vec3, Option<usize>) {
        let a = self.atom(id).unwrap();
        let free = self.free(id) as usize;
        let hint = hint.normalize_or_zero();
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
        if let Some((_, i)) = best {
            return (a.domains[i], Some(i));
        }
        let sum: Vec3 = self
            .neighbors(id)
            .filter_map(|(n, _)| self.atom(n))
            .map(|n| (n.pos - a.pos).normalize_or_zero())
            .sum();
        let away = -sum;
        let d = if away.length_squared() > 1e-4 {
            away.normalize()
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
    }

    // -----------------------------------------------------------------------
    // Bond-order solver
    // -----------------------------------------------------------------------

    /// Reset orders to 1, then raise them to fill as many open sites as
    /// possible: greedy round-robin, then augmenting alternating paths.
    pub(crate) fn solve(&mut self, comp: &[AtomId]) {
        let mut bond_ids: Vec<BondId> = Vec::new();
        for &a in comp {
            for &b in &self.atom(a).unwrap().bonds {
                if !bond_ids.contains(&b) {
                    bond_ids.push(b);
                }
            }
        }
        bond_ids.sort_unstable();
        for &b in &bond_ids {
            self.bonds[b as usize].as_mut().unwrap().order = 1;
        }
        // Safety: capacity must cover σ bonds.
        for &a in comp {
            let sigma = self.sigma(a);
            if self.capacity(a) < sigma {
                let at = self.atom(a).unwrap();
                let need = sigma - at.protonated as u8;
                if let Some(v) = smallest_valence_at_least(at.z, need) {
                    self.atom_mut(a).unwrap().valence = v;
                }
            }
        }
        // Greedy, one increment per bond per pass so multiple bonds spread evenly.
        loop {
            let mut changed = false;
            for &b in &bond_ids {
                let bd = self.bonds[b as usize].as_ref().unwrap();
                let (x, y, o) = (bd.a, bd.b, bd.order);
                if o < 3 && self.free(x) > 0 && self.free(y) > 0 {
                    self.bonds[b as usize].as_mut().unwrap().order += 1;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        // Augmenting paths: free(s) -inc- x -dec- y -inc- ... -inc- free(t).
        while self.augment(comp) {}
    }

    fn augment(&mut self, comp: &[AtomId]) -> bool {
        for &s in comp {
            if self.free(s) == 0 {
                continue;
            }
            let mut path: Vec<BondId> = Vec::new();
            if self.dfs_augment(s, s, true, &mut path, comp.len() * 2 + 2) {
                for (i, &b) in path.iter().enumerate() {
                    let bd = self.bonds[b as usize].as_mut().unwrap();
                    if i % 2 == 0 {
                        bd.order += 1;
                    } else {
                        bd.order -= 1;
                    }
                }
                return true;
            }
        }
        false
    }

    fn dfs_augment(
        &self,
        start: AtomId,
        u: AtomId,
        inc: bool,
        path: &mut Vec<BondId>,
        limit: usize,
    ) -> bool {
        if path.len() >= limit {
            return false;
        }
        for &b in &self.atom(u).unwrap().bonds {
            if path.contains(&b) {
                continue;
            }
            let bd = self.bond(b).unwrap();
            let v = bd.other(u);
            if inc {
                if bd.order >= 3 {
                    continue;
                }
                path.push(b);
                let need = if v == start { 2 } else { 1 };
                if self.free(v) >= need {
                    return true;
                }
                if self.dfs_augment(start, v, false, path, limit) {
                    return true;
                }
                path.pop();
            } else {
                if bd.order < 2 {
                    continue;
                }
                path.push(b);
                if self.dfs_augment(start, v, true, path, limit) {
                    return true;
                }
                path.pop();
            }
        }
        false
    }

    /// Keep one direction per open site / lone pair, preserving existing ones.
    fn sync_domains(&mut self, id: AtomId) {
        let want = (self.free(id) + self.lone_pairs(id)) as usize;
        let a = self.atom(id).unwrap();
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
            let d = (-crowd + j * 0.6).normalize_or(j);
            domains.push(d);
        }
        self.atom_mut(id).unwrap().domains = domains;
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
    let r = (1.0 - zc * zc).sqrt();
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
        let o = lab.add_atom(8, Vec3::ZERO);
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
        let n = lab.add_atom(7, Vec3::ZERO);
        let n2 = lab.attach(n, 7, Vec3::X).unwrap();
        assert_eq!(order_between(&lab, n, n2), 3);
        let o = lab.add_atom(8, Vec3::ZERO);
        let o2 = lab.attach(o, 8, Vec3::X).unwrap();
        assert_eq!(order_between(&lab, o, o2), 2);
    }

    #[test]
    fn carbon_dioxide_and_methane() {
        let mut lab = Lab::new();
        let c = lab.add_atom(6, Vec3::ZERO);
        let os = build(&mut lab, c, &[8, 8]);
        assert_eq!(order_between(&lab, c, os[0]), 2);
        assert_eq!(order_between(&lab, c, os[1]), 2);
        assert_eq!(open(&lab, c), 0);
        // A third O lowers one C=O: the carbonate skeleton with two open O sites.
        lab.attach(c, 8, Vec3::X).unwrap();
        assert_eq!(open(&lab, c), 2);

        let c = lab.add_atom(6, Vec3::ZERO);
        build(&mut lab, c, &[1, 1, 1, 1]);
        assert_eq!(open(&lab, c), 0);
        assert!(lab.check_attach(c, 1).is_err());
    }

    #[test]
    fn hydrogenation_of_acetylene() {
        let mut lab = Lab::new();
        let c1 = lab.add_atom(6, Vec3::ZERO);
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
        let s = lab.add_atom(16, Vec3::ZERO);
        build(&mut lab, s, &[8, 8]);
        assert_eq!(open(&lab, s), 0, "SO2 should be closed shell");
        lab.attach(s, 8, Vec3::X).unwrap();
        assert_eq!(open(&lab, s), 0, "SO3");
        assert_eq!(lab.atom(s).unwrap().valence, 6);
        let f = lab.add_atom(16, Vec3::ZERO);
        build(&mut lab, f, &[9, 9, 9, 9, 9, 9]);
        assert_eq!(open(&lab, f), 0, "SF6");
        assert!(lab.check_attach(f, 9).is_err());
    }

    #[test]
    fn removal_shrinks_valence() {
        let mut lab = Lab::new();
        let s = lab.add_atom(16, Vec3::ZERO);
        let os = build(&mut lab, s, &[8, 8, 8]);
        lab.remove_atom(os[2]);
        assert_eq!(lab.atom(s).unwrap().valence, 4);
        assert_eq!(open(&lab, s), 0);
    }

    #[test]
    fn ionic_salts() {
        let mut lab = Lab::new();
        let cl = lab.add_atom(17, Vec3::ZERO);
        let na = lab.attach(cl, 11, Vec3::X).unwrap();
        assert_eq!(lab.charge(na), 1);
        assert_eq!(lab.charge(cl), -1);
        assert_eq!(open(&lab, na), 0);
        assert!(lab.check_attach(cl, 11).is_err());

        let mg = lab.add_atom(12, Vec3::ZERO);
        let o = lab.attach(mg, 8, Vec3::X).unwrap();
        assert_eq!(lab.charge(mg), 2);
        assert_eq!(lab.charge(o), -2);
        assert_eq!(open(&lab, mg), 0);

        let mg = lab.add_atom(12, Vec3::ZERO);
        build(&mut lab, mg, &[17, 17]);
        assert_eq!(open(&lab, mg), 0);
        assert_eq!(lab.charge(mg), 2);
    }

    #[test]
    fn sodium_hydroxide() {
        let mut lab = Lab::new();
        let o = lab.add_atom(8, Vec3::ZERO);
        lab.attach(o, 1, Vec3::X).unwrap();
        let na = lab.attach(o, 11, Vec3::X).unwrap();
        assert_eq!(lab.charge(na), 1);
        assert_eq!(lab.charge(o), -1);
        assert_eq!(open(&lab, o), 0);
    }

    #[test]
    fn ammonium_and_rejections() {
        let mut lab = Lab::new();
        let n = lab.add_atom(7, Vec3::ZERO);
        build(&mut lab, n, &[1, 1, 1, 1]);
        assert_eq!(lab.charge(n), 1);
        assert_eq!(open(&lab, n), 0);
        let ne = lab.add_atom(10, Vec3::ZERO);
        assert!(lab.check_attach(ne, 1).is_err());
        let fe = lab.add_atom(26, Vec3::ZERO);
        assert!(lab.check_attach(fe, 11).is_err());
        let xe = lab.add_atom(54, Vec3::ZERO);
        assert!(lab.check_attach(xe, 1).is_err());
        build(&mut lab, xe, &[9, 9]);
        assert_eq!(open(&lab, xe), 0);
        assert_eq!(lab.lone_pairs(xe), 3);
    }

    #[test]
    fn benzene_ring_closure() {
        let mut lab = Lab::new();
        let c0 = lab.add_atom(6, Vec3::ZERO);
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
    fn merging_molecules() {
        let mut lab = Lab::new();
        let c1 = lab.add_atom(6, Vec3::ZERO);
        build(&mut lab, c1, &[1, 1, 1]);
        let c2 = lab.add_atom(6, Vec3::new(5.0, 0.0, 0.0));
        build(&mut lab, c2, &[1, 1, 1]);
        lab.connect(c1, c2).unwrap();
        assert_eq!(open(&lab, c1), 0);
        assert_eq!(lab.component(c1).len(), 8);
    }
}
