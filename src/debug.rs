//! Developer helpers.
//!
//! - `--demo`: start with a shelf of example molecules.
//! - `VALENCE_SCREENSHOT=path.png`: save a screenshot after the layout
//!   settles, then exit (used for automated visual checks).
//! - `VALENCE_FAKE_DRAG=z,x,y`: pretend element `z` is being dragged with the
//!   cursor at window position (x, y), to capture drop feedback in a screenshot.

use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use valence_chem::{AtomId, Lab, Vec3 as V};

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

fn chain(lab: &mut Lab, target: AtomId, zs: &[u8]) {
    for &z in zs {
        lab.attach(target, z, V::Y).expect("demo molecule is valid");
    }
}

fn demo_scene(
    mut chem: ResMut<Chem>,
    mut rig: Single<&mut OrbitCam>,
    mut selection: ResMut<Selection>,
) {
    let chem = &mut *chem;
    let lab = &mut chem.lab;
    let grid = |i: usize| V::new((i % 4) as f32 * 8.0 - 12.0, 6.0 - (i / 4) as f32 * 8.0, 0.0);
    let mut i = 0;
    let mut next = || {
        i += 1;
        grid(i - 1)
    };

    // Water
    let o = lab.add_atom(8, next());
    chain(lab, o, &[1, 1]);
    // Carbon dioxide
    let c = lab.add_atom(6, next());
    chain(lab, c, &[8, 8]);
    // Table salt
    let cl = lab.add_atom(17, next());
    chain(lab, cl, &[11]);
    // Ammonium (protonated ammonia)
    let n = lab.add_atom(7, next());
    chain(lab, n, &[1, 1, 1, 1]);
    // Benzene
    let c0 = lab.add_atom(6, next());
    let mut ring = vec![c0];
    for _ in 0..5 {
        let last = *ring.last().unwrap();
        ring.push(lab.attach(last, 6, V::X).unwrap());
    }
    lab.connect(ring[0], ring[5]).unwrap();
    for &c in &ring {
        chain(lab, c, &[1]);
    }
    // Ethanol
    let c1 = lab.add_atom(6, next());
    let c2 = lab.attach(c1, 6, V::X).unwrap();
    let oh = lab.attach(c2, 8, V::X).unwrap();
    chain(lab, oh, &[1]);
    chain(lab, c1, &[1, 1, 1]);
    chain(lab, c2, &[1, 1]);
    // Sulfuric acid
    let s = lab.add_atom(16, next());
    chain(lab, s, &[8, 8, 8, 8]);
    let open: Vec<AtomId> = lab
        .neighbors(s)
        .map(|(a, _)| a)
        .filter(|&a| lab.free(a) > 0)
        .collect();
    for o in open {
        chain(lab, o, &[1]);
    }
    // Calcium hydroxide
    let ca = lab.add_atom(20, next());
    for _ in 0..2 {
        let o = lab.attach(ca, 8, V::X).unwrap();
        chain(lab, o, &[1]);
    }
    // Acetylene
    let a1 = lab.add_atom(6, next());
    let a2 = lab.attach(a1, 6, V::X).unwrap();
    chain(lab, a1, &[1]);
    chain(lab, a2, &[1]);
    // Methylene: an incomplete fragment with two open sites
    let m = lab.add_atom(6, next());
    chain(lab, m, &[1, 1]);
    // Xenon difluoride
    let xe = lab.add_atom(54, next());
    chain(lab, xe, &[9, 9]);
    // Lone neon
    lab.add_atom(10, next());

    for _ in 0..600 {
        lab.relax(4, None);
    }
    selection.atom = Some(o);
    frame_all(&mut rig, chem);
    rig.yaw = 0.0;
    rig.pitch = -0.1;
    // VALENCE_ZOOM=i,yaw,pitch: close-up on the i-th demo molecule.
    if let Ok(spec) = std::env::var("VALENCE_ZOOM") {
        let v: Vec<f32> = spec
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        let roots = [o, c, cl, n, c0, c1, s, ca, a1, m, xe];
        if let Some(&root) = v.first().and_then(|&i| roots.get(i as usize)) {
            let comp = chem.lab.component(root);
            let sum: V = comp.iter().map(|&a| chem.lab.atom(a).unwrap().pos).sum();
            rig.focus = sum / comp.len() as f32;
            rig.dist = 7.0;
            rig.yaw = v.get(1).copied().unwrap_or(0.0);
            rig.pitch = v.get(2).copied().unwrap_or(-0.1);
        }
    }
}
