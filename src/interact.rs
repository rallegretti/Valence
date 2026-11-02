//! Mouse picking and drag-and-drop.
//!
//! - Drag an element from the table: drop on empty space → new atom; drop on
//!   an atom → try to bond to it.
//! - Drag an atom in the scene: moves its whole molecule; drop it onto
//!   another atom to bond them (joins molecules or closes a ring).
//! - Right-click (no drag) or Delete on an atom removes it.

use bevy::{prelude::*, window::PrimaryWindow};
use bevy_egui::input::EguiWantsInput;
use valence_chem::{AtomId, lab::atom_radius};

use crate::camera::OrbitCam;
use crate::state::{Chem, Drag, Effects, Pointer, Selection, ViewRect};

const DRAG_THRESHOLD: f32 = 5.0;

/// Closest atom hit by the ray (spheres slightly enlarged to be easy to hit).
fn pick(chem: &Chem, ray: Ray3d, exclude: &[AtomId]) -> Option<(AtomId, f32)> {
    let mut best: Option<(AtomId, f32)> = None;
    for (id, a) in chem.lab.atoms() {
        if exclude.contains(&id) {
            continue;
        }
        let r = atom_radius(a.z) * 1.25 + 0.05;
        let oc = ray.origin - a.pos;
        let b = oc.dot(*ray.direction);
        let c = oc.length_squared() - r * r;
        let disc = b * b - c;
        if disc < 0.0 {
            continue;
        }
        let t = -b - disc.sqrt();
        if t > 0.0 && best.is_none_or(|(_, bt)| t < bt) {
            best = Some((id, t));
        }
    }
    best
}

/// Where a new free atom lands: the plane through the camera focus facing the camera.
fn drop_point(ray: Ray3d, rig: &OrbitCam, cam: &Transform) -> Vec3 {
    let n = cam.rotation * Vec3::Z;
    let denom = ray.direction.dot(n);
    if denom.abs() < 1e-4 {
        return rig.focus;
    }
    let t = (rig.focus - ray.origin).dot(n) / denom;
    ray.origin + *ray.direction * t.max(0.5)
}

#[allow(clippy::too_many_arguments)]
pub fn update_pointer(
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<OrbitCam>>,
    egui: Res<EguiWantsInput>,
    view: Res<ViewRect>,
    chem: Res<Chem>,
    drag: Res<Drag>,
    mut pointer: ResMut<Pointer>,
    fake: Option<Res<crate::debug::FakeDrag>>,
    mut cache: Local<Option<((u8, AtomId, AtomId), u64, Result<String, String>)>>,
) {
    let (cam, cam_gt) = *camera;
    let over_ui = fake.is_none() && egui.is_pointer_over_area();
    pointer.pos = fake.map(|f| f.pos).or_else(|| window.cursor_position());
    pointer.in_scene = pointer.pos.is_some_and(|p| view.0.contains(p)) && !over_ui;
    pointer.ray = pointer
        .pos
        .and_then(|p| cam.viewport_to_world(cam_gt, p).ok());

    let exclude: Vec<AtomId> = match *drag {
        Drag::Atom { atom, .. } => chem
            .molecule_of(atom)
            .map(|m| {
                vec![atom]
                    .into_iter()
                    .chain(
                        m.atoms
                            .iter()
                            .copied()
                            .filter(|&a| chem.lab.bonded(a, atom)),
                    )
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    pointer.hovered_atom = match (pointer.in_scene, pointer.ray) {
        (true, Some(ray)) => pick(&chem, ray, &exclude).map(|h| h.0),
        _ => None,
    };

    // Feedback for what would happen on release (cached per lab revision).
    pointer.drop_check = None;
    let Some(target) = pointer.hovered_atom else {
        return;
    };
    let key = match *drag {
        Drag::Palette(z) => (z, target, 0),
        Drag::Atom { atom, .. } => (0, target, atom),
        _ => return,
    };
    let rev = chem.lab.revision();
    if let Some((k, r, res)) = &*cache
        && *k == key
        && *r == rev
    {
        pointer.drop_check = Some(res.clone());
        return;
    }
    let res = match *drag {
        Drag::Palette(z) => chem
            .lab
            .check_attach(target, z)
            .map(|p| p.note)
            .map_err(|e| e.0),
        Drag::Atom { atom, .. } => chem
            .lab
            .check_connect(atom, target)
            .map(|(k, _, _)| match k {
                valence_chem::BondKind::Ionic => "Ionic bond".to_string(),
                _ => "Covalent bond".to_string(),
            })
            .map_err(|e| e.0),
        _ => return,
    };
    *cache = Some((key, rev, res.clone()));
    pointer.drop_check = Some(res);
}

#[allow(clippy::too_many_arguments)]
pub fn handle_mouse(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    camera: Single<(&OrbitCam, &Transform)>,
    pointer: Res<Pointer>,
    mut drag: ResMut<Drag>,
    mut chem: ResMut<Chem>,
    mut effects: ResMut<Effects>,
    mut selection: ResMut<Selection>,
    mut right_press: Local<Option<(Vec2, Option<AtomId>)>>,
) {
    let (rig, cam_t) = *camera;
    let now = time.elapsed_secs_f64();

    if keys.just_pressed(KeyCode::Escape) {
        *drag = Drag::None;
    }

    // Start pressing on an atom.
    if buttons.just_pressed(MouseButton::Left)
        && *drag == Drag::None
        && let (Some(atom), Some(at)) = (pointer.hovered_atom, pointer.pos)
    {
        *drag = Drag::Pressed { atom, at };
    }

    // Promote a press to a drag once the mouse moves.
    if let (Drag::Pressed { atom, at }, Some(pos), Some(ray)) = (*drag, pointer.pos, pointer.ray)
        && pos.distance(at) > DRAG_THRESHOLD
        && let Some(a) = chem.lab.atom(atom)
    {
        let depth = (a.pos - ray.origin).dot(*ray.direction);
        let rigid = !(keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight));
        *drag = Drag::Atom { atom, depth, rigid };
    }

    // Move the dragged molecule rigidly so the grabbed atom follows the cursor.
    if let (Drag::Atom { atom, depth, rigid }, Some(ray)) = (*drag, pointer.ray)
        && let Some(a) = chem.lab.atom(atom)
    {
        let target = ray.origin + *ray.direction * depth;
        let delta = target - a.pos;
        if delta.length_squared() > 1e-8 {
            if rigid {
                let comp = chem.lab.component(atom);
                chem.lab.translate(&comp, delta);
            } else {
                chem.lab.set_pos(atom, target);
            }
        }
    }

    if buttons.just_released(MouseButton::Left) {
        match *drag {
            Drag::Palette(z) if pointer.in_scene => {
                if let Some(target) = pointer.hovered_atom {
                    let hint = pointer.ray.map(|r| -*r.direction).unwrap_or(Vec3::Y);
                    let at = chem.lab.atom(target).map(|a| a.pos).unwrap_or_default();
                    match chem.lab.check_attach(target, z) {
                        Ok(plan) => {
                            // Prefer the open site facing the camera-side of the cursor.
                            let toward = pointer.ray.map(|r| {
                                let p = r.origin + *r.direction * (at - r.origin).dot(*r.direction);
                                (p - at).normalize_or_zero() + hint * 0.3
                            });
                            if let Ok(id) = chem.lab.attach(target, z, toward.unwrap_or(hint)) {
                                effects.push(at, plan.note, true, now);
                                selection.atom = Some(id);
                            }
                        }
                        Err(e) => effects.push(at, e.0, false, now),
                    }
                } else if let Some(ray) = pointer.ray {
                    let p = drop_point(ray, rig, cam_t);
                    let id = chem.lab.add_atom(z, p);
                    selection.atom = Some(id);
                }
            }
            Drag::Atom { atom, .. } => {
                if let Some(target) = pointer.hovered_atom {
                    let at = chem.lab.atom(target).map(|a| a.pos).unwrap_or_default();
                    match chem.lab.connect(atom, target) {
                        Ok(()) => effects.push(at, "Bonded!", true, now),
                        Err(e) => effects.push(at, e.0, false, now),
                    }
                }
                selection.atom = Some(atom);
            }
            Drag::Pressed { atom, .. } => selection.atom = Some(atom),
            _ => {}
        }
        *drag = Drag::None;
    }

    // Right-click (without dragging the camera) deletes an atom.
    if buttons.just_pressed(MouseButton::Right) && pointer.in_scene {
        *right_press = pointer.pos.map(|p| (p, pointer.hovered_atom));
    }
    if buttons.just_released(MouseButton::Right) {
        if let (Some((at, Some(atom))), Some(pos)) = (*right_press, pointer.pos)
            && pos.distance(at) < DRAG_THRESHOLD
            && pointer.hovered_atom == Some(atom)
        {
            delete_atom(&mut chem, &mut effects, &mut selection, atom, now);
        }
        *right_press = None;
    }
    if (keys.just_pressed(KeyCode::Delete) || keys.just_pressed(KeyCode::Backspace))
        && *drag == Drag::None
        && let Some(atom) = pointer.hovered_atom
    {
        delete_atom(&mut chem, &mut effects, &mut selection, atom, now);
    }
}

fn delete_atom(
    chem: &mut Chem,
    effects: &mut Effects,
    selection: &mut Selection,
    atom: AtomId,
    now: f64,
) {
    if let Some(a) = chem.lab.atom(atom) {
        let pos = a.pos;
        let name = valence_chem::elements::element(a.z).name;
        // Keep the selection on the remaining molecule if possible.
        let neighbor = chem.lab.neighbors(atom).map(|(n, _)| n).next();
        chem.lab.remove_atom(atom);
        effects.push(pos, format!("Removed {name}"), true, now);
        if selection.atom == Some(atom) {
            selection.atom = neighbor;
        }
    }
}
