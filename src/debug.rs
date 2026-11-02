//! Developer helpers.
//!
//! - `--demo`: start with a shelf of example molecules.
//! - `VALENCE_SCREENSHOT=path.png`: save a screenshot after the layout
//!   settles, then exit (used for automated visual checks).
//! - `VALENCE_WINDOW=w,h`: start with a specific window size.
//! - `VALENCE_FAKE_DRAG=z,x,y`: pretend element `z` is being dragged with the
//!   cursor at window position (x, y), to capture drop feedback in a screenshot.

use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use valence_chem::{AtomId, Lab, Reject, Vec3 as V};

use crate::camera::{OrbitCam, frame_all};
use crate::state::{Chem, Drag, Selection};

#[derive(Resource)]
pub struct FakeDrag {
    pub z: u8,
    pub pos: Vec2,
}

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        if std::env::args().any(|a| a == "--demo") {
            app.add_systems(PostStartup, demo_scene);
        }
        if let Ok(path) = std::env::var("VALENCE_SCREENSHOT") {
            app.insert_resource(ScreenshotPath(path))
                .add_systems(Update, screenshot_then_exit);
        }
        if let Ok(spec) = std::env::var("VALENCE_WINDOW") {
            let v: Vec<u32> = spec
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            if let [w, h] = v[..] {
                app.add_systems(Startup, move |mut win: Single<&mut Window>| {
                    win.resolution.set(w as f32, h as f32)
                });
            }
        }
        if let Ok(spec) = std::env::var("VALENCE_FAKE_DRAG") {
            let v: Vec<f32> = spec
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            if v.len() == 3 {
                app.insert_resource(FakeDrag {
                    z: v[0] as u8,
                    pos: Vec2::new(v[1], v[2]),
                })
                .add_systems(Update, |fake: Res<FakeDrag>, mut drag: ResMut<Drag>| {
                    *drag = Drag::Palette(fake.z)
                });
            }
        }
    }
}

#[derive(Resource)]
struct ScreenshotPath(String);

fn screenshot_then_exit(
    mut commands: Commands,
    path: Res<ScreenshotPath>,
    mut frame: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    *frame += 1;
    if *frame == 240 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.0.clone()));
    }
    if *frame == 270 {
        exit.write(AppExit::Success);
    }
}

fn chain(lab: &mut Lab, target: AtomId, zs: &[u8]) -> Result<(), Reject> {
    for &z in zs {
        lab.attach(target, z, V::Y)?;
    }
    Ok(())
}

/// Builds the demo shelf; returns one atom of each molecule.
fn build_demo(lab: &mut Lab) -> Result<Vec<AtomId>, Reject> {
    let grid = |i: usize| V::new((i % 4) as f32 * 8.0 - 12.0, 6.0 - (i / 4) as f32 * 8.0, 0.0);
    let mut roots = Vec::new();
    let mut add = |lab: &mut Lab, z: u8| -> Result<AtomId, Reject> {
        let id = lab.add_atom(z, grid(roots.len()))?;
        roots.push(id);
        Ok(id)
    };

    let o = add(lab, 8)?; // water
    chain(lab, o, &[1, 1])?;
    let c = add(lab, 6)?; // carbon dioxide
    chain(lab, c, &[8, 8])?;
    let cl = add(lab, 17)?; // table salt
    chain(lab, cl, &[11])?;
    let n = add(lab, 7)?; // ammonium
    chain(lab, n, &[1, 1, 1, 1])?;
    let c0 = add(lab, 6)?; // benzene
    let mut ring = vec![c0];
    let mut last = c0;
    for _ in 0..5 {
        last = lab.attach(last, 6, V::X)?;
        ring.push(last);
    }
    lab.connect(c0, last)?;
    for &c in &ring {
        chain(lab, c, &[1])?;
    }
    let c1 = add(lab, 6)?; // ethanol
    let c2 = lab.attach(c1, 6, V::X)?;
    let oh = lab.attach(c2, 8, V::X)?;
    chain(lab, oh, &[1])?;
    chain(lab, c1, &[1, 1, 1])?;
    chain(lab, c2, &[1, 1])?;
    let s = add(lab, 16)?; // sulfuric acid
    chain(lab, s, &[8, 8, 8, 8])?;
    let open: Vec<AtomId> = lab
        .neighbors(s)
        .map(|(a, _)| a)
        .filter(|&a| lab.free(a) > 0)
        .collect();
    for o in open {
        chain(lab, o, &[1])?;
    }
    let ca = add(lab, 20)?; // calcium hydroxide
    for _ in 0..2 {
        let o = lab.attach(ca, 8, V::X)?;
        chain(lab, o, &[1])?;
    }
    let a1 = add(lab, 6)?; // acetylene
    let a2 = lab.attach(a1, 6, V::X)?;
    chain(lab, a1, &[1])?;
    chain(lab, a2, &[1])?;
    let m = add(lab, 6)?; // methylene: two open sites
    chain(lab, m, &[1, 1])?;
    let xe = add(lab, 54)?; // xenon difluoride
    chain(lab, xe, &[9, 9])?;
    add(lab, 10)?; // a lone neon atom
    for _ in 0..600 {
        lab.relax(4, None);
    }
    Ok(roots)
}

fn demo_scene(
    mut chem: ResMut<Chem>,
    mut rig: Single<&mut OrbitCam>,
    mut selection: ResMut<Selection>,
) {
    let roots = match chem.edit("demo", build_demo) {
        crate::state::Edit::Done(r) => r,
        _ => {
            bevy::log::error!("demo scene failed to build");
            return;
        }
    };
    selection.atom = roots.first().copied();
    frame_all(&mut rig, &chem);
    rig.yaw = 0.0;
    rig.pitch = -0.1;
    // VALENCE_ZOOM=i,yaw,pitch: close-up on the i-th demo molecule.
    if let Ok(spec) = std::env::var("VALENCE_ZOOM") {
        let v: Vec<f32> = spec
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if let Some(&root) = v.first().and_then(|&i| roots.get(i as usize)) {
            let pts: Vec<V> = chem
                .lab
                .component(root)
                .iter()
                .filter_map(|&a| chem.lab.atom(a))
                .map(|a| a.pos)
                .collect();
            rig.focus = pts.iter().copied().sum::<V>() / pts.len().max(1) as f32;
            rig.dist = 7.0;
            rig.yaw = v.get(1).copied().unwrap_or(0.0);
            rig.pitch = v.get(2).copied().unwrap_or(-0.1);
        }
    }
}
