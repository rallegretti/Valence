//! Structure fingerprint (Weisfeiler–Lehman refinement) used to recognise
//! known compounds regardless of how the player built them. Isomers such as
//! ethanol vs dimethyl ether get different hashes. Six-membered rings with
//! alternating double bonds are perceived as aromatic so every Kekulé form of
//! a benzene ring hashes the same.

use crate::graph::{BondKind, MolGraph};

#[inline]
fn mix(h: u64, v: u64) -> u64 {
    let mut x = h ^ v.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

pub fn structure_hash(g: &MolGraph) -> u64 {
    let n = g.len();
    let arom = aromatic_edges(g);
    let edge_code = |e: usize| -> u64 {
        if arom[e] {
            15
        } else {
            g.edges[e].order as u64
                + if g.edges[e].kind == BondKind::Ionic {
                    8
                } else {
                    0
                }
        }
    };
    let mut label: Vec<u64> = (0..n)
        .map(|i| {
            mix(
                mix(mix(1, g.z[i] as u64), (g.charge[i] as i64 + 64) as u64),
                g.degree(i) as u64,
            )
        })
        .collect();
    let mut distinct = 0;
    let mut scratch: Vec<u64> = Vec::new();
    for _ in 0..n.max(1) {
        let next: Vec<u64> = (0..n)
            .map(|i| {
                scratch.clear();
                scratch.extend(
                    g.neighbors(i)
                        .iter()
                        .map(|&(j, e)| mix(edge_code(e), label[j])),
                );
                scratch.sort_unstable();
                scratch
                    .iter()
                    .fold(mix(label[i], 0xA5A5), |h, &x| mix(h, x))
            })
            .collect();
        label = next;
        let mut sorted = label.clone();
        sorted.sort_unstable();
        sorted.dedup();
        if sorted.len() == distinct {
            break;
        }
        distinct = sorted.len();
    }
    label.sort_unstable();
    label.iter().fold(mix(0x5EED, n as u64), |h, &x| mix(h, x))
}

/// All simple 6-cycles, each as a list of atom indices in ring order.
pub fn six_rings(g: &MolGraph) -> Vec<Vec<usize>> {
    let mut rings: Vec<Vec<usize>> = Vec::new();
    let mut seen: Vec<Vec<usize>> = Vec::new();
    for s in 0..g.len() {
        let mut path = vec![s];
        dfs_ring(g, s, &mut path, &mut rings, &mut seen);
    }
    rings
}

fn dfs_ring(
    g: &MolGraph,
    s: usize,
    path: &mut Vec<usize>,
    rings: &mut Vec<Vec<usize>>,
    seen: &mut Vec<Vec<usize>>,
) {
    let u = *path.last().unwrap();
    for &(v, _) in g.neighbors(u) {
        if v == s && path.len() == 6 {
            let mut key = path.clone();
            key.sort_unstable();
            if !seen.contains(&key) {
                seen.push(key);
                rings.push(path.clone());
            }
        } else if v > s && path.len() < 6 && !path.contains(&v) {
            path.push(v);
            dfs_ring(g, s, path, rings, seen);
            path.pop();
        }
    }
}

pub fn edge_between(g: &MolGraph, a: usize, b: usize) -> Option<usize> {
    g.neighbors(a)
        .iter()
        .find(|&&(n, _)| n == b)
        .map(|&(_, e)| e)
}

/// Edges in aromatic six-membered rings.
pub fn aromatic_edges(g: &MolGraph) -> Vec<bool> {
    let mut arom = vec![false; g.edges.len()];
    let rings = six_rings(g);
    if rings.is_empty() {
        return arom;
    }
    let mut in_ring = vec![false; g.len()];
    for r in &rings {
        for &a in r {
            in_ring[a] = true;
        }
    }
    for r in &rings {
        let ok = r.iter().all(|&a| {
            if !matches!(g.z[a], 6 | 7) {
                return false;
            }
            let doubles: Vec<usize> = g
                .neighbors(a)
                .iter()
                .filter(|&&(_, e)| g.edges[e].order == 2)
                .map(|&(n, _)| n)
                .collect();
            doubles.len() == 1 && in_ring[doubles[0]]
        });
        if ok {
            for k in 0..6 {
                if let Some(e) = edge_between(g, r[k], r[(k + 1) % 6]) {
                    arom[e] = true;
                }
            }
        }
    }
    arom
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smiles::parse;

    fn h(s: &str) -> u64 {
        structure_hash(&parse(s).unwrap())
    }

    #[test]
    fn isomers_differ_and_orderings_match() {
        assert_ne!(h("CCO"), h("COC"));
        assert_eq!(h("CCO"), h("OCC"));
        assert_eq!(h("CC(C)O"), h("OC(C)C"));
        assert_ne!(h("CCCO"), h("CC(C)O"));
    }

    #[test]
    fn kekule_forms_match() {
        assert_eq!(h("CC1=CC=CC=C1C"), h("CC1=C(C)C=CC=C1"));
        assert_eq!(h("C1=CC2=CC=CC=C2C=C1"), h("C1=CC=C2C=CC=CC2=C1"));
    }
}
