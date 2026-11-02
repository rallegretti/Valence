//! Turns a molecular graph into display names: a curated common/IUPAC name
//! when the structure is known, otherwise a generated systematic name.

use std::collections::HashMap;

use crate::canon::structure_hash;
use crate::compounds::{ANIONS, IONIC_COMMON, KNOWN};
use crate::elements::{anion_stem, category, element, formula_rank};
use crate::graph::{BondKind, MolGraph, charge_superscript, subscript};
use crate::{organic, smiles};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Names {
    /// Headline: the common name if there is one, else the systematic name.
    pub title: String,
    /// IUPAC / systematic name ("" if none could be derived).
    pub iupac: String,
    pub common: Option<String>,
    /// Unicode formula, e.g. "H₂O", "Ca(OH)₂", "NH₄⁺".
    pub formula: String,
    pub charge: i32,
    pub open_sites: u32,
    pub complete: bool,
    pub mass: f32,
}

pub struct Namer {
    known: HashMap<u64, (&'static str, &'static str)>,
    anions: HashMap<String, (&'static str, &'static str)>,
    ionic_common: HashMap<&'static str, &'static str>,
}

impl Default for Namer {
    fn default() -> Self {
        Self::new()
    }
}

const ROMAN: [&str; 9] = ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII"];
const PREFIX: [&str; 11] = [
    "", "mono", "di", "tri", "tetra", "penta", "hexa", "hepta", "octa", "nona", "deca",
];

fn capitalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut done = false;
    for c in s.chars() {
        if !done && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            done = true;
        } else {
            out.push(c);
        }
    }
    out
}

impl Namer {
    pub fn new() -> Self {
        let mut known = HashMap::new();
        for &(smi, common, iupac) in KNOWN {
            match smiles::parse(smi) {
                Ok(g) => {
                    known.insert(structure_hash(&g), (common, iupac));
                }
                Err(e) => debug_assert!(false, "bad SMILES {smi}: {e}"),
            }
        }
        let mut anions = HashMap::new();
        for &(smi, name, display) in ANIONS {
            if let Ok(g) = smiles::parse(smi) {
                anions.insert(g.formula_key(), (name, display));
            }
        }
        let ionic_common = IONIC_COMMON.iter().copied().collect();
        Namer {
            known,
            anions,
            ionic_common,
        }
    }

    pub fn name(&self, g: &MolGraph) -> Names {
        let mut out = Names {
            formula: g.formula(),
            charge: g.total_charge(),
            open_sites: g.total_free(),
            complete: g.is_complete(),
            mass: g.mass(),
            ..Default::default()
        };
        if g.is_empty() {
            return out;
        }
        if g.len() == 1 {
            let e = element(g.z[0]);
            out.iupac = e.name.to_lowercase();
            out.title = if out.charge != 0 {
                format!("{} ion", e.name)
            } else {
                format!("{} atom", e.name)
            };
            return out;
        }
        if let Some(&(common, iupac)) = self.known.get(&structure_hash(g)) {
            out.iupac = iupac.to_string();
            out.common = (!common.is_empty()).then(|| common.to_string());
        } else if g.has_ionic() {
            if let Some((name, formula)) = self.ionic(g) {
                out.iupac = name;
                out.formula = formula;
            }
            out.common = self
                .ionic_common
                .get(g.formula_key().as_str())
                .map(|s| s.to_string());
        } else if let Some(name) = binary(g).or_else(|| organic::name(g)) {
            out.iupac = name;
        }
        out.title = match (&out.common, out.iupac.is_empty()) {
            (Some(c), _) => c.clone(),
            (None, false) => capitalize(&out.iupac),
            (None, true) => {
                if !out.complete {
                    "Reactive fragment".into()
                } else if g.z.contains(&6) {
                    "Unnamed organic compound".into()
                } else {
                    "Unnamed compound".into()
                }
            }
        };
        out
    }

    /// "sodium chloride", "iron(III) oxide", "calcium hydroxide" + a
    /// conventional formula such as "Ca(OH)₂".
    fn ionic(&self, g: &MolGraph) -> Option<(String, String)> {
        // (name, display, count, is_polyatomic, sort rank)
        let mut cations: Vec<(String, String, usize, bool, u32)> = Vec::new();
        let mut anions: Vec<(String, String, usize, bool, u32)> = Vec::new();
        for frag in g.fragments_without(|e| e.kind == BondKind::Ionic) {
            let sub = g.subgraph(&frag);
            let q = sub.total_charge();
            let entry = if frag.len() == 1 && category(sub.z[0]).is_metal() {
                let z = sub.z[0];
                let e = element(z);
                let mut name = e.name.to_lowercase();
                if e.valences.len() > 1 && q > 0 {
                    name.push_str(&format!(
                        "({})",
                        ROMAN.get(q as usize).copied().unwrap_or("?")
                    ));
                }
                (true, name, e.symbol.to_string(), false, formula_rank(z))
            } else if frag.len() == 1 {
                let z = sub.z[0];
                let stem = anion_stem(z);
                if stem.is_empty() {
                    return None;
                }
                (
                    false,
                    format!("{stem}ide"),
                    element(z).symbol.to_string(),
                    false,
                    formula_rank(z),
                )
            } else if frag.iter().any(|&i| category(g.z[i]).is_metal()) || q >= 0 {
                return None;
            } else {
                let &(name, display) = self.anions.get(&sub.formula_key())?;
                (false, name.to_string(), display.to_string(), true, 5000)
            };
            let list = if entry.0 { &mut cations } else { &mut anions };
            match list.iter_mut().find(|x| x.0 == entry.1) {
                Some(x) => x.2 += 1,
                None => list.push((entry.1, entry.2, 1, entry.3, entry.4)),
            }
        }
        if cations.is_empty() || anions.is_empty() {
            return None;
        }
        cations.sort_by_key(|c| (c.4, c.0.clone()));
        anions.sort_by(|a, b| a.0.cmp(&b.0));
        let name = cations
            .iter()
            .chain(anions.iter())
            .map(|x| x.0.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let mut formula = String::new();
        for (_, display, count, poly, _) in cations.iter().chain(anions.iter()) {
            if *count > 1 && *poly {
                formula.push_str(&format!("({display}){}", subscript(*count)));
            } else if *count > 1 {
                formula.push_str(&format!("{display}{}", subscript(*count)));
            } else {
                formula.push_str(display);
            }
        }
        formula.push_str(&charge_superscript(g.total_charge()));
        Some((name, formula))
    }
}

/// Two-element covalent compounds: "dinitrogen tetroxide", "sulfur
/// hexafluoride", and metal–nonmetal ones as "iron(III) chloride".
fn binary(g: &MolGraph) -> Option<String> {
    if g.has_ionic() || g.total_charge() != 0 {
        return None;
    }
    let mut parts = g.element_counts();
    if parts.len() != 2 || (parts.iter().any(|p| p.0 == 6) && parts.iter().any(|p| p.0 == 1)) {
        return None;
    }
    parts.sort_by_key(|p| formula_rank(p.0));
    let ((z1, n1), (z2, n2)) = (parts[0], parts[1]);
    let stem = anion_stem(z2);
    if stem.is_empty() || z1 == z2 {
        return None;
    }
    let e1 = element(z1);
    if category(z1).is_metal() {
        let mut name = e1.name.to_lowercase();
        if e1.valences.len() > 1 && n1 == 1 {
            let idx = g.z.iter().position(|&z| z == z1)?;
            let ox: u8 = g
                .neighbors(idx)
                .iter()
                .map(|&(_, e)| g.edges[e].order)
                .sum();
            name.push_str(&format!(
                "({})",
                ROMAN.get(ox as usize).copied().unwrap_or("?")
            ));
        }
        return Some(format!("{name} {stem}ide"));
    }
    let first = if n1 == 1 {
        String::new()
    } else {
        PREFIX.get(n1)?.to_string()
    };
    let mut second = PREFIX.get(n2)?.to_string();
    let anion = format!("{stem}ide");
    if anion.starts_with('o') && (second.ends_with('a') || second.ends_with('o')) {
        second.pop();
    }
    Some(format!("{first}{} {second}{anion}", e1.name.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab::Lab;
    use glam::Vec3;

    fn names_of(lab: &Lab, any: u32) -> Names {
        let comp = lab.component(any);
        Namer::new().name(&lab.graph(&comp))
    }

    #[test]
    fn all_table_smiles_parse() {
        for &(smi, _, _) in KNOWN {
            smiles::parse(smi).unwrap_or_else(|e| panic!("{smi}: {e}"));
        }
        for &(smi, _, _) in ANIONS {
            smiles::parse(smi).unwrap_or_else(|e| panic!("{smi}: {e}"));
        }
        assert_eq!(
            Namer::new().known.len(),
            KNOWN.len(),
            "duplicate structures in KNOWN"
        );
    }

    #[test]
    fn built_water_is_water() {
        let mut lab = Lab::new();
        let o = lab.add_atom(8, Vec3::ZERO).unwrap();
        lab.attach(o, 1, Vec3::X).unwrap();
        let n = names_of(&lab, o);
        assert_eq!(n.title, "Hydroxyl radical");
        assert!(!n.complete);
        lab.attach(o, 1, Vec3::Y).unwrap();
        let n = names_of(&lab, o);
        assert_eq!(n.title, "Water");
        assert_eq!(n.iupac, "oxidane");
        assert_eq!(n.formula, "H₂O");
        assert!(n.complete);
    }

    #[test]
    fn ionic_names() {
        let mut lab = Lab::new();
        let cl = lab.add_atom(17, Vec3::ZERO).unwrap();
        lab.attach(cl, 11, Vec3::X).unwrap();
        let n = names_of(&lab, cl);
        assert_eq!(
            (n.iupac.as_str(), n.title.as_str(), n.formula.as_str()),
            ("sodium chloride", "Table salt", "NaCl")
        );

        let ca = lab.add_atom(20, Vec3::ZERO).unwrap();
        for _ in 0..2 {
            let o = lab.attach(ca, 8, Vec3::X).unwrap();
            lab.attach(o, 1, Vec3::X).unwrap();
        }
        let n = names_of(&lab, ca);
        assert_eq!(
            (n.iupac.as_str(), n.formula.as_str()),
            ("calcium hydroxide", "Ca(OH)₂")
        );
        assert_eq!(n.title, "Slaked lime");

        // Calcium carbonate: C + 3 O, then Ca bridging the two O radicals.
        let c = lab.add_atom(6, Vec3::ZERO).unwrap();
        let os: Vec<_> = (0..3).map(|_| lab.attach(c, 8, Vec3::X).unwrap()).collect();
        let open: Vec<_> = os.iter().copied().filter(|&o| lab.free(o) > 0).collect();
        assert_eq!(open.len(), 2);
        let ca = lab.attach(open[0], 20, Vec3::X).unwrap();
        lab.connect(ca, open[1]).unwrap();
        let n = names_of(&lab, c);
        assert_eq!(n.iupac, "calcium carbonate");
        assert_eq!(n.formula, "CaCO₃");
        assert!(n.complete);

        let fe = lab.add_atom(26, Vec3::ZERO).unwrap();
        for _ in 0..3 {
            lab.attach(fe, 17, Vec3::X).unwrap();
        }
        assert_eq!(names_of(&lab, fe).iupac, "iron(III) chloride");
    }

    #[test]
    fn binary_names() {
        let ng = |s: &str| binary(&smiles::parse(s).unwrap()).unwrap();
        assert_eq!(ng("FS(F)(F)(F)(F)F"), "sulfur hexafluoride");
        assert_eq!(ng("ClP(Cl)Cl"), "phosphorus trichloride");
        assert_eq!(ng("[O]N=O"), "nitrogen dioxide");
        assert_eq!(ng("[C]=O"), "carbon monoxide");
        assert_eq!(ng("FB(F)F"), "boron trifluoride");
        assert_eq!(ng("F[Xe]F"), "xenon difluoride");
        assert_eq!(ng("O=[Si]=O"), "silicon dioxide");
    }

    #[test]
    fn generated_and_known() {
        let n = Namer::new();
        let nm = |s: &str| n.name(&smiles::parse(s).unwrap());
        assert_eq!(nm("CCO").title, "Ethanol (drinking alcohol)");
        assert_eq!(nm("COC").iupac, "methoxymethane");
        assert_eq!(nm("CCCCCCC").title, "Heptane");
        assert_eq!(nm("CC(C)CCO").iupac, "3-methylbutan-1-ol");
        assert_eq!(nm("C1=CC=CC=C1").title, "Benzene");
    }
}
