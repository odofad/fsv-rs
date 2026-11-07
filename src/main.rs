mod node;
mod scan;
mod layout;

use std::f32::consts::PI;
use std::path::{PathBuf, Path};
use clap::Parser;
use bevy::input::mouse::MouseScrollUnit;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use bevy_egui::{egui, EguiContexts, EguiPlugin};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(default_value = ".")]
    path: PathBuf,
    #[arg(long)]
    print: bool,
    #[arg(long)]
    treev: bool,
}

#[derive(Resource)]
struct AppState {
    root: node::Node,
    mode: layout::LayoutMode,
    selected: Option<PathBuf>,
    /// Block under the cursor. Readout only, does not rebuild.
    hovered: Option<PathBuf>,
    /// color.c COLOR_BY_TIMESTAMP. Files take a rainbow, directories keep type.
    color_age: bool,
    /// World height the TreeV camera should be able to see.
    frame: f32,
    frame_pending: bool,
    /// Rebuild the meshes. Not Bevy's change flag: writing geometry
    /// onto the tree must not schedule another rebuild by itself.
    dirty: bool,
}

#[derive(Component)]
struct Pickable {
    path: PathBuf,
}

#[derive(Component)]
struct MapPiece;

struct FlyTo {
    start_focus: Vec3,
    start_yaw: f32,
    start_pitch: f32,
    start_dist: f32,
    end_focus: Vec3,
    end_yaw: f32,
    end_pitch: f32,
    end_dist: f32,
    mid_dist: f32,
    swing: bool,
    t: f32,
    duration: f32,
}

struct SavedCam {
    yaw: f32,
    pitch: f32,
    distance: f32,
    focus: Vec3,
}

#[derive(Component)]
struct OrbitCamera {
    yaw: f32,
    pitch: f32,
    distance: f32,
    focus: Vec3,
    fly: Option<FlyTo>,
    /// Toolbar bird's-eye toggle. Straight down, framing the root.
    birdseye: bool,
    /// Camera pose from just before the bird's-eye toggle went on.
    saved: Option<SavedCam>,
}

fn main() {
    let args = Args::parse();

    match scan::scan(&args.path) {
        Ok(root) => {
            if args.print {
                print_tree(&root, 0);
                return;
            }

            let mode = if args.treev {
                layout::LayoutMode::TreeV
            } else {
                layout::LayoutMode::MapV
            };

            App::new()
                .add_plugins(DefaultPlugins)
                .add_plugins(EguiPlugin)
                .insert_resource(AppState {
                    root,
                    mode,
                    selected: None,
                    hovered: None,
                    color_age: true,
                    frame: 36.0,
                    frame_pending: true,
                    dirty: true,
                })
                .add_systems(Startup, setup)
                .add_systems(Update, (orbit_camera, hover_pick, click_expand, face_labels, side_panel, rebuild_layout, apply_frame))
                .run();
        }
        Err(e) => {
            eprintln!("scan error: {}", e);
            std::process::exit(1);
        }
    }
}

fn setup(
    mut commands: Commands,
    mut state: ResMut<AppState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(8.0, 26.0, 18.0).looking_at(Vec3::ZERO, Vec3::Y),
        OrbitCamera {
            yaw: 0.5,
            pitch: -0.85,
            distance: 32.0,
            focus: Vec3::ZERO,
            fly: None,
            birdseye: false,
            saved: None,
        },
    ));

    commands.spawn((DirectionalLight {
        illuminance: 22_000.0,
        shadows_enabled: false,
        ..default()
    }, Transform::from_xyz(8.0, 24.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y)));

    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 400.0,
        ..default()
    });

    spawn_geometry(&mut commands, &mut state, &mut meshes, &mut materials);
    state.dirty = false;
}

fn apply_frame(mut state: ResMut<AppState>, mut orbits: Query<&mut OrbitCamera>) {
    if !state.frame_pending || state.dirty {
        return;
    }
    state.frame_pending = false;
    if !matches!(state.mode, layout::LayoutMode::TreeV) {
        return;
    }
    let Ok(mut orbit) = orbits.get_single_mut() else { return; };
    // camera.c frames TreeV with the tallest leaf, not just the footprint.
    let dist = (state.frame * 1.15).clamp(18.0, 700.0);
    orbit.distance = dist;
    orbit.pitch = -0.65;
    orbit.fly = None;
}

fn orbit_camera(
    time: Res<Time>,
    mut cameras: Query<(&mut Transform, &mut OrbitCamera)>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut scroll: EventReader<bevy::input::mouse::MouseWheel>,
    mut motion: EventReader<bevy::input::mouse::MouseMotion>,
    mut egui: EguiContexts,
) {
    let Ok((mut transform, mut orbit)) = cameras.get_single_mut() else { return; };

    let over_ui = egui.ctx_mut().is_pointer_over_area();

    let mut delta = Vec2::ZERO;
    for ev in motion.read() {
        delta += ev.delta;
    }

    let mut manual = false;

    if !over_ui && buttons.pressed(MouseButton::Middle) {
        manual = true;
        orbit.yaw -= delta.x * 0.005;
        orbit.pitch = (orbit.pitch - delta.y * 0.005).clamp(-1.2, 1.2);
    }

    if !over_ui && buttons.pressed(MouseButton::Right) {
        manual = true;
        let right = transform.right();
        let up = transform.up();
        let pan = orbit.distance * 0.0016;
        orbit.focus += (-right * delta.x + up * delta.y) * pan;
    }

    for ev in scroll.read() {
        if over_ui {
            continue;
        }
        manual = true;
        let ticks = match ev.unit {
            MouseScrollUnit::Line => ev.y,
            MouseScrollUnit::Pixel => ev.y / 60.0,
        };
        let zoom = ticks * orbit.distance * 0.12;
        orbit.distance = (orbit.distance - zoom).clamp(1.5, 400.0);
    }

    if manual {
        orbit.fly = None;
    } else if orbit.fly.is_some() {
        step_fly(&mut orbit, time.delta_secs());
    }

    apply_orbit(&mut transform, &orbit);
}

/// Yaw then pitch, no roll. Local -Z points at the focus, so a pitch of
/// -90 degrees sits straight above it without `look_at` flipping the heading.
fn apply_orbit(transform: &mut Transform, orbit: &OrbitCamera) {
    let rot = Quat::from_euler(EulerRot::YXZ, orbit.yaw, orbit.pitch, 0.0);
    transform.translation = orbit.focus + rot * Vec3::new(0.0, 0.0, orbit.distance);
    transform.rotation = rot;
}

fn step_fly(orbit: &mut OrbitCamera, dt: f32) {
    let Some(mut fly) = orbit.fly.take() else { return; };
    fly.t = (fly.t + dt / fly.duration.max(0.05)).min(1.0);
    let s = smoothstep(fly.t);
    orbit.focus = fly.start_focus.lerp(fly.end_focus, s);
    orbit.yaw = lerp_angle(fly.start_yaw, fly.end_yaw, s);
    orbit.pitch = lerp_angle(fly.start_pitch, fly.end_pitch, s);
    orbit.distance = if fly.swing {
        if fly.t < 0.5 {
            lerp(fly.start_dist, fly.mid_dist, smoothstep(fly.t * 2.0))
        } else {
            lerp(fly.mid_dist, fly.end_dist, smoothstep((fly.t - 0.5) * 2.0))
        }
    } else {
        lerp(fly.start_dist, fly.end_dist, s)
    };
    if fly.t < 1.0 {
        orbit.fly = Some(fly);
    }
}

fn begin_fly(orbit: &mut OrbitCamera, translation: Vec3, scale: Vec3, is_dir: bool, fov: f32) {
    let mut focus = translation + Vec3::new(0.0, scale.y * 0.5, 0.0);
    if is_dir {
        focus.y += scale.y * 0.25;
    }

    let width = scale.x.max(0.05);
    let depth = scale.z.max(0.05);
    let height = scale.y.max(0.05);
    let footprint = (width * depth).sqrt();
    let mut diameter = std::f32::consts::SQRT_2 * footprint.max(0.5 * width.max(depth));
    let k = if is_dir {
        diameter = diameter.max(width.max(depth * 1.5)).max(height * 2.0);
        1.35
    } else {
        2.2
    };
    let half = (fov * 0.5).max(0.2);
    let end_dist = (k * diameter * (0.5 / half.tan())).clamp(1.8, 80.0);

    // Same heading rule as fsv mapv_camera_theta: a bit more side-on
    // toward the edges of the map, about 50 degrees up.
    let x_norm = (focus.x / 20.0).clamp(-0.45, 0.45);
    let theta = (270.0 + 45.0 * x_norm).to_radians();
    let phi = if is_dir { 50.0_f32 } else { 40.0 };
    let end_pitch = -phi.to_radians();
    let end_yaw = theta.cos().atan2(theta.sin());

    let travel = orbit.focus.distance(focus);
    let duration = (0.55 + travel * 0.06).clamp(0.55, 1.6);
    let swing = travel > 3.0 * orbit.distance.max(end_dist);
    let mid_dist = if swing {
        1.15 * end_dist.max(travel * 0.65)
    } else {
        (orbit.distance + end_dist) * 0.5
    };

    // A click is a look-at. The original leaves bird's-eye for that,
    // and it does not restore the old pose first.
    orbit.birdseye = false;
    orbit.saved = None;
    orbit.fly = Some(FlyTo {
        start_focus: orbit.focus,
        start_yaw: orbit.yaw,
        start_pitch: orbit.pitch,
        start_dist: orbit.distance,
        end_focus: focus,
        end_yaw,
        end_pitch,
        end_dist,
        mid_dist,
        swing,
        t: 0.0,
        duration,
    });
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut d = (b - a) % (PI * 2.0);
    if d > PI {
        d -= PI * 2.0;
    }
    if d < -PI {
        d += PI * 2.0;
    }
    a + d * t
}

fn spawn_geometry(
    commands: &mut Commands,
    state: &mut AppState,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
) {
    let result = layout::layout(&mut state.root, state.mode);
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let line_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.75, 0.78, 0.85),
        unlit: true,
        ..default()
    });
    let folder_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.1, 0.14),
        unlit: true,
        ..default()
    });
    let cursor_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 1.0, 1.0),
        unlit: true,
        ..default()
    });

    let mut top = result.span;
    for geom in result.boxes {
        top = top.max(geom.z + geom.height * 0.5);
        let mut color = if state.color_age {
            match geom.age {
                Some(age) => age_color(age),
                None => color_for_kind(&geom.kind),
            }
        } else {
            color_for_kind(&geom.kind)
        };
        if geom.floor {
            let [r, g, b, a] = color.to_srgba().to_f32_array();
            color = Color::srgba(r * 0.12, g * 0.12, b * 0.12, a);
        }

        let mesh = meshes.add(slanted_box(geom.slant_x, geom.slant_y));
        let translation = Vec3::new(geom.x, geom.z, geom.y);
        let scale = Vec3::new(geom.width.max(0.01), geom.height.max(0.01), geom.depth.max(0.01));

        commands.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: 0.55,
                ..default()
            })),
            Transform {
                translation,
                rotation: Quat::from_rotation_y(geom.yaw),
                scale,
            },
            Pickable { path: geom.path.clone() },
            MapPiece,
        ));

        if geom.folder {
            let pts = layout::folder_outline(geom.slant_x, geom.slant_y);
            let y = translation.y + scale.y * 0.5 + 0.012;
            let thick = (scale.x.min(scale.z) * 0.012).clamp(0.008, 0.04);
            let yaw_q = Quat::from_rotation_y(geom.yaw);
            for w in pts.windows(2) {
                let la = yaw_q * Vec3::new(w[0][0] * scale.x, 0.0, w[0][1] * scale.z);
                let lb = yaw_q * Vec3::new(w[1][0] * scale.x, 0.0, w[1][1] * scale.z);
                let a = Vec3::new(translation.x + la.x, y, translation.z + la.z);
                let b = Vec3::new(translation.x + lb.x, y, translation.z + lb.z);
                spawn_segment(commands, cube.clone(), folder_mat.clone(), a, b, thick);
            }
        }

        if geom.label && !geom.name.is_empty() {
            spawn_label(commands, &geom, translation, scale);
        }

        if state.selected.as_ref() == Some(&geom.path) {
            spawn_cursor(commands, cube.clone(), cursor_mat.clone(), translation, scale, geom.yaw);
        }
    }

    for line in result.lines {
        let from = Vec3::new(line.from[0], line.from[1], line.from[2]);
        let to = Vec3::new(line.to[0], line.to[1], line.to[2]);
        spawn_segment(commands, cube.clone(), line_mat.clone(), from, to, 0.02);
    }

    for arc in result.arcs {
        spawn_arc(commands, meshes, materials, &arc);
    }

    state.frame = top.max(8.0);
}

/// Unit box (-0.5..0.5) with the top face inset. Scale it by the block size.
fn slanted_box(ox: f32, oy: f32) -> Mesh {
    let ox = ox.clamp(0.0, 0.45);
    let oy = oy.clamp(0.0, 0.45);
    let b = [
        Vec3::new(-0.5, -0.5, -0.5),
        Vec3::new(0.5, -0.5, -0.5),
        Vec3::new(0.5, -0.5, 0.5),
        Vec3::new(-0.5, -0.5, 0.5),
    ];
    let t = [
        Vec3::new(-0.5 + ox, 0.5, -0.5 + oy),
        Vec3::new(0.5 - ox, 0.5, -0.5 + oy),
        Vec3::new(0.5 - ox, 0.5, 0.5 - oy),
        Vec3::new(-0.5 + ox, 0.5, 0.5 - oy),
    ];
    let quads = [
        [b[0], b[1], b[2], b[3]],
        [t[0], t[3], t[2], t[1]],
        [b[0], t[0], t[1], b[1]],
        [b[1], t[1], t[2], b[2]],
        [b[2], t[2], t[3], b[3]],
        [b[3], t[3], t[0], b[0]],
    ];

    let mut positions = Vec::with_capacity(24);
    let mut normals = Vec::with_capacity(24);
    let mut uvs = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for quad in quads {
        let n = (quad[1] - quad[0]).cross(quad[2] - quad[0]).normalize_or_zero();
        let base = positions.len() as u32;
        for p in quad {
            positions.push([p.x, p.y, p.z]);
            normals.push([n.x, n.y, n.z]);
            uvs.push([0.0, 0.0]);
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

fn spawn_segment(
    commands: &mut Commands,
    cube: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
    from: Vec3,
    to: Vec3,
    thickness: f32,
) {
    let mid = (from + to) * 0.5;
    let dir = to - from;
    let len = dir.length().max(0.001);
    let rot = Quat::from_rotation_arc(Vec3::Y, dir.normalize_or_zero());
    commands.spawn((
        Mesh3d(cube),
        MeshMaterial3d(mat),
        Transform {
            translation: mid,
            rotation: rot,
            scale: Vec3::new(thickness, len, thickness),
        },
        MapPiece,
    ));
}

#[derive(Component)]
struct FaceLabel {
    name: String,
    center: Vec3,
    axis_u: Vec3,
    axis_v: Vec3,
}

fn spawn_label(commands: &mut Commands, geom: &layout::BoxGeom, translation: Vec3, scale: Vec3) {
    // mapv_apply_label: 0.8125 of the top width, (2 - phi) of the depth, black.
    let shown: String = geom.name.chars().take(48).collect();
    let top_w = scale.x * (1.0 - 2.0 * geom.slant_x) * 0.8125;
    let top_d = scale.z * (1.0 - 2.0 * geom.slant_y) * 0.382;
    if top_w < 0.02 || top_d < 0.02 || shown.is_empty() {
        return;
    }
    let yaw_q = Quat::from_rotation_y(geom.yaw);
    commands.spawn((
        FaceLabel {
            name: shown,
            center: Vec3::new(translation.x, translation.y + scale.y * 0.5 + 0.03, translation.z),
            axis_u: yaw_q * Vec3::new(top_w, 0.0, 0.0),
            axis_v: yaw_q * Vec3::new(0.0, 0.0, top_d),
        },
        MapPiece,
    ));
}

fn spawn_arc(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    arc: &layout::ArcGeom,
) {
    let (mesh, translation, scale) = arc_mesh(arc);
    let color = if arc.branch {
        Color::srgb(0.45, 0.08, 0.08)
    } else {
        Color::srgb(0.22, 0.42, 0.72)
    };
    let mut entity = commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.7,
            ..default()
        })),
        Transform {
            translation,
            scale,
            ..default()
        },
        MapPiece,
    ));
    if !arc.branch {
        entity.insert(Pickable {
            path: arc.path.clone(),
        });
    }
    if arc.label && !arc.name.is_empty() {
        let mid = (arc.theta0 + arc.theta1) * 0.5;
        let r = arc.r0 + (arc.r1 - arc.r0) * 0.18;
        let pos = Vec3::new(r * mid.cos(), translation.y + scale.y * 0.5 + 0.04, r * mid.sin());
        let tangent = Vec3::new(-mid.sin(), 0.0, mid.cos());
        let radial = Vec3::new(mid.cos(), 0.0, mid.sin());
        let span = (arc.theta1 - arc.theta0).abs().max(0.05);
        let shown: String = arc.name.chars().take(48).collect();
        commands.spawn((
            FaceLabel {
                name: shown,
                center: pos,
                axis_u: tangent * (r * span * 0.55),
                axis_v: radial * ((arc.r1 - arc.r0).abs() * 0.22).max(0.15),
            },
            MapPiece,
        ));
    }
}

fn arc_mesh(arc: &layout::ArcGeom) -> (Mesh, Vec3, Vec3) {
    let mut theta0 = arc.theta0;
    let mut theta1 = arc.theta1;
    if theta1 < theta0 {
        std::mem::swap(&mut theta0, &mut theta1);
    }
    // Don't fatten a thin branch. 0.01 rad is a whole block at these radii.
    let span = (theta1 - theta0).max(0.0002);
    let segs = ((span / 0.09).ceil() as usize).clamp(2, 96);
    let y0 = arc.y;
    let y1 = arc.y + arc.height.max(0.01);
    let mut pts = Vec::with_capacity((segs + 1) * 2);
    for s in 0..=segs {
        let t = theta0 + span * (s as f32 / segs as f32);
        let (sin, cos) = t.sin_cos();
        pts.push(Vec3::new(arc.r0 * cos, y0, arc.r0 * sin));
        pts.push(Vec3::new(arc.r1 * cos, y1, arc.r1 * sin));
    }
    let mut min = pts[0];
    let mut max = pts[0];
    for p in &pts {
        min = min.min(*p);
        max = max.max(*p);
    }
    min.y = min.y.min(y0);
    max.y = max.y.max(y1);
    let center = (min + max) * 0.5;
    let size = (max - min).max(Vec3::splat(0.02));
    // Rebuild a proper quad mesh in local space so the picker AABB matches.
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    let to_local = |p: Vec3| (p - center) / size;
    let mut ring = Vec::with_capacity(segs + 1);
    for s in 0..=segs {
        let t = theta0 + span * (s as f32 / segs as f32);
        let (sin, cos) = t.sin_cos();
        let inner = Vec3::new(arc.r0 * cos, 0.0, arc.r0 * sin);
        let outer = Vec3::new(arc.r1 * cos, 0.0, arc.r1 * sin);
        ring.push((inner, outer));
    }
    let push_quad = |positions: &mut Vec<[f32; 3]>,
                     normals: &mut Vec<[f32; 3]>,
                     uvs: &mut Vec<[f32; 2]>,
                     indices: &mut Vec<u32>,
                     quad: [Vec3; 4]| {
        let n = (quad[1] - quad[0]).cross(quad[2] - quad[0]).normalize_or_zero();
        let base = positions.len() as u32;
        for p in quad {
            let l = to_local(p);
            positions.push([l.x, l.y, l.z]);
            normals.push([n.x, n.y, n.z]);
            uvs.push([0.0, 0.0]);
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    for s in 0..segs {
        let (i0, o0) = ring[s];
        let (i1, o1) = ring[s + 1];
        let b0 = Vec3::new(i0.x, y0, i0.z);
        let b1 = Vec3::new(o0.x, y0, o0.z);
        let b2 = Vec3::new(i1.x, y0, i1.z);
        let b3 = Vec3::new(o1.x, y0, o1.z);
        let t0 = Vec3::new(i0.x, y1, i0.z);
        let t1 = Vec3::new(o0.x, y1, o0.z);
        let t2 = Vec3::new(i1.x, y1, i1.z);
        let t3 = Vec3::new(o1.x, y1, o1.z);
        push_quad(&mut positions, &mut normals, &mut uvs, &mut indices, [t0, t2, t3, t1]);
        push_quad(&mut positions, &mut normals, &mut uvs, &mut indices, [b0, b1, b3, b2]);
        push_quad(&mut positions, &mut normals, &mut uvs, &mut indices, [b0, t0, t2, b2]);
        push_quad(&mut positions, &mut normals, &mut uvs, &mut indices, [b1, b3, t3, t1]);
    }
    let (i0, o0) = ring[0];
    let (i1, o1) = ring[segs];
    push_quad(
        &mut positions,
        &mut normals,
        &mut uvs,
        &mut indices,
        [
            Vec3::new(i0.x, y0, i0.z),
            Vec3::new(o0.x, y0, o0.z),
            Vec3::new(o0.x, y1, o0.z),
            Vec3::new(i0.x, y1, i0.z),
        ],
    );
    push_quad(
        &mut positions,
        &mut normals,
        &mut uvs,
        &mut indices,
        [
            Vec3::new(i1.x, y1, i1.z),
            Vec3::new(o1.x, y1, o1.z),
            Vec3::new(o1.x, y0, o1.z),
            Vec3::new(i1.x, y0, i1.z),
        ],
    );
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    (mesh, center, size)
}

fn spawn_cursor(
    commands: &mut Commands,
    cube: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
    translation: Vec3,
    scale: Vec3,
    yaw: f32,
) {
    let yaw_q = Quat::from_rotation_y(yaw);
    let half = scale * 0.52;
    let c0 = translation - half;
    let c1 = translation + half;
    let bar = (c1 - c0) * layout::cursor_bar();
    let thick = (scale.x.min(scale.y).min(scale.z) * 0.04).clamp(0.01, 0.05);
    for c in 0..8 {
        let (px, dx) = if c & 1 != 0 {
            (c1.x, -bar.x)
        } else {
            (c0.x, bar.x)
        };
        let (py, dy) = if c & 2 != 0 {
            (c1.y, -bar.y)
        } else {
            (c0.y, bar.y)
        };
        let (pz, dz) = if c & 4 != 0 {
            (c1.z, -bar.z)
        } else {
            (c0.z, bar.z)
        };
        let p = translation + yaw_q * (Vec3::new(px, py, pz) - translation);
        spawn_segment(commands, cube.clone(), mat.clone(), p, p + yaw_q * Vec3::new(dx, 0.0, 0.0), thick);
        spawn_segment(commands, cube.clone(), mat.clone(), p, p + yaw_q * Vec3::new(0.0, dy, 0.0), thick);
        spawn_segment(commands, cube.clone(), mat.clone(), p, p + yaw_q * Vec3::new(0.0, 0.0, dz), thick);
    }
}

fn color_for_kind(kind: &node::NodeType) -> Color {
    match kind {
        node::NodeType::Directory => Color::srgb(0.3, 0.6, 1.0),
        node::NodeType::File => Color::srgb(0.9, 0.8, 0.3),
        node::NodeType::Symlink => Color::srgb(1.0, 0.5, 0.8),
        node::NodeType::Other => Color::srgb(0.6, 0.6, 0.6),
    }
}

/// color.c rainbow_color(1 - x). Old files sit at the blue end, new ones at red.
fn age_color(age: f32) -> Color {
    let hue = (1.0 - age.clamp(0.0, 1.0)) * 270.0;
    Color::hsl(hue, 0.78, 0.55)
}

fn click_expand(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform, &Projection)>,
    mut orbits: Query<&mut OrbitCamera>,
    pickables: Query<(&Pickable, &Transform)>,
    mut state: ResMut<AppState>,
    mut egui: EguiContexts,
) {
    if egui.ctx_mut().is_pointer_over_area() {
        return;
    }

    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(window) = windows.get_single() else { return; };
    let Some(cursor) = window.cursor_position() else { return; };
    let Ok((camera, cam_transform, projection)) = cameras.get_single() else { return; };

    let Some(ray) = camera.viewport_to_world(cam_transform, cursor).ok() else { return; };

    let mut best: Option<(f32, PathBuf, Vec3, Vec3)> = None;
    for (pick, transform) in &pickables {
        let half = transform.scale * 0.5;
        let min = transform.translation - half;
        let max = transform.translation + half;
        if let Some(dist) = ray_aabb(ray.origin, *ray.direction, min, max) {
            if best.as_ref().map_or(true, |(d, _, _, _)| dist < *d) {
                best = Some((
                    dist,
                    pick.path.clone(),
                    transform.translation,
                    transform.scale,
                ));
            }
        }
    }

    let Some((_, path, translation, scale)) = best else { return; };
    let is_dir = is_dir_path(&state.root, &path);
    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        Projection::Orthographic(_) => std::f32::consts::FRAC_PI_4,
    };
    if let Ok(mut orbit) = orbits.get_single_mut() {
        begin_fly(&mut orbit, translation, scale, is_dir, fov);
    }
    state.selected = Some(path.clone());
    state.dirty = true;
    toggle_expanded(&mut state.root, &path);
}

fn is_dir_path(node: &node::Node, path: &Path) -> bool {
    if node.path == path {
        return node.is_dir();
    }
    node.children.iter().any(|child| is_dir_path(child, path))
}

fn ray_aabb(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let inv = Vec3::new(
        if dir.x != 0.0 { 1.0 / dir.x } else { f32::INFINITY },
        if dir.y != 0.0 { 1.0 / dir.y } else { f32::INFINITY },
        if dir.z != 0.0 { 1.0 / dir.z } else { f32::INFINITY },
    );
    let t1 = (min - origin) * inv;
    let t2 = (max - origin) * inv;
    let tmin = t1.min(t2);
    let tmax = t1.max(t2);
    let t_near = tmin.x.max(tmin.y).max(tmin.z);
    let t_far = tmax.x.min(tmax.y).min(tmax.z);
    if t_near <= t_far && t_far >= 0.0 {
        Some(t_near.max(0.0))
    } else {
        None
    }
}

fn toggle_expanded(node: &mut node::Node, path: &Path) -> bool {
    if node.path == path {
        if node.is_dir() {
            node.expanded = !node.expanded;
            return true;
        }
        return false;
    }
    for child in &mut node.children {
        if toggle_expanded(child, path) {
            return true;
        }
    }
    false
}

fn rebuild_layout(
    mut commands: Commands,
    mut state: ResMut<AppState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<Entity, With<MapPiece>>,
) {
    if !state.dirty {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    spawn_geometry(&mut commands, &mut state, &mut meshes, &mut materials);
    state.dirty = false;
}

fn toggle_birdseye(orbit: &mut OrbitCamera, root_width: f32, fov: f32) {
    if orbit.birdseye {
        orbit.birdseye = false;
        if let Some(saved) = orbit.saved.take() {
            fly_between(
                orbit,
                saved.focus,
                saved.yaw,
                saved.pitch,
                saved.distance,
            );
        }
        return;
    }

    orbit.saved = Some(SavedCam {
        yaw: orbit.yaw,
        pitch: orbit.pitch,
        distance: orbit.distance,
        focus: orbit.focus,
    });
    orbit.birdseye = true;
    // camera_birdseye_view, MapV: phi = 90, theta = 270,
    // distance = field_distance(fov, root width).
    // Yaw 0 with pitch -90 puts label-up toward -Z, so names stay readable.
    let half = (fov * 0.5).max(0.2);
    let distance = (root_width * (0.5 / half.tan())).clamp(2.0, 400.0);
    fly_between(
        orbit,
        orbit.focus,
        0.0,
        -std::f32::consts::FRAC_PI_2,
        distance,
    );
}

fn fly_between(orbit: &mut OrbitCamera, focus: Vec3, yaw: f32, pitch: f32, distance: f32) {
    let travel = orbit.focus.distance(focus) + (orbit.distance - distance).abs() * 0.15;
    let duration = (0.55 + travel * 0.06).clamp(0.7, 1.6);
    orbit.fly = Some(FlyTo {
        start_focus: orbit.focus,
        start_yaw: orbit.yaw,
        start_pitch: orbit.pitch,
        start_dist: orbit.distance,
        end_focus: focus,
        end_yaw: yaw,
        end_pitch: pitch,
        end_dist: distance,
        mid_dist: (orbit.distance + distance) * 0.5,
        swing: false,
        t: 0.0,
        duration,
    });
}


fn hover_pick(
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    pickables: Query<(&Pickable, &Transform)>,
    mut state: ResMut<AppState>,
    mut egui: EguiContexts,
) {
    if egui.ctx_mut().is_pointer_over_area() {
        return;
    }
    let Ok(window) = windows.get_single() else { return; };
    let Some(cursor) = window.cursor_position() else {
        state.hovered = None;
        return;
    };
    let Ok((camera, cam_transform)) = cameras.get_single() else { return; };
    let Some(ray) = camera.viewport_to_world(cam_transform, cursor).ok() else { return; };

    let mut best: Option<(f32, PathBuf)> = None;
    for (pick, transform) in &pickables {
        let half = transform.scale * 0.5;
        let min = transform.translation - half;
        let max = transform.translation + half;
        if let Some(dist) = ray_aabb(ray.origin, *ray.direction, min, max) {
            if best.as_ref().map_or(true, |(d, _)| dist < *d) {
                best = Some((dist, pick.path.clone()));
            }
        }
    }
    state.hovered = best.map(|(_, path)| path);
}

struct Readout {
    name: String,
    kind: String,
    size: String,
    tree: Option<String>,
    path: String,
}

fn describe(root: &node::Node, path: Option<&Path>) -> Option<Readout> {
    let path = path?;
    let node = find_node(root, path)?;
    let kind = match node.kind {
        node::NodeType::Directory => "Directory",
        node::NodeType::File => "File",
        node::NodeType::Symlink => "Symlink",
        node::NodeType::Other => "Other",
    };
    Some(Readout {
        name: node.name.clone(),
        kind: kind.to_string(),
        size: human_size(node.size),
        tree: node.is_dir().then(|| human_size(node.subtree_size)),
        path: node.path.display().to_string(),
    })
}

fn find_node<'a>(node: &'a node::Node, path: &Path) -> Option<&'a node::Node> {
    if node.path == path {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find_node(child, path) {
            return Some(found);
        }
    }
    None
}

/// The directory whose children belong in the file list.
/// A file points at its parent. Nothing selected means the root.
fn list_dir<'a>(root: &'a node::Node, selected: Option<&Path>) -> &'a node::Node {
    let Some(path) = selected else { return root; };
    let Some(node) = find_node(root, path) else { return root; };
    if node.is_dir() {
        return node;
    }
    node.path
        .parent()
        .and_then(|parent| find_node(root, parent))
        .unwrap_or(root)
}

struct FileRow {
    name: String,
    kind: &'static str,
    size: String,
    path: PathBuf,
    is_dir: bool,
}

fn file_rows(dir: &node::Node) -> Vec<FileRow> {
    let mut rows: Vec<FileRow> = dir
        .children
        .iter()
        .map(|child| {
            let kind = match child.kind {
                node::NodeType::Directory => "dir",
                node::NodeType::File => "file",
                node::NodeType::Symlink => "link",
                node::NodeType::Other => "other",
            };
            let bytes = if child.is_dir() { child.subtree_size } else { child.size };
            FileRow {
                name: child.name.clone(),
                kind,
                size: human_size(bytes),
                path: child.path.clone(),
                is_dir: child.is_dir(),
            }
        })
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

fn node_count(n: usize) -> String {
    match n {
        0 => String::new(),
        1 => "1 node".to_string(),
        n => format!("{n} nodes"),
    }
}

fn face_labels(
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    labels: Query<&FaceLabel>,
    mut egui: EguiContexts,
) {
    let Ok(window) = windows.get_single() else { return; };
    let Ok((camera, cam_transform)) = cameras.get_single() else { return; };
    let scale = window.scale_factor() as f32;
    if scale <= 0.0 {
        return;
    }
    let ctx = egui.ctx_mut().clone();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("face-labels"),
    ));
    let color = egui::Color32::from_rgb(8, 10, 14);

    for label in &labels {
        let Ok(center) = camera.world_to_viewport(cam_transform, label.center) else { continue; };
        let Ok(end_u) = camera.world_to_viewport(cam_transform, label.center + label.axis_u) else { continue; };
        let Ok(end_v) = camera.world_to_viewport(cam_transform, label.center + label.axis_v) else { continue; };
        let px_w = (end_u - center).length() / scale;
        let px_h = (end_v - center).length() / scale;
        if px_w < 36.0 || px_h < 12.0 {
            continue;
        }
        let pos = egui::pos2(center.x / scale, center.y / scale);
        if ctx.layer_id_at(pos).is_some() {
            continue;
        }
        let mut font_size = (px_h * 0.85).clamp(10.0, 22.0);
        let mut shown = label.name.clone();
        for _ in 0..6 {
            let width = ctx.fonts(|fonts| {
                fonts
                    .layout_no_wrap(shown.clone(), egui::FontId::proportional(font_size), color)
                    .rect
                    .width()
            });
            if width <= px_w * 0.92 {
                break;
            }
            if shown.chars().count() > 4 && font_size <= 11.0 {
                let keep = shown.chars().count().saturating_sub(2);
                shown = shown.chars().take(keep.max(3)).collect();
                shown.push('…');
            } else {
                font_size = (font_size * 0.82).max(10.0);
            }
        }
        painter.text(
            pos,
            egui::Align2::CENTER_CENTER,
            shown,
            egui::FontId::proportional(font_size),
            color,
        );
    }
}

fn side_panel(
    mut contexts: EguiContexts,
    mut state: ResMut<AppState>,
    mut orbits: Query<&mut OrbitCamera>,
    projections: Query<&Projection>,
) {
    let mut changed = false;
    let bird_now = orbits.get_single().map(|o| o.birdseye).unwrap_or(false);
    let mut bird_on = bird_now;
    let focus = state.hovered.clone().or_else(|| state.selected.clone());
    let readout = describe(&state.root, focus.as_deref());
    let listed = list_dir(&state.root, state.selected.as_deref());
    let list_name = listed.name.clone();
    let rows = file_rows(listed);
    let count = node_count(rows.len());
    egui::SidePanel::left("tree_panel").show(contexts.ctx_mut(), |ui| {
        ui.heading("fsv-rs");
        ui.separator();

        let mode_label = match state.mode {
            layout::LayoutMode::MapV => "MapV",
            layout::LayoutMode::TreeV => "TreeV",
        };
        ui.label(format!("Mode: {}", mode_label));
        ui.label(match state.mode {
            layout::LayoutMode::MapV => "Area shows size. Height is fixed.",
            layout::LayoutMode::TreeV => "Platforms and branches. File height is size.",
        });

        if ui.button(if state.color_age { "Color: age" } else { "Color: type" }).clicked() {
            state.color_age = !state.color_age;
            changed = true;
        }

        if ui.button("Toggle MapV / TreeV").clicked() {
            state.mode = match state.mode {
                layout::LayoutMode::MapV => layout::LayoutMode::TreeV,
                layout::LayoutMode::TreeV => layout::LayoutMode::MapV,
            };
            state.frame_pending = true;
            changed = true;
        }

        if ui.toggle_value(&mut bird_on, "Bird's eye").changed() {
            if let Ok(mut orbit) = orbits.get_single_mut() {
                let fov = match projections.get_single() {
                    Ok(Projection::Perspective(p)) => p.fov,
                    _ => std::f32::consts::FRAC_PI_4,
                };
                let width = match state.mode {
                    layout::LayoutMode::MapV => layout::View::default().target_width as f32,
                    layout::LayoutMode::TreeV => 36.0,
                };
                toggle_birdseye(&mut orbit, width, fov);
            }
        }

        ui.separator();
        match readout {
            Some(info) => {
                ui.label(egui::RichText::new(info.name).strong());
                ui.label(info.kind);
                ui.label(format!("Size  {}", info.size));
                if let Some(tree) = info.tree {
                    ui.label(format!("Tree  {tree}"));
                }
                ui.label(egui::RichText::new(info.path).small());
            }
            None => {
                ui.label("Point at a block");
            }
        }

        ui.separator();
        ui.label(egui::RichText::new(list_name).strong());
        if !count.is_empty() {
            ui.label(count);
        }
        egui::ScrollArea::vertical()
            .id_salt("file_list")
            .max_height(320.0)
            .show(ui, |ui| {
                for row in &rows {
                    let text = format!("{:<5} {}   {}", row.kind, row.name, row.size);
                    let on = state.selected.as_ref() == Some(&row.path);
                    if ui.selectable_label(on, text).clicked() {
                        state.selected = Some(row.path.clone());
                        if row.is_dir && toggle_expanded(&mut state.root, &row.path) {
                            changed = true;
                        }
                    }
                }
            });

        ui.separator();
        egui::CollapsingHeader::new("Directory tree")
            .default_open(false)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("dir_tree")
                    .max_height(240.0)
                    .show(ui, |ui| {
                        changed |= show_node(ui, &mut state.root, 0);
                    });
            });
    });
    if changed {
        state.dirty = true;
    }
}

fn show_node(ui: &mut egui::Ui, node: &mut node::Node, depth: usize) -> bool {
    let indent = "  ".repeat(depth);
    let marker = if node.is_dir() {
        if node.expanded { "[-]" } else { "[+]" }
    } else {
        "   "
    };

    let size_str = human_size(node.subtree_size);
    let label = format!("{}{} {}  ({})", indent, marker, node.name, size_str);

    let mut changed = false;
    if ui.button(label).clicked() && node.is_dir() {
        node.expanded = !node.expanded;
        changed = true;
    }

    if node.expanded {
        for child in &mut node.children {
            changed |= show_node(ui, child, depth + 1);
        }
    }
    changed
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{:.1} {}", size, UNITS[unit])
    }
}

fn print_tree(node: &node::Node, depth: usize) {
    let indent = "  ".repeat(depth);
    let kind = match node.kind {
        node::NodeType::Directory => "dir",
        node::NodeType::File => "file",
        node::NodeType::Symlink => "link",
        node::NodeType::Other => "other",
    };
    let exp = if node.expanded { "+" } else { "-" };
    println!(
        "{}{} [{}] {} size={} subtree={}",
        indent, node.name, kind, exp, node.size, node.subtree_size
    );
    for child in &node.children {
        print_tree(child, depth + 1);
    }
}
