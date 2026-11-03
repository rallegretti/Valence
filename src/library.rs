//! The Library: a collapsible panel on the right edge of the 3D view with a
//! searchable list of built-in compounds plus "My molecules" (saved by the
//! user, stored in lab data files). One click adds a molecule to the lab.

use bevy::prelude::*;
use bevy_egui::egui::{self, Color32, RichText, Sense, vec2};
use valence_chem::{
    Lab, MolGraph, Namer, Vec3 as V, canon::structure_hash, compounds::KNOWN, smiles,
};

use crate::camera::OrbitCam;
use crate::labfile::SavedMolecule;
use crate::state::{Chem, Edit, Effects, Selection};

const MUTED: Color32 = Color32::from_rgb(150, 158, 175);
const PANEL_BG: Color32 = Color32::from_rgb(17, 19, 25);

/// One entry in the Library list.
pub struct CatalogEntry {
    pub name: String,
    pub iupac: String,
    pub formula: String,
    pub graph: MolGraph,
    search: String,
}

/// Lower-case, subscripts → digits, charges stripped: "H₂O" → "h2o".
fn normalize(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '₀'..='₉' => char::from_digit(c as u32 - '₀' as u32, 10),
            '⁰' | '¹' | '²' | '³' | '⁴' | '⁵' | '⁶' | '⁷' | '⁸' | '⁹' | '⁺' | '⁻' => {
                None
            }
            c => Some(c.to_ascii_lowercase()),
        })
        .collect()
}

fn entry(name: String, iupac: String, formula: String, graph: MolGraph) -> CatalogEntry {
    let search = normalize(&format!("{name} {iupac} {formula}"));
    CatalogEntry {
        name,
        iupac,
        formula,
        graph,
        search,
    }
}

/// Built-in compounds, prepared once at start-up.
#[derive(Resource)]
pub struct Catalog(pub Vec<CatalogEntry>);

impl Default for Catalog {
    fn default() -> Self {
        let namer = Namer::new();
        let mut list: Vec<CatalogEntry> = KNOWN
            .iter()
            .filter_map(|&(smi, _, _)| {
                let g = smiles::parse(smi).ok()?;
                // Name it exactly as the lab would once built.
                let mut lab = Lab::new();
                let ids = lab.add_molecule(&g, V::ZERO).ok()?;
                let n = namer.name(&lab.graph(&lab.component(*ids.first()?)));
                Some(entry(n.title, n.iupac, n.formula, g))
            })
            .collect();
        list.sort_by_key(|e| e.name.to_lowercase());
        Catalog(list)
    }
}

/// The user's own molecules (saved in lab data files).
#[derive(Resource, Default)]
pub struct MyLibrary {
    pub molecules: Vec<SavedMolecule>,
}

impl MyLibrary {
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for m in &self.molecules {
            m.name.hash(&mut h);
            for a in &m.atoms {
                (a.z, a.charge).hash(&mut h);
            }
            for b in &m.bonds {
                (b.a, b.b, b.order).hash(&mut h);
            }
        }
        h.finish()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Filter {
    #[default]
    All,
    BuiltIn,
    Mine,
}

/// Panel state.
#[derive(Resource)]
pub struct LibraryUi {
    pub open: bool,
    query: String,
    filter: Filter,
    /// Name typed for "save selected molecule".
    new_name: String,
    /// Molecule the name field was last filled from.
    name_for: Option<usize>,
    renaming: Option<(usize, String)>,
    confirm_delete: Option<usize>,
    message: Option<(String, bool)>,
    /// Formula + search text per saved molecule, rebuilt when the list changes.
    mine_cache: (u64, Vec<Option<(String, String)>>),
}

impl Default for LibraryUi {
    fn default() -> Self {
        Self {
            open: true,
            query: String::new(),
            filter: Filter::All,
            new_name: String::new(),
            name_for: None,
            renaming: None,
            confirm_delete: None,
            message: None,
            mine_cache: (0, Vec::new()),
        }
    }
}

impl LibraryUi {
    /// Set the search text (used by the feature test).
    pub fn set_query(&mut self, q: &str) {
        self.query = q.to_string();
    }
}

/// Place a molecule in a free spot near the camera focus and select it.
fn add_to_lab(
    chem: &mut Chem,
    g: &MolGraph,
    name: &str,
    rig: &OrbitCam,
    selection: &mut Selection,
    effects: &mut Effects,
    now: f64,
) {
    let right = rig.transform().rotation * V::X;
    let mut spot = rig.focus;
    for _ in 0..40 {
        let clear = chem.lab.atoms().all(|(_, a)| a.pos.distance(spot) > 6.0);
        if clear {
            break;
        }
        spot += right * 4.0;
    }
    let edit = chem.edit("library add", |lab| lab.add_molecule(g, spot));
    effects.report(&edit, spot, &format!("Added {name}"), now);
    if let Edit::Done(ids) = edit {
        selection.atom = ids.first().copied();
    }
}

/// Draws the Library on the right of `root`. Returns the width it uses.
#[allow(clippy::too_many_arguments)]
pub fn library_panel(
    root: &mut egui::Ui,
    ui_state: &mut LibraryUi,
    catalog: &Catalog,
    mine: &mut MyLibrary,
    chem: &mut Chem,
    selection: &mut Selection,
    effects: &mut Effects,
    rig: &OrbitCam,
    now: f64,
) -> f32 {
    if !ui_state.open {
        // Collapsed: a slim tab with vertical "Library" text.
        let resp = egui::Panel::right("library_tab")
            .exact_size(30.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(PANEL_BG))
            .show(root, |ui| {
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(30.0, ui.available_height()), Sense::click());
                let hovered = resp.hovered();
                ui.painter().rect_filled(
                    rect,
                    0.0,
                    if hovered {
                        Color32::from_rgb(30, 34, 44)
                    } else {
                        PANEL_BG
                    },
                );
                let galley = ui.painter().layout_no_wrap(
                    "◀  Library".into(),
                    egui::FontId::proportional(15.0),
                    if hovered {
                        Color32::WHITE
                    } else {
                        Color32::from_gray(200)
                    },
                );
                let size = galley.size();
                let pos = egui::pos2(rect.center().x - size.y / 2.0, 90.0 + size.x);
                ui.painter().add(
                    egui::epaint::TextShape::new(pos, galley, Color32::WHITE)
                        .with_angle(-std::f32::consts::FRAC_PI_2),
                );
                resp.on_hover_text("Show the Library of compounds")
            });
        if resp.inner.clicked() {
            ui_state.open = true;
        }
        return resp.response.rect.width();
    }

    let resp = egui::Panel::right("library")
        .resizable(true)
        .default_size(320.0)
        .min_size(240.0)
        .max_size(520.0)
        .frame(egui::Frame::new().fill(PANEL_BG).inner_margin(12.0))
        .show(root, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Library")
                        .size(20.0)
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("▶").on_hover_text("Hide the Library").clicked() {
                        ui_state.open = false;
                    }
                });
            });
            ui.add(
                egui::TextEdit::singleline(&mut ui_state.query)
                    .hint_text("Search name or formula (e.g. H2O)")
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                for (f, label) in [
                    (Filter::All, "All"),
                    (Filter::BuiltIn, "Built-in"),
                    (Filter::Mine, "My molecules"),
                ] {
                    ui.selectable_value(&mut ui_state.filter, f, label);
                }
            });
            ui.add_space(4.0);
            save_selected(ui, ui_state, mine, chem, selection);
            if let Some((msg, ok)) = &ui_state.message {
                let col = if *ok {
                    Color32::from_rgb(80, 220, 120)
                } else {
                    Color32::from_rgb(255, 110, 110)
                };
                ui.label(RichText::new(msg).size(12.0).color(col));
            }
            ui.separator();

            let fp = mine.fingerprint();
            if ui_state.mine_cache.0 != fp || ui_state.mine_cache.1.len() != mine.molecules.len() {
                let namer = Namer::new();
                let rows = mine
                    .molecules
                    .iter()
                    .map(|m| {
                        let formula = namer.name(&m.to_graph().ok()?).formula;
                        let search = normalize(&format!("{} {formula}", m.name));
                        Some((formula, search))
                    })
                    .collect();
                ui_state.mine_cache = (fp, rows);
            }
            let q = normalize(ui_state.query.trim());
            let matches =
                |search: &str| q.is_empty() || q.split_whitespace().all(|w| search.contains(w));
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if ui_state.filter != Filter::BuiltIn {
                        ui.label(RichText::new("My molecules").strong().color(MUTED));
                        let mut shown = 0;
                        let mut delete: Option<usize> = None;
                        let mut add: Option<usize> = None;
                        let mut rename: Option<(usize, String)> = None;
                        for (i, m) in mine.molecules.iter().enumerate() {
                            let Some(Some((formula, search))) = ui_state.mine_cache.1.get(i) else {
                                continue;
                            };
                            if !matches(search) {
                                continue;
                            }
                            let formula = formula.clone();
                            shown += 1;
                            ui.horizontal(|ui| {
                                if let Some((idx, name)) = &mut ui_state.renaming
                                    && *idx == i
                                {
                                    let r = ui
                                        .add(egui::TextEdit::singleline(name).desired_width(150.0));
                                    if ui.small_button("✓").clicked()
                                        || (r.lost_focus()
                                            && ui.input(|inp| inp.key_pressed(egui::Key::Enter)))
                                    {
                                        rename = Some((i, name.trim().to_string()));
                                    }
                                    return;
                                }
                                row(ui, &m.name, &formula, "");
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let del_label = if ui_state.confirm_delete == Some(i) {
                                            "Delete?"
                                        } else {
                                            "✕"
                                        };
                                        if ui
                                            .small_button(del_label)
                                            .on_hover_text("Remove from My molecules")
                                            .clicked()
                                        {
                                            if ui_state.confirm_delete == Some(i) {
                                                delete = Some(i);
                                            } else {
                                                ui_state.confirm_delete = Some(i);
                                            }
                                        }
                                        if ui.small_button("✎").on_hover_text("Rename").clicked()
                                        {
                                            ui_state.renaming = Some((i, m.name.clone()));
                                        }
                                        if ui
                                            .button("Add")
                                            .on_hover_text("Add to the lab")
                                            .clicked()
                                        {
                                            add = Some(i);
                                        }
                                    },
                                );
                            });
                        }
                        if let Some((i, name)) = rename {
                            ui_state.renaming = None;
                            if !name.is_empty()
                                && let Some(m) = mine.molecules.get_mut(i)
                            {
                                ui_state.message = Some((format!("Renamed to {name}"), true));
                                m.name = name;
                            }
                        }
                        if let Some(i) = add
                            && let Some(m) = mine.molecules.get(i)
                            && let Ok(g) = m.to_graph()
                        {
                            add_to_lab(chem, &g, &m.name, rig, selection, effects, now);
                        }
                        if let Some(i) = delete {
                            let name = mine.molecules.remove(i).name;
                            ui_state.confirm_delete = None;
                            ui_state.message = Some((format!("Removed {name}"), true));
                        }
                        if shown == 0 {
                            ui.label(
                                RichText::new(if mine.molecules.is_empty() {
                                    "None yet. Select a molecule in the lab and save it here."
                                } else {
                                    "No matches."
                                })
                                .size(12.0)
                                .italics()
                                .color(MUTED),
                            );
                        }
                        ui.add_space(8.0);
                    }
                    if ui_state.filter != Filter::Mine {
                        ui.label(RichText::new("Built-in compounds").strong().color(MUTED));
                        let mut add: Option<usize> = None;
                        let mut shown = 0;
                        for (i, e) in catalog.0.iter().enumerate() {
                            if !matches(&e.search) {
                                continue;
                            }
                            shown += 1;
                            ui.horizontal(|ui| {
                                let iupac = if e.iupac.to_lowercase() != e.name.to_lowercase() {
                                    e.iupac.as_str()
                                } else {
                                    ""
                                };
                                row(ui, &e.name, &e.formula, iupac);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .button("Add")
                                            .on_hover_text("Add to the lab")
                                            .clicked()
                                        {
                                            add = Some(i);
                                        }
                                    },
                                );
                            });
                        }
                        if shown == 0 {
                            ui.label(
                                RichText::new("No matches.")
                                    .size(12.0)
                                    .italics()
                                    .color(MUTED),
                            );
                        }
                        if let Some(e) = add.and_then(|i| catalog.0.get(i)) {
                            add_to_lab(chem, &e.graph, &e.name, rig, selection, effects, now);
                        }
                    }
                });
        });
    resp.response.rect.width()
}

fn row(ui: &mut egui::Ui, name: &str, formula: &str, iupac: &str) {
    ui.vertical(|ui| {
        ui.set_max_width(ui.available_width() - 70.0);
        ui.label(RichText::new(name).strong().color(Color32::WHITE));
        let detail = if iupac.is_empty() {
            formula.to_string()
        } else {
            format!("{formula} · {iupac}")
        };
        ui.label(RichText::new(detail).size(11.5).color(MUTED));
    });
}

/// "Save the selected molecule to My molecules".
fn save_selected(
    ui: &mut egui::Ui,
    ui_state: &mut LibraryUi,
    mine: &mut MyLibrary,
    chem: &Chem,
    selection: &Selection,
) {
    let Some(mol_idx) = selection
        .atom
        .and_then(|a| chem.atom_mol.get(a as usize).copied())
        .filter(|&m| m < chem.molecules.len())
    else {
        ui.label(
            RichText::new("Select a molecule in the lab to save it here.")
                .size(12.0)
                .color(MUTED),
        );
        return;
    };
    let m = &chem.molecules[mol_idx];
    if ui_state.name_for != Some(mol_idx) {
        ui_state.name_for = Some(mol_idx);
        ui_state.new_name = m.names.title.clone();
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut ui_state.new_name)
                .desired_width(ui.available_width() - 70.0)
                .hint_text("Name"),
        );
        if ui
            .button("Save")
            .on_hover_text("Save the selected molecule to My molecules")
            .clicked()
        {
            let g = chem.lab.graph(&m.atoms);
            let name = ui_state.new_name.trim();
            let hash = structure_hash(&g);
            let duplicate = mine
                .molecules
                .iter()
                .find(|x| x.to_graph().is_ok_and(|xg| structure_hash(&xg) == hash));
            ui_state.message = Some(if name.is_empty() {
                ("Give it a name first.".into(), false)
            } else if let Some(d) = duplicate {
                (format!("Already in My molecules as \"{}\".", d.name), false)
            } else {
                mine.molecules.push(SavedMolecule::from_graph(name, &g));
                (format!("Saved \"{name}\" to My molecules."), true)
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_builds_and_searches() {
        let c = Catalog::default();
        assert!(c.0.len() > 100, "catalog has {}", c.0.len());
        let find = |q: &str| {
            c.0.iter()
                .filter(|e| e.search.contains(&normalize(q)))
                .map(|e| e.name.clone())
                .collect::<Vec<_>>()
        };
        assert!(find("h2o").iter().any(|n| n == "Water"));
        assert!(find("H₂O").iter().any(|n| n == "Water"));
        assert!(find("caffeine").iter().any(|n| n == "Caffeine"));
        assert!(
            find("ethanoic")
                .iter()
                .any(|n| n.starts_with("Acetic acid")),
            "IUPAC search"
        );
    }
}
