//! Reactions tab: assign molecules from the lab as inputs, catalysts or
//! outputs; see the equation, whether it balances, an energy estimate, and
//! known reactions that match (which can build the products for you).

use bevy::prelude::*;
use bevy_egui::egui::{self, Color32, RichText};
use valence_chem::{AtomId, MolGraph, reactions::ReactionLibrary};

use crate::camera::OrbitCam;
use crate::export::DetailsCard;
use crate::palette::{BAD, GOOD, OPEN_SITE, to_egui};
use crate::state::{Chem, Edit, Effects, Selection};

const MUTED: Color32 = Color32::from_rgb(150, 158, 175);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Input,
    Catalyst,
    Output,
}

impl Role {
    pub fn key(self) -> &'static str {
        match self {
            Role::Input => "input",
            Role::Catalyst => "catalyst",
            Role::Output => "output",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Role::Input => "Inputs (reactants)",
            Role::Catalyst => "Catalyst",
            Role::Output => "Outputs (products)",
        }
    }
}

/// One molecule's part in the reaction, tracked by any one of its atoms.
#[derive(Clone, Copy)]
struct Entry {
    anchor: AtomId,
    role: Role,
    coeff: u32,
}

#[derive(Resource)]
pub struct ReactionLib(pub ReactionLibrary);

impl Default for ReactionLib {
    fn default() -> Self {
        Self(ReactionLibrary::new())
    }
}

#[derive(Resource, Default)]
pub struct ReactionState {
    entries: Vec<Entry>,
}

/// An entry resolved against the current molecules.
pub struct Part {
    pub molecule: usize,
    pub role: Role,
    pub coeff: u32,
}

impl ReactionState {
    /// Entries whose molecules still exist, one per molecule (first wins).
    pub fn parts(&self, chem: &Chem) -> Vec<Part> {
        let mut out: Vec<Part> = Vec::new();
        for e in &self.entries {
            if let Some(&m) = chem.atom_mol.get(e.anchor as usize)
                && m < chem.molecules.len()
                && !out.iter().any(|p| p.molecule == m)
            {
                out.push(Part {
                    molecule: m,
                    role: e.role,
                    coeff: e.coeff,
                });
            }
        }
        out
    }

    fn prune(&mut self, chem: &Chem) {
        let mut seen = Vec::new();
        self.entries.retain(|e| {
            let m = chem
                .atom_mol
                .get(e.anchor as usize)
                .copied()
                .filter(|&m| m < chem.molecules.len());
            match m {
                Some(m) if !seen.contains(&m) => {
                    seen.push(m);
                    true
                }
                _ => false,
            }
        });
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Live parts as (anchor atom, role, coefficient), for saving.
    pub fn to_saved(&self, chem: &Chem) -> Vec<(AtomId, Role, u32)> {
        let parts = self.parts(chem);
        self.entries
            .iter()
            .filter(|e| {
                parts
                    .iter()
                    .any(|p| chem.atom_mol.get(e.anchor as usize) == Some(&p.molecule))
            })
            .map(|e| (e.anchor, e.role, e.coeff))
            .collect()
    }

    /// Replace everything (after loading a file).
    pub fn replace(&mut self, parts: Vec<(AtomId, Role, u32)>) {
        self.entries = parts
            .into_iter()
            .map(|(anchor, role, coeff)| Entry {
                anchor,
                role,
                coeff,
            })
            .collect();
    }

    /// Changes whenever the reaction set-up changes (for unsaved-changes tracking).
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for e in &self.entries {
            (e.anchor, e.role as u8, e.coeff).hash(&mut h);
        }
        h.finish()
    }

    fn role_of(&self, chem: &Chem, molecule: usize) -> Option<usize> {
        self.entries
            .iter()
            .position(|e| chem.atom_mol.get(e.anchor as usize) == Some(&molecule))
    }

    pub fn set(&mut self, chem: &Chem, molecule: usize, role: Role, coeff: u32) {
        let Some(&anchor) = chem.molecules.get(molecule).and_then(|m| m.atoms.first()) else {
            return;
        };
        match self.role_of(chem, molecule) {
            Some(i) => {
                self.entries[i] = Entry {
                    anchor,
                    role,
                    coeff,
                }
            }
            None => self.entries.push(Entry {
                anchor,
                role,
                coeff,
            }),
        }
    }

    /// Every atom taking part (for isolating it in an export).
    pub fn atoms(&self, chem: &Chem) -> Vec<AtomId> {
        self.parts(chem)
            .iter()
            .flat_map(|p| chem.molecules[p.molecule].atoms.iter().copied())
            .collect()
    }

    /// "2 H₂ + O₂ → 2 H₂O" (catalysts shown over the arrow).
    pub fn equation(&self, chem: &Chem) -> String {
        let parts = self.parts(chem);
        let side = |role: Role| {
            parts
                .iter()
                .filter(|p| p.role == role)
                .map(|p| {
                    let f = &chem.molecules[p.molecule].names.formula;
                    if p.coeff > 1 {
                        format!("{} {f}", p.coeff)
                    } else {
                        f.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" + ")
        };
        let cat = side(Role::Catalyst);
        let arrow = if cat.is_empty() {
            " → ".to_string()
        } else {
            format!(" —[{cat}]→ ")
        };
        let (i, o) = (side(Role::Input), side(Role::Output));
        format!(
            "{}{arrow}{}",
            if i.is_empty() { "?" } else { &i },
            if o.is_empty() { "?" } else { &o }
        )
    }

    pub fn title(&self, chem: &Chem) -> String {
        let lib = ReactionLibrary::new();
        let (a, _) = self.analysis(chem, &lib);
        a.matches
            .first()
            .and_then(|&i| lib.get(i))
            .map_or_else(|| "reaction".into(), |r| r.info.name.to_string())
    }

    /// Card for exported images.
    pub fn card(&self, chem: &Chem) -> DetailsCard {
        let lib = ReactionLibrary::new();
        let (a, _) = self.analysis(chem, &lib);
        let known = a.matches.first().and_then(|&i| lib.get(i));
        let mut rows = Vec::new();
        for p in self.parts(chem) {
            let n = &chem.molecules[p.molecule].names;
            let role = match p.role {
                Role::Input => "Input",
                Role::Catalyst => "Catalyst",
                Role::Output => "Output",
            };
            rows.push((role.to_string(), format!("{} ({})", n.title, n.formula)));
        }
        if let Some(r) = known {
            rows.push(("About".into(), r.info.summary.into()));
        }
        rows.push((
            "Balanced".into(),
            if a.balanced {
                "yes".into()
            } else {
                "no".into()
            },
        ));
        if let Some(dh) = a.delta_h_kj {
            rows.push(("Energy change".into(), energy_text(dh)));
        }
        DetailsCard {
            title: known.map_or_else(|| "Reaction".to_string(), |r| r.info.name.to_string()),
            subtitle: self.equation(chem),
            rows,
        }
    }

    fn analysis(
        &self,
        chem: &Chem,
        lib: &ReactionLibrary,
    ) -> (
        valence_chem::reactions::Analysis,
        Vec<(Role, MolGraph, u32)>,
    ) {
        let graphs: Vec<(Role, MolGraph, u32)> = self
            .parts(chem)
            .into_iter()
            .map(|p| {
                (
                    p.role,
                    chem.lab.graph(&chem.molecules[p.molecule].atoms),
                    p.coeff,
                )
            })
            .collect();
        let side = |role: Role| {
            graphs
                .iter()
                .filter(|g| g.0 == role)
                .map(|g| (&g.1, g.2))
                .collect::<Vec<_>>()
        };
        let a = lib.analyse(&side(Role::Input), &side(Role::Output));
        (a, graphs)
    }
}

fn energy_text(dh: f32) -> String {
    if dh < -1.0 {
        format!("≈ {dh:.0} kJ: releases energy (exothermic)")
    } else if dh > 1.0 {
        format!("≈ +{dh:.0} kJ: absorbs energy (endothermic)")
    } else {
        "≈ 0 kJ".into()
    }
}

/// Move the reaction's molecules into a left-to-right layout as seen from
/// the camera: inputs, (catalyst above) the arrow gap, outputs.
pub fn arrange(chem: &mut Chem, state: &ReactionState, rig: &mut OrbitCam) {
    let parts = state.parts(chem);
    if parts.is_empty() {
        return;
    }
    let t = rig.transform();
    let (right, up) = (t.rotation * Vec3::X, t.rotation * Vec3::Y);
    let info: Vec<(Vec<AtomId>, Vec3, f32, Role)> = parts
        .iter()
        .map(|p| {
            let atoms = chem.molecules[p.molecule].atoms.clone();
            let pts: Vec<Vec3> = atoms
                .iter()
                .filter_map(|&a| chem.lab.atom(a))
                .map(|a| a.pos)
                .collect();
            let c = pts.iter().copied().sum::<Vec3>() / pts.len().max(1) as f32;
            let r = pts.iter().map(|p| p.distance(c)).fold(0.0f32, f32::max) + 1.2;
            (atoms, c, r, p.role)
        })
        .collect();
    let width = |role: Role| {
        info.iter()
            .filter(|i| i.3 == role)
            .map(|i| i.2 * 2.0 + 1.0)
            .sum::<f32>()
    };
    let gap = 4.0;
    let total = width(Role::Input) + gap + width(Role::Output);
    let origin = rig.focus;
    let mut x = -total / 2.0;
    let mut moves: Vec<(Vec<AtomId>, Vec3)> = Vec::new();
    for role in [Role::Input, Role::Output] {
        for (atoms, c, r, _) in info.iter().filter(|i| i.3 == role) {
            x += r + 0.5;
            moves.push((atoms.clone(), origin + right * x - *c));
            x += r + 0.5;
        }
        if role == Role::Input {
            x += gap;
        }
    }
    let arrow_mid = -total / 2.0 + width(Role::Input) + gap / 2.0;
    let cats: Vec<_> = info.iter().filter(|i| i.3 == Role::Catalyst).collect();
    let mut cx = arrow_mid - cats.iter().map(|i| i.2).sum::<f32>() / 2.0;
    for (atoms, c, r, _) in cats {
        cx += r;
        moves.push((atoms.clone(), origin + right * cx + up * (r + 2.5) - *c));
        cx += r;
    }
    for (atoms, delta) in moves {
        chem.lab.translate(&atoms, delta);
    }
    chem.user_moved = true;
    // Fit the whole row across the visible width (45° vertical field of view).
    let aspect = rig.view_aspect.clamp(0.2, 5.0);
    rig.dist = rig
        .dist
        .max(total * 0.9f32.max(1.25 / aspect) + 6.0)
        .min(200.0);
}

/// The Reactions tab.
pub fn reactions_tab(
    ui: &mut egui::Ui,
    chem: &mut Chem,
    state: &mut ReactionState,
    lib: &ReactionLibrary,
    selection: &mut Selection,
    effects: &mut Effects,
    rig: &mut OrbitCam,
    show_diagram: &mut bool,
    now: f64,
) {
    state.prune(chem);
    ui.label(
        RichText::new(
            "Build molecules on the Build tab, then select one (click an atom) and add it as an input, \
             catalyst or output. Valence checks the balance, estimates the energy change and recognises \
             well-known reactions.",
        )
        .color(MUTED),
    );
    ui.add_space(6.0);

    // Add the selected molecule.
    let sel_mol = selection
        .atom
        .and_then(|a| chem.atom_mol.get(a as usize).copied())
        .filter(|&m| m < chem.molecules.len());
    match sel_mol {
        Some(m) => {
            ui.label(
                RichText::new(format!(
                    "Selected: {} ({})",
                    chem.molecules[m].names.title, chem.molecules[m].names.formula
                ))
                .strong(),
            );
            ui.horizontal_wrapped(|ui| {
                for (role, text) in [
                    (Role::Input, "Add as input"),
                    (Role::Catalyst, "Add as catalyst"),
                    (Role::Output, "Add as output"),
                ] {
                    if ui.button(text).clicked() {
                        state.set(chem, m, role, 1);
                    }
                }
            });
        }
        None => {
            ui.label(
                RichText::new("Select a molecule in the 3D view to add it.")
                    .italics()
                    .color(MUTED),
            );
        }
    }
    ui.add_space(8.0);

    // The three sections.
    let parts = state.parts(chem);
    let mut remove: Option<usize> = None;
    let mut coeff_change: Option<(usize, u32)> = None;
    for role in [Role::Input, Role::Catalyst, Role::Output] {
        ui.label(RichText::new(role.label()).strong().color(Color32::WHITE));
        let mine: Vec<&Part> = parts.iter().filter(|p| p.role == role).collect();
        if mine.is_empty() {
            ui.label(RichText::new("  none").color(MUTED));
        }
        for p in mine {
            let n = &chem.molecules[p.molecule].names;
            ui.horizontal(|ui| {
                if role != Role::Catalyst {
                    let mut c = p.coeff;
                    ui.add(
                        egui::DragValue::new(&mut c)
                            .range(1..=20)
                            .speed(0.1)
                            .prefix("×"),
                    );
                    if c != p.coeff {
                        coeff_change = Some((p.molecule, c));
                    }
                }
                if ui
                    .selectable_label(
                        selection.atom.and_then(|a| chem.atom_mol.get(a as usize))
                            == Some(&p.molecule),
                        format!("{}  ·  {}", n.title, n.formula),
                    )
                    .clicked()
                {
                    selection.atom = chem.molecules[p.molecule].atoms.first().copied();
                }
                if ui
                    .small_button("✕")
                    .on_hover_text("Remove from reaction")
                    .clicked()
                {
                    remove = Some(p.molecule);
                }
            });
        }
        ui.add_space(4.0);
    }
    if let Some((m, c)) = coeff_change
        && let Some(i) = state.role_of(chem, m)
    {
        state.entries[i].coeff = c;
    }
    if let Some(m) = remove
        && let Some(i) = state.role_of(chem, m)
    {
        state.entries.remove(i);
    }
    if state.is_empty() {
        return;
    }

    // Equation and analysis.
    ui.add_space(4.0);
    egui::Frame::new()
        .fill(Color32::from_rgb(26, 29, 37))
        .corner_radius(8)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(state.equation(chem))
                    .size(17.0)
                    .strong()
                    .color(Color32::from_rgb(170, 200, 255)),
            );
            let (a, graphs) = state.analysis(chem, lib);
            let has_outputs = graphs.iter().any(|g| g.0 == Role::Output);
            if has_outputs {
                if a.balanced {
                    ui.label(
                        RichText::new("✓ Balanced: every atom is accounted for")
                            .color(to_egui(GOOD)),
                    );
                } else {
                    let mut msg = Vec::new();
                    for &(z, d) in &a.imbalance {
                        let sym = valence_chem::elements::element(z).symbol;
                        msg.push(if d > 0 {
                            format!("{d} {sym} lost")
                        } else {
                            format!("{} {sym} appeared from nowhere", -d)
                        });
                    }
                    if a.charge_diff != 0 {
                        msg.push(format!("charge differs by {}", a.charge_diff));
                    }
                    ui.label(
                        RichText::new(format!("✗ Not balanced: {}", msg.join(", ")))
                            .color(to_egui(BAD)),
                    );
                    if let Some(coeffs) = &a.balancing
                        && ui.button("Balance the coefficients").clicked()
                    {
                        let order: Vec<usize> = {
                            let p = state.parts(chem);
                            let mut v: Vec<usize> = p
                                .iter()
                                .filter(|x| x.role == Role::Input)
                                .map(|x| x.molecule)
                                .collect();
                            v.extend(
                                p.iter()
                                    .filter(|x| x.role == Role::Output)
                                    .map(|x| x.molecule),
                            );
                            v
                        };
                        for (m, &c) in order.iter().zip(coeffs) {
                            if let Some(i) = state.role_of(chem, *m) {
                                state.entries[i].coeff = c;
                            }
                        }
                    }
                    if a.balancing.is_none() {
                        ui.label(
                            RichText::new(
                                "No whole-number coefficients can balance these species.",
                            )
                            .size(12.0)
                            .color(MUTED),
                        );
                    }
                }
                match a.delta_h_kj {
                    Some(dh) => {
                        ui.label(format!("Energy change {}", energy_text(dh)));
                        ui.label(
                            RichText::new("Estimated from average bond energies.")
                                .size(11.5)
                                .color(MUTED),
                        );
                    }
                    None if a.balanced => {
                        ui.label(
                            RichText::new("Energy change: not estimated (ionic or unlisted bonds)")
                                .size(12.0)
                                .color(MUTED),
                        );
                    }
                    None => {}
                }
            }

            // Known reactions matching these inputs.
            let catalysts: Vec<&MolGraph> = graphs
                .iter()
                .filter(|g| g.0 == Role::Catalyst)
                .map(|g| &g.1)
                .collect();
            let outputs: Vec<&MolGraph> = graphs
                .iter()
                .filter(|g| g.0 == Role::Output)
                .map(|g| &g.1)
                .collect();
            for &i in &a.matches {
                let Some(r) = lib.get(i) else { continue };
                ui.separator();
                ui.label(
                    RichText::new(format!("Known reaction: {}", r.info.name))
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.label(r.info.summary);
                if !r.info.catalysts.is_empty() || !r.info.catalyst_note.is_empty() {
                    let present = lib.catalyst_present(i, &catalysts);
                    let (txt, col) = if present {
                        ("✓ Catalyst present".to_string(), to_egui(GOOD))
                    } else {
                        (
                            format!("Needs: {}", r.info.catalyst_note),
                            to_egui(OPEN_SITE),
                        )
                    };
                    ui.label(RichText::new(txt).color(col));
                }
                if !lib.products_match(i, &outputs)
                    && ui
                        .button("Build the products")
                        .on_hover_text("Adds the product molecules to the lab as outputs")
                        .clicked()
                {
                    build_products(chem, state, lib, i, rig, effects, now);
                    arrange(chem, state, rig);
                }
            }
        });

    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Arrange in 3D")
            .on_hover_text("Lay out inputs → outputs left to right, catalyst above the arrow")
            .clicked()
        {
            arrange(chem, state, rig);
        }
        ui.checkbox(show_diagram, "Show + and → in the view");
        if ui.button("Clear reaction").clicked() {
            state.entries.clear();
        }
    });
}

pub fn build_products(
    chem: &mut Chem,
    state: &mut ReactionState,
    lib: &ReactionLibrary,
    i: usize,
    rig: &OrbitCam,
    effects: &mut Effects,
    now: f64,
) {
    let Some(r) = lib.get(i) else { return };
    let products: Vec<(MolGraph, u32)> = r.products.clone();
    let base = rig.focus + rig.transform().rotation * Vec3::X * 6.0;
    // Drop existing outputs from the reaction first (the new ones replace them).
    state.entries.retain(|e| e.role != Role::Output);
    let edit = chem.edit("build products", |lab| {
        let mut anchors = Vec::new();
        for (k, (g, c)) in products.iter().enumerate() {
            let ids = lab.add_molecule(g, base + Vec3::Y * (k as f32 * -4.0))?;
            anchors.push((ids[0], *c));
        }
        Ok(anchors)
    });
    effects.report(&edit, base, "Products built", now);
    if let Edit::Done(anchors) = edit {
        chem.refresh();
        for (anchor, c) in anchors {
            state.entries.push(Entry {
                anchor,
                role: Role::Output,
                coeff: c,
            });
        }
        // Use the library's coefficients for the inputs too.
        let input_coeffs: Vec<(u64, u32)> = r
            .info
            .reactants
            .iter()
            .filter_map(|(c, s)| {
                let g = valence_chem::smiles::parse(s).ok()?;
                let mut lab = valence_chem::Lab::new();
                let ids = lab.add_molecule(&g, Vec3::ZERO).ok()?;
                Some((
                    valence_chem::canon::structure_hash(&lab.graph(&lab.component(ids[0]))),
                    *c,
                ))
            })
            .collect();
        for e in state.entries.iter_mut().filter(|e| e.role == Role::Input) {
            if let Some(&m) = chem.atom_mol.get(e.anchor as usize)
                && let Some(mol) = chem.molecules.get(m)
            {
                let h = valence_chem::canon::structure_hash(&chem.lab.graph(&mol.atoms));
                if let Some(&(_, c)) = input_coeffs.iter().find(|(hh, _)| *hh == h) {
                    e.coeff = c;
                }
            }
        }
    }
}

/// Screen-space boxes of the reaction's molecules by role, for the diagram.
pub struct DiagramBox {
    pub role: Role,
    pub rect: egui::Rect,
    pub coeff: u32,
}

/// Draw "+" between inputs and between outputs, an arrow from inputs to
/// outputs (catalyst noted above it), coefficient badges and captions.
pub fn draw_diagram(p: &egui::Painter, boxes: &[DiagramBox]) {
    use egui::{Align2, FontId, Stroke, pos2, vec2};
    let col = Color32::from_gray(225);
    let group = |role: Role| -> Vec<&DiagramBox> {
        let mut v: Vec<&DiagramBox> = boxes.iter().filter(|b| b.role == role).collect();
        v.sort_by(|a, b| a.rect.center().x.total_cmp(&b.rect.center().x));
        v
    };
    let (inputs, outputs, cats) = (
        group(Role::Input),
        group(Role::Output),
        group(Role::Catalyst),
    );
    for g in [&inputs, &outputs] {
        for w in g.windows(2) {
            let mid = pos2(
                (w[0].rect.right() + w[1].rect.left()) / 2.0,
                (w[0].rect.center().y + w[1].rect.center().y) / 2.0,
            );
            p.text(
                mid,
                Align2::CENTER_CENTER,
                "+",
                FontId::proportional(30.0),
                col,
            );
        }
    }
    for b in boxes
        .iter()
        .filter(|b| b.coeff > 1 && b.role != Role::Catalyst)
    {
        p.text(
            b.rect.left_center() - vec2(6.0, 0.0),
            Align2::RIGHT_CENTER,
            format!("{}×", b.coeff),
            FontId::proportional(22.0),
            to_egui(OPEN_SITE),
        );
    }
    let union = |g: &[&DiagramBox]| g.iter().map(|b| b.rect).reduce(|a, b| a.union(b));
    if let (Some(i), Some(o)) = (union(&inputs), union(&outputs)) {
        let y = (i.center().y + o.center().y) / 2.0;
        // Stop short of the first output's "2×" badge, if it has one.
        let badge = outputs
            .first()
            .filter(|b| b.coeff > 1)
            .map_or(0.0, |_| 40.0);
        let (x0, x1) = (i.right() + 14.0, o.left() - 14.0 - badge);
        if x1 > x0 + 20.0 {
            let stroke = Stroke::new(4.0, col);
            p.line_segment([pos2(x0, y), pos2(x1 - 4.0, y)], stroke);
            p.line_segment([pos2(x1 - 16.0, y - 10.0), pos2(x1, y)], stroke);
            p.line_segment([pos2(x1 - 16.0, y + 10.0), pos2(x1, y)], stroke);
            if let Some(c) = union(&cats) {
                p.line_segment(
                    [
                        pos2(c.center().x, c.bottom() + 4.0),
                        pos2(c.center().x, y - 8.0),
                    ],
                    Stroke::new(1.5, Color32::from_gray(140)),
                );
                p.text(
                    pos2(c.center().x, c.top() - 6.0),
                    Align2::CENTER_BOTTOM,
                    "catalyst",
                    FontId::proportional(13.0),
                    MUTED,
                );
            }
        }
        for (rect, caption) in [(i, "Reactants"), (o, "Products")] {
            p.text(
                pos2(rect.center().x, rect.bottom() + 70.0),
                Align2::CENTER_TOP,
                caption,
                FontId::proportional(15.0),
                MUTED,
            );
        }
    }
}
