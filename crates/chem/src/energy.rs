//! Estimated bond energies from average bond enthalpies (kJ/mol, gas phase,
//! standard textbook values). Good for "roughly how much energy holds this
//! together" and for the sign of a reaction's energy change; not a
//! substitute for measured thermochemistry.

use crate::elements::by_symbol;
use crate::graph::{BondKind, MolGraph};

/// (element, element, bond order, kJ/mol).
const TABLE: &[(&str, &str, u8, f32)] = &[
    ("H", "H", 1, 436.0),
    ("C", "H", 1, 413.0),
    ("N", "H", 1, 391.0),
    ("O", "H", 1, 463.0),
    ("S", "H", 1, 339.0),
    ("P", "H", 1, 322.0),
    ("Si", "H", 1, 318.0),
    ("B", "H", 1, 389.0),
    ("F", "H", 1, 567.0),
    ("Cl", "H", 1, 431.0),
    ("Br", "H", 1, 366.0),
    ("I", "H", 1, 299.0),
    ("C", "C", 1, 348.0),
    ("C", "C", 2, 614.0),
    ("C", "C", 3, 839.0),
    ("C", "N", 1, 293.0),
    ("C", "N", 2, 615.0),
    ("C", "N", 3, 891.0),
    ("C", "O", 1, 358.0),
    ("C", "O", 2, 799.0),
    ("C", "O", 3, 1072.0),
    ("C", "S", 1, 259.0),
    ("C", "S", 2, 573.0),
    ("C", "F", 1, 485.0),
    ("C", "Cl", 1, 328.0),
    ("C", "Br", 1, 276.0),
    ("C", "I", 1, 240.0),
    ("C", "P", 1, 264.0),
    ("C", "Si", 1, 318.0),
    ("N", "N", 1, 163.0),
    ("N", "N", 2, 418.0),
    ("N", "N", 3, 941.0),
    ("N", "O", 1, 201.0),
    ("N", "O", 2, 607.0),
    ("N", "F", 1, 272.0),
    ("N", "Cl", 1, 200.0),
    ("O", "O", 1, 146.0),
    ("O", "O", 2, 495.0),
    ("O", "F", 1, 190.0),
    ("O", "Cl", 1, 203.0),
    ("O", "Cl", 2, 243.0),
    ("O", "S", 1, 265.0),
    ("O", "S", 2, 523.0),
    ("O", "P", 1, 335.0),
    ("O", "P", 2, 544.0),
    ("O", "Si", 1, 452.0),
    ("O", "Si", 2, 640.0),
    ("O", "B", 1, 536.0),
    ("O", "Xe", 2, 368.0),
    ("S", "S", 1, 266.0),
    ("S", "F", 1, 327.0),
    ("S", "Cl", 1, 253.0),
    ("P", "Cl", 1, 326.0),
    ("P", "F", 1, 490.0),
    ("Si", "Si", 1, 226.0),
    ("Si", "F", 1, 565.0),
    ("Si", "Cl", 1, 381.0),
    ("B", "F", 1, 613.0),
    ("B", "Cl", 1, 456.0),
    ("F", "F", 1, 155.0),
    ("Cl", "Cl", 1, 242.0),
    ("Br", "Br", 1, 193.0),
    ("I", "I", 1, 151.0),
    ("Cl", "F", 1, 253.0),
    ("Xe", "F", 1, 130.0),
    ("Kr", "F", 1, 50.0),
];

/// Average bond enthalpy for a covalent bond, if tabulated.
pub fn bond_enthalpy(za: u8, zb: u8, order: u8) -> Option<f32> {
    TABLE.iter().find_map(|&(a, b, o, kj)| {
        let (a, b) = (by_symbol(a)?.z, by_symbol(b)?.z);
        (o == order && ((a, b) == (za, zb) || (a, b) == (zb, za))).then_some(kj)
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BondEnergy {
    /// Sum of tabulated covalent bond enthalpies (kJ/mol).
    pub covalent_kj: f32,
    /// Covalent bonds with no tabulated value (not included in the sum).
    pub unknown_bonds: usize,
    /// Ionic bonds (lattice energies aren't modelled).
    pub ionic_bonds: usize,
}

impl BondEnergy {
    /// True when every bond was accounted for.
    pub fn is_complete(&self) -> bool {
        self.unknown_bonds == 0 && self.ionic_bonds == 0
    }
}

pub fn bond_energy(g: &MolGraph) -> BondEnergy {
    let mut e = BondEnergy::default();
    for edge in &g.edges {
        match edge.kind {
            BondKind::Ionic => e.ionic_bonds += 1,
            BondKind::Covalent => match bond_enthalpy(g.z[edge.a], g.z[edge.b], edge.order) {
                Some(kj) => e.covalent_kj += kj,
                None => e.unknown_bonds += 1,
            },
        }
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smiles::parse;

    #[test]
    fn energies() {
        let water = bond_energy(&parse("O").unwrap());
        assert_eq!(water.covalent_kj, 926.0);
        assert!(water.is_complete());
        assert_eq!(bond_energy(&parse("O=C=O").unwrap()).covalent_kj, 1598.0);
        assert_eq!(bond_enthalpy(1, 6, 1), Some(413.0));
        assert_eq!(bond_enthalpy(6, 1, 1), Some(413.0));
        assert!(!bond_energy(&parse("[Fe]O").unwrap()).is_complete());
    }
}
