//! The ☰ menu (File / Edit / Settings / Help) and the windows it opens.

use bevy::{prelude::*, render::renderer::RenderAdapterInfo};
use bevy_egui::egui::{self, Color32, RichText};

use crate::gpu::ApiSetting;
use crate::quality::{GraphicsQuality, Quality, QualitySetting};
use crate::state::{Chem, Effects, Selection, Tab, UiState};

const MUTED: Color32 = Color32::from_rgb(150, 158, 175);

/// The ☰ button and its menus.
pub fn menu_button(
    ui: &mut egui::Ui,
    chem: &mut Chem,
    state: &mut UiState,
    selection: &mut Selection,
    effects: &mut Effects,
    doc: &mut crate::document::Document,
    now: f64,
) {
    ui.menu_button(RichText::new("☰").size(22.0).color(Color32::WHITE), |ui| {
        ui.set_min_width(180.0);
        ui.menu_button("File", |ui| {
            use crate::document::{Action, Request};
            if ui
                .add(egui::Button::new("New lab").shortcut_text("Ctrl/Cmd+N"))
                .clicked()
            {
                doc.request(Request::Do(Action::New));
                ui.close();
            }
            if ui
                .add(egui::Button::new("Open Lab Data…").shortcut_text("Ctrl/Cmd+O"))
                .clicked()
            {
                doc.request(Request::Do(Action::Open));
                ui.close();
            }
            if ui
                .add(egui::Button::new("Save Lab Data").shortcut_text("Ctrl/Cmd+S"))
                .clicked()
            {
                doc.request(Request::Save);
                ui.close();
            }
            if ui
                .add(egui::Button::new("Save Lab Data As…").shortcut_text("Ctrl/Cmd+Shift+S"))
                .clicked()
            {
                doc.request(Request::SaveAs);
                ui.close();
            }
            ui.separator();
            if ui
                .button("Open example shelf")
                .on_hover_text("Replaces the lab with 12 example molecules")
                .clicked()
            {
                doc.request(Request::Do(Action::OpenExample));
                ui.close();
            }
            if ui.button("Export image…").clicked() {
                state.tab = Tab::Export;
                ui.close();
            }
            ui.separator();
            if ui.button("Quit").clicked() {
                doc.request(Request::Do(Action::Quit));
                ui.close();
            }
        });
        ui.menu_button("Edit", |ui| {
            if ui
                .add_enabled(
                    chem.can_undo(),
                    egui::Button::new("Undo").shortcut_text("Ctrl/Cmd+Z"),
                )
                .clicked()
            {
                chem.undo();
                ui.close();
            }
            if ui
                .add_enabled(
                    chem.can_redo(),
                    egui::Button::new("Redo").shortcut_text("Ctrl/Cmd+Shift+Z"),
                )
                .clicked()
            {
                chem.redo();
                ui.close();
            }
            ui.separator();
            let has_sel = selection.atom.is_some_and(|a| chem.lab.atom(a).is_some());
            if ui
                .add_enabled(
                    has_sel,
                    egui::Button::new("Delete selected atom").shortcut_text("Del"),
                )
                .clicked()
            {
                if let Some(a) = selection.atom {
                    crate::interact::delete_atom(chem, effects, selection, a, now);
                }
                ui.close();
            }
            if ui
                .add_enabled(has_sel, egui::Button::new("Select none"))
                .clicked()
            {
                selection.atom = None;
                ui.close();
            }
        });
        if ui.button("Settings…").clicked() {
            state.settings_open = true;
            ui.close();
        }
        ui.menu_button("Help", |ui| {
            if ui.button("Controls").clicked() {
                state.help_open = true;
                ui.close();
            }
            if ui.button("About Valence").clicked() {
                state.about_open = true;
                ui.close();
            }
        });
    });
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        RichText::new(text)
            .strong()
            .size(15.0)
            .color(Color32::WHITE),
    );
}

/// Settings window: graphics quality, graphics API, file locations.
pub fn settings_window(
    ctx: &egui::Context,
    state: &mut UiState,
    quality: &mut GraphicsQuality,
    adapter: Option<&RenderAdapterInfo>,
) {
    if !state.settings_open {
        return;
    }
    let response = egui::Modal::new("settings".into()).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.label(RichText::new("Settings").size(20.0).strong().color(Color32::WHITE));
        ui.label(RichText::new("Graphics changes apply the next time Valence starts.").color(MUTED));

        heading(ui, "Graphics quality");
        let mut q = quality.saved;
        ui.radio_value(&mut q, QualitySetting::Auto, "Auto: pick for this computer's GPU");
        ui.radio_value(&mut q, QualitySetting::Full, "Full: glow, film tone mapping, translucent halos");
        ui.radio_value(&mut q, QualitySetting::Lite, "Lite: much faster start-up on older GPUs");
        if q != quality.saved {
            quality.save(q);
        }
        let active = match quality.active {
            Quality::Full => "Full",
            Quality::Lite => "Lite",
        };
        ui.label(RichText::new(format!("In use now: {active} ({})", quality.reason)).size(12.0).color(MUTED));

        heading(ui, "Graphics API");
        let saved_api = ApiSetting::saved();
        let mut api = saved_api;
        egui::ComboBox::from_id_salt("graphics_api").selected_text(api.label()).show_ui(ui, |ui| {
            for a in ApiSetting::ALL.into_iter().filter(|a| a.available()) {
                ui.selectable_value(&mut api, a, a.label());
            }
        });
        if api != saved_api {
            api.save();
        }
        let gpu = adapter.map_or_else(
            || "unknown".to_string(),
            |a| format!("{} via {:?} (driver {})", a.name, a.backend, if a.driver_info.is_empty() { &a.driver } else { &a.driver_info }),
        );
        ui.label(RichText::new(format!("GPU in use: {gpu}")).size(12.0).color(MUTED));
        ui.label(
            RichText::new("If the chosen API isn't available, Valence falls back to Auto rather than failing to start.")
                .size(12.0)
                .color(MUTED),
        );

        heading(ui, "Files");
        for (what, path) in [
            ("Settings", crate::settings_file::path()),
            ("Exported images", crate::export::export_dir()),
            ("Crash log", crate::crash::log_path()),
        ] {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{what}:")).color(MUTED));
                ui.label(RichText::new(path.display().to_string()).monospace().size(11.5));
            });
        }
        if ui.button("Open exported images folder").clicked() {
            crate::export::open_folder(&crate::export::export_dir());
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Reset to defaults").clicked() {
                quality.save(QualitySetting::Auto);
                ApiSetting::Auto.save();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    state.settings_open = false;
                }
            });
        });
    });
    if response.should_close() {
        state.settings_open = false;
    }
}

pub fn help_window(ctx: &egui::Context, state: &mut UiState) {
    if !state.help_open {
        return;
    }
    let response = egui::Modal::new("help".into()).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.label(
            RichText::new("Controls")
                .size(20.0)
                .strong()
                .color(Color32::WHITE),
        );
        let rows = [
            (
                "Drag an element from the table",
                "Drop it in empty space for a free atom, or onto an atom to bond",
            ),
            (
                "Select an atom, click elements",
                "Builds onto the selected atom",
            ),
            (
                "Drag an atom",
                "Moves its molecule; drop on another atom to bond them",
            ),
            ("Shift + drag an atom", "Moves just that atom (close rings)"),
            (
                "Drag empty space / Shift + drag",
                "Orbit / pan the view (works on trackpads)",
            ),
            ("Right / middle drag", "Orbit / pan"),
            ("Scroll or pinch", "Zoom"),
            (
                "Right-click, Delete or Backspace",
                "Remove the atom under the mouse (or the selected one)",
            ),
            ("Ctrl/Cmd + Z, Ctrl/Cmd + Shift + Z", "Undo / redo"),
            ("F", "Frame everything"),
            ("Esc", "Cancel a drag"),
        ];
        egui::Grid::new("controls_grid")
            .num_columns(2)
            .spacing([16.0, 6.0])
            .show(ui, |ui| {
                for (k, v) in rows {
                    ui.label(RichText::new(k).strong());
                    ui.label(v);
                    ui.end_row();
                }
            });
        ui.add_space(8.0);
        if ui.button("Close").clicked() {
            state.help_open = false;
        }
    });
    if response.should_close() {
        state.help_open = false;
    }
}

pub fn about_window(ctx: &egui::Context, state: &mut UiState) {
    if !state.about_open {
        return;
    }
    let response = egui::Modal::new("about".into()).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.label(
            RichText::new("Valence")
                .size(24.0)
                .strong()
                .color(Color32::WHITE),
        );
        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
        ui.add_space(6.0);
        ui.label("Build molecules from the periodic table under simplified real bonding rules.");
        ui.add_space(6.0);
        ui.label(
            RichText::new("Built with Bevy and egui. Fonts: DejaVu Sans.")
                .size(12.0)
                .color(MUTED),
        );
        ui.add_space(8.0);
        if ui.button("Close").clicked() {
            state.about_open = false;
        }
    });
    if response.should_close() {
        state.about_open = false;
    }
}
