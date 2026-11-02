//! Mirrors the chemistry `Lab` into Bevy entities.
//!
//! Structure (which entities exist) is rebuilt only when the lab's revision
//! changes. Positions are written every frame only while something moves
//! (physics awake, camera moving, or a drag in progress), so an idle scene
//! costs nothing.

use std::collections::HashMap;

use bevy::prelude::*;
use valence_chem::{AtomId, BondId, BondKind, lab::atom_radius};

use crate::camera::OrbitCam;
use crate::palette::{self, ANION, CATION, OPEN_SITE};
use crate::state::{Chem, Drag, Pointer, Settings};

#[derive(Resource)]
pub struct SceneAssets {
    sphere: Handle<Mesh>,
    tube: Handle<Mesh>,
    dot: Handle<Mesh>,
    bond: Handle<StandardMaterial>,
    ionic: Handle<StandardMaterial>,
    open_site: Handle<StandardMaterial>,
    lone_pair: Handle<StandardMaterial>,
    cation: Handle<StandardMaterial>,
    anion: Handle<StandardMaterial>,
    elements: Vec<Handle<StandardMaterial>>,
    elements_hot: Vec<Handle<StandardMaterial>>,
}

#[derive(Component)]
pub struct AtomVis {
    pub id: AtomId,
    z: u8,
    hot: bool,
}

/// One tube of a bond. Double/triple bonds have several lanes; ionic bonds
/// are drawn as dashes (several segments).
#[derive(Component)]
pub struct BondVis {
    a: AtomId,
    b: AtomId,
    lane: f32,
    radius: f32,
    /// (index, count) for dashed ionic bonds.
    dash: Option<(u8, u8)>,
}

/// An electron marker: open site (single bright dot) or lone pair member.
#[derive(Component)]
pub struct DotVis {
    atom: AtomId,
    domain: usize,
    side: f32,
}

#[derive(Component)]
pub struct HaloVis {
    atom: AtomId,
    base: f32,
}

#[derive(Resource, Default)]
pub struct VisState {
    rev: u64,
    lone_pairs: bool,
    atoms: HashMap<AtomId, Entity>,
    force_update: bool,
}

fn srgb(c: palette::Rgb) -> Color {
    Color::srgb(c[0], c[1], c[2])
}

pub fn setup_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let sphere = meshes.add(Sphere::new(1.0).mesh().ico(4).expect("valid icosphere"));
    let tube = meshes.add(Cylinder::new(1.0, 1.0).mesh().resolution(24));
    let dot = meshes.add(Sphere::new(1.0).mesh().ico(2).expect("valid icosphere"));
    let glow = |c: palette::Rgb, k: f32| LinearRgba::from(srgb(c)) * k;
    let bond = mats.add(StandardMaterial {
        base_color: Color::srgb(0.93, 0.93, 0.95),
        emissive: LinearRgba::rgb(0.08, 0.08, 0.09),
        perceptual_roughness: 0.45,
        ..default()
    });
    let ionic = mats.add(StandardMaterial {
        base_color: Color::srgba(0.95, 0.95, 1.0, 0.55),
        emissive: LinearRgba::rgb(0.25, 0.25, 0.35),
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let open_site = mats.add(StandardMaterial {
        base_color: srgb(OPEN_SITE),
        emissive: glow(OPEN_SITE, 6.0),
        unlit: true,
        ..default()
    });
    let lone_pair = mats.add(StandardMaterial {
        base_color: Color::srgba(0.75, 0.85, 1.0, 0.55),
        emissive: LinearRgba::rgb(0.3, 0.35, 0.45),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    let halo = |c: palette::Rgb| StandardMaterial {
        base_color: Color::srgba(c[0], c[1], c[2], 0.22),
        emissive: glow(c, 1.6),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        cull_mode: None,
        ..default()
    };
    let cation = mats.add(halo(CATION));
    let anion = mats.add(halo(ANION));
    let mut elements = vec![Handle::default()];
    let mut elements_hot = vec![Handle::default()];
    for z in 1..=118u8 {
        let c = palette::element_color(z);
        let metal = valence_chem::elements::category(z).is_metal();
        let m = StandardMaterial {
            base_color: srgb(c),
            perceptual_roughness: if metal { 0.3 } else { 0.42 },
            metallic: if metal { 0.45 } else { 0.0 },
            reflectance: 0.45,
            ..default()
        };
        let hot = StandardMaterial {
            emissive: glow(c, 0.7),
            ..m.clone()
        };
        elements.push(mats.add(m));
        elements_hot.push(mats.add(hot));
    }
    commands.insert_resource(SceneAssets {
        sphere,
        tube,
        dot,
        bond,
        ionic,
        open_site,
        lone_pair,
        cation,
        anion,
        elements,
        elements_hot,
    });
}

/// Spawn/despawn entities to match the lab's structure.
pub fn sync_structure(
    mut commands: Commands,
    mut chem: ResMut<Chem>,
    settings: Res<Settings>,
    assets: Res<SceneAssets>,
    mut vis: ResMut<VisState>,
    old: Query<Entity, Or<(With<BondVis>, With<DotVis>, With<HaloVis>)>>,
) {
    chem.refresh();
    let lab = &chem.lab;
    if vis.rev == lab.revision() && vis.lone_pairs == settings.show_lone_pairs {
        return;
    }
    vis.rev = lab.revision();
    vis.lone_pairs = settings.show_lone_pairs;
    vis.force_update = true;

    for e in &old {
        commands.entity(e).despawn();
    }

    // Atoms: keep entities whose slot still holds the same element.
    let mut keep = HashMap::with_capacity(lab.atom_count());
    for (id, atom) in lab.atoms() {
        let entity = match vis.atoms.remove(&id) {
            Some(e) => e,
            None => commands
                .spawn((
                    Mesh3d(assets.sphere.clone()),
                    MeshMaterial3d(assets.elements[atom.z as usize].clone()),
                    Transform::from_translation(atom.pos)
                        .with_scale(Vec3::splat(atom_radius(atom.z))),
                    AtomVis {
                        id,
                        z: atom.z,
                        hot: false,
                    },
                ))
                .id(),
        };
        commands.entity(entity).insert((
            AtomVis {
                id,
                z: atom.z,
                hot: false,
            },
            MeshMaterial3d(assets.elements[atom.z as usize].clone()),
        ));
        keep.insert(id, entity);
    }
    for (_, e) in vis.atoms.drain() {
        commands.entity(e).despawn();
    }
    vis.atoms = keep;

    // Bonds.
    let bonds: Vec<(BondId, AtomId, AtomId, u8, BondKind)> = lab
        .bonds()
        .map(|(id, b)| (id, b.a, b.b, b.order, b.kind))
        .collect();
    for (_, a, b, order, kind) in bonds {
        match kind {
            BondKind::Ionic => {
                let n = 6u8;
                for i in 0..n {
                    commands.spawn((
                        Mesh3d(assets.dot.clone()),
                        MeshMaterial3d(assets.ionic.clone()),
                        Transform::default(),
                        BondVis {
                            a,
                            b,
                            lane: 0.0,
                            radius: 0.06,
                            dash: Some((i, n)),
                        },
                    ));
                }
            }
            BondKind::Covalent => {
                let (lanes, radius): (&[f32], f32) = match order {
                    1 => (&[0.0], 0.1),
                    2 => (&[-1.0, 1.0], 0.07),
                    _ => (&[-1.0, 0.0, 1.0], 0.06),
                };
                for &lane in lanes {
                    commands.spawn((
                        Mesh3d(assets.tube.clone()),
                        MeshMaterial3d(assets.bond.clone()),
                        Transform::default(),
                        BondVis {
                            a,
                            b,
                            lane,
                            radius,
                            dash: None,
                        },
                    ));
                }
            }
        }
    }

    // Electrons and ion halos.
    let ids: Vec<AtomId> = lab.atoms().map(|(id, _)| id).collect();
    for id in ids {
        let free = lab.free(id) as usize;
        let lone = if settings.show_lone_pairs {
            lab.lone_pairs(id) as usize
        } else {
            0
        };
        for d in 0..free {
            commands.spawn((
                Mesh3d(assets.dot.clone()),
                MeshMaterial3d(assets.open_site.clone()),
                Transform::default(),
                DotVis {
                    atom: id,
                    domain: d,
                    side: 0.0,
                },
            ));
        }
        for d in free..free + lone {
            for side in [-1.0, 1.0] {
                commands.spawn((
                    Mesh3d(assets.dot.clone()),
                    MeshMaterial3d(assets.lone_pair.clone()),
                    Transform::default(),
                    DotVis {
                        atom: id,
                        domain: d,
                        side,
                    },
                ));
            }
        }
        let q = lab.charge(id);
        if q != 0 {
            let z = lab.atom(id).unwrap().z;
            commands.spawn((
                Mesh3d(assets.sphere.clone()),
                MeshMaterial3d(if q > 0 {
                    assets.cation.clone()
                } else {
                    assets.anion.clone()
                }),
                Transform::default(),
                HaloVis {
                    atom: id,
                    base: atom_radius(z) * (1.35 + 0.08 * q.unsigned_abs() as f32),
                },
            ));
        }
    }
}

/// Write transforms from lab positions. Skipped entirely when nothing moves.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn sync_transforms(
    chem: Res<Chem>,
    assets: Res<SceneAssets>,
    pointer: Res<Pointer>,
    drag: Res<Drag>,
    time: Res<Time>,
    mut vis: ResMut<VisState>,
    cam: Single<(Ref<Transform>, &OrbitCam), Without<AtomVis>>,
    mut atoms: Query<
        (
            &mut Transform,
            &mut AtomVis,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        Without<OrbitCam>,
    >,
    mut bonds: Query<(&mut Transform, &BondVis), (Without<AtomVis>, Without<OrbitCam>)>,
    mut dots: Query<
        (&mut Transform, &DotVis),
        (Without<AtomVis>, Without<BondVis>, Without<OrbitCam>),
    >,
    mut halos: Query<
        (&mut Transform, &HaloVis),
        (
            Without<AtomVis>,
            Without<BondVis>,
            Without<DotVis>,
            Without<OrbitCam>,
        ),
    >,
    mut last_hot: Local<Option<AtomId>>,
) {
    let lab = &chem.lab;
    let (cam_t, _) = &*cam;
    let hot = match *drag {
        Drag::None | Drag::Palette(_) | Drag::Atom { .. } => pointer.hovered_atom,
        Drag::Pressed { atom, .. } => Some(atom),
    };
    let hot_changed = *last_hot != hot;
    *last_hot = hot;

    // Halos pulse, so they always update (cheap: only ions have one).
    let t = time.elapsed_secs();
    for (mut tr, h) in &mut halos {
        if let Some(a) = lab.atom(h.atom) {
            let pulse = 1.0 + 0.06 * (t * 3.0).sin();
            *tr = Transform::from_translation(a.pos).with_scale(Vec3::splat(h.base * pulse));
        }
    }

    let moving = lab.is_awake() || cam_t.is_changed() || vis.force_update || hot_changed;
    if !moving {
        return;
    }
    vis.force_update = false;
    let view_dir = (cam_t.rotation * Vec3::NEG_Z).normalize();

    for (mut tr, mut av, mut mat) in &mut atoms {
        let Some(a) = lab.atom(av.id) else { continue };
        let is_hot = hot == Some(av.id);
        let scale = atom_radius(a.z) * if is_hot { 1.1 } else { 1.0 };
        let new = Transform::from_translation(a.pos).with_scale(Vec3::splat(scale));
        if *tr != new {
            *tr = new;
        }
        if av.hot != is_hot {
            av.hot = is_hot;
            mat.0 = if is_hot {
                assets.elements_hot[av.z as usize].clone()
            } else {
                assets.elements[av.z as usize].clone()
            };
        }
    }

    for (mut tr, b) in &mut bonds {
        let (Some(aa), Some(ab)) = (lab.atom(b.a), lab.atom(b.b)) else {
            continue;
        };
        let (pa, pb) = (aa.pos, ab.pos);
        let axis = pb - pa;
        let len = axis.length().max(1e-3);
        let dir = axis / len;
        // Lanes sit side by side as seen from the camera so all are visible.
        let side = dir
            .cross(view_dir)
            .try_normalize()
            .unwrap_or_else(|| dir.any_orthonormal_vector());
        let spacing = 0.17;
        let offset = side * b.lane * spacing;
        let rot = Quat::from_rotation_arc(Vec3::Y, dir);
        let new = match b.dash {
            None => {
                // Ends sit inside the spheres so the tube looks continuous.
                let ra = atom_radius(aa.z) * 0.6;
                let rb = atom_radius(ab.z) * 0.6;
                let start = pa + dir * ra;
                let end = pb - dir * rb;
                Transform {
                    translation: (start + end) * 0.5 + offset,
                    rotation: rot,
                    scale: Vec3::new(b.radius, (len - ra - rb).max(0.02), b.radius),
                }
            }
            Some((i, n)) => {
                // Ionic: a dotted line of beads between the two ions.
                let ra = atom_radius(aa.z) * 1.1;
                let rb = atom_radius(ab.z) * 1.1;
                let span = (len - ra - rb).max(0.1);
                let step = span / n as f32;
                Transform::from_translation(pa + dir * (ra + step * (i as f32 + 0.5)))
                    .with_scale(Vec3::splat(b.radius))
            }
        };
        if *tr != new {
            *tr = new;
        }
    }

    for (mut tr, d) in &mut dots {
        let Some(a) = lab.atom(d.atom) else { continue };
        let Some(&dir) = a.domains.get(d.domain) else {
            continue;
        };
        let r = atom_radius(a.z);
        let new = if d.side == 0.0 {
            Transform::from_translation(a.pos + dir * (r + 0.17)).with_scale(Vec3::splat(0.085))
        } else {
            let perp = dir
                .cross(view_dir)
                .try_normalize()
                .unwrap_or_else(|| dir.any_orthonormal_vector());
            Transform::from_translation(a.pos + dir * (r + 0.12) + perp * d.side * 0.075)
                .with_scale(Vec3::splat(0.05))
        };
        if *tr != new {
            *tr = new;
        }
    }
}

/// Run the layout physics a few iterations per frame while awake.
pub fn step_physics(mut chem: ResMut<Chem>, drag: Res<Drag>) {
    let pinned = match *drag {
        Drag::Atom { atom, .. } => Some(atom),
        _ => None,
    };
    chem.lab.relax(3, pinned);
}
