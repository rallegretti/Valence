//! Human-readable facts about a molecule, for the info card and exports.

use valence_chem::{
    MolGraph,
    elements::{element, is_radioactive},
    energy::bond_energy,
    graph::{BondKind, subscript},
    naming::Names,
};

/// (label, value) rows describing a molecule.
pub fn molecule_facts(names: &Names, g: &MolGraph) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    if let Some(common) = &names.common
        && !names.iupac.is_empty()
        && common.to_lowercase() != names.iupac.to_lowercase()
    {
        rows.push(("IUPAC name".into(), names.iupac.clone()));
    }
    rows.push(("Formula".into(), names.formula.clone()));
    rows.push(("Molar mass".into(), format!("{:.2} g/mol", names.mass)));

    let composition: Vec<String> = g
        .formula_parts()
        .iter()
        .map(|&(z, n)| {
            format!(
                "{}{}",
                element(z).symbol,
                if n > 1 { subscript(n) } else { String::new() }
            )
        })
        .collect();
    rows.push((
        "Atoms".into(),
        format!("{} ({})", g.len(), composition.join(" ")),
    ));

    let (mut single, mut double, mut triple, mut ionic) = (0, 0, 0, 0);
    for e in &g.edges {
        match (e.kind, e.order) {
            (BondKind::Ionic, _) => ionic += 1,
            (_, 1) => single += 1,
            (_, 2) => double += 1,
            _ => triple += 1,
        }
    }
    let mut bonds = Vec::new();
    for (n, what) in [
        (single, "single"),
        (double, "double"),
        (triple, "triple"),
        (ionic, "ionic"),
    ] {
        if n > 0 {
            bonds.push(format!("{n} {what}"));
        }
    }
    if !bonds.is_empty() {
        rows.push(("Bonds".into(), bonds.join(", ")));
    }

    let e = bond_energy(g);
    if !g.edges.is_empty() {
        let value = if e.is_complete() {
            format!("≈ {:.0} kJ/mol (to break every bond)", e.covalent_kj)
        } else if e.covalent_kj > 0.0 {
            format!(
                "≈ {:.0} kJ/mol for covalent bonds (ionic/unlisted bonds not counted)",
                e.covalent_kj
            )
        } else {
            "not estimated (ionic or unlisted bonds)".into()
        };
        rows.push(("Bond energy".into(), value));
    }

    if names.charge != 0 {
        rows.push(("Charge".into(), format!("{:+}", names.charge)));
    }
    rows.push((
        "Reactivity".into(),
        if names.open_sites > 0 {
            format!(
                "reactive: {} open bonding site{} (radical)",
                names.open_sites,
                if names.open_sites == 1 { "" } else { "s" }
            )
        } else if names.charge != 0 {
            "ion: attracts oppositely charged ions".into()
        } else {
            "stable: every bonding site is filled".into()
        },
    ));

    let mut hot: Vec<&str> =
        g.z.iter()
            .copied()
            .filter(|&z| is_radioactive(z))
            .map(|z| element(z).name)
            .collect();
    hot.sort_unstable();
    hot.dedup();
    rows.push((
        "Radioactive".into(),
        if hot.is_empty() {
            "no".into()
        } else {
            format!("yes ({})", hot.join(", "))
        },
    ));
    rows
}
