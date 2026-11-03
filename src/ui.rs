//! egui UI: periodic table panel on the left, info cards, and 2D overlays
//! drawn over the 3D view (atom symbols, charge badges, molecule names,
//! drop feedback).

use std::sync::Arc;

use bevy::{
    camera::SubCameraView, ecs::system::SystemParam, prelude::*,
    render::renderer::RenderAdapterInfo, window::PrimaryWindow,
};
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
use crate::export::{DetailsCard, ExportJob};
use crate::palette::{self, ANION, BAD, CATION, GOOD, OPEN_SITE, to_egui};
use crate::quality::{GraphicsQuality, Phase, Quality, QualitySetting, RenderStats, Startup};
use crate::reactions_ui::{DiagramBox, ReactionLib, ReactionState, Role, draw_diagram};
use crate::state::{
    Chem, Drag, Edit, Effects, ExportTarget, Isolation, Pointer, Selection, Settings, Tab, UiState,
    ViewRect,
};

const PANEL_BG: Color32 = Color32::from_rgb(17, 19, 25);
const CARD_BG: Color32 = Color32::from_rgb(26, 29, 37);
const MUTED: Color32 = Color32::from_rgb(150, 158, 175);

/// Snap a computed font size to a small set of sizes. egui caches glyphs per
/// size in one texture atlas and rebuilds the whole atlas when it fills up;
/// while it rebuilds, the entire interface disappears for a frame. Labels
/// whose size follows zoom/rotation would mint new sizes every frame and
/// cause exactly that flicker, so sizes come from this fixed ladder.
fn snap_font(size: f32) -> f32 {
    if !size.is_finite() {
        return 12.0;
    }
    if size < 12.0 {
        size.round().max(6.0)
    } else {
        ((size / 2.0).round() * 2.0).min(64.0)
    }
}

fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// Fonts registered with egui only become usable on the following pass, so
/// the UI waits until this has run.
#[derive(Resource, Default)]
pub struct FontsReady(u8);

impl FontsReady {
    pub fn is_ready(&self) -> bool {
        self.0 >= 3
    }
}

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

/// Everything the UI reads or changes, grouped to stay within Bevy's
/// system-parameter limit.
#[derive(SystemParam)]
pub struct UiRes<'w> {
    chem: ResMut<'w, Chem>,
    drag: ResMut<'w, Drag>,
    pointer: Res<'w, Pointer>,
    settings: ResMut<'w, Settings>,
    selection: ResMut<'w, Selection>,
    effects: ResMut<'w, Effects>,
    view: ResMut<'w, ViewRect>,
    time: Res<'w, Time>,
    quality: ResMut<'w, GraphicsQuality>,
    startup: ResMut<'w, Startup>,
    stats: Res<'w, RenderStats>,
    state: ResMut<'w, UiState>,
    reaction: ResMut<'w, ReactionState>,
    library: Res<'w, ReactionLib>,
    export: ResMut<'w, ExportJob>,
    isolation: Res<'w, Isolation>,
    adapter: Option<Res<'w, RenderAdapterInfo>>,
}

/// The Library panel and the open lab document.
#[derive(SystemParam)]
pub struct LibRes<'w> {
    catalog: Res<'w, crate::library::Catalog>,
    mine: ResMut<'w, crate::library::MyLibrary>,
    lib_ui: ResMut<'w, crate::library::LibraryUi>,
    doc: ResMut<'w, crate::document::Document>,
}

pub fn ui_system(
    mut contexts: EguiContexts,
    mut camera: Single<(&mut Camera, &GlobalTransform, &mut OrbitCam)>,
    window: Single<&Window, With<PrimaryWindow>>,
    ready: Res<FontsReady>,
    r: UiRes,
    lib: LibRes,
    mut fades: Local<std::collections::HashMap<AtomId, f32>>,
) -> Result {
    if ready.0 < 3 {
        return Ok(());
    }
    let UiRes {
        mut chem,
        mut drag,
        pointer,
        mut settings,
        mut selection,
        mut effects,
        mut view,
        time,
        mut quality,
        mut startup,
        stats,
        mut state,
        mut reaction,
        library,
        mut export,
        isolation,
        adapter,
    } = r;
    let LibRes {
        catalog,
        mut mine,
        mut lib_ui,
        mut doc,
    } = lib;
    let now = time.elapsed_secs_f64();
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
    // Keep at least ~240 px of 3D view however narrow the window gets.
    let max_panel = (window.width() - 240.0).clamp(300.0, 1100.0);
    let panel = egui::Panel::left("periodic_table")
        .resizable(true)
        .default_size(620.0f32.min(max_panel))
        .min_size(300.0f32.min(max_panel))
        .max_size(max_panel)
        .frame(egui::Frame::new().fill(PANEL_BG).inner_margin(14.0))
        .show(&mut root, |ui| {
            // Header: ☰ menu, title, tabs.
            ui.horizontal(|ui| {
                crate::menu::menu_button(
                    ui,
                    &mut chem,
                    &mut state,
                    &mut selection,
                    &mut effects,
                    &mut doc,
                    now,
                );
                ui.label(
                    RichText::new("Valence")
                        .font(bold(26.0))
                        .color(Color32::WHITE),
                );
                ui.add_space(8.0);
                ui.label(RichText::new("drag elements into the lab →").color(MUTED));
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (Tab::Build, "Build"),
                    (Tab::Reactions, "Reactions"),
                    (Tab::Export, "Export"),
                ] {
                    let selected = state.tab == tab;
                    let text = RichText::new(label).size(15.0).strong();
                    if ui
                        .add(egui::Button::selectable(selected, text).min_size(vec2(96.0, 28.0)))
                        .clicked()
                    {
                        state.tab = tab;
                    }
                }
            });
            ui.add_space(8.0);
            let mut gfx = Gfx {
                quality: &mut quality,
                startup: &mut startup,
                compiling: stats.compiling(),
            };
            match state.tab {
                Tab::Build => build_tab(
                    ui,
                    &mut chem,
                    &mut drag,
                    &mut settings,
                    &mut selection,
                    &mut effects,
                    rig,
                    &pointer,
                    now,
                    &mut gfx,
                ),
                Tab::Reactions => {
                    // The periodic table stays handy for building inputs.
                    let hovered = periodic_table(
                        ui,
                        &mut chem,
                        &mut drag,
                        &mut selection,
                        &mut effects,
                        rig,
                        now,
                    );
                    if hovered.is_some() {
                        selection.element = hovered;
                    }
                    ui.add_space(8.0);
                    let mut show = state.show_reaction_diagram;
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            crate::reactions_ui::reactions_tab(
                                ui,
                                &mut chem,
                                &mut reaction,
                                &library.0,
                                &mut selection,
                                &mut effects,
                                rig,
                                &mut show,
                                now,
                            );
                        });
                    state.show_reaction_diagram = show;
                }
                Tab::Export => {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            crate::export::export_tab(
                                ui,
                                &mut state,
                                &mut export,
                                &chem,
                                &selection,
                                &reaction,
                            );
                        });
                }
            }
        });
    let left = panel.response.rect.width();
    let right = crate::library::library_panel(
        &mut root,
        &mut lib_ui,
        &catalog,
        &mut mine,
        &mut chem,
        &mut selection,
        &mut effects,
        rig,
        now,
    );

    crate::menu::settings_window(&ctx, &mut state, &mut quality, adapter.as_deref());
    crate::menu::help_window(&ctx, &mut state);
    crate::menu::about_window(&ctx, &mut state);
    delete_prompt(&ctx, &mut chem, &mut effects, &mut selection, now);

    // 3D view = the area between the periodic table and the Library.
    let scale = window.scale_factor();
    let (pw, ph) = (window.physical_width(), window.physical_height());
    let phys = |w: f32| {
        if w.is_finite() {
            (w.max(0.0) * scale) as u32
        } else {
            0
        }
    };
    let (pl, pr) = (phys(left), phys(right));
    cam.viewport = None;
    cam.sub_camera_view = centred_projection(pw, ph, pl, pr);
    let w = window.width();
    let l = left.clamp(0.0, w);
    let r_edge = (w - right.max(0.0)).clamp(l, w);
    view.0 = Rect::new(l, 0.0, r_edge, window.height());
    if view.0.height() > 1.0 {
        let aspect = view.0.width() / view.0.height();
        if (rig.view_aspect - aspect).abs() > 0.01 {
            rig.view_aspect = aspect;
        }
    }
    crate::document::document_ui(
        &ctx,
        &mut doc,
        egui::Rect::from_min_max(pos2(view.0.min.x, 0.0), pos2(view.0.max.x, view.0.max.y)),
        now,
    );

    // The reaction diagram shows on the Reactions tab (or in a reaction export).
    let diagram = ((state.tab == Tab::Reactions && state.show_reaction_diagram)
        || (export.capturing() && state.export_target == ExportTarget::Reaction))
        .then(|| {
            reaction
                .parts(&chem)
                .into_iter()
                .map(|p| (p.role, chem.molecules[p.molecule].atoms.clone(), p.coeff))
                .collect::<Vec<_>>()
        });
    let gfx = Gfx {
        quality: &mut quality,
        startup: &mut startup,
        compiling: stats.compiling(),
    };
    let mut scene = SceneOverlay {
        isolation: &isolation,
        capturing: export.capturing(),
        card: export.card.as_ref(),
        diagram,
        fades: &mut fades,
        dt: time.delta_secs(),
    };
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
        now,
        &gfx,
        &mut scene,
    );
    Ok(())
}

/// "Remove just this atom or its whole molecule?", shown by the cursor.
fn delete_prompt(
    ctx: &egui::Context,
    chem: &mut Chem,
    effects: &mut Effects,
    selection: &mut Selection,
    now: f64,
) {
    let Some((atom, at)) = selection.delete_ask else {
        return;
    };
    let Some(z) = chem.lab.atom(atom).map(|a| a.z) else {
        selection.delete_ask = None; // gone (e.g. undone)
        return;
    };
    let element_name = element(z).name;
    let (mol_name, count) = chem.molecule_of(atom).map_or_else(
        || ("its molecule".to_string(), chem.lab.component(atom).len()),
        |m| (m.names.title.clone(), m.atoms.len()),
    );
    #[derive(Clone, Copy)]
    enum Choice {
        Cancel,
        Atom,
        Molecule,
    }
    let mut choice = None;
    let area = egui::Area::new("delete_ask".into())
        .order(Order::Foreground)
        .fixed_pos(pos2(at.x + 10.0, at.y + 10.0))
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(250.0);
                ui.label(
                    RichText::new(format!("Remove {element_name}?"))
                        .strong()
                        .size(15.0)
                        .color(Color32::WHITE),
                );
                ui.label(
                    RichText::new(format!("It's part of {mol_name}."))
                        .size(12.0)
                        .color(MUTED),
                );
                ui.add_space(4.0);
                let w = ui.available_width();
                let button = |text: String, key: &str| {
                    egui::Button::new(text)
                        .shortcut_text(key)
                        .min_size(vec2(w, 26.0))
                };
                if ui
                    .add(button(
                        format!("Just this {} atom", element_name.to_lowercase()),
                        "A",
                    ))
                    .clicked()
                {
                    choice = Some(Choice::Atom);
                }
                if ui
                    .add(button(format!("Whole molecule ({count} atoms)"), "M"))
                    .clicked()
                {
                    choice = Some(Choice::Molecule);
                }
                if ui.add(button("Cancel".into(), "Esc")).clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
        });
    if choice.is_none() {
        let typing = ctx.egui_wants_keyboard_input();
        choice = ctx.input(|i| {
            if i.key_pressed(egui::Key::Escape) {
                Some(Choice::Cancel)
            } else if !typing && i.key_pressed(egui::Key::A) {
                Some(Choice::Atom)
            } else if !typing && i.key_pressed(egui::Key::M) {
                Some(Choice::Molecule)
            } else if i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| !area.response.rect.contains(p))
            {
                // Clicking anywhere else dismisses it.
                Some(Choice::Cancel)
            } else {
                None
            }
        });
    }
    match choice {
        None => {}
        Some(c) => {
            selection.delete_ask = None;
            match c {
                Choice::Cancel => {}
                Choice::Atom => crate::interact::delete_atom(chem, effects, selection, atom, now),
                Choice::Molecule => {
                    crate::interact::delete_molecule(chem, effects, selection, atom, now)
                }
            }
        }
    }
}

/// Shift the 3D projection so the scene is centred in the area between the
/// two side panels: the window is rendered as part of a wider virtual view
/// whose centre is the centre of that area. Same scale as a viewport, but the
/// interface and 3D view are drawn by one camera in one pass. A minimised or
/// degenerate window gets no sub-view (renders full-window, invisible anyway).
fn centred_projection(pw: u32, ph: u32, left: u32, right: u32) -> Option<SubCameraView> {
    if ph <= 1 || left.saturating_add(right).saturating_add(1) >= pw {
        return None;
    }
    // Twice the centre of the 3D area, in physical pixels.
    let c2 = left + (pw - right);
    Some(if c2 >= pw {
        // Centre right of the window's middle: the window is the left part.
        SubCameraView {
            full_size: UVec2::new(c2, ph),
            offset: Vec2::ZERO,
            size: UVec2::new(pw, ph),
        }
    } else {
        // Centre left of middle: the window is the right part.
        let full = 2 * pw - c2;
        SubCameraView {
            full_size: UVec2::new(full, ph),
            offset: Vec2::new((full - pw) as f32, 0.0),
            size: UVec2::new(pw, ph),
        }
    })
}

/// Extra overlay inputs: export isolation/card and the reaction diagram.
struct SceneOverlay<'a> {
    isolation: &'a Isolation,
    capturing: bool,
    card: Option<&'a DetailsCard>,
    diagram: Option<Vec<(Role, Vec<AtomId>, u32)>>,
    /// Per-atom label opacity, eased over time so labels fade rather than
    /// pop as atoms pass in front of each other.
    fades: &'a mut std::collections::HashMap<AtomId, f32>,
    dt: f32,
}

/// Graphics quality + startup progress, passed to the UI pieces.
struct Gfx<'a> {
    quality: &'a mut GraphicsQuality,
    startup: &'a mut Startup,
    /// Shader pipelines still compiling.
    compiling: usize,
}

// ---------------------------------------------------------------------------
// Left panel
// ---------------------------------------------------------------------------

fn build_tab(
    ui: &mut egui::Ui,
    chem: &mut Chem,
    drag: &mut Drag,
    settings: &mut Settings,
    selection: &mut Selection,
    effects: &mut Effects,
    rig: &mut OrbitCam,
    pointer: &Pointer,
    now: f64,
    gfx: &mut Gfx,
) {
    let hovered = periodic_table(ui, chem, drag, selection, effects, rig, now);
    if hovered.is_some() {
        selection.element = hovered;
    }
    ui.add_space(10.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            slow_start_hint(ui, gfx);
            if let Some(z) = selection.element {
                element_card(ui, z);
                ui.add_space(8.0);
            }
            // Molecule under the mouse wins over the selected one.
            let focus_atom = pointer.hovered_atom.or(selection.atom);
            if let Some(m) = focus_atom.and_then(|a| chem.molecule_of(a)) {
                molecule_card(ui, &m.names, &chem.lab.graph(&m.atoms));
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
            FontId::proportional(snap_font(cell * 0.19)),
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
            bold(snap_font(cell * 0.4)),
            text_col,
        );
        if cell >= 26.0 {
            painter.text(
                r.min + vec2(3.0, 2.0),
                Align2::LEFT_TOP,
                z.to_string(),
                FontId::proportional(snap_font(cell * 0.2)),
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
        let note = chem
            .lab
            .check_attach(sel, z)
            .map(|p| p.note)
            .unwrap_or_default();
        let edit = chem.edit("attach", |lab| lab.attach(sel, z, Vec3::Y));
        effects.report(&edit, at, &note, now);
        // Keep building on the same atom while it has room.
        if let Edit::Done(id) = edit
            && chem.lab.free(sel) == 0
            && chem.lab.free(id) > 0
        {
            selection.atom = Some(id);
        }
        return;
    }
    let n = chem.lab.atom_count() as f32;
    let jitter = Vec3::new((n * 2.4).sin(), (n * 1.7).cos(), (n * 0.9).sin()) * 1.8;
    let p = rig.focus + jitter;
    match chem.edit("add", |lab| lab.add_atom(z, p)) {
        Edit::Done(id) => selection.atom = Some(id),
        other => effects.report(&other, p, "", now),
    }
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

fn molecule_card(ui: &mut egui::Ui, n: &Names, g: &valence_chem::MolGraph) {
    card(ui, |ui| {
        ui.label(
            RichText::new(&n.title)
                .font(bold(20.0))
                .color(Color32::WHITE),
        );
        if !n.iupac.is_empty()
            && n.iupac.to_lowercase() != n.title.to_lowercase()
            && n.common.is_none()
        {
            ui.label(RichText::new(format!("IUPAC: {}", n.iupac)).color(Color32::from_gray(210)));
        }
        ui.label(
            RichText::new(&n.formula)
                .font(bold(18.0))
                .color(Color32::from_rgb(170, 200, 255)),
        );
        egui::Grid::new("molecule_facts")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                for (k, v) in crate::facts::molecule_facts(n, g) {
                    if k == "Formula" {
                        continue;
                    }
                    ui.label(RichText::new(k).size(12.5).color(MUTED));
                    ui.label(RichText::new(v).size(12.5));
                    ui.end_row();
                }
            });
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
        if ui
            .add_enabled(chem.can_undo(), egui::Button::new("↶ Undo"))
            .on_hover_text("Ctrl/Cmd+Z")
            .clicked()
        {
            chem.undo();
        }
        if ui
            .add_enabled(chem.can_redo(), egui::Button::new("↷ Redo"))
            .on_hover_text("Ctrl/Cmd+Shift+Z")
            .clicked()
        {
            chem.redo();
        }
        if ui
            .add_enabled(!chem.lab.is_empty(), egui::Button::new("Clear lab"))
            .clicked()
        {
            let _ = chem.edit("clear", |lab| {
                lab.clear();
                Ok(())
            });
            selection.atom = None;
        }
    });
    ui.add_space(6.0);
    ui.label(
        RichText::new("Full controls: ☰ menu → Help → Controls")
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
    gfx: &Gfx,
    scene: &mut SceneOverlay,
) {
    let capturing = scene.capturing;
    let clip = egui::Rect::from_min_max(pos2(view.min.x, view.min.y), pos2(view.max.x, view.max.y));
    preparing_indicator(ctx, gfx, clip, now);
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
        if !scene.isolation.shows(id) {
            continue;
        }
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
        // Ease towards the target opacity (~0.1 s) so labels never flicker.
        let ease = 1.0 - (-scene.dt.max(0.0) * 14.0).exp();
        let live: std::collections::HashSet<AtomId> = order.iter().map(|p| p.id).collect();
        scene.fades.retain(|id, _| live.contains(id));
        for (i, p) in order.iter().enumerate() {
            // How covered is this atom's centre by nearer atoms? 0 = clear, 1 = hidden.
            let covered = if check_occlusion {
                order[i + 1..]
                    .iter()
                    .filter(|q| q.depth < p.depth)
                    .map(|q| (1.0 - q.pos.distance(p.pos) / (q.r * 1.05)).clamp(0.0, 1.0))
                    .fold(0.0f32, f32::max)
            } else {
                0.0
            };
            let size_ok = ((p.r - 5.0) / 3.0).clamp(0.0, 1.0);
            let target = (1.0 - covered * 2.2).clamp(0.0, 1.0) * size_ok;
            let alpha = scene.fades.entry(p.id).or_insert(target);
            *alpha += (target - *alpha) * if scene.capturing { 1.0 } else { ease };
            let alpha = *alpha;
            if alpha < 0.03 {
                continue;
            }
            let Some(a) = chem.lab.atom(p.id) else {
                continue;
            };
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
                bold(snap_font((p.r * 0.85).clamp(9.0, 30.0))),
                tc.gamma_multiply(alpha),
            );
            let q = chem.lab.charge(p.id);
            // Lite has no translucent 3D halos, so ring ions here instead.
            if q != 0 && gfx.quality.active == Quality::Lite {
                let col = to_egui(if q > 0 { CATION } else { ANION }).gamma_multiply(alpha);
                painter.circle_stroke(
                    p.pos,
                    p.r * 1.3,
                    Stroke::new((p.r * 0.12).clamp(1.5, 4.0), col),
                );
            }
            if q != 0 {
                let br = (p.r * 0.38).clamp(8.0, 15.0);
                let bc = p.pos + vec2(p.r * 0.8, -p.r * 0.8);
                let col = to_egui(if q > 0 { CATION } else { ANION }).gamma_multiply(alpha);
                painter.circle_filled(bc, br, col);
                painter.circle_stroke(
                    bc,
                    br,
                    Stroke::new(1.5, Color32::WHITE.gamma_multiply(alpha)),
                );
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
                    bold(snap_font(br * 1.05)),
                    Color32::WHITE.gamma_multiply(alpha),
                );
            }
        }
    }

    // Selection ring.
    if !capturing && let Some(p) = selection.atom.and_then(screen_of) {
        painter.circle_stroke(
            p.pos,
            p.r + 5.0,
            Stroke::new(1.5, Color32::from_white_alpha(170)),
        );
    }

    // Molecule name tags under each molecule.
    // Placed in priority order (focused molecule, then nearest first); a tag
    // that would overlap one already placed is skipped to avoid clutter.
    if settings.show_names {
        let focus = pointer
            .hovered_atom
            .or(selection.atom)
            .and_then(|a| chem.atom_mol.get(a as usize).copied());
        let mut tags: Vec<(bool, f32, usize, Pos2)> = Vec::new();
        for (i, m) in chem.molecules.iter().enumerate() {
            let pts: Vec<&Projected> = m.atoms.iter().filter_map(|&a| screen_of(a)).collect();
            if pts.is_empty() {
                continue;
            }
            let cx = pts.iter().map(|p| p.pos.x).sum::<f32>() / pts.len() as f32;
            let bottom = pts.iter().map(|p| p.pos.y + p.r).fold(f32::MIN, f32::max);
            let depth = pts.iter().map(|p| p.depth).fold(f32::MAX, f32::min);
            let cy = pts.iter().map(|p| p.pos.y).sum::<f32>() / pts.len() as f32;
            if !clip.contains(pos2(cx, cy)) {
                continue; // molecule is (mostly) off-screen
            }
            tags.push((focus != Some(i), depth, i, pos2(cx, bottom + 12.0)));
        }
        tags.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut placed: Vec<egui::Rect> = Vec::with_capacity(tags.len());
        for (_, _, i, at) in tags {
            if let Some(m) = chem.molecules.get(i) {
                name_tag(&painter, ctx, &m.names, at, clip, &mut placed);
            }
        }
    }

    // Reaction diagram (+, arrow, coefficients, captions).
    if let Some(parts) = &scene.diagram {
        let boxes: Vec<DiagramBox> = parts
            .iter()
            .filter_map(|(role, atoms, coeff)| {
                let pts: Vec<&Projected> = atoms.iter().filter_map(|&a| screen_of(a)).collect();
                let rect = pts
                    .iter()
                    .map(|p| egui::Rect::from_center_size(p.pos, vec2(p.r * 2.0, p.r * 2.0)))
                    .reduce(|a, b| a.union(b))?;
                Some(DiagramBox {
                    role: *role,
                    rect,
                    coeff: *coeff,
                })
            })
            .collect();
        draw_diagram(&painter, &boxes);
    }

    // Details card on exported images.
    if let Some(card) = scene.card {
        details_card(&painter, ctx, clip, card);
    }

    // Drag feedback and hover tooltips (never in exported images).
    if let Some(cursor) = pointer.pos.map(|p| pos2(p.x, p.y)).filter(|_| !capturing) {
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
    for e in effects.0.iter().filter(|_| !capturing) {
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

fn name_tag(
    p: &egui::Painter,
    ctx: &egui::Context,
    n: &Names,
    top_center: Pos2,
    clip: egui::Rect,
    placed: &mut Vec<egui::Rect>,
) {
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
    let mut rect = egui::Rect::from_min_size(pos2(top_center.x - size.x / 2.0, top_center.y), size)
        .expand2(vec2(8.0, 5.0));
    // Nudge the tag fully inside the 3D view.
    let dx =
        (clip.left() + 4.0 - rect.left()).max(0.0) + (clip.right() - 4.0 - rect.right()).min(0.0);
    let dy = (clip.bottom() - 4.0 - rect.bottom()).min(0.0);
    rect = rect.translate(vec2(dx, dy));
    let top_center = top_center + vec2(dx, dy);
    if placed.iter().any(|r| r.intersects(rect)) {
        return;
    }
    placed.push(rect);
    p.rect_filled(rect, CornerRadius::same(6), Color32::from_black_alpha(150));
    p.galley(pos2(top_center.x, top_center.y), galley, Color32::WHITE);
}

// ---------------------------------------------------------------------------
// Graphics quality & startup progress
// ---------------------------------------------------------------------------

/// "Preparing 3D graphics…" in the middle of the 3D view while shaders compile.
fn preparing_indicator(ctx: &egui::Context, gfx: &Gfx, clip: egui::Rect, now: f64) {
    if gfx.startup.phase == Phase::Ready || clip.width() < 50.0 {
        return;
    }
    let p = ctx
        .layer_painter(LayerId::new(Order::Middle, "preparing".into()))
        .with_clip_rect(clip);
    let c = clip.center();
    // Spinner: a bright dot circling the centre.
    let a = (now * 3.0) as f32;
    p.circle_stroke(
        c - vec2(0.0, 30.0),
        14.0,
        Stroke::new(2.0, Color32::from_gray(60)),
    );
    p.circle_filled(
        c - vec2(0.0, 30.0) + vec2(a.cos(), a.sin()) * 14.0,
        4.0,
        Color32::from_gray(220),
    );
    p.text(
        c + vec2(0.0, 6.0),
        Align2::CENTER_CENTER,
        "Preparing 3D graphics…",
        bold(16.0),
        Color32::from_gray(225),
    );
    let detail = if gfx.compiling > 0 {
        format!(
            "compiling {} shader{}, first launch on older GPUs can take a while",
            gfx.compiling,
            if gfx.compiling == 1 { "" } else { "s" }
        )
    } else {
        "almost there".to_string()
    };
    p.text(
        c + vec2(0.0, 28.0),
        Align2::CENTER_CENTER,
        detail,
        FontId::proportional(12.5),
        MUTED,
    );
}

/// Offer Lite if Full took a long time to show the 3D scene on this computer.
fn slow_start_hint(ui: &mut egui::Ui, gfx: &mut Gfx) {
    const SLOW_SECS: f64 = 10.0;
    let Some(secs) = gfx.startup.scene_secs() else {
        return;
    };
    if gfx.quality.active != Quality::Full
        || secs < SLOW_SECS
        || gfx.startup.slow_hint_dismissed
        || gfx.quality.saved == QualitySetting::Lite
    {
        return;
    }
    card(ui, |ui| {
        ui.label(
            RichText::new("Slow graphics start-up")
                .font(bold(15.0))
                .color(to_egui(OPEN_SITE)),
        );
        ui.label(format!(
            "The 3D view took {secs:.0} s to appear on this computer. Lite graphics start much faster \
             and keep every feature; only some glow and transparency effects are simplified."
        ));
        ui.horizontal(|ui| {
            if ui.button("Use Lite graphics from next launch").clicked() {
                gfx.quality.save(QualitySetting::Lite);
            }
            if ui.button("Dismiss").clicked() {
                gfx.startup.slow_hint_dismissed = true;
            }
        });
    });
    ui.add_space(8.0);
}

/// The card drawn on exported images: title, subtitle and label/value rows.
fn details_card(p: &egui::Painter, ctx: &egui::Context, clip: egui::Rect, card: &DetailsCard) {
    let width = (clip.width() * 0.42).clamp(260.0, 440.0);
    let mut job = egui::text::LayoutJob::default();
    let fmt = |font: FontId, color: Color32| egui::TextFormat {
        font_id: font,
        color,
        ..Default::default()
    };
    job.wrap.max_width = width - 28.0;
    job.append(&card.title, 0.0, fmt(bold(21.0), Color32::WHITE));
    if !card.subtitle.is_empty() {
        job.append(
            &format!("\n{}", card.subtitle),
            0.0,
            fmt(bold(16.0), Color32::from_rgb(170, 200, 255)),
        );
    }
    for (k, v) in &card.rows {
        job.append(&format!("\n{k}: "), 0.0, fmt(bold(13.0), MUTED));
        job.append(
            v,
            0.0,
            fmt(FontId::proportional(13.0), Color32::from_gray(225)),
        );
    }
    let galley = ctx.fonts_mut(|f| f.layout_job(job));
    let rect = egui::Rect::from_min_size(
        clip.min + vec2(16.0, 16.0),
        galley.size() + vec2(28.0, 24.0),
    );
    p.rect_filled(
        rect,
        CornerRadius::same(10),
        Color32::from_rgba_unmultiplied(14, 16, 22, 235),
    );
    p.rect_stroke(
        rect,
        CornerRadius::same(10),
        Stroke::new(1.0, Color32::from_gray(70)),
        StrokeKind::Inside,
    );
    p.galley(rect.min + vec2(14.0, 12.0), galley, Color32::WHITE);
}
