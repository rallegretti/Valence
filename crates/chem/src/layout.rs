//! Cheap "molecular mechanics lite" layout.
//!
//! - Bonds are springs toward an ideal length.
//! - Electron domains around each atom (bonds, lone pairs, open sites) repel
//!   each other on the unit sphere — this is VSEPR, and it produces linear,
//!   trigonal, tetrahedral, trigonal-bipyramidal and octahedral shapes, with
//!   lone pairs bending water and ammonia.
//! - Non-bonded atoms push apart softly so molecules don't overlap.
//!
//! Integration is damped dynamics with a clamped step ("heavy ball"
//! descent): fast on soft modes like ring puckering, stable on stiff bonds.
//! The lab goes to sleep once nothing moves.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use glam::Vec3;

use crate::graph::BondKind;
use crate::lab::{AtomId, Lab, bond_length};

const K_BOND: f32 = 6.0;
const K_ANGLE: f32 = 1.2;
const K_REPEL: f32 = 1.5;
const REPEL_RANGE: f32 = 3.2;
const STEP: f32 = 0.035;
const FRICTION: f32 = 0.92;
const MAX_MOVE: f32 = 0.25;
const W_LONE_PAIR: f32 = 1.35;
const W_OPEN_SITE: f32 = 0.85;

/// Tiny FxHash-style hasher for small integer keys (std's SipHash is
/// needlessly slow here, and we don't need DoS resistance).
#[derive(Default)]
struct FxHasher(u64);

impl Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }
    fn write_i32(&mut self, i: i32) {
        self.write_u64(i as u32 as u64);
    }
    fn write_u64(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

impl Lab {
    /// Advance the layout by `iterations` relaxation steps. `pinned` atoms are
    /// held in place (e.g. while being dragged). Returns false when asleep.
    pub fn relax(&mut self, iterations: usize, pinned: Option<AtomId>) -> bool {
        if self.awake == 0 {
            return false;
        }
        let n = self.atom_slots();
        let mut force = vec![Vec3::ZERO; n];
        let mut max_move = 0.0f32;
        // Long-range repulsion changes slowly: evaluate it once per call and
        // reuse it for each sub-step (it's the only non-local term).
        let adj = self.adjacency();
        let mut repel = vec![Vec3::ZERO; n];
        self.repulsion(&adj, &mut repel);

        for _ in 0..iterations {
            force.copy_from_slice(&repel);
            self.bond_forces(&mut force);
            self.angle_forces(&mut force);

            max_move = 0.0;
            let ids: Vec<AtomId> = self.atoms().map(|(id, _)| id).collect();
            for id in ids {
                let Some(a) = self.atom_mut(id) else { continue };
                if Some(id) == pinned {
                    a.vel = Vec3::ZERO;
                    continue;
                }
                let f = force.get(id as usize).copied().unwrap_or(Vec3::ZERO);
                // Never let a NaN/inf (e.g. from coincident atoms) poison the
                // scene: drop the bad step and stop the atom instead.
                if !f.is_finite() || !a.vel.is_finite() {
                    a.vel = Vec3::ZERO;
                    continue;
                }
                let mut v = a.vel * FRICTION + f * STEP;
                let len = v.length();
                if len > MAX_MOVE {
                    v *= MAX_MOVE / len;
                }
                let next = a.pos + v;
                if !next.is_finite() {
                    a.vel = Vec3::ZERO;
                    continue;
                }
                a.vel = v;
                a.pos = next;
                max_move = max_move.max(len.min(MAX_MOVE));
            }
        }

        if pinned.is_some() {
            self.awake = self.awake.max(30);
        } else if max_move < 2e-4 {
            self.awake = self.awake.saturating_sub(iterations as u32 * 4);
        } else {
            self.awake = self.awake.saturating_sub(1).max(30);
        }
        true
    }

    fn bond_forces(&self, force: &mut [Vec3]) {
        for (_, b) in self.bonds() {
            let (Some(aa), Some(ab)) = (self.atom(b.a), self.atom(b.b)) else {
                continue;
            };
            let ((pa, za), (pb, zb)) = ((aa.pos, aa.z), (ab.pos, ab.z));
            let d = pb - pa;
            let len = d.length().max(1e-4);
            let target = bond_length(za, zb, b.order, b.kind);
            let f = d / len * (len - target) * K_BOND;
            force[b.a as usize] += f;
            force[b.b as usize] -= f;
        }
    }

    fn angle_forces(&mut self, force: &mut [Vec3]) {
        let ids: Vec<AtomId> = self.atoms().map(|(id, _)| id).collect();
        let mut nbrs: Vec<(AtomId, Vec3, f32)> = Vec::with_capacity(8);
        for id in ids {
            let Some(a) = self.atom(id) else { continue };
            let center = a.pos;
            let free = self.free(id) as usize;
            nbrs.clear();
            for (n, b) in self.neighbors(id) {
                let Some(p) = self.atom(n).map(|a| a.pos) else {
                    continue;
                };
                let r = p - center;
                let len = r.length().max(1e-4);
                // Ionic neighbours are weaker domains: ions arrange more loosely.
                let w = if b.kind == BondKind::Ionic { 0.6 } else { 1.0 };
                nbrs.push((n, r / len, w * len));
            }
            let mut domains = a.domains.clone();
            let total = nbrs.len() + domains.len();
            if total < 2 {
                continue;
            }
            // Conjugation: an atom with a lone pair, only single bonds itself,
            // bonded to an atom that has a double/triple bond (amide N, ring N
            // in caffeine, phenol O) has one lone pair delocalised into the
            // p system. That pair isn't a VSEPR domain (the atom goes flat,
            // sp2); it sits perpendicular to the plane instead.
            let conjugated = nbrs.len() >= 2
                && domains.len() > free
                && self.lone_pairs(id) > 0
                && !crate::elements::category(a.z).is_metal()
                && self.neighbors(id).all(|(_, b)| b.order == 1)
                && self.neighbors(id).any(|(n, b)| {
                    b.kind == BondKind::Covalent
                        && self
                            .neighbors(n)
                            .any(|(_, b2)| b2.order >= 2 && b2.kind == BondKind::Covalent)
                });
            let p_orbital = conjugated.then_some(nbrs.len() + free);
            let mut dom_push = vec![Vec3::ZERO; domains.len()];
            let dir = |i: usize, nbrs: &[(AtomId, Vec3, f32)], domains: &[Vec3]| {
                if i < nbrs.len() {
                    nbrs[i].1
                } else {
                    domains[i - nbrs.len()]
                }
            };
            let weight = |i: usize| {
                if i < nbrs.len() {
                    1.0
                } else if i - nbrs.len() < free {
                    W_OPEN_SITE
                } else {
                    W_LONE_PAIR
                }
            };
            for i in 0..total {
                if Some(i) == p_orbital {
                    continue;
                }
                let ui = dir(i, &nbrs, &domains);
                let mut f = Vec3::ZERO;
                for j in 0..total {
                    if i == j || Some(j) == p_orbital {
                        continue;
                    }
                    let mut diff = ui - dir(j, &nbrs, &domains);
                    if diff.length_squared() < 1e-6 {
                        // Coincident domains: push apart along a stable perpendicular.
                        diff = ui.any_orthogonal_vector() * if i < j { 0.05 } else { -0.05 };
                    }
                    let dist = diff.length().max(0.05);
                    f += diff / (dist * dist * dist) * weight(j);
                }
                let tangential = (f - ui * f.dot(ui)) * weight(i) * K_ANGLE;
                if i < nbrs.len() {
                    let (n, _, lever) = nbrs[i];
                    let push = tangential * lever.min(2.5) * 0.5;
                    force[n as usize] += push;
                    force[id as usize] -= push;
                } else {
                    dom_push[i - nbrs.len()] = tangential;
                }
            }
            for (d, p) in domains.iter_mut().zip(dom_push) {
                *d = (*d + p * 0.2).normalize_or(*d);
            }
            if conjugated && let Some(d) = domains.get_mut(free) {
                let normal = nbrs[0].1.cross(nbrs[1].1).normalize_or_zero();
                if normal != Vec3::ZERO {
                    *d = if normal.dot(*d) >= 0.0 {
                        normal
                    } else {
                        -normal
                    };
                }
            }
            if let Some(a) = self.atom_mut(id) {
                a.domains = domains;
            }
        }
    }

    /// Neighbour ids per atom slot (flat, cache-friendly copy of the bond graph).
    fn adjacency(&self) -> Vec<Vec<AtomId>> {
        let mut adj = vec![Vec::new(); self.atom_slots()];
        for (_, b) in self.bonds() {
            adj[b.a as usize].push(b.b);
            adj[b.b as usize].push(b.a);
        }
        adj
    }

    /// Soft repulsion between non-bonded atoms within `REPEL_RANGE`.
    /// Uses a uniform grid (cell = range) so the cost is O(n), not O(n²).
    fn repulsion(&self, adj: &[Vec<AtomId>], force: &mut [Vec3]) {
        let atoms: Vec<(AtomId, Vec3)> = self.atoms().map(|(id, a)| (id, a.pos)).collect();
        let cell_of = |p: Vec3| -> (i32, i32, i32) {
            let c = (p / REPEL_RANGE).floor();
            (c.x as i32, c.y as i32, c.z as i32)
        };
        let mut grid: HashMap<(i32, i32, i32), Vec<u32>, BuildHasherDefault<FxHasher>> =
            HashMap::with_capacity_and_hasher(atoms.len(), Default::default());
        for (i, &(_, p)) in atoms.iter().enumerate() {
            grid.entry(cell_of(p)).or_default().push(i as u32);
        }
        let r2 = REPEL_RANGE * REPEL_RANGE;
        for (i, &(ia, pa)) in atoms.iter().enumerate() {
            let (cx, cy, cz) = cell_of(pa);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        let Some(bucket) = grid.get(&(cx + dx, cy + dy, cz + dz)) else {
                            continue;
                        };
                        for &j in bucket {
                            let j = j as usize;
                            if j <= i {
                                continue;
                            }
                            let (ib, pb) = atoms[j];
                            let d = pb - pa;
                            let dist2 = d.length_squared();
                            if dist2 > r2 {
                                continue;
                            }
                            // Skip 1-2 and 1-3 pairs: springs and VSEPR own those.
                            let na = &adj[ia as usize];
                            if na.contains(&ib) || na.iter().any(|&n| adj[n as usize].contains(&ib))
                            {
                                continue;
                            }
                            let dist = dist2.sqrt().max(0.05);
                            let mag = K_REPEL * (1.0 / dist2.max(0.0025) - 1.0 / r2);
                            let f = if dist2 < 1e-8 {
                                // Exactly coincident: separate along a stable direction.
                                crate::lab::jitter_dir((ia as u64) << 32 | ib as u64) * mag
                            } else {
                                d / dist * mag
                            };
                            force[ia as usize] -= f;
                            force[ib as usize] += f;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::lab::Lab;
    use glam::Vec3;

    fn angle(lab: &Lab, c: u32, a: u32, b: u32) -> f32 {
        let pc = lab.atom(c).unwrap().pos;
        let u = (lab.atom(a).unwrap().pos - pc).normalize();
        let v = (lab.atom(b).unwrap().pos - pc).normalize();
        u.dot(v).clamp(-1.0, 1.0).acos().to_degrees()
    }

    fn settle(lab: &mut Lab) {
        for _ in 0..2000 {
            lab.relax(4, None);
        }
    }

    #[test]
    fn methane_is_tetrahedral() {
        let mut lab = Lab::new();
        let c = lab.add_atom(6, Vec3::ZERO).unwrap();
        let hs: Vec<_> = (0..4)
            .map(|i| lab.attach(c, 1, Vec3::new(i as f32, 1.0, 0.3)).unwrap())
            .collect();
        settle(&mut lab);
        let a = angle(&lab, c, hs[0], hs[1]);
        assert!((a - 109.5).abs() < 4.0, "H-C-H = {a}");
    }

    #[test]
    fn water_is_bent_and_co2_linear() {
        let mut lab = Lab::new();
        let o = lab.add_atom(8, Vec3::ZERO).unwrap();
        let h1 = lab.attach(o, 1, Vec3::X).unwrap();
        let h2 = lab.attach(o, 1, Vec3::Y).unwrap();
        let c = lab.add_atom(6, Vec3::new(20.0, 0.0, 0.0)).unwrap();
        let o1 = lab.attach(c, 8, Vec3::X).unwrap();
        let o2 = lab.attach(c, 8, Vec3::Y).unwrap();
        settle(&mut lab);
        let w = angle(&lab, o, h1, h2);
        assert!(w > 95.0 && w < 112.0, "H-O-H = {w}");
        let l = angle(&lab, c, o1, o2);
        assert!(l > 172.0, "O-C-O = {l}");
    }

    #[test]
    fn goes_to_sleep() {
        let mut lab = Lab::new();
        let c = lab.add_atom(6, Vec3::ZERO).unwrap();
        for _ in 0..4 {
            lab.attach(c, 1, Vec3::X).unwrap();
        }
        let mut frames = 0;
        while lab.relax(4, None) {
            frames += 1;
            assert!(frames < 5000, "never settled");
        }
    }
}

#[cfg(test)]
mod ring_tests {
    use crate::lab::Lab;
    use glam::Vec3;

    pub(crate) fn benzene(lab: &mut Lab) -> Vec<u32> {
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
        ring
    }

    #[test]
    fn benzene_is_flat_hexagon() {
        let mut lab = Lab::new();
        let ring = benzene(&mut lab);
        let mut frames = 0;
        let worst = |lab: &Lab| -> f32 {
            let p: Vec<Vec3> = ring.iter().map(|&a| lab.atom(a).unwrap().pos).collect();
            (0..6)
                .map(|i| {
                    let (a, b, c) = (p[(i + 5) % 6], p[i], p[(i + 1) % 6]);
                    ((a - b)
                        .normalize()
                        .dot((c - b).normalize())
                        .acos()
                        .to_degrees()
                        - 120.0)
                        .abs()
                })
                .fold(0.0, f32::max)
        };
        while worst(&lab) > 6.0 && frames < 3000 {
            lab.relax(3, None);
            frames += 1;
        }
        eprintln!("benzene settled in {frames} frames");
        assert!(frames < 60, "too slow: {frames} frames");
        let p: Vec<Vec3> = ring.iter().map(|&a| lab.atom(a).unwrap().pos).collect();
        let mut angles = Vec::new();
        for i in 0..6 {
            let (a, b, c) = (p[(i + 5) % 6], p[i], p[(i + 1) % 6]);
            angles.push(
                (a - b)
                    .normalize()
                    .dot((c - b).normalize())
                    .acos()
                    .to_degrees(),
            );
        }
        eprintln!("ring angles {angles:?}");
        for a in angles {
            assert!((a - 120.0).abs() < 6.0, "angle {a}");
        }
    }
}

#[cfg(test)]
mod perf {
    use crate::lab::Lab;
    use glam::Vec3;

    /// `cargo test -p valence_chem --release perf -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn relax_cost_large_scene() {
        let mut lab = Lab::new();
        // 50 hexanes (C6H14) = 1000 atoms.
        for m in 0..50 {
            let base = Vec3::new((m % 8) as f32 * 12.0, (m / 8) as f32 * 12.0, 0.0);
            let mut prev = lab.add_atom(6, base).unwrap();
            let mut carbons = vec![prev];
            for _ in 0..5 {
                prev = lab.attach(prev, 6, Vec3::X).unwrap();
                carbons.push(prev);
            }
            for c in carbons {
                while lab.sigma(c) < 4 {
                    lab.attach(c, 1, Vec3::Y).unwrap();
                }
            }
        }
        let n = lab.atom_count();
        let t = std::time::Instant::now();
        let frames = 200;
        for _ in 0..frames {
            lab.wake();
            lab.relax(3, None);
        }
        let per = t.elapsed().as_secs_f64() * 1000.0 / frames as f64;
        eprintln!("{n} atoms: {per:.3} ms per frame (3 iterations)");
        let mut force = vec![Vec3::ZERO; lab.atom_slots()];
        let time = |name: &str, f: &mut dyn FnMut()| {
            let t = std::time::Instant::now();
            for _ in 0..300 {
                f();
            }
            eprintln!(
                "  {name}: {:.3} ms",
                t.elapsed().as_secs_f64() * 1000.0 / 300.0
            );
        };
        time("bonds", &mut || lab.bond_forces(&mut force));
        let adj = lab.adjacency();
        time("repulsion", &mut || lab.repulsion(&adj, &mut force));
        time("angles", &mut || lab.angle_forces(&mut force));
    }
}

#[cfg(test)]
mod placed_molecule_tests {
    use crate::lab::Lab;
    use glam::Vec3;

    /// Max distance of the given atoms from their best-fit plane, and the
    /// closest approach between any two atoms.
    fn shape_stats(lab: &Lab, ring: &[u32]) -> (f32, f32) {
        let p: Vec<Vec3> = ring.iter().map(|&a| lab.atom(a).unwrap().pos).collect();
        let c = p.iter().copied().sum::<Vec3>() / p.len() as f32;
        // Plane normal: smallest-variance direction (power iteration on the inverse is overkill;
        // use the normal of the two most separated in-plane vectors).
        let mut best = Vec3::Y;
        let mut best_area = 0.0;
        for i in 0..p.len() {
            for j in i + 1..p.len() {
                let n = (p[i] - c).cross(p[j] - c);
                if n.length() > best_area {
                    best_area = n.length();
                    best = n.normalize();
                }
            }
        }
        let flat = p
            .iter()
            .map(|q| (*q - c).dot(best).abs())
            .fold(0.0, f32::max);
        let all: Vec<Vec3> = lab.atoms().map(|(_, a)| a.pos).collect();
        let mut closest = f32::MAX;
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                closest = closest.min(all[i].distance(all[j]));
            }
        }
        (flat, closest)
    }

    #[test]
    fn caffeine_unfolds() {
        let g = crate::smiles::parse("CN1C=NC2=C1C(=O)N(C)C(=O)N2C").unwrap();
        let mut lab = Lab::new();
        let ids = lab.add_molecule(&g, Vec3::ZERO).unwrap();
        for _ in 0..1500 {
            lab.relax(4, None);
        }
        // Ring atoms (SMILES order): N1 C C=N... indices of the 9 ring atoms.
        let ring: Vec<u32> = [1, 2, 3, 4, 5, 6, 8, 10, 12]
            .iter()
            .map(|&i| ids[i])
            .collect();
        let (flat, closest) = shape_stats(&lab, &ring);
        eprintln!("caffeine: ring non-planarity {flat:.2}, closest atoms {closest:.2}");
        assert!(closest > 0.6, "atoms overlapping: {closest}");
        assert!(
            flat < 0.6,
            "fused rings should be nearly flat, off by {flat}"
        );
    }
}
