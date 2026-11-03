//! Developer helpers.
//!
//! - `--demo`: start with a shelf of example molecules.
//! - `VALENCE_SCREENSHOT=path.png`: save a screenshot after the layout
//!   settles and the game is visible (retrying for up to 90 s while e.g.
//!   shaders compile), then exit: code 0 if the game showed up, 3 if every
//!   frame was blank (e.g. locked screen), 4 if no screenshot arrived.
//! - `VALENCE_DOC_SHOT=path.png`: documentation screenshot with labels shown,
//!   taken once the 3D scene is ready and settled, then exit. Optional
//!   `VALENCE_TAB=build|reactions|export` and `VALENCE_SETTINGS=1`.
//! - `VALENCE_SHOWCASE=name|SMILES`: show one molecule from the compound
//!   table (e.g. `caffeine`) framed and slowly spinning (`VALENCE_SPIN` deg/s,
//!   default 30), for screen recordings.
//! - `VALENCE_WINDOW=w,h`: start with a specific window size.
//! - `VALENCE_FAKE_DRAG=z,x,y`: pretend element `z` is being dragged with the
//!   cursor at window position (x, y), to capture drop feedback in a screenshot.

use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
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
            // Hide text labels so the content check can see the 3D bonds.
            app.add_systems(Startup, |mut s: ResMut<crate::state::Settings>| {
                s.show_names = false;
                s.show_symbols = false;
            });
            app.insert_resource(ScreenshotPath(path))
                .add_systems(Update, screenshot_then_exit);
        }
        if let Ok(dir) = std::env::var("VALENCE_BURST") {
            app.insert_resource(Burst {
                dir: dir.into(),
                frame: 0,
                start: None,
            })
            .add_systems(Update, burst);
        }
        if let Ok(what) = std::env::var("VALENCE_SHOWCASE") {
            app.insert_resource(Showcase { what, built: false })
                .add_systems(Update, showcase);
        }
        if let Ok(path) = std::env::var("VALENCE_DOC_SHOT") {
            app.insert_resource(DocShot {
                path,
                taken_at: None,
            })
            .add_systems(Update, doc_shot);
        }
        if let Ok(dir) = std::env::var("VALENCE_FEATURE_TEST") {
            app.insert_resource(FeatureTest {
                dir: dir.into(),
                step: 0,
                wait: 0,
                shots: Vec::new(),
            })
            .add_systems(Update, feature_test);
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

/// Smoke-test verdict, written by the screenshot observer (0 = pending).
const DREW: u8 = 1;
const BLANK: u8 = 2;

/// Does the captured frame actually contain the game? Two checks:
/// - the UI drew: plenty of bright pixels (text, table cells);
/// - the 3D scene drew: white bond tubes in the 3D view (right of the panel).
///   Labels are hidden in smoke-test mode so only bonds are near-white there.
///   A locked screen, a hidden window or a broken render pass fails these.
fn frame_has_content(captured: &ScreenshotCaptured) -> bool {
    let img = &captured.image;
    let Some(data) = img.data.as_ref() else {
        return false;
    };
    let (w, h) = (img.width() as usize, img.height() as usize);
    if w == 0 || h == 0 || data.len() < w * h * 4 {
        return false;
    }
    let px = data.as_chunks::<4>().0;
    let bright = px.iter().filter(|p| p[0].max(p[1]).max(p[2]) > 180).count();
    let view_left = w * 45 / 100;
    let mut bond_white = 0usize;
    for y in 0..h {
        for p in &px[y * w + view_left..(y + 1) * w] {
            let (mx, mn) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
            if mn > 170 && mx - mn < 30 {
                bond_white += 1;
            }
        }
    }
    let view_px = (w - view_left) * h;
    bright > 500 && bond_white * 10_000 > view_px * 8 // > 0.08% of the 3D view
}

/// Retry state for the smoke test.
#[derive(Default)]
struct SmokeTest {
    next_shot: f64,
    requested_at: Option<f64>,
    attempts: u32,
    ever_captured: bool,
    first_visible: Option<f64>,
}

const FIRST_SHOT_SECS: f64 = 0.5;
const SETTLED_SECS: f64 = 3.0; // final screenshot no earlier than this
const RETRY_SECS: f64 = 1.0;
const GIVE_UP_SECS: f64 = 150.0; // slow first-run shader compiles can take a while

/// Screenshot the window until it shows the game (or give up), then exit.
/// Time-based with retries, because the first visible frame can take many
/// seconds on slow machines while shaders compile.
fn screenshot_then_exit(
    mut commands: Commands,
    path: Res<ScreenshotPath>,
    time: Res<Time<Real>>,
    startup: Res<crate::quality::Startup>,
    quality: Res<crate::quality::GraphicsQuality>,
    mut state: Local<SmokeTest>,
    verdict: Local<Arc<AtomicU8>>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.elapsed_secs_f64();
    if state.next_shot == 0.0 {
        state.next_shot = FIRST_SHOT_SECS;
    }

    match verdict.swap(0, Ordering::SeqCst) {
        DREW => {
            if state.first_visible.is_none() {
                state.first_visible = Some(now);
                info!(
                    "smoke test: game first visible {now:.1}s after launch (attempt {})",
                    state.attempts
                );
            }
            if now >= SETTLED_SECS {
                info!(
                    "smoke test: OK ({:?} graphics: {}; interface at {:.1}s, 3D scene ready {:.1}s after launch); screenshot: {}",
                    quality.active,
                    quality.reason,
                    startup.ui_ready_at.unwrap_or(0.0),
                    startup.scene_ready_at.unwrap_or(0.0),
                    path.0
                );
                exit.write(AppExit::Success);
                return;
            }
            state.requested_at = None;
            state.next_shot = SETTLED_SECS;
        }
        BLANK => {
            state.ever_captured = true;
            state.requested_at = None;
            state.next_shot = now + RETRY_SECS;
        }
        _ => {}
    }
    // A request that never comes back (window hidden, surface lost): retry.
    if state.requested_at.is_some_and(|t| now - t > 10.0) {
        state.requested_at = None;
        state.next_shot = now;
    }

    if now > GIVE_UP_SECS {
        if state.ever_captured {
            error!(
                "smoke test: every frame was blank for {GIVE_UP_SECS}s. Is the screen locked, asleep \
                 or the window hidden? (screenshot: {})",
                path.0
            );
            exit.write(AppExit::from_code(3));
        } else {
            error!("smoke test: no screenshot was captured in {GIVE_UP_SECS}s");
            exit.write(AppExit::from_code(4));
        }
        return;
    }

    // Only judge the frame once the 3D scene has finished compiling.
    if startup.phase != crate::quality::Phase::Ready {
        state.next_shot = state.next_shot.max(now);
        return;
    }
    if state.requested_at.is_none() && now >= state.next_shot {
        state.requested_at = Some(now);
        state.attempts += 1;
        let v = verdict.clone();
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.0.clone()))
            .observe(move |captured: On<ScreenshotCaptured>| {
                v.store(
                    if frame_has_content(&captured) {
                        DREW
                    } else {
                        BLANK
                    },
                    Ordering::SeqCst,
                );
            });
    }
}

fn chain(lab: &mut Lab, target: AtomId, zs: &[u8]) -> Result<(), Reject> {
    for &z in zs {
        lab.attach(target, z, V::Y)?;
    }
    Ok(())
}

/// Builds the demo shelf; returns one atom of each molecule.
pub(crate) fn build_demo(lab: &mut Lab) -> Result<Vec<AtomId>, Reject> {
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

// ---------------------------------------------------------------------------
// VALENCE_FEATURE_TEST=dir: drive the reactions, export and settings UI
// end to end, saving window screenshots and exports into `dir`, then exit.
// ---------------------------------------------------------------------------

#[derive(Resource)]
pub struct FeatureTest {
    dir: std::path::PathBuf,
    step: u32,
    wait: u32,
    shots: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
fn feature_test(
    mut commands: Commands,
    mut test: ResMut<FeatureTest>,
    mut chem: ResMut<Chem>,
    mut ui: ResMut<crate::state::UiState>,
    mut reaction: ResMut<crate::reactions_ui::ReactionState>,
    lib: Res<crate::reactions_ui::ReactionLib>,
    mut job: ResMut<crate::export::ExportJob>,
    mut selection: ResMut<Selection>,
    mut effects: ResMut<crate::state::Effects>,
    mut rig: Single<&mut OrbitCam>,
    startup: Res<crate::quality::Startup>,
    mut exit: MessageWriter<AppExit>,
) {
    use crate::reactions_ui::Role;
    use crate::state::{ExportTarget, Tab};
    if startup.phase != crate::quality::Phase::Ready {
        return;
    }
    if test.wait > 0 {
        test.wait -= 1;
        return;
    }
    let shot = |commands: &mut Commands, test: &mut FeatureTest, name: &str| {
        let path = test.dir.join(name);
        test.shots.push(path.display().to_string());
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    };
    let export_done = |job: &crate::export::ExportJob| !job.busy() && job.last.is_some();
    match test.step {
        0 => {
            // H2 and O2 as inputs.
            let built = chem.edit("test", |lab| {
                lab.clear();
                let h2 = lab.add_molecule(
                    &valence_chem::smiles::parse("[H][H]").map_err(Reject)?,
                    V::new(-4.0, 0.0, 0.0),
                )?;
                let o2 = lab.add_molecule(
                    &valence_chem::smiles::parse("O=O").map_err(Reject)?,
                    V::new(4.0, 0.0, 0.0),
                )?;
                Ok((h2[0], o2[0]))
            });
            let crate::state::Edit::Done((h2, o2)) = built else {
                error!("feature test: couldn't build inputs");
                exit.write(AppExit::from_code(5));
                return;
            };
            chem.refresh();
            for a in [h2, o2] {
                if let Some(&m) = chem.atom_mol.get(a as usize) {
                    reaction.set(&chem, m, Role::Input, 1);
                }
            }
            ui.tab = Tab::Reactions;
            test.step = 1;
            test.wait = 10;
        }
        1 => {
            let parts = reaction.parts(&chem);
            let graphs: Vec<(valence_chem::MolGraph, u32)> = parts
                .iter()
                .map(|p| (chem.lab.graph(&chem.molecules[p.molecule].atoms), p.coeff))
                .collect();
            let inputs: Vec<(&valence_chem::MolGraph, u32)> =
                graphs.iter().map(|(g, c)| (g, *c)).collect();
            let a = lib.0.analyse(&inputs, &[]);
            let Some(&i) = a.matches.first() else {
                error!("feature test: H2 + O2 not recognised");
                exit.write(AppExit::from_code(5));
                return;
            };
            info!(
                "feature test: recognised '{}'",
                lib.0.reactions()[i].info.name
            );
            crate::reactions_ui::build_products(
                &mut chem,
                &mut reaction,
                &lib.0,
                i,
                &rig,
                &mut effects,
                0.0,
            );
            chem.refresh();
            crate::reactions_ui::arrange(&mut chem, &reaction, &mut rig);
            info!("feature test: equation {}", reaction.equation(&chem));
            test.step = 2;
            test.wait = 90;
        }
        2 => {
            shot(&mut commands, &mut test, "1-reactions-tab.png");
            test.step = 3;
            test.wait = 20;
        }
        3 => {
            ui.tab = Tab::Export;
            ui.export_target = ExportTarget::Reaction;
            job.request();
            test.step = 4;
        }
        4 if export_done(&job) => {
            info!("feature test: reaction export {:?}", job.last);
            // Select the water molecule (an output) for a single-molecule export.
            let water = chem
                .molecules
                .iter()
                .find(|m| m.names.formula == "H₂O")
                .and_then(|m| m.atoms.first().copied());
            selection.atom = water;
            ui.export_target = ExportTarget::Molecule;
            job.request();
            test.step = 5;
        }
        5 if export_done(&job) => {
            info!("feature test: molecule export {:?}", job.last);
            ui.export_target = ExportTarget::Lab;
            job.request();
            test.step = 6;
        }
        6 if export_done(&job) => {
            info!("feature test: lab export {:?}", job.last);
            test.wait = 10;
            test.step = 7;
        }
        7 => {
            shot(&mut commands, &mut test, "2-export-tab.png");
            ui.settings_open = true;
            test.wait = 15;
            test.step = 8;
        }
        8 => {
            shot(&mut commands, &mut test, "3-settings.png");
            test.wait = 20;
            test.step = 9;
        }
        9 => {
            info!("feature test: done; window shots {:?}", test.shots);
            exit.write(AppExit::Success);
            test.step = 10;
        }
        _ => {}
    }
}

#[derive(Resource)]
struct DocShot {
    path: String,
    taken_at: Option<f64>,
}

fn doc_shot(
    mut commands: Commands,
    mut shot: ResMut<DocShot>,
    mut ui: ResMut<crate::state::UiState>,
    startup: Res<crate::quality::Startup>,
    time: Res<Time<Real>>,
    mut exit: MessageWriter<AppExit>,
) {
    use crate::state::Tab;
    let now = time.elapsed_secs_f64();
    match std::env::var("VALENCE_TAB").as_deref() {
        Ok("reactions") => ui.tab = Tab::Reactions,
        Ok("export") => ui.tab = Tab::Export,
        _ => {}
    }
    ui.settings_open = std::env::var_os("VALENCE_SETTINGS").is_some();
    let Some(ready) = startup.scene_ready_at else {
        return;
    };
    match shot.taken_at {
        None if now > ready + 3.0 => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(shot.path.clone()));
            shot.taken_at = Some(now);
        }
        Some(t) if now > t + 1.5 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}

#[derive(Resource)]
struct Showcase {
    what: String,
    built: bool,
}

/// Build the named molecule (or SMILES), select and frame it, then spin.
fn showcase(
    mut state: ResMut<Showcase>,
    mut chem: ResMut<Chem>,
    mut selection: ResMut<Selection>,
    mut rig: Single<&mut OrbitCam>,
    time: Res<Time>,
) {
    if !state.built {
        state.built = true;
        let want = state.what.to_lowercase();
        let smiles = valence_chem::compounds::KNOWN
            .iter()
            .find(|(_, common, iupac)| {
                common.to_lowercase().starts_with(&want) || iupac.to_lowercase() == want
            })
            .map_or(state.what.as_str(), |(smi, _, _)| smi);
        let built = chem.edit("showcase", |lab| {
            lab.clear();
            let g = valence_chem::smiles::parse(smiles).map_err(Reject)?;
            let ids = lab.add_molecule(&g, V::ZERO)?;
            for _ in 0..1500 {
                lab.relax(4, None);
            }
            Ok(ids)
        });
        match built {
            crate::state::Edit::Done(ids) => {
                selection.atom = None; // no selection ring in recordings
                let pts: Vec<V> = ids
                    .iter()
                    .filter_map(|&a| chem.lab.atom(a))
                    .map(|a| a.pos)
                    .collect();
                let c = pts.iter().copied().sum::<V>() / pts.len().max(1) as f32;
                let r = pts.iter().map(|p| p.distance(c)).fold(0.0f32, f32::max);
                rig.focus = c;
                rig.dist = r * 2.6 + 4.0;
                rig.pitch = -0.3;
            }
            _ => error!("showcase: couldn't build {}", state.what),
        }
    }
    let spin: f32 = std::env::var("VALENCE_SPIN")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30.0);
    rig.yaw += spin.to_radians() * time.delta_secs();
}

/// VALENCE_BURST=dir: save every rendered frame for ~2.5 s (150 frames) once
/// the scene is ready, then exit. For diagnosing flicker.
#[derive(Resource)]
struct Burst {
    dir: std::path::PathBuf,
    frame: u32,
    start: Option<f64>,
}

fn burst(
    mut commands: Commands,
    mut contexts: bevy_egui::EguiContexts,
    mut b: ResMut<Burst>,
    startup: Res<crate::quality::Startup>,
    time: Res<Time<Real>>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.elapsed_secs_f64();
    let Some(ready) = startup.scene_ready_at else {
        return;
    };
    if now < ready + 2.0 {
        return;
    }
    let start = *b.start.get_or_insert(now);
    if b.frame < 150
        && b.frame.is_multiple_of(30)
        && let Ok(ctx) = contexts.ctx_mut()
    {
        let fill = ctx.fonts(|f| f.font_atlas_fill_ratio());
        info!(
            "burst: frame {} egui font atlas {:.0}% full",
            b.frame,
            fill * 100.0
        );
    }
    if b.frame < 150 {
        let path = b.dir.join(format!("f{:03}.png", b.frame));
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
        b.frame += 1;
    } else if now > start + 6.0 {
        exit.write(AppExit::Success);
    }
}
