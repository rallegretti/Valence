//! Chemical reactions: atom balance, automatic balancing, energy estimates,
//! and a small library of well-known reactions that can suggest products.
//!
//! General product prediction is beyond a valence model, so outputs are
//! either built by the player or taken from the library when the inputs
//! match a known reaction.

use crate::canon::structure_hash;
use crate::energy::bond_energy;
use crate::graph::MolGraph;
use crate::lab::Lab;
use crate::smiles;

/// A curated reaction. Species are SMILES (Kekulé, explicit radicals);
/// catalysts are formula keys (see `MolGraph::formula_key`).
pub struct KnownReaction {
    pub name: &'static str,
    pub summary: &'static str,
    pub reactants: &'static [(u32, &'static str)],
    pub products: &'static [(u32, &'static str)],
    /// Accepted catalysts (formula keys) and how to describe them.
    pub catalysts: &'static [&'static str],
    pub catalyst_note: &'static str,
}

pub static KNOWN_REACTIONS: &[KnownReaction] = &[
    KnownReaction {
        name: "Combustion of hydrogen",
        summary: "Hydrogen burns in oxygen to make water, releasing a lot of energy.",
        reactants: &[(2, "[H][H]"), (1, "O=O")],
        products: &[(2, "O")],
        catalysts: &[],
        catalyst_note: "",
    },
    KnownReaction {
        name: "Electrolysis of water",
        summary: "An electric current splits water into hydrogen and oxygen gas.",
        reactants: &[(2, "O")],
        products: &[(2, "[H][H]"), (1, "O=O")],
        catalysts: &[],
        catalyst_note: "needs electrical energy",
    },
    KnownReaction {
        name: "Combustion of methane",
        summary: "Natural gas burns to carbon dioxide and water.",
        reactants: &[(1, "C"), (2, "O=O")],
        products: &[(1, "O=C=O"), (2, "O")],
        catalysts: &[],
        catalyst_note: "",
    },
    KnownReaction {
        name: "Combustion of ethanol",
        summary: "Alcohol burns to carbon dioxide and water.",
        reactants: &[(1, "CCO"), (3, "O=O")],
        products: &[(2, "O=C=O"), (3, "O")],
        catalysts: &[],
        catalyst_note: "",
    },
    KnownReaction {
        name: "Cellular respiration",
        summary: "Cells oxidise glucose to carbon dioxide and water to release energy.",
        reactants: &[(1, "OCC1OC(O)C(O)C(O)C1O"), (6, "O=O")],
        products: &[(6, "O=C=O"), (6, "O")],
        catalysts: &[],
        catalyst_note: "enzymes in living cells",
    },
    KnownReaction {
        name: "Photosynthesis",
        summary: "Plants use light to turn carbon dioxide and water into glucose and oxygen.",
        reactants: &[(6, "O=C=O"), (6, "O")],
        products: &[(1, "OCC1OC(O)C(O)C(O)C1O"), (6, "O=O")],
        catalysts: &[],
        catalyst_note: "chlorophyll and sunlight",
    },
    KnownReaction {
        name: "Formation of carbonic acid",
        summary: "Carbon dioxide dissolves in water to form carbonic acid (fizzy drinks).",
        reactants: &[(1, "O=C=O"), (1, "O")],
        products: &[(1, "OC(=O)O")],
        catalysts: &[],
        catalyst_note: "",
    },
    KnownReaction {
        name: "Haber–Bosch process",
        summary: "Nitrogen and hydrogen combine to make ammonia for fertiliser.",
        reactants: &[(1, "N#N"), (3, "[H][H]")],
        products: &[(2, "N")],
        catalysts: &["Fe"],
        catalyst_note: "iron catalyst, high pressure and temperature",
    },
    KnownReaction {
        name: "Decomposition of hydrogen peroxide",
        summary: "Hydrogen peroxide breaks down into water and oxygen.",
        reactants: &[(2, "OO")],
        products: &[(2, "O"), (1, "O=O")],
        catalysts: &["MnO2"],
        catalyst_note: "manganese dioxide catalyst",
    },
    KnownReaction {
        name: "Neutralisation",
        summary: "An acid and a base react to form a salt and water.",
        reactants: &[(1, "[Na]O"), (1, "Cl")],
        products: &[(1, "[Na]Cl"), (1, "O")],
        catalysts: &[],
        catalyst_note: "",
    },
    KnownReaction {
        name: "Synthesis of hydrogen chloride",
        summary: "Hydrogen and chlorine combine (explosively in light).",
        reactants: &[(1, "[H][H]"), (1, "ClCl")],
        products: &[(2, "Cl")],
        catalysts: &[],
        catalyst_note: "",
    },
    KnownReaction {
        name: "Hydrogenation of ethylene",
        summary: "Hydrogen adds across the C=C double bond to give ethane.",
        reactants: &[(1, "C=C"), (1, "[H][H]")],
        products: &[(1, "CC")],
        catalysts: &["Ni", "Pt", "Pd"],
        catalyst_note: "nickel, platinum or palladium catalyst",
    },
    KnownReaction {
        name: "Contact process",
        summary: "Sulfur dioxide is oxidised to sulfur trioxide, the key step in making sulfuric acid.",
        reactants: &[(2, "O=S=O"), (1, "O=O")],
        products: &[(2, "O=S(=O)=O")],
        catalysts: &["V2O5"],
        catalyst_note: "vanadium(V) oxide catalyst",
    },
    KnownReaction {
        name: "Thermal decomposition of limestone",
        summary: "Heating calcium carbonate gives quicklime and carbon dioxide.",
        reactants: &[(1, "O=C1O[Ca]O1")],
        products: &[(1, "[Ca]=O"), (1, "O=C=O")],
        catalysts: &[],
        catalyst_note: "needs strong heating",
    },
    KnownReaction {
        name: "Synthesis of table salt",
        summary: "Sodium metal burns in chlorine gas to form sodium chloride.",
        reactants: &[(2, "[Na]"), (1, "ClCl")],
        products: &[(2, "[Na]Cl")],
        catalysts: &[],
        catalyst_note: "",
    },
    KnownReaction {
        name: "Rusting of iron",
        summary: "Iron slowly combines with oxygen to form iron(III) oxide (rust).",
        reactants: &[(4, "[Fe]"), (3, "O=O")],
        products: &[(2, "O=[Fe]O[Fe]=O")],
        catalysts: &[],
        catalyst_note: "speeded up by water and salt",
    },
    KnownReaction {
        name: "Burning magnesium",
        summary: "Magnesium burns with a brilliant white flame to form magnesium oxide.",
        reactants: &[(2, "[Mg]"), (1, "O=O")],
        products: &[(2, "[Mg]=O")],
        catalysts: &[],
        catalyst_note: "",
    },
];

/// A library entry with structures resolved to the lab's own representation.
pub struct LibraryReaction {
    pub info: &'static KnownReaction,
    reactant_hashes: Vec<u64>,
    product_hashes: Vec<u64>,
    /// Products (graph, coefficient), ready to be built in the lab.
    pub products: Vec<(MolGraph, u32)>,
}

/// Lab-normalised structure for a SMILES (ionic bonds, charges, etc. exactly
/// as the player would build it).
fn lab_graph(smi: &str) -> Option<MolGraph> {
    let parsed = smiles::parse(smi).ok()?;
    let mut lab = Lab::new();
    let ids = lab.add_molecule(&parsed, glam::Vec3::ZERO).ok()?;
    Some(lab.graph(&lab.component(*ids.first()?)))
}

fn hash_set(mut v: Vec<u64>) -> Vec<u64> {
    v.sort_unstable();
    v.dedup();
    v
}

pub struct ReactionLibrary {
    reactions: Vec<LibraryReaction>,
}

impl Default for ReactionLibrary {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of checking a reaction the player has set up.
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    /// Atoms of each element and charge match on both sides.
    pub balanced: bool,
    /// Per element: atoms in inputs minus atoms in outputs (only non-zero).
    pub imbalance: Vec<(u8, i64)>,
    pub charge_diff: i64,
    /// Smallest whole-number coefficients that balance it (inputs then
    /// outputs), if exactly one such ratio exists.
    pub balancing: Option<Vec<u32>>,
    /// Estimated energy change (kJ per reaction as written): negative =
    /// releases energy. Only when balanced and every bond is tabulated.
    pub delta_h_kj: Option<f32>,
    /// Library reactions whose reactants are exactly these inputs.
    pub matches: Vec<usize>,
}

impl ReactionLibrary {
    pub fn new() -> Self {
        let mut reactions = Vec::new();
        for info in KNOWN_REACTIONS {
            let reactants: Option<Vec<u64>> = info
                .reactants
                .iter()
                .map(|(_, s)| lab_graph(s).map(|g| structure_hash(&g)))
                .collect();
            let products: Option<Vec<(MolGraph, u32)>> = info
                .products
                .iter()
                .map(|&(c, s)| lab_graph(s).map(|g| (g, c)))
                .collect();
            match (reactants, products) {
                (Some(r), Some(p)) => {
                    let product_hashes =
                        hash_set(p.iter().map(|(g, _)| structure_hash(g)).collect());
                    reactions.push(LibraryReaction {
                        info,
                        reactant_hashes: hash_set(r),
                        product_hashes,
                        products: p,
                    });
                }
                _ => debug_assert!(
                    false,
                    "library reaction '{}' has an unbuildable species",
                    info.name
                ),
            }
        }
        ReactionLibrary { reactions }
    }

    pub fn reactions(&self) -> &[LibraryReaction] {
        &self.reactions
    }

    pub fn get(&self, i: usize) -> Option<&LibraryReaction> {
        self.reactions.get(i)
    }

    /// Does the player's output set match this library reaction's products?
    pub fn products_match(&self, i: usize, outputs: &[&MolGraph]) -> bool {
        self.get(i).is_some_and(|r| {
            hash_set(outputs.iter().map(|g| structure_hash(g)).collect()) == r.product_hashes
        })
    }

    /// Is one of the reaction's accepted catalysts among `catalysts`?
    pub fn catalyst_present(&self, i: usize, catalysts: &[&MolGraph]) -> bool {
        self.get(i).is_some_and(|r| {
            catalysts
                .iter()
                .any(|g| r.info.catalysts.contains(&g.formula_key().as_str()))
        })
    }

    pub fn analyse(&self, inputs: &[(&MolGraph, u32)], outputs: &[(&MolGraph, u32)]) -> Analysis {
        let mut a = Analysis::default();
        let input_set = hash_set(inputs.iter().map(|(g, _)| structure_hash(g)).collect());
        if !inputs.is_empty() {
            a.matches = (0..self.reactions.len())
                .filter(|&i| self.reactions[i].reactant_hashes == input_set)
                .collect();
        }
        if outputs.is_empty() {
            return a;
        }
        // Element and charge balance with the coefficients as given.
        let mut counts = [0i64; 119];
        let mut charge = 0i64;
        for (side, sign) in [(inputs, 1i64), (outputs, -1i64)] {
            for (g, c) in side {
                for &z in &g.z {
                    counts[z as usize] += sign * *c as i64;
                }
                charge += sign * *c as i64 * g.total_charge() as i64;
            }
        }
        a.imbalance = (1..=118u8)
            .filter(|&z| counts[z as usize] != 0)
            .map(|z| (z, counts[z as usize]))
            .collect();
        a.charge_diff = charge;
        a.balanced = a.imbalance.is_empty() && charge == 0;

        let species: Vec<&MolGraph> = inputs
            .iter()
            .chain(outputs.iter())
            .map(|(g, _)| *g)
            .collect();
        a.balancing = balance(&species, inputs.len());

        if a.balanced {
            let mut dh = 0.0f32;
            let mut ok = true;
            for (side, sign) in [(inputs, 1.0f32), (outputs, -1.0f32)] {
                for (g, c) in side {
                    let e = bond_energy(g);
                    ok &= e.is_complete();
                    dh += sign * *c as f32 * e.covalent_kj;
                }
            }
            a.delta_h_kj = ok.then_some(dh);
        }
        a
    }
}

// ---------------------------------------------------------------------------
// Balancing: smallest positive integer vector in the null space of the
// element/charge matrix, with exact rational arithmetic.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
struct Q {
    n: i128,
    d: i128,
}

fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

impl Q {
    fn new(n: i128, d: i128) -> Q {
        if d == 0 {
            return Q { n: 0, d: 1 };
        }
        let g = gcd(n, d).max(1);
        let s = if d < 0 { -1 } else { 1 };
        Q {
            n: s * n / g,
            d: s * d / g,
        }
    }
    fn int(n: i64) -> Q {
        Q { n: n as i128, d: 1 }
    }
    fn is_zero(self) -> bool {
        self.n == 0
    }
    fn sub(self, o: Q) -> Q {
        Q::new(self.n * o.d - o.n * self.d, self.d * o.d)
    }
    fn mul(self, o: Q) -> Q {
        Q::new(self.n * o.n, self.d * o.d)
    }
    fn div(self, o: Q) -> Q {
        Q::new(self.n * o.d, self.d * o.n)
    }
}

/// Coefficients for `species` (the first `n_inputs` are inputs) that conserve
/// every element and charge. `None` if impossible or if more than one
/// independent balancing exists (the reaction is under-specified).
pub fn balance(species: &[&MolGraph], n_inputs: usize) -> Option<Vec<u32>> {
    let cols = species.len();
    if cols < 2 || n_inputs == 0 || n_inputs >= cols {
        return None;
    }
    let mut elems: Vec<u8> = species.iter().flat_map(|g| g.z.iter().copied()).collect();
    elems.sort_unstable();
    elems.dedup();
    let mut rows: Vec<Vec<Q>> = Vec::new();
    for &z in &elems {
        rows.push(
            species
                .iter()
                .enumerate()
                .map(|(j, g)| {
                    let n = g.z.iter().filter(|&&x| x == z).count() as i64;
                    Q::int(if j < n_inputs { n } else { -n })
                })
                .collect(),
        );
    }
    rows.push(
        species
            .iter()
            .enumerate()
            .map(|(j, g)| {
                let q = g.total_charge() as i64;
                Q::int(if j < n_inputs { q } else { -q })
            })
            .collect(),
    );

    // Reduced row echelon form.
    let mut pivots: Vec<usize> = Vec::new();
    let mut r = 0;
    for c in 0..cols {
        let Some(p) = (r..rows.len()).find(|&i| !rows[i][c].is_zero()) else {
            continue;
        };
        rows.swap(r, p);
        let pv = rows[r][c];
        for x in rows[r].iter_mut() {
            *x = x.div(pv);
        }
        for i in 0..rows.len() {
            if i != r && !rows[i][c].is_zero() {
                let f = rows[i][c];
                let pivot_row = rows[r].clone();
                for (x, y) in rows[i].iter_mut().zip(pivot_row) {
                    *x = x.sub(f.mul(y));
                }
            }
        }
        pivots.push(c);
        r += 1;
        if r == rows.len() {
            break;
        }
    }
    let free: Vec<usize> = (0..cols).filter(|c| !pivots.contains(c)).collect();
    if free.len() != 1 {
        return None;
    }
    let f = free[0];
    let mut x = vec![Q::int(0); cols];
    x[f] = Q::int(1);
    for (i, &pc) in pivots.iter().enumerate() {
        x[pc] = Q::int(0).sub(rows[i][f]);
    }
    // Scale to the smallest positive integers.
    let lcm = x.iter().fold(1i128, |l, q| l / gcd(l, q.d).max(1) * q.d);
    let mut ints: Vec<i128> = x.iter().map(|q| q.n * (lcm / q.d)).collect();
    if ints.iter().all(|&v| v <= 0) {
        ints.iter_mut().for_each(|v| *v = -*v);
    }
    if ints.iter().any(|&v| v <= 0) {
        return None;
    }
    let g = ints.iter().fold(0i128, |g, &v| gcd(g, v));
    let out: Vec<u32> = ints.iter().map(|&v| (v / g.max(1)) as u32).collect();
    (out.iter().all(|&v| v <= 1000)).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(s: &str) -> MolGraph {
        lab_graph(s).unwrap()
    }

    #[test]
    fn library_builds() {
        let lib = ReactionLibrary::new();
        assert_eq!(
            lib.reactions().len(),
            KNOWN_REACTIONS.len(),
            "every library reaction must be buildable"
        );
    }

    #[test]
    fn balancing() {
        let (h2, o2, h2o) = (g("[H][H]"), g("O=O"), g("O"));
        assert_eq!(balance(&[&h2, &o2, &h2o], 2), Some(vec![2, 1, 2]));
        let (ch4, co2) = (g("C"), g("O=C=O"));
        assert_eq!(balance(&[&ch4, &o2, &co2, &h2o], 2), Some(vec![1, 2, 1, 2]));
        let glucose = g("OCC1OC(O)C(O)C(O)C1O");
        assert_eq!(
            balance(&[&glucose, &o2, &co2, &h2o], 2),
            Some(vec![1, 6, 6, 6])
        );
        let (fe, rust) = (g("[Fe]"), g("O=[Fe]O[Fe]=O"));
        assert_eq!(balance(&[&fe, &o2, &rust], 2), Some(vec![4, 3, 2]));
        // Impossible: no way to make CO2 from H2.
        assert_eq!(balance(&[&h2, &co2], 1), None);
    }

    #[test]
    fn analysis_matches_and_energy() {
        let lib = ReactionLibrary::new();
        let (h2, o2, h2o) = (g("[H][H]"), g("O=O"), g("O"));
        let a = lib.analyse(&[(&h2, 2), (&o2, 1)], &[(&h2o, 2)]);
        assert!(a.balanced);
        let dh = a.delta_h_kj.unwrap();
        assert!(
            dh < -400.0 && dh > -560.0,
            "2H2 + O2 -> 2H2O should be strongly exothermic, got {dh}"
        );
        assert_eq!(a.matches.len(), 1);
        assert_eq!(
            lib.reactions()[a.matches[0]].info.name,
            "Combustion of hydrogen"
        );

        let unbalanced = lib.analyse(&[(&h2, 1), (&o2, 1)], &[(&h2o, 1)]);
        assert!(!unbalanced.balanced);
        assert_eq!(unbalanced.imbalance, vec![(8, 1)]);
        assert_eq!(unbalanced.balancing, Some(vec![2, 1, 2]));

        // Inputs alone suggest products (CO2 + H2O has two known outcomes).
        let co2 = g("O=C=O");
        let names: Vec<_> = lib
            .analyse(&[(&co2, 1), (&h2o, 1)], &[])
            .matches
            .iter()
            .map(|&i| lib.reactions()[i].info.name)
            .collect();
        assert!(
            names.contains(&"Photosynthesis") && names.contains(&"Formation of carbonic acid"),
            "{names:?}"
        );

        // Catalyst recognition by formula (iron atom for Haber).
        let haber = lib
            .reactions()
            .iter()
            .position(|r| r.info.name == "Haber–Bosch process")
            .unwrap();
        assert!(lib.catalyst_present(haber, &[&g("[Fe]")]));
        assert!(!lib.catalyst_present(haber, &[&g("[Ni]")]));
        // Ionic species match regardless of how they were drawn.
        let neut = lib.analyse(
            &[(&g("[Na]O"), 1), (&g("Cl"), 1)],
            &[(&g("[Na]Cl"), 1), (&h2o, 1)],
        );
        assert!(neut.balanced && !neut.matches.is_empty());
    }
}
