//! Static periodic-table data. Everything is `const`, so lookups are a plain array index.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    AlkaliMetal,
    AlkalineEarth,
    Transition,
    PostTransition,
    Metalloid,
    Nonmetal,
    Halogen,
    NobleGas,
    Lanthanide,
    Actinide,
}

impl Category {
    pub fn is_metal(self) -> bool {
        matches!(
            self,
            Category::AlkaliMetal
                | Category::AlkalineEarth
                | Category::Transition
                | Category::PostTransition
                | Category::Lanthanide
                | Category::Actinide
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            Category::AlkaliMetal => "Alkali metal",
            Category::AlkalineEarth => "Alkaline earth metal",
            Category::Transition => "Transition metal",
            Category::PostTransition => "Post-transition metal",
            Category::Metalloid => "Metalloid",
            Category::Nonmetal => "Reactive nonmetal",
            Category::Halogen => "Halogen",
            Category::NobleGas => "Noble gas",
            Category::Lanthanide => "Lanthanide",
            Category::Actinide => "Actinide",
        }
    }
}

#[derive(Debug)]
pub struct Element {
    pub z: u8,
    pub symbol: &'static str,
    pub name: &'static str,
    pub mass: f32,
    /// Pauling electronegativity; 0.0 = unknown / not meaningful.
    pub en: f32,
    /// Covalent radius in picometres.
    pub radius_pm: u16,
    /// Bonding capacities this element can take, ascending. The atom uses the
    /// smallest one that fits and "expands" to the next when more is needed.
    /// Empty = does not bond at all.
    pub valences: &'static [u8],
}

macro_rules! el {
    ($z:expr, $s:expr, $n:expr, $m:expr, $en:expr, $r:expr, [$($v:expr),*]) => {
        Element { z: $z, symbol: $s, name: $n, mass: $m, en: $en, radius_pm: $r, valences: &[$($v),*] }
    };
}

pub static ELEMENTS: [Element; 118] = [
    el!(1, "H", "Hydrogen", 1.008, 2.20, 31, [1]),
    el!(2, "He", "Helium", 4.0026, 0.0, 28, []),
    el!(3, "Li", "Lithium", 6.94, 0.98, 128, [1]),
    el!(4, "Be", "Beryllium", 9.0122, 1.57, 96, [2]),
    el!(5, "B", "Boron", 10.81, 2.04, 84, [3]),
    el!(6, "C", "Carbon", 12.011, 2.55, 76, [4]),
    el!(7, "N", "Nitrogen", 14.007, 3.04, 71, [3]),
    el!(8, "O", "Oxygen", 15.999, 3.44, 66, [2]),
    el!(9, "F", "Fluorine", 18.998, 3.98, 57, [1]),
    el!(10, "Ne", "Neon", 20.180, 0.0, 58, []),
    el!(11, "Na", "Sodium", 22.990, 0.93, 166, [1]),
    el!(12, "Mg", "Magnesium", 24.305, 1.31, 141, [2]),
    el!(13, "Al", "Aluminium", 26.982, 1.61, 121, [3]),
    el!(14, "Si", "Silicon", 28.085, 1.90, 111, [4]),
    el!(15, "P", "Phosphorus", 30.974, 2.19, 107, [3, 5]),
    el!(16, "S", "Sulfur", 32.06, 2.58, 105, [2, 4, 6]),
    el!(17, "Cl", "Chlorine", 35.45, 3.16, 102, [1, 3, 5, 7]),
    el!(18, "Ar", "Argon", 39.948, 0.0, 106, []),
    el!(19, "K", "Potassium", 39.098, 0.82, 203, [1]),
    el!(20, "Ca", "Calcium", 40.078, 1.00, 176, [2]),
    el!(21, "Sc", "Scandium", 44.956, 1.36, 170, [3]),
    el!(22, "Ti", "Titanium", 47.867, 1.54, 160, [2, 3, 4]),
    el!(23, "V", "Vanadium", 50.942, 1.63, 153, [2, 3, 4, 5]),
    el!(24, "Cr", "Chromium", 51.996, 1.66, 139, [2, 3, 6]),
    el!(25, "Mn", "Manganese", 54.938, 1.55, 139, [2, 3, 4, 7]),
    el!(26, "Fe", "Iron", 55.845, 1.83, 132, [2, 3]),
    el!(27, "Co", "Cobalt", 58.933, 1.88, 126, [2, 3]),
    el!(28, "Ni", "Nickel", 58.693, 1.91, 124, [2]),
    el!(29, "Cu", "Copper", 63.546, 1.90, 132, [1, 2]),
    el!(30, "Zn", "Zinc", 65.38, 1.65, 122, [2]),
    el!(31, "Ga", "Gallium", 69.723, 1.81, 122, [3]),
    el!(32, "Ge", "Germanium", 72.630, 2.01, 120, [2, 4]),
    el!(33, "As", "Arsenic", 74.922, 2.18, 119, [3, 5]),
    el!(34, "Se", "Selenium", 78.971, 2.55, 120, [2, 4, 6]),
    el!(35, "Br", "Bromine", 79.904, 2.96, 120, [1, 3, 5]),
    el!(36, "Kr", "Krypton", 83.798, 3.00, 116, [0, 2]),
    el!(37, "Rb", "Rubidium", 85.468, 0.82, 220, [1]),
    el!(38, "Sr", "Strontium", 87.62, 0.95, 195, [2]),
    el!(39, "Y", "Yttrium", 88.906, 1.22, 190, [3]),
    el!(40, "Zr", "Zirconium", 91.224, 1.33, 175, [4]),
    el!(41, "Nb", "Niobium", 92.906, 1.6, 164, [3, 5]),
    el!(42, "Mo", "Molybdenum", 95.95, 2.16, 154, [4, 6]),
    el!(43, "Tc", "Technetium", 98.0, 1.9, 147, [4, 7]),
    el!(44, "Ru", "Ruthenium", 101.07, 2.2, 146, [3, 4]),
    el!(45, "Rh", "Rhodium", 102.91, 2.28, 142, [3]),
    el!(46, "Pd", "Palladium", 106.42, 2.20, 139, [2, 4]),
    el!(47, "Ag", "Silver", 107.87, 1.93, 145, [1]),
    el!(48, "Cd", "Cadmium", 112.41, 1.69, 144, [2]),
    el!(49, "In", "Indium", 114.82, 1.78, 142, [3]),
    el!(50, "Sn", "Tin", 118.71, 1.96, 139, [2, 4]),
    el!(51, "Sb", "Antimony", 121.76, 2.05, 139, [3, 5]),
    el!(52, "Te", "Tellurium", 127.60, 2.1, 138, [2, 4, 6]),
    el!(53, "I", "Iodine", 126.90, 2.66, 139, [1, 3, 5, 7]),
    el!(54, "Xe", "Xenon", 131.29, 2.6, 140, [0, 2, 4, 6]),
    el!(55, "Cs", "Caesium", 132.91, 0.79, 244, [1]),
    el!(56, "Ba", "Barium", 137.33, 0.89, 215, [2]),
    el!(57, "La", "Lanthanum", 138.91, 1.10, 207, [3]),
    el!(58, "Ce", "Cerium", 140.12, 1.12, 204, [3, 4]),
    el!(59, "Pr", "Praseodymium", 140.91, 1.13, 203, [3]),
    el!(60, "Nd", "Neodymium", 144.24, 1.14, 201, [3]),
    el!(61, "Pm", "Promethium", 145.0, 1.13, 199, [3]),
    el!(62, "Sm", "Samarium", 150.36, 1.17, 198, [2, 3]),
    el!(63, "Eu", "Europium", 151.96, 1.2, 198, [2, 3]),
    el!(64, "Gd", "Gadolinium", 157.25, 1.2, 196, [3]),
    el!(65, "Tb", "Terbium", 158.93, 1.1, 194, [3]),
    el!(66, "Dy", "Dysprosium", 162.50, 1.22, 192, [3]),
    el!(67, "Ho", "Holmium", 164.93, 1.23, 192, [3]),
    el!(68, "Er", "Erbium", 167.26, 1.24, 189, [3]),
    el!(69, "Tm", "Thulium", 168.93, 1.25, 190, [3]),
    el!(70, "Yb", "Ytterbium", 173.05, 1.1, 187, [2, 3]),
    el!(71, "Lu", "Lutetium", 174.97, 1.27, 187, [3]),
    el!(72, "Hf", "Hafnium", 178.49, 1.3, 175, [4]),
    el!(73, "Ta", "Tantalum", 180.95, 1.5, 170, [5]),
    el!(74, "W", "Tungsten", 183.84, 2.36, 162, [4, 6]),
    el!(75, "Re", "Rhenium", 186.21, 1.9, 151, [4, 7]),
    el!(76, "Os", "Osmium", 190.23, 2.2, 144, [4, 8]),
    el!(77, "Ir", "Iridium", 192.22, 2.20, 141, [3, 4]),
    el!(78, "Pt", "Platinum", 195.08, 2.28, 136, [2, 4]),
    el!(79, "Au", "Gold", 196.97, 2.54, 136, [1, 3]),
    el!(80, "Hg", "Mercury", 200.59, 2.00, 132, [1, 2]),
    el!(81, "Tl", "Thallium", 204.38, 1.62, 145, [1, 3]),
    el!(82, "Pb", "Lead", 207.2, 1.87, 146, [2, 4]),
    el!(83, "Bi", "Bismuth", 208.98, 2.02, 148, [3, 5]),
    el!(84, "Po", "Polonium", 209.0, 2.0, 140, [2, 4]),
    el!(85, "At", "Astatine", 210.0, 2.2, 150, [1]),
    el!(86, "Rn", "Radon", 222.0, 2.2, 150, [0, 2]),
    el!(87, "Fr", "Francium", 223.0, 0.7, 260, [1]),
    el!(88, "Ra", "Radium", 226.0, 0.9, 221, [2]),
    el!(89, "Ac", "Actinium", 227.0, 1.1, 215, [3]),
    el!(90, "Th", "Thorium", 232.04, 1.3, 206, [4]),
    el!(91, "Pa", "Protactinium", 231.04, 1.5, 200, [5]),
    el!(92, "U", "Uranium", 238.03, 1.38, 196, [3, 4, 6]),
    el!(93, "Np", "Neptunium", 237.0, 1.36, 190, [5]),
    el!(94, "Pu", "Plutonium", 244.0, 1.28, 187, [4]),
    el!(95, "Am", "Americium", 243.0, 1.13, 180, [3]),
    el!(96, "Cm", "Curium", 247.0, 1.28, 169, [3]),
    el!(97, "Bk", "Berkelium", 247.0, 1.3, 168, [3]),
    el!(98, "Cf", "Californium", 251.0, 1.3, 168, [3]),
    el!(99, "Es", "Einsteinium", 252.0, 1.3, 165, [3]),
    el!(100, "Fm", "Fermium", 257.0, 1.3, 167, [3]),
    el!(101, "Md", "Mendelevium", 258.0, 1.3, 173, [3]),
    el!(102, "No", "Nobelium", 259.0, 1.3, 176, [2]),
    el!(103, "Lr", "Lawrencium", 262.0, 1.3, 161, [3]),
    // Superheavy elements: half-lives of seconds or less, so no chemistry to speak of.
    el!(104, "Rf", "Rutherfordium", 267.0, 0.0, 157, []),
    el!(105, "Db", "Dubnium", 268.0, 0.0, 149, []),
    el!(106, "Sg", "Seaborgium", 269.0, 0.0, 143, []),
    el!(107, "Bh", "Bohrium", 270.0, 0.0, 141, []),
    el!(108, "Hs", "Hassium", 269.0, 0.0, 134, []),
    el!(109, "Mt", "Meitnerium", 278.0, 0.0, 129, []),
    el!(110, "Ds", "Darmstadtium", 281.0, 0.0, 128, []),
    el!(111, "Rg", "Roentgenium", 282.0, 0.0, 121, []),
    el!(112, "Cn", "Copernicium", 285.0, 0.0, 122, []),
    el!(113, "Nh", "Nihonium", 286.0, 0.0, 136, []),
    el!(114, "Fl", "Flerovium", 289.0, 0.0, 143, []),
    el!(115, "Mc", "Moscovium", 290.0, 0.0, 162, []),
    el!(116, "Lv", "Livermorium", 293.0, 0.0, 175, []),
    el!(117, "Ts", "Tennessine", 294.0, 0.0, 165, []),
    el!(118, "Og", "Oganesson", 294.0, 0.0, 157, []),
];

#[inline]
pub fn element(z: u8) -> &'static Element {
    &ELEMENTS[(z as usize).clamp(1, 118) - 1]
}

pub fn by_symbol(sym: &str) -> Option<&'static Element> {
    ELEMENTS.iter().find(|e| e.symbol == sym)
}

/// Where an element sits in the standard 18-column layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TablePos {
    /// Main table: group 1..=18, period 1..=7.
    Main { group: u8, period: u8 },
    /// f-block row (0 = lanthanides, 1 = actinides), column 0..15.
    FBlock { row: u8, col: u8 },
}

pub fn table_pos(z: u8) -> TablePos {
    let z = z.clamp(1, 118);
    let (period, first) = match z {
        1..=2 => (1, 1),
        3..=10 => (2, 3),
        11..=18 => (3, 11),
        19..=36 => (4, 19),
        37..=54 => (5, 37),
        55..=86 => (6, 55),
        _ => (7, 87),
    };
    match z {
        1 => TablePos::Main {
            group: 1,
            period: 1,
        },
        2 => TablePos::Main {
            group: 18,
            period: 1,
        },
        57..=71 => TablePos::FBlock {
            row: 0,
            col: z - 57,
        },
        89..=103 => TablePos::FBlock {
            row: 1,
            col: z - 89,
        },
        _ => {
            let offset = z - first;
            let group = match period {
                2 | 3 => {
                    if offset < 2 {
                        offset + 1
                    } else {
                        offset + 11
                    }
                }
                4 | 5 => offset + 1,
                _ => {
                    // Periods 6/7: s-block, then 14 f-block elements removed.
                    if offset < 2 {
                        offset + 1
                    } else {
                        offset - 14 + 1
                    }
                }
            };
            TablePos::Main { group, period }
        }
    }
}

/// Group (column) number, or `None` for the f-block.
pub fn group(z: u8) -> Option<u8> {
    match table_pos(z) {
        TablePos::Main { group, .. } => Some(group),
        TablePos::FBlock { .. } => None,
    }
}

pub fn period(z: u8) -> u8 {
    match table_pos(z) {
        TablePos::Main { period, .. } => period,
        TablePos::FBlock { row, .. } => 6 + row,
    }
}

pub fn category(z: u8) -> Category {
    match z {
        1 | 6 | 7 | 8 | 15 | 16 | 34 => Category::Nonmetal,
        9 | 17 | 35 | 53 | 85 | 117 => Category::Halogen,
        2 | 10 | 18 | 36 | 54 | 86 | 118 => Category::NobleGas,
        3 | 11 | 19 | 37 | 55 | 87 => Category::AlkaliMetal,
        4 | 12 | 20 | 38 | 56 | 88 => Category::AlkalineEarth,
        5 | 14 | 32 | 33 | 51 | 52 => Category::Metalloid,
        57..=71 => Category::Lanthanide,
        89..=103 => Category::Actinide,
        13 | 31 | 49 | 50 | 81 | 82 | 83 | 84 | 113..=116 => Category::PostTransition,
        _ => Category::Transition,
    }
}

/// Valence (outer-shell) electrons for main-group elements; 0 for metals
/// (we never draw lone pairs on metals).
pub fn valence_electrons(z: u8) -> u8 {
    if category(z).is_metal() {
        return 0;
    }
    match z {
        1 => 1,
        2 => 2,
        _ => match group(z) {
            Some(g @ 13..=18) => g - 10,
            Some(g) if g <= 2 => g,
            _ => 0,
        },
    }
}

/// No stable isotope: technetium, promethium, and everything from polonium
/// (84) up. (Bismuth-209 decays with a half-life ~10⁹ × the age of the
/// universe, so it's treated as stable.)
pub fn is_radioactive(z: u8) -> bool {
    z == 43 || z == 61 || z >= 84
}

/// Elements that can accept a proton onto a lone pair to form an "-onium"
/// cation (NH4+, H3O+, PH4+, H3S+).
pub fn can_protonate(z: u8) -> bool {
    matches!(z, 7 | 8 | 15 | 16 | 33 | 34)
}

/// Electropositive-first ordering used for inorganic formulas and binary
/// names (IUPAC element sequence, simplified).
pub fn formula_rank(z: u8) -> u32 {
    const NONMETAL_SEQ: [u8; 17] = [5, 14, 6, 51, 33, 15, 7, 1, 52, 34, 16, 85, 53, 35, 17, 8, 9];
    if category(z) == Category::NobleGas {
        return z as u32;
    }
    if let Some(i) = NONMETAL_SEQ.iter().position(|&n| n == z) {
        return 1000 + i as u32;
    }
    // Metals and the rest: by electronegativity, most electropositive first.
    200 + (element(z).en * 100.0) as u32
}

/// Root used for "-ide" anion names (chloride, oxide, ...).
pub fn anion_stem(z: u8) -> &'static str {
    match z {
        1 => "hydr",
        5 => "bor",
        6 => "carb",
        7 => "nitr",
        8 => "ox",
        9 => "fluor",
        14 => "silic",
        15 => "phosph",
        16 => "sulf",
        17 => "chlor",
        32 => "german",
        33 => "arsen",
        34 => "selen",
        35 => "brom",
        51 => "antimon",
        52 => "tellur",
        53 => "iod",
        85 => "astat",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_consistent() {
        for (i, e) in ELEMENTS.iter().enumerate() {
            assert_eq!(e.z as usize, i + 1, "{} out of order", e.symbol);
            assert!(
                e.valences.windows(2).all(|w| w[0] < w[1]),
                "{} valences unsorted",
                e.symbol
            );
        }
    }

    #[test]
    fn positions() {
        assert_eq!(
            table_pos(1),
            TablePos::Main {
                group: 1,
                period: 1
            }
        );
        assert_eq!(
            table_pos(8),
            TablePos::Main {
                group: 16,
                period: 2
            }
        );
        assert_eq!(
            table_pos(13),
            TablePos::Main {
                group: 13,
                period: 3
            }
        );
        assert_eq!(
            table_pos(26),
            TablePos::Main {
                group: 8,
                period: 4
            }
        );
        assert_eq!(
            table_pos(72),
            TablePos::Main {
                group: 4,
                period: 6
            }
        );
        assert_eq!(
            table_pos(86),
            TablePos::Main {
                group: 18,
                period: 6
            }
        );
        assert_eq!(
            table_pos(118),
            TablePos::Main {
                group: 18,
                period: 7
            }
        );
        assert_eq!(table_pos(57), TablePos::FBlock { row: 0, col: 0 });
        assert_eq!(table_pos(103), TablePos::FBlock { row: 1, col: 14 });
        // Every main-table cell is used at most once.
        let mut seen = std::collections::HashSet::new();
        for z in 1..=118 {
            assert!(seen.insert(table_pos(z)), "duplicate cell for {z}");
        }
    }

    #[test]
    fn radioactivity() {
        assert!(is_radioactive(43) && is_radioactive(92) && is_radioactive(86));
        assert!(!is_radioactive(83) && !is_radioactive(6) && !is_radioactive(82));
    }

    #[test]
    fn valence_electron_counts() {
        assert_eq!(valence_electrons(6), 4);
        assert_eq!(valence_electrons(8), 6);
        assert_eq!(valence_electrons(17), 7);
        assert_eq!(valence_electrons(54), 8);
        assert_eq!(valence_electrons(11), 0);
    }
}
