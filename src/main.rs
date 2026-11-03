//! Valence: drag elements from the periodic table into a 3D lab and build
//! molecules under (simplified) real bonding rules.
//!
//! The chemistry lives in the engine-independent `valence_chem` crate; this
//! binary is a thin Bevy front end over it.

// Release builds on Windows are GUI apps (no console window).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![allow(clippy::too_many_arguments, clippy::type_complexity)] // normal for Bevy systems
// No-crash policy: no unwrap/expect/panic in app code either.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::todo
    )
)]

mod camera;
mod crash;
mod debug;
mod export;
mod facts;
mod gpu;
mod interact;
mod menu;
mod palette;
mod quality;
mod reactions_ui;
mod scene;
mod settings_file;
mod state;
mod ui;

use bevy::{
    pbr::PbrPlugin,
    prelude::*,
    render::RenderPlugin,
    window::{WindowResizeConstraints, WindowResolution},
    winit::{UpdateMode, WinitSettings},
};
use bevy_egui::{EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass};
use std::time::Duration;

/// Returning `AppExit` makes the process exit code reflect how the app ended
/// (the smoke test relies on this).
fn main() -> AppExit {
    crash::install_panic_hook();
    let backends = gpu::choose_backends();
    let quality = quality::resolve(backends);

    App::new()
        // A failing system logs a warning instead of taking the game down.
        .set_error_handler(bevy::ecs::error::warn)
        .add_plugins(
            DefaultPlugins
                .set(RenderPlugin {
                    render_creation: gpu::render_creation(backends),
                    ..default()
                })
                // Skip pipelines for rendering features we don't use (depth
                // prepass, deferred lighting): less start-up shader work, same
                // look. NB: keep GPU mesh preprocessing on; turning it off
                // stops opaque meshes drawing in Bevy 0.20.
                .set(PbrPlugin {
                    prepass_enabled: false,
                    add_default_deferred_lighting_plugin: false,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Valence: build molecules".into(),
                        resolution: WindowResolution::new(1600, 960),
                        resize_constraints: WindowResizeConstraints {
                            min_width: 720.0,
                            min_height: 480.0,
                            ..default()
                        },
                        ..default()
                    }),
                    ..default()
                }),
        )
        // Full frame rate while focused; in the background too while anything
        // moves (see adapt_background_rate), otherwise mostly idle.
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        })
        .add_plugins(EguiPlugin::default())
        .add_plugins(quality::StartupPlugin)
        .insert_resource(quality)
        .add_plugins(debug::DebugPlugin)
        .init_resource::<state::Chem>()
        .init_resource::<state::Drag>()
        .init_resource::<state::Pointer>()
        .init_resource::<state::Settings>()
        .init_resource::<state::Selection>()
        .init_resource::<state::ViewRect>()
        .init_resource::<state::Effects>()
        .init_resource::<scene::VisState>()
        .init_resource::<ui::FontsReady>()
        .init_resource::<state::UiState>()
        .init_resource::<state::Isolation>()
        .init_resource::<export::ExportJob>()
        .init_resource::<reactions_ui::ReactionState>()
        .init_resource::<reactions_ui::ReactionLib>()
        .add_systems(
            Startup,
            (configure_egui, camera::spawn_camera, scene::setup_assets),
        )
        .add_systems(
            EguiPrimaryContextPass,
            (ui::setup_fonts, ui::ui_system).chain(),
        )
        .add_systems(
            Update,
            (
                interact::update_pointer,
                interact::handle_keys,
                camera::orbit_controls,
                camera::apply_full_look,
                interact::handle_mouse,
                scene::step_physics,
                scene::sync_structure,
                export::run_export,
                scene::apply_isolation,
                scene::sync_transforms,
                menu::handle_quit,
                adapt_background_rate,
            )
                .chain(),
        )
        .run()
}

/// The interface is drawn by the 3D camera itself (see camera.rs). A second,
/// overlay camera caused the UI and 3D view to fall out of sync and flicker.
fn configure_egui(mut egui_settings: ResMut<EguiGlobalSettings>) {
    egui_settings.auto_create_primary_context = false;
}

/// Unfocused windows drop to ~4 fps only while nothing is moving; anything
/// animating (layout settling, the camera turning, effects, an export,
/// start-up) keeps full frame rate even in the background.
fn adapt_background_rate(
    mut winit: ResMut<WinitSettings>,
    chem: Res<state::Chem>,
    camera: Single<Ref<Transform>, With<camera::OrbitCam>>,
    effects: Res<state::Effects>,
    export: Res<export::ExportJob>,
    startup: Res<quality::Startup>,
    drag: Res<state::Drag>,
) {
    let animating = chem.lab.is_awake()
        || camera.is_changed()
        || !effects.0.is_empty()
        || export.busy()
        || startup.phase != quality::Phase::Ready
        || *drag != state::Drag::None;
    let want = if animating {
        UpdateMode::Continuous
    } else {
        UpdateMode::reactive_low_power(Duration::from_millis(250))
    };
    if winit.unfocused_mode != want {
        winit.unfocused_mode = want;
    }
}
