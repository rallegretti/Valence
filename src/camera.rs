//! Orbit camera: right-drag to orbit, middle-drag (or shift+right) to pan,
//! scroll to zoom. Only reacts when the gesture starts over the 3D view.

use bevy::{
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    post_process::bloom::Bloom,
    prelude::*,
};

use crate::quality::{GraphicsQuality, Quality, Startup};
use crate::state::{Chem, Drag, Pointer};
use bevy_egui::input::EguiWantsInput;

#[derive(Component, Clone, Copy)]
pub struct OrbitCam {
    pub focus: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
}

impl Default for OrbitCam {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            yaw: 0.5,
            pitch: -0.35,
            dist: 14.0,
        }
    }
}

impl OrbitCam {
    /// Recover from any non-finite state rather than rendering garbage.
    pub fn sanitize(&mut self) {
        if !(self.focus.is_finite()
            && self.yaw.is_finite()
            && self.pitch.is_finite()
            && self.dist.is_finite())
        {
            *self = OrbitCam::default();
        }
        self.focus = self.focus.clamp(Vec3::splat(-5000.0), Vec3::splat(5000.0));
    }

    pub fn transform(&self) -> Transform {
        let rot = Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0);
        Transform::from_translation(self.focus + rot * Vec3::Z * self.dist).with_rotation(rot)
    }
}

/// Which gesture currently owns the mouse.
#[derive(Default, PartialEq)]
pub enum Gesture {
    #[default]
    Idle,
    Orbit,
    Pan,
}

pub fn spawn_camera(mut commands: Commands) {
    let rig = OrbitCam::default();
    commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgb(0.035, 0.04, 0.06)),
            ..default()
        },
        // Post-processing is added with the 3D scene (Full only), so the
        // interface's shaders compile first and alone.
        Tonemapping::None,
        rig.transform(),
        rig,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 9000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 2500.0,
            color: Color::srgb(0.7, 0.8, 1.0),
            ..default()
        },
        Transform::from_xyz(-6.0, -2.0, -4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 350.0,
        ..default()
    });
}

/// When the 3D scene is built, give Full quality its film tone mapping and
/// bloom glow. Lite keeps neither (fewer shaders to compile).
pub fn apply_full_look(
    mut commands: Commands,
    camera: Single<Entity, With<OrbitCam>>,
    startup: Res<Startup>,
    quality: Res<GraphicsQuality>,
    mut done: Local<bool>,
) {
    if *done || !startup.scene_allowed() {
        return;
    }
    *done = true;
    if quality.active == Quality::Full {
        commands.entity(*camera).insert((
            Tonemapping::TonyMcMapface,
            Bloom {
                intensity: 0.12,
                ..Bloom::NATURAL
            },
        ));
    }
}

pub fn orbit_controls(
    mut cam: Single<(&mut OrbitCam, &mut Transform)>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    pointer: Res<Pointer>,
    drag: Res<Drag>,
    egui: Res<EguiWantsInput>,
    chem: Res<Chem>,
    mut gesture: Local<Gesture>,
) {
    let (rig, transform) = &mut *cam;
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);

    if pointer.in_scene {
        if buttons.just_pressed(MouseButton::Right) {
            *gesture = if shift { Gesture::Pan } else { Gesture::Orbit };
        }
        if buttons.just_pressed(MouseButton::Middle) {
            *gesture = Gesture::Pan;
        }
        // Left-drag on empty space also orbits (Shift: pans), for trackpads
        // and one-button mice.
        if buttons.just_pressed(MouseButton::Left)
            && pointer.hovered_atom.is_none()
            && *drag == Drag::None
        {
            *gesture = if shift { Gesture::Pan } else { Gesture::Orbit };
        }
    }
    if !buttons.any_pressed([MouseButton::Left, MouseButton::Right, MouseButton::Middle]) {
        *gesture = Gesture::Idle;
    }

    // Clamp per-frame motion so a hitch or focus change can't fling the view.
    let d = motion.delta.clamp(Vec2::splat(-200.0), Vec2::splat(200.0));
    match *gesture {
        Gesture::Orbit => {
            rig.yaw -= d.x * 0.006;
            rig.pitch = (rig.pitch - d.y * 0.006).clamp(-1.5, 1.5);
        }
        Gesture::Pan => {
            let right = transform.rotation * Vec3::X;
            let up = transform.rotation * Vec3::Y;
            let k = rig.dist * 0.0016;
            rig.focus += (-right * d.x + up * d.y) * k;
        }
        Gesture::Idle => {}
    }

    if pointer.in_scene && scroll.delta.y != 0.0 {
        let lines = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
        rig.dist = (rig.dist * (1.0 - lines.clamp(-5.0, 5.0) * 0.1)).clamp(2.5, 200.0);
    }

    if keys.just_pressed(KeyCode::KeyF) && !egui.wants_keyboard_input() {
        frame_all(rig, &chem);
    }

    rig.sanitize();
    let t = rig.transform();
    if **transform != t {
        **transform = t;
    }
}

/// Point the camera at everything in the lab.
pub fn frame_all(rig: &mut OrbitCam, chem: &Chem) {
    let pts: Vec<Vec3> = chem
        .lab
        .atoms()
        .map(|(_, a)| a.pos)
        .filter(|p| p.is_finite())
        .collect();
    if pts.is_empty() {
        *rig = OrbitCam::default();
        return;
    }
    let center = pts.iter().copied().sum::<Vec3>() / pts.len() as f32;
    let radius = pts
        .iter()
        .map(|p| p.distance(center))
        .fold(0.0f32, f32::max);
    rig.focus = center;
    rig.dist = (radius * 2.6 + 6.0).clamp(4.0, 200.0);
}
