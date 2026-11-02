//! Randomised stress test: thousands of arbitrary player actions must never
//! panic, never break the lab's invariants, and rejected actions must leave
//! the lab untouched.

use valence_chem::{Lab, MAX_ATOMS, Namer, Vec3};

/// Small deterministic PRNG (xorshift*), so failures are reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    }
}

const COMMON: [u8; 16] = [1, 1, 1, 6, 6, 7, 8, 8, 9, 15, 16, 17, 11, 12, 26, 54];

fn random_z(r: &mut Rng) -> u8 {
    match r.below(10) {
        0 => r.below(256) as u8, // includes invalid 0 and >118
        1 => 1 + r.below(118) as u8,
        _ => COMMON[r.below(COMMON.len())],
    }
}

fn random_atom(r: &mut Rng, lab: &Lab) -> u32 {
    if r.below(20) == 0 {
        return r.below(lab.atom_slots() + 5) as u32; // possibly dead / out of range
    }
    let ids: Vec<u32> = lab.atoms().map(|(id, _)| id).collect();
    if ids.is_empty() {
        0
    } else {
        ids[r.below(ids.len())]
    }
}

fn check(lab: &Lab, seed: u64, step: usize, what: &str) {
    if let Err(e) = lab.validate() {
        panic!("seed {seed} step {step} after {what}: {e}");
    }
}

fn run(seed: u64, steps: usize) -> (usize, usize, usize) {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut lab = Lab::new();
    let namer = Namer::new();
    for step in 0..steps {
        let op = r.below(100);
        let rev = lab.revision();
        match op {
            0..=14 => {
                let p = Vec3::new(r.f(), r.f(), r.f()) * 10.0;
                let _ = lab.add_atom(random_z(&mut r), p);
                check(&lab, seed, step, "add");
            }
            15..=54 => {
                let t = random_atom(&mut r, &lab);
                let z = random_z(&mut r);
                if lab.attach(t, z, Vec3::new(r.f(), r.f(), r.f())).is_err() {
                    assert_eq!(
                        lab.revision(),
                        rev,
                        "seed {seed} step {step}: rejected attach mutated the lab"
                    );
                }
                check(&lab, seed, step, "attach");
            }
            55..=69 => {
                let (a, b) = (random_atom(&mut r, &lab), random_atom(&mut r, &lab));
                if lab.connect(a, b).is_err() {
                    assert_eq!(
                        lab.revision(),
                        rev,
                        "seed {seed} step {step}: rejected connect mutated the lab"
                    );
                }
                check(&lab, seed, step, "connect");
            }
            70..=79 => {
                lab.remove_atom(random_atom(&mut r, &lab));
                check(&lab, seed, step, "remove");
            }
            80..=89 => {
                let pinned = (r.below(3) == 0).then(|| random_atom(&mut r, &lab));
                lab.relax(1 + r.below(4), pinned);
                check(&lab, seed, step, "relax");
            }
            90..=93 => {
                // Garbage input must be ignored, not stored.
                let a = random_atom(&mut r, &lab);
                lab.set_pos(a, Vec3::new(f32::NAN, 0.0, f32::INFINITY));
                lab.translate(&[a], Vec3::splat(f32::NAN));
                let comp = lab.component(a);
                lab.translate(&comp, Vec3::new(r.f(), r.f(), r.f()));
                check(&lab, seed, step, "move");
            }
            94..=97 => {
                // Queries must never mutate.
                let (a, b) = (random_atom(&mut r, &lab), random_atom(&mut r, &lab));
                let _ = lab.check_attach(a, random_z(&mut r));
                let _ = lab.check_connect(a, b);
                assert_eq!(lab.revision(), rev);
            }
            _ => {
                for comp in lab.components() {
                    let names = namer.name(&lab.graph(&comp));
                    assert!(!names.title.is_empty() || comp.is_empty());
                }
            }
        }
    }
    let biggest = lab.components().iter().map(|c| c.len()).max().unwrap_or(0);
    (lab.atom_count(), lab.bonds().count(), biggest)
}

#[test]
fn random_actions_never_break_the_lab() {
    let (mut atoms, mut bonds, mut biggest) = (0, 0, 0);
    for seed in 0..200 {
        let (a, b, m) = run(seed, 1500);
        atoms += a;
        bonds += b;
        biggest = biggest.max(m);
    }
    eprintln!("stress: {atoms} atoms, {bonds} bonds left across runs; largest molecule {biggest}");
    assert!(
        bonds > 1000,
        "stress test isn't building much: {bonds} bonds"
    );
}

#[test]
fn fused_carbon_sheet_solves_quickly() {
    // Lots of rings sharing atoms is the worst case for the bond-order solver.
    let mut lab = Lab::new();
    let w = 12;
    let mut grid = Vec::new();
    for i in 0..w * w {
        grid.push(
            lab.add_atom(6, Vec3::new((i % w) as f32, (i / w) as f32, 0.0) * 1.4)
                .unwrap(),
        );
    }
    let t = std::time::Instant::now();
    for y in 0..w {
        for x in 0..w {
            let i = y * w + x;
            if x + 1 < w {
                let _ = lab.connect(grid[i], grid[i + 1]);
            }
            if y + 1 < w && (x + y) % 2 == 0 {
                let _ = lab.connect(grid[i], grid[i + w]);
            }
        }
    }
    lab.validate().unwrap();
    let names = Namer::new().name(&lab.graph(&lab.component(grid[0])));
    assert!(!names.formula.is_empty());
    assert!(
        t.elapsed().as_secs_f32() < 10.0,
        "solver too slow: {:?}",
        t.elapsed()
    );
}

#[test]
fn atom_cap_is_enforced() {
    let mut lab = Lab::new();
    for i in 0..MAX_ATOMS {
        lab.add_atom(2, Vec3::new(i as f32, 0.0, 0.0)).unwrap();
    }
    assert!(lab.add_atom(1, Vec3::ZERO).is_err());
    assert!(lab.check_attach(0, 1).is_err());
    lab.relax(2, None); // still runs
}

#[test]
fn restore_keeps_revisions_increasing() {
    let mut lab = Lab::new();
    let snap = lab.clone();
    let a = lab.add_atom(6, Vec3::ZERO).unwrap();
    lab.attach(a, 1, Vec3::X).unwrap();
    let rev = lab.revision();
    lab.restore(snap);
    assert!(lab.revision() > rev);
    assert!(lab.is_empty());
    lab.validate().unwrap();
}
