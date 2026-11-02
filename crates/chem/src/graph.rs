//! A lightweight, index-based molecular graph used for naming and hashing.
//! Both the live `Lab` and the SMILES parser produce this type, so naming
//! never depends on the game engine.

use crate::elements::{element, formula_rank};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BondKind {
    Covalent,
    /// Electron transfer from a metal (cation) to a nonmetal (anion).
    Ionic,
}

#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub a: usize,
    pub b: usize,
    pub order: u8,
    pub kind: BondKind,
}

#[derive(Clone, Debug, Default)]
pub struct MolGraph {
    pub z: Vec<u8>,
    pub charge: Vec<i8>,
    /// Unfilled bonding sites (unpaired electrons available for bonding).
    pub free: Vec<u8>,
    pub edges: Vec<Edge>,
    adj: Vec<Vec<(usize, usize)>>,
}

impl MolGraph {
    pub fn add_atom(&mut self, z: u8, charge: i8, free: u8) -> usize {
        self.z.push(z);
        self.charge.push(charge);
        self.free.push(free);
        self.adj.push(Vec::new());
        self.z.len() - 1
    }

    pub fn add_edge(&mut self, a: usize, b: usize, order: u8, kind: BondKind) -> usize {
        let e = self.edges.len();
        self.edges.push(Edge { a, b, order, kind });
        self.adj[a].push((b, e));
        self.adj[b].push((a, e));
        e
    }

    pub fn len(&self) -> usize {
        self.z.len()
    }

    pub fn is_empty(&self) -> bool {
        self.z.is_empty()
    }

    /// (neighbour index, edge index) pairs.
    pub fn neighbors(&self, i: usize) -> &[(usize, usize)] {
        &self.adj[i]
    }

    pub fn degree(&self, i: usize) -> usize {
        self.adj[i].len()
    }

    pub fn total_charge(&self) -> i32 {
        self.charge.iter().map(|&c| c as i32).sum()
    }

    pub fn total_free(&self) -> u32 {
        self.free.iter().map(|&f| f as u32).sum()
    }

    pub fn is_complete(&self) -> bool {
        self.total_free() == 0
    }

    pub fn has_ionic(&self) -> bool {
        self.edges.iter().any(|e| e.kind == BondKind::Ionic)
    }

    pub fn count(&self, z: u8) -> usize {
        self.z.iter().filter(|&&x| x == z).count()
    }

    /// Element counts sorted by atomic number.
    pub fn element_counts(&self) -> Vec<(u8, usize)> {
        let mut counts = [0usize; 119];
        for &z in &self.z {
            counts[z as usize] += 1;
        }
        (1..=118u8)
            .filter(|&z| counts[z as usize] > 0)
            .map(|z| (z, counts[z as usize]))
            .collect()
    }

    pub fn mass(&self) -> f32 {
        self.z.iter().map(|&z| element(z).mass).sum()
    }

    /// Element counts in display order: Hill system for carbon compounds,
    /// otherwise electropositive element first (NaCl, NH3, H2O, SF6).
    pub fn formula_parts(&self) -> Vec<(u8, usize)> {
        let mut parts = self.element_counts();
        let has_c = parts.iter().any(|&(z, _)| z == 6);
        if has_c && self.len() > 1 {
            parts.sort_by_key(|&(z, _)| match z {
                6 => (0, String::new()),
                1 => (1, String::new()),
                _ => (2, element(z).symbol.to_string()),
            });
        } else {
            parts.sort_by_key(|&(z, _)| formula_rank(z));
        }
        parts
    }

    /// Unicode formula such as "H₂O" or "NH₄⁺".
    pub fn formula(&self) -> String {
        let mut s = String::new();
        for (z, n) in self.formula_parts() {
            s.push_str(element(z).symbol);
            if n > 1 {
                s.push_str(&subscript(n));
            }
        }
        s.push_str(&charge_superscript(self.total_charge()));
        s
    }

    /// Plain ASCII formula key, e.g. "CO3-2", used for table lookups.
    pub fn formula_key(&self) -> String {
        let mut s = String::new();
        for (z, n) in self.formula_parts() {
            s.push_str(element(z).symbol);
            if n > 1 {
                s.push_str(&n.to_string());
            }
        }
        let q = self.total_charge();
        if q != 0 {
            s.push_str(&format!("{:+}", q));
        }
        s
    }

    /// Split into connected components after deleting the given edges.
    pub fn fragments_without(&self, skip: impl Fn(&Edge) -> bool) -> Vec<Vec<usize>> {
        let n = self.len();
        let mut seen = vec![false; n];
        let mut out = Vec::new();
        for s in 0..n {
            if seen[s] {
                continue;
            }
            let mut comp = vec![s];
            seen[s] = true;
            let mut i = 0;
            while i < comp.len() {
                let u = comp[i];
                i += 1;
                for &(v, e) in &self.adj[u] {
                    if !seen[v] && !skip(&self.edges[e]) {
                        seen[v] = true;
                        comp.push(v);
                    }
                }
            }
            comp.sort_unstable();
            out.push(comp);
        }
        out
    }

    /// Induced subgraph over `atoms` (indices into self).
    pub fn subgraph(&self, atoms: &[usize]) -> MolGraph {
        let mut map = vec![usize::MAX; self.len()];
        let mut g = MolGraph::default();
        for &a in atoms {
            map[a] = g.add_atom(self.z[a], self.charge[a], self.free[a]);
        }
        for e in &self.edges {
            if map[e.a] != usize::MAX && map[e.b] != usize::MAX {
                g.add_edge(map[e.a], map[e.b], e.order, e.kind);
            }
        }
        g
    }
}

const SUB: [char; 10] = ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈', '₉'];
const SUP: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];

pub fn subscript(n: usize) -> String {
    n.to_string()
        .chars()
        .map(|c| c.to_digit(10).map_or(c, |d| SUB[d as usize % 10]))
        .collect()
}

pub fn superscript_digit(d: u32) -> char {
    SUP[d as usize % 10]
}

/// "", "⁺", "²⁻", ...
pub fn charge_superscript(q: i32) -> String {
    if q == 0 {
        return String::new();
    }
    let mut s = String::new();
    if q.abs() > 1 {
        for c in q.abs().to_string().chars() {
            s.push(c.to_digit(10).map_or(c, superscript_digit));
        }
    }
    s.push(if q > 0 { '⁺' } else { '⁻' });
    s
}
