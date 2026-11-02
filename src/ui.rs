//! egui UI: periodic table panel on the left, info cards, and 2D overlays
//! drawn over the 3D view (atom symbols, charge badges, molecule names,
//! drop feedback).

use std::sync::Arc;

use bevy::{camera::Viewport, prelude::*, window::PrimaryWindow};
use bevy_egui::{
    EguiContexts,
    egui::{
        self, Align2, Color32, CornerRadius, FontFamily, FontId, LayerId, Order, Pos2, RichText,
        Sense, Stroke, StrokeKind, UiBuilder, pos2, vec2,
    },
};
use valence_chem::{
    AtomId,
    elements::{self, Category, TablePos, category, element},
    graph::charge_superscript,
    lab::atom_radius,
    naming::Names,
};

use crate::camera::{OrbitCam, frame_all};
use crate::palette::{self, ANION, BAD, CATION, GOOD, OPEN_SITE, to_egui};
use crate::state::{Chem, Drag, Effects, Pointer, Selection, Settings, ViewRect};

const PANEL_BG: Color32 = Color32::from_rgb(17, 19, 25);
const CARD_BG: Color32 = Color32::from_rgb(26, 29, 37);
const MUTED: Color32 = Color32::from_rgb(150, 158, 175);

fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// Fonts registered with egui only become usable on the following pass, so
/// the UI waits until this has run.
#[derive(Resource, Default)]
pub struct FontsReady(u8);

pub fn setup_fonts(mut contexts: EguiContexts, mut ready: ResMut<FontsReady>) -> Result {
    if ready.0 > 0 {
        ready.0 = ready.0.saturating_add(1).min(3);
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "dejavu".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/DejaVuSans.ttf"
        ))),
    );
    fonts.font_data.insert(
        "dejavu-bold".into(),
        Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/DejaVuSans-Bold.ttf"
        ))),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "dejavu".into());
    fonts.families.insert(
        FontFamily::Name("bold".into()),
        vec!["dejavu-bold".into(), "dejavu".into()],
    );
    ctx.set_fonts(fonts);
    ctx.set_visuals(egui::Visuals::dark());
    ready.0 = 1;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn ui_system(
    mut contexts: EguiContexts,
    mut camera: Single<(&mut Camera, &GlobalTransform, &mut OrbitCam)>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut chem: ResMut<Chem>,
    mut drag: ResMut<Drag>,
    pointer: Res<Pointer>,
    mut settings: ResMut<Settings>,
    mut selection: ResMut<Selection>,
    mut effects: ResMut<Effects>,
    mut view: ResMut<ViewRect>,
    time: Res<Time>,
    ready: Res<FontsReady>,
) -> Result {
    if ready.0 < 3 {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?.clone();
    chem.refresh();
    let (cam, cam_gt, rig) = &mut *camera;

    // Drop a stale selection.
    if selection.atom.is_some_and(|a| chem.lab.atom(a).is_none()) {
        selection.atom = None;
    }

    let mut root = egui::Ui::new(
        ctx.clone(),
        "root".into(),
        UiBuilder::new()
            .layer_id(LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let panel = egui::Panel::left("periodic_table")
        .resizable(true)
        .default_size(620.0)
        .min_size(460.0)
        .max_size(1100.0)
        .frame(egui::Frame::new().fill(PANEL_BG).inner_margin(14.0))
        .show(&mut root, |ui| {
            panel(
                ui,
                &mut chem,
                &mut drag,
                &mut settings,
                &mut selection,
                &mut effects,
                rig,
                &pointer,
                time.elapsed_secs_f64(),
            );
        });
    let left = panel.response.rect.width();

    // 3D viewport = everything right of the panel.
    let scale = window.scale_factor();
    let phys_left = ((left * scale) as u32).min(window.physical_width().saturating_sub(1));
    let size = UVec2::new(
        window.physical_width().saturating_sub(phys_left).max(1),
        window.physical_height().max(1),
    );
    cam.viewport = Some(Viewport {
        physical_position: UVec2::new(phys_left, 0),
        physical_size: size,
        ..default()
    });
    view.0 = Rect::new(left, 0.0, window.width(), window.height());

    overlays(
        &ctx,
        &chem,
        cam,
        cam_gt,
        &drag,
        &pointer,
        &settings,
        &selection,
        &mut effects,
        view.0,
        time.elapsed_secs_f64(),
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Left panel
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn panel(
    ui: &mut egui::Ui,
    chem: &mut Chem,
    drag: &mut Drag,
    settings: &mut Settings,
    selection: &mut Selection,
    effects: &mut Effects,
    rig: &mut OrbitCam,
    pointer: &Pointer,
    now: f64,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Valence")
                .font(bold(26.0))
                .color(Color32::WHITE),
        );
        ui.add_space(8.0);
        ui.label(RichText::new("drag elements into the lab →").color(MUTED));
    });
    ui.add_space(8.0);

    let hovered = periodic_table(ui, chem, drag, selection, effects, rig, now);
    if hovered.is_some() {
        selection.element = hovered;
    }
    ui.add_space(10.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if let Some(z) = selection.element {
                element_card(ui, z);
                ui.add_space(8.0);
            }
            // Molecule under the mouse wins over the selected one.
            let focus_atom = pointer.hovered_atom.or(selection.atom);
            if let Some(m) = focus_atom.and_then(|a| chem.molecule_of(a)) {
                molecule_card(ui, &m.names, m.atoms.len());
                ui.add_space(8.0);
            }
            molecule_list(ui, chem, selection, rig);
            ui.add_space(8.0);
            controls(ui, chem, settings, selection, rig);
        });
}

/// Draws the 18-column table. Returns the element under the mouse.
fn periodic_table(
    ui: &mut egui::Ui,
    chem: &mut Chem,
    drag: &mut Drag,
    selection: &mut Selection,
    effects: &mut Effects,
    rig: &OrbitCam,
    now: f64,
) -> Option<u8> {
    let w = ui.available_width();
    let gap = 2.0;
    let cell = ((w - gap * 17.0) / 18.0).floor().clamp(18.0, 64.0);
    let f_gap = cell * 0.35;
    let height = 7.0 * cell + 6.0 * gap + f_gap + 2.0 * cell + gap;
    let (area, _) = ui.allocate_exact_size(vec2(w, height), Sense::hover());
    let origin = area.min;
    let painter = ui.painter_at(area);
    let mut hovered = None;

    let cell_rect = |pos: TablePos| -> egui::Rect {
        let (col, row) = match pos {
            TablePos::Main { group, period } => (group as f32 - 1.0, period as f32 - 1.0),
            TablePos::FBlock { row, col } => (col as f32 + 2.0, 7.0 + row as f32),
        };
        let y = origin.y + row * (cell + gap) + if row >= 7.0 { f_gap } else { 0.0 };
        egui::Rect::from_min_size(pos2(origin.x + col * (cell + gap), y), vec2(cell, cell))
    };

    // f-block placeholders in group 3.
    for (period, label) in [(6u8, "57–71"), (7u8, "89–103")] {
        let r = cell_rect(TablePos::Main { group: 3, period });
        painter.rect_stroke(
            r.shrink(1.0),
            CornerRadius::same(4),
            Stroke::new(1.0, Color32::from_gray(70)),
            StrokeKind::Inside,
        );
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(cell * 0.19),
            MUTED,
        );
    }

    for z in 1..=118u8 {
        let r = cell_rect(elements::table_pos(z));
        let resp = ui.interact(r, ui.id().with(("element", z)), Sense::click_and_drag());
        let base = palette::element_color(z);
        let inert = element(z).valences.is_empty();
        let mut fill = to_egui(base);
        if inert {
            fill = fill.gamma_multiply(0.55);
        }
        let text_col = if palette::luminance(base) > 0.5 && !inert {
            Color32::from_gray(15)
        } else {
            Color32::WHITE
        };
        painter.rect_filled(r, CornerRadius::same(4), fill);
        if resp.hovered() || *drag == Drag::Palette(z) {
            painter.rect_stroke(
                r,
                CornerRadius::same(4),
                Stroke::new(2.0, Color32::WHITE),
                StrokeKind::Inside,
            );
            hovered = Some(z);
        }
        painter.text(
            r.center() + vec2(0.0, cell * 0.08),
            Align2::CENTER_CENTER,
            element(z).symbol,
            bold(cell * 0.4),
            text_col,
        );
        if cell >= 26.0 {
            painter.text(
                r.min + vec2(3.0, 2.0),
                Align2::LEFT_TOP,
                z.to_string(),
                FontId::proportional(cell * 0.2),
                text_col.gamma_multiply(0.8),
            );
        }
        if resp.drag_started() {
            *drag = Drag::Palette(z);
        }
        if resp.clicked() {
            click_add(chem, selection, effects, rig, z, now);
        }
        resp.on_hover_cursor(egui::CursorIcon::Grab);
    }

    hovered
}

/// Click on a table cell: bond to the selected atom if possible, otherwise
/// drop a free atom near the camera focus.
fn click_add(
    chem: &mut Chem,
    selection: &mut Selection,
    effects: &mut Effects,
    rig: &OrbitCam,
    z: u8,
    now: f64,
) {
    if let Some(sel) = selection.atom
        && let Some(at) = chem.lab.atom(sel).map(|a| a.pos)
    {
        match chem.lab.check_attach(sel, z) {
            Ok(plan) => {
                if let Ok(id) = chem.lab.attach(sel, z, Vec3::Y) {
                    effects.push(at, plan.note, true, now);
                    // Keep building on the same atom while it has room.
                    if chem.lab.free(sel) == 0 && chem.lab.free(id) > 0 {
                        selection.atom = Some(id);
                    }
                }
                return;
            }
            Err(e) => {
                effects.push(at, e.0, false, now);
                return;
            }
        }
    }
    let n = chem.lab.atom_count() as f32;
    let jitter = Vec3::new((n * 2.4).sin(), (n * 1.7).cos(), (n * 0.9).sin()) * 1.8;
    let id = chem.lab.add_atom(z, rig.focus + jitter);
    selection.atom = Some(id);
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(CARD_BG)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(12.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

fn element_card(ui: &mut egui::Ui, z: u8) {
    let e = element(z);
    card(ui, |ui| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
            let c = palette::element_color(z);
            ui.painter()
                .rect_filled(r, CornerRadius::same(6), to_egui(c));
            let tc = if palette::luminance(c) > 0.5 {
                Color32::from_gray(15)
            } else {
                Color32::WHITE
            };
            ui.painter()
                .text(r.center(), Align2::CENTER_CENTER, e.symbol, bold(24.0), tc);
            ui.vertical(|ui| {
                ui.label(RichText::new(e.name).font(bold(18.0)).color(Color32::WHITE));
                let pos = match elements::group(z) {
                    Some(g) => format!("Group {g} · Period {}", elements::period(z)),
                    None => format!("f-block · Period {}", elements::period(z)),
                };
                ui.label(
                    RichText::new(format!(
                        "{} · Z = {} · {:.3} u",
                        category(z).label(),
                        z,
                        e.mass
                    ))
                    .color(MUTED),
                );
                ui.label(RichText::new(pos).color(MUTED));
            });
        });
        ui.add_space(4.0);
        let bonding = if e.valences.is_empty() {
            "Does not bond".to_string()
        } else {
            let v: Vec<String> = e
                .valences
                .iter()
                .filter(|&&v| v > 0)
                .map(|v| v.to_string())
                .collect();
            if category(z).is_metal() {
                format!("Gives away {} electron(s) as a cation", v.join(" or "))
            } else {
                format!("Forms {} bond(s)", v.join(" or "))
            }
        };
        ui.label(bonding);
        if e.en > 0.0 {
            ui.label(RichText::new(format!("Electronegativity {:.2}", e.en)).color(MUTED));
        }
        let note = match (category(z), z) {
            (Category::NobleGas, 36 | 54 | 86) => {
                Some("Noble gas, but heavy enough to react with F (and O for Xe).")
            }
            (Category::NobleGas, _) => Some("Noble gas: full outer shell, completely unreactive."),
            (_, 104..) => Some("Superheavy: decays in seconds, so it never forms compounds."),
            (Category::AlkaliMetal, _) => {
                Some("Very reactive metal: forms ionic salts with nonmetals.")
            }
            (Category::Halogen, _) => {
                Some("Grabs one electron: covalent with nonmetals, ionic with metals.")
            }
            _ => None,
        };
        if let Some(n) = note {
            ui.label(RichText::new(n).italics().color(MUTED));
        }
    });
}

fn status_text(n: &Names) -> (String, Color32) {
    if n.complete {
        ("✓ Complete: no open bonding sites".into(), to_egui(GOOD))
    } else {
        (
            format!(
                "● {} open bonding site{}",
                n.open_sites,
                if n.open_sites == 1 { "" } else { "s" }
            ),
            to_egui(OPEN_SITE),
        )
    }
}

fn molecule_card(ui: &mut egui::Ui, n: &Names, atoms: usize) {
    card(ui, |ui| {
        ui.label(
            RichText::new(&n.title)
                .font(bold(20.0))
                .color(Color32::WHITE),
        );
        if !n.iupac.is_empty() && n.iupac.to_lowercase() != n.title.to_lowercase() {
            ui.label(RichText::new(format!("IUPAC: {}", n.iupac)).color(Color32::from_gray(210)));
        }
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&n.formula)
                    .font(bold(18.0))
                    .color(Color32::from_rgb(170, 200, 255)),
            );
            ui.label(
                RichText::new(format!(
                    "{:.2} g/mol · {} atom{}",
                    n.mass,
                    atoms,
                    if atoms == 1 { "" } else { "s" }
                ))
                .color(MUTED),
            );
        });
        if n.charge != 0 {
            let c = if n.charge > 0 { CATION } else { ANION };
            ui.label(
                RichText::new(format!("Ion, net charge {}", signed(n.charge))).color(to_egui(c)),
            );
        }
        let (s, c) = status_text(n);
        ui.label(RichText::new(s).font(bold(14.0)).color(c));
    });
}

fn signed(q: i32) -> String {
    if q > 0 {
        format!("+{q}")
    } else {
        format!("−{}", -q)
    }
}

fn molecule_list(ui: &mut egui::Ui, chem: &Chem, selection: &mut Selection, rig: &mut OrbitCam) {
    if chem.molecules.is_empty() {
        card(ui, |ui| {
            ui.label(RichText::new("The lab is empty").font(bold(15.0)));
            ui.label(RichText::new("Drag an element from the table into the space on the right, then drop more elements onto it to build molecules. Or select an atom and click elements to bond them.").color(MUTED));
        });
        return;
    }
    ui.label(
        RichText::new(format!("In the lab ({})", chem.molecules.len()))
            .font(bold(14.0))
            .color(MUTED),
    );
    for m in &chem.molecules {
        let (icon, col) = if m.names.complete {
            ("✓", to_egui(GOOD))
        } else {
            ("●", to_egui(OPEN_SITE))
        };
        let selected = selection.atom.is_some_and(|a| m.atoms.contains(&a));
        let text = RichText::new(format!("{icon}  {}  ·  {}", m.names.title, m.names.formula))
            .color(if selected { Color32::WHITE } else { col });
        if ui.selectable_label(selected, text).clicked() {
            selection.atom = m.atoms.first().copied();
            let c = m
                .atoms
                .iter()
                .filter_map(|&a| chem.lab.atom(a))
                .map(|a| a.pos)
                .sum::<Vec3>()
                / m.atoms.len() as f32;
            rig.focus = c;
        }
    }
}

fn controls(
    ui: &mut egui::Ui,
    chem: &mut Chem,
    settings: &mut Settings,
    selection: &mut Selection,
    rig: &mut OrbitCam,
) {
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut settings.show_symbols, "Symbols");
        ui.checkbox(&mut settings.show_names, "Names");
        ui.checkbox(&mut settings.show_lone_pairs, "Lone pairs");
    });
    ui.horizontal(|ui| {
        if ui.button("Frame all (F)").clicked() {
            frame_all(rig, chem);
        }
        if ui.button("Clear lab").clicked() {
            chem.lab.clear();
            selection.atom = None;
        }
    });
    ui.add_space(6.0);
    ui.label(
        RichText::new(
            "Left-drag atom: move molecule · Shift+drag: move one atom (close rings)\n\
             Drop an atom on another: bond them · Right-drag: orbit · Middle/Shift+right: pan\n\
             Scroll: zoom · Right-click / Delete: remove atom · Esc: cancel drag",
        )
        .size(11.5)
        .color(MUTED),
    );
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        legend(ui, OPEN_SITE, "open bonding site");
        legend(ui, [0.75, 0.85, 1.0], "lone pair");
        legend(ui, CATION, "cation (+)");
        legend(ui, ANION, "anion (−)");
    });
}

fn legend(ui: &mut egui::Ui, c: palette::Rgb, text: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().circle_filled(r.center(), 4.5, to_egui(c));
    ui.label(RichText::new(text).size(11.5).color(MUTED));
}

// ---------------------------------------------------------------------------
// Overlays on the 3D view
// ---------------------------------------------------------------------------

struct Projected {
    id: AtomId,
    pos: Pos2,
    r: f32,
    depth: f32,
}

#[allow(clippy::too_many_arguments)]
fn overlays(
    ctx: &egui::Context,
    chem: &Chem,
    cam: &Camera,
    cam_gt: &GlobalTransform,
    drag: &Drag,
    pointer: &Pointer,
    settings: &Settings,
    selection: &Selection,
    effects: &mut Effects,
    view: Rect,
    now: f64,
) {
    let clip = egui::Rect::from_min_max(pos2(view.min.x, view.min.y), pos2(view.max.x, view.max.y));
    let painter = ctx
        .layer_painter(LayerId::new(Order::Middle, "scene_labels".into()))
        .with_clip_rect(clip);
    let top = ctx.layer_painter(LayerId::new(Order::Tooltip, "drag_overlay".into()));
    let cam_pos = cam_gt.translation();
    let right = cam_gt.right();
    let fwd = cam_gt.forward();

    let project = |p: Vec3| {
        cam.world_to_viewport(cam_gt, p)
            .ok()
            .map(|v| pos2(v.x, v.y))
    };
    let mut atoms: Vec<Projected> = Vec::with_capacity(chem.lab.atom_count());
    for (id, a) in chem.lab.atoms() {
        let depth = (a.pos - cam_pos).dot(*fwd);
        if depth <= 0.1 {
            continue;
        }
        let (Some(c), Some(e)) = (project(a.pos), project(a.pos + *right * atom_radius(a.z)))
        else {
            continue;
        };
        atoms.push(Projected {
            id,
            pos: c,
            r: (e - c).length(),
            depth,
        });
    }
    let screen_of = |id: AtomId| atoms.iter().find(|p| p.id == id);

    // Atom symbols and charge badges, far to near, skipping hidden ones.
    if settings.show_symbols {
        let mut order: Vec<&Projected> = atoms.iter().collect();
        order.sort_by(|a, b| b.depth.total_cmp(&a.depth));
        let check_occlusion = order.len() < 600;
        for (i, p) in order.iter().enumerate() {
            if p.r < 6.0 {
                continue;
            }
            if check_occlusion
                && order[i + 1..]
                    .iter()
                    .any(|q| q.depth < p.depth && q.pos.distance(p.pos) < q.r * 0.75)
            {
                continue;
            }
            let a = chem.lab.atom(p.id).unwrap();
            let c = palette::element_color(a.z);
            let tc = if palette::luminance(c) > 0.5 {
                Color32::from_gray(10)
            } else {
                Color32::WHITE
            };
            painter.text(
                p.pos,
                Align2::CENTER_CENTER,
                element(a.z).symbol,
                bold((p.r * 0.85).clamp(9.0, 30.0)),
                tc,
            );
            let q = chem.lab.charge(p.id);
            if q != 0 {
                let br = (p.r * 0.38).clamp(8.0, 15.0);
                let bc = p.pos + vec2(p.r * 0.8, -p.r * 0.8);
                let col = to_egui(if q > 0 { CATION } else { ANION });
                painter.circle_filled(bc, br, col);
                painter.circle_stroke(bc, br, Stroke::new(1.5, Color32::WHITE));
                let label = charge_superscript(q as i32)
                    .replace('⁺', "+")
                    .replace('⁻', "−");
                let label = label
                    .chars()
                    .map(|ch| match ch {
                        '¹' => '1',
                        '²' => '2',
                        '³' => '3',
                        '⁴' => '4',
                        '⁵' => '5',
                        '⁶' => '6',
                        '⁷' => '7',
                        '⁸' => '8',
                        c => c,
                    })
                    .collect::<String>();
                painter.text(
                    bc,
                    Align2::CENTER_CENTER,
                    label,
                    bold(br * 1.05),
                    Color32::WHITE,
                );
            }
        }
    }

    // Selection ring.
    if let Some(p) = selection.atom.and_then(screen_of) {
        painter.circle_stroke(
            p.pos,
            p.r + 5.0,
            Stroke::new(1.5, Color32::from_white_alpha(170)),
        );
    }

    // Molecule name tags under each molecule.
    if settings.show_names {
        for m in &chem.molecules {
            let pts: Vec<&Projected> = m.atoms.iter().filter_map(|&a| screen_of(a)).collect();
            if pts.is_empty() {
                continue;
            }
            let cx = pts.iter().map(|p| p.pos.x).sum::<f32>() / pts.len() as f32;
            let bottom = pts.iter().map(|p| p.pos.y + p.r).fold(f32::MIN, f32::max);
            name_tag(&painter, ctx, &m.names, pos2(cx, bottom + 12.0));
        }
    }

    // Drag feedback.
    if let Some(cursor) = pointer.pos.map(|p| pos2(p.x, p.y)) {
        let dragging = matches!(drag, Drag::Palette(_) | Drag::Atom { .. });
        if dragging
            && let (Some(t), Some(check)) = (
                pointer.hovered_atom.and_then(screen_of),
                &pointer.drop_check,
            )
        {
            let (col, text) = match check {
                Ok(s) => (to_egui(GOOD), format!("✓ {s}")),
                Err(s) => (to_egui(BAD), format!("✗ {s}")),
            };
            let pulse = 1.0 + 0.08 * ((now * 8.0).sin() as f32);
            top.circle_stroke(t.pos, (t.r + 8.0) * pulse, Stroke::new(3.0, col));
            tooltip(&top, ctx, cursor + vec2(18.0, 18.0), &text, col);
        } else if let Drag::Palette(z) = drag
            && pointer.in_scene
        {
            tooltip(
                &top,
                ctx,
                cursor + vec2(18.0, 18.0),
                &format!("Drop to add a free {} atom", element(*z).name),
                MUTED,
            );
        } else if !dragging
            && pointer.in_scene
            && let Some(id) = pointer.hovered_atom
            && let Some(a) = chem.lab.atom(id)
        {
            let free = chem.lab.free(id);
            let lp = chem.lab.lone_pairs(id);
            let q = chem.lab.charge(id);
            let mut s = element(a.z).name.to_string();
            if q != 0 {
                s.push_str(&format!(" ion ({})", signed(q as i32)));
            }
            s.push_str(&format!(
                " · {free} open site{}",
                if free == 1 { "" } else { "s" }
            ));
            if lp > 0 {
                s.push_str(&format!(
                    " · {lp} lone pair{}",
                    if lp == 1 { "" } else { "s" }
                ));
            }
            tooltip(
                &top,
                ctx,
                cursor + vec2(18.0, 18.0),
                &s,
                Color32::from_gray(220),
            );
        }

        if let Drag::Palette(z) = drag {
            let c = palette::element_color(*z);
            top.circle_filled(cursor, 20.0, to_egui(c).gamma_multiply(0.9));
            top.circle_stroke(cursor, 20.0, Stroke::new(2.0, Color32::WHITE));
            let tc = if palette::luminance(c) > 0.5 {
                Color32::from_gray(10)
            } else {
                Color32::WHITE
            };
            top.text(
                cursor,
                Align2::CENTER_CENTER,
                element(*z).symbol,
                bold(16.0),
                tc,
            );
        }
    }

    // Success / failure bursts.
    effects.0.retain(|e| now - e.born < 2.2);
    for e in &effects.0 {
        let Some(p) = project(e.at) else { continue };
        let age = (now - e.born) as f32;
        let k = (age / 2.2).clamp(0.0, 1.0);
        let col = to_egui(if e.ok { GOOD } else { BAD });
        if age < 0.6 {
            let ring = age / 0.6;
            top.circle_stroke(
                p,
                12.0 + 40.0 * ring,
                Stroke::new(3.0 * (1.0 - ring), col.gamma_multiply(1.0 - ring)),
            );
        }
        let alpha = 1.0 - k * k;
        tooltip(
            &top,
            ctx,
            p + vec2(0.0, -30.0 - 25.0 * k),
            &e.text,
            col.gamma_multiply(alpha),
        );
    }
}

fn tooltip(p: &egui::Painter, ctx: &egui::Context, at: Pos2, text: &str, col: Color32) {
    let galley =
        ctx.fonts_mut(|f| f.layout(text.to_string(), FontId::proportional(13.5), col, 340.0));
    let rect = egui::Rect::from_min_size(at, galley.size()).expand(6.0);
    p.rect_filled(
        rect,
        CornerRadius::same(6),
        Color32::from_black_alpha(215).gamma_multiply(col.a() as f32 / 255.0),
    );
    p.galley(rect.min + vec2(6.0, 6.0), galley, col);
}

fn name_tag(p: &egui::Painter, ctx: &egui::Context, n: &Names, top_center: Pos2) {
    let mut job = egui::text::LayoutJob::default();
    let fmt = |font: FontId, color: Color32| egui::TextFormat {
        font_id: font,
        color,
        ..Default::default()
    };
    job.append(&n.title, 0.0, fmt(bold(14.0), Color32::WHITE));
    let mut second = n.formula.clone();
    if !n.iupac.is_empty() && n.iupac.to_lowercase() != n.title.to_lowercase() {
        second.push_str(&format!("  ·  {}", n.iupac));
    }
    job.append(
        &format!("\n{second}"),
        0.0,
        fmt(FontId::proportional(12.0), Color32::from_gray(190)),
    );
    let (status, col) = if n.complete {
        ("✓ complete".to_string(), to_egui(GOOD))
    } else {
        (
            format!(
                "● {} open site{}",
                n.open_sites,
                if n.open_sites == 1 { "" } else { "s" }
            ),
            to_egui(OPEN_SITE),
        )
    };
    job.append(&format!("\n{status}"), 0.0, fmt(bold(11.5), col));
    job.halign = egui::Align::Center;
    let galley = ctx.fonts_mut(|f| f.layout_job(job));
    let size = galley.size();
    let rect = egui::Rect::from_min_size(pos2(top_center.x - size.x / 2.0, top_center.y), size)
        .expand2(vec2(8.0, 5.0));
    p.rect_filled(rect, CornerRadius::same(6), Color32::from_black_alpha(150));
    p.galley(pos2(top_center.x, top_center.y), galley, Color32::WHITE);
}
