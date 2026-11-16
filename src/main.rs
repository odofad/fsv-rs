mod node;
mod scan;
mod layout;

#[cfg(test)]
mod golden;

use std::f32::consts::PI;
use std::path::{PathBuf, Path};
use clap::Parser;
use bevy::input::mouse::MouseScrollUnit;
use bevy::prelude::*;
use bevy::image::{Image, ImageSampler};
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::render_asset::RenderAssetUsages;
use bevy_egui::{egui, EguiContexts, EguiPlugin};
use font8x8::UnicodeFonts;

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Paint {
    Type,
    Age,
    Wild,
}

#[derive(Resource)]
struct AppState {
    root: node::Node,
    mode: layout::LayoutMode,
    selected: Option<PathBuf>,
    /// Block under the cursor. Readout only, does not rebuild.
    hovered: Option<PathBuf>,
    /// color.c. Directories keep the type color in every mode.
    paint: Paint,
    /// World size the TreeV camera should fit, and the point it looks at.
    frame: f32,
    focus: Vec3,
    frame_pending: bool,
    /// Clicked node. The fly starts after the rebuild, at the new spot.
    fly_to: Option<PathBuf>,
    /// Folder the camera was on before that click, so the flight starts there.
    fly_from: Option<PathBuf>,
    fly_from_pos: Vec3,
    fly_from_span: f32,
    fly_from_place: Option<(Vec3, Vec3)>,
    fly_to_place: Option<(Vec3, Vec3)>,
    fly_from_area: f32,
    fly_to_area: f32,
    /// Nodes the camera has already visited. Back walks this.
    history: Vec<PathBuf>,
    backtracking: bool,
    /// Properties window, from the list's right-click menu.
    props: Option<PathBuf>,
    root_input: String,
    root_error: String,
    /// Directories easing open or closed. TreeV only.
    deploys: Vec<DeployAnim>,
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

            let root_input = root.path.display().to_string();
            App::new()
                .add_plugins(DefaultPlugins)
                .add_plugins(EguiPlugin)
                .insert_resource(AppState {
                    root,
                    mode,
                    selected: None,
                    hovered: None,
                    paint: Paint::Type,
                    frame: 36.0,
                    focus: Vec3::ZERO,
                    frame_pending: true,
                    fly_to: None,
                    fly_from: None,
                    fly_from_pos: Vec3::ZERO,
                    fly_from_span: 1.0,
                    fly_from_place: None,
                    fly_to_place: None,
                    fly_from_area: 0.0,
                    fly_to_area: 0.0,
                    history: Vec::new(),
                    backtracking: false,
                    props: None,
                    root_input,
                    root_error: String::new(),
                    deploys: Vec::new(),
                    dirty: true,
                })
                .add_systems(Startup, setup)
                .add_systems(Update, (
                    side_panel,
                    face_labels.after(side_panel),
                    orbit_camera.after(side_panel),
                    hover_pick.after(side_panel),
                    click_expand.after(side_panel).before(rebuild_layout),
                    step_deploy.after(click_expand).before(rebuild_layout),
                    rebuild_layout,
                    apply_frame,
                    fly_to_opened.after(rebuild_layout),
                ))
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
    mut images: ResMut<Assets<Image>>,
) {
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 400.0,
        ..default()
    });

    commands.spawn((
        DirectionalLight {
            illuminance: 22_000.0,
            shadows_enabled: false,
            ..default()
        },
        Transform::from_xyz(8.0, 24.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    spawn_geometry(&mut commands, &mut state, &mut meshes, &mut materials, &mut images);
    state.dirty = false;

    let tree = matches!(state.mode, layout::LayoutMode::TreeV);
    let dist = if tree {
        (state.frame * 1.35).clamp(2.5, 24.0)
    } else {
        32.0
    };
    let focus = if tree { state.focus } else { Vec3::ZERO };
    let pitch = if tree { -1.15 } else { -0.85 };
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 60.0_f32.to_radians(),
            ..default()
        }),
        Transform::from_xyz(focus.x, focus.y + dist * 0.9, focus.z + dist * 0.4)
            .looking_at(focus, Vec3::Y),
        OrbitCamera {
            yaw: 0.0,
            pitch,
            distance: dist,
            focus,
            fly: None,
            birdseye: false,
            saved: None,
        },
    ));
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
    let dist = (state.frame * 1.35).clamp(2.5, 24.0);
    orbit.focus = state.focus;
    orbit.distance = dist;
    // Initial TreeV pitch is overhead. A shallow pitch turns the platform
    // into the stick in front of the core ring.
    orbit.pitch = -1.35;
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

    let over_ui = pointer_over_ui(egui.ctx_mut());

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

    // Keep the heading you already have. The flight is the focus moving
    // from the folder you were on to the one that just opened.
    let end_yaw = orbit.yaw;
    let end_pitch = orbit.pitch;
    let travel = orbit.focus.distance(focus);
    let duration = (2.4 + travel * 0.22).clamp(2.4, 5.5);
    let mid_dist = (orbit.distance + end_dist) * 0.5;

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
        swing: false,
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
    images: &mut ResMut<Assets<Image>>,
) {
    state.fly_from_place = None;
    state.fly_to_place = None;
    state.fly_from_area = 0.0;
    state.fly_to_area = 0.0;
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
        let mut color = match state.paint {
            Paint::Age => match geom.age {
                Some(age) => age_color(age),
                None => color_for_kind(&geom.kind),
            },
            Paint::Wild => wild_color(&geom.name, &geom.kind),
            Paint::Type => color_for_kind(&geom.kind),
        };
        if geom.floor {
            let [r, g, b, a] = color.to_srgba().to_f32_array();
            color = Color::srgba(r * 0.12, g * 0.12, b * 0.12, a);
        }

        let mesh = meshes.add(slanted_box(geom.slant_x, geom.slant_y));
        let mut translation = Vec3::new(geom.x, geom.z, geom.y);
        let mut scale = Vec3::new(geom.width.max(0.01), geom.height.max(0.01), geom.depth.max(0.01));
        grown(&mut translation, &mut scale, geom.grow, geom.anchor);
        if !geom.x_mark {
            note_place(state, &geom.path, translation, scale);
        }

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

        if geom.x_mark {
            let y = translation.y + scale.y * 0.5 + 0.012;
            let yaw_q = Quat::from_rotation_y(geom.yaw);
            let thick = (scale.x.min(scale.z) * 0.045).clamp(0.006, 0.035);
            let h = 0.40;
            let corner = |sx: f32, sz: f32| {
                let local = yaw_q * Vec3::new(sx * scale.x, 0.0, sz * scale.z);
                Vec3::new(translation.x + local.x, y, translation.z + local.z)
            };
            spawn_segment(commands, cube.clone(), folder_mat.clone(), corner(-h, -h), corner(h, h), thick);
            spawn_segment(commands, cube.clone(), folder_mat.clone(), corner(h, -h), corner(-h, h), thick);
        }

        if geom.label && !geom.name.is_empty() {
            if matches!(state.mode, layout::LayoutMode::TreeV) {
                spawn_label(commands, meshes, materials, images, &geom, translation, scale);
            } else {
                spawn_screen_label(commands, meshes, materials, images, &geom, translation, scale);
            }
        }

        if state.selected.as_ref() == Some(&geom.path) && !geom.x_mark {
            spawn_cursor(commands, cube.clone(), cursor_mat.clone(), translation, scale, geom.yaw);
        }
    }

    for line in result.lines {
        let from = Vec3::new(line.from[0], line.from[1], line.from[2]);
        let to = Vec3::new(line.to[0], line.to[1], line.to[2]);
        spawn_segment(commands, cube.clone(), line_mat.clone(), from, to, 0.02);
    }

    let root_path = state.root.path.clone();
    for arc in result.arcs {
        let current = match &state.selected {
            Some(path) => arc.path == *path,
            None => arc.path == root_path,
        };
        spawn_arc(commands, meshes, materials, images, &arc);
        if !arc.branch {
            let (translation, scale) = arc_focus(&arc);
            note_place(state, &arc.path, translation, scale);
        }
        if current && !arc.branch {
            spawn_arc_cursor(commands, cube.clone(), cursor_mat.clone(), &arc);
        }
    }

    state.frame = if matches!(state.mode, layout::LayoutMode::TreeV) {
        result.span
    } else {
        top.max(8.0)
    };
    state.focus = Vec3::new(result.focus[0], result.focus[1], result.focus[2]);
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

fn spawn_screen_label(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    images: &mut ResMut<Assets<Image>>,
    geom: &layout::BoxGeom,
    translation: Vec3,
    scale: Vec3,
) {
    let shown: String = geom.name.chars().take(24).collect();
    let (width, depth) = fit_label(scale.x * 0.72, scale.z * 0.5, shown.chars().count());
    if shown.is_empty() {
        return;
    }
    let center = Vec3::new(translation.x, translation.y + scale.y * 0.5 + 0.02, translation.z);
    spawn_flat_label(
        commands, meshes, materials, images, &shown, center, geom.yaw, width, depth,
        [10, 10, 14, 255],
    );
}

fn spawn_label(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    images: &mut ResMut<Assets<Image>>,
    geom: &layout::BoxGeom,
    translation: Vec3,
    scale: Vec3,
) {
    let shown: String = geom.name.chars().take(24).collect();
    let (width, depth) = fit_label(scale.x * 0.72, scale.z * 0.5, shown.chars().count());
    if shown.is_empty() {
        return;
    }
    let center = Vec3::new(translation.x, translation.y + scale.y * 0.5 + 0.006, translation.z);
    spawn_flat_label(
        commands, meshes, materials, images, &shown, center, geom.yaw, width, depth,
        [12, 12, 16, 255],
    );
}

/// Keep the bitmap's shape. A long name shrinks instead of being squashed.
fn fit_label(max_w: f32, max_d: f32, chars: usize) -> (f32, f32) {
    let aspect = chars.max(1) as f32;
    let mut width = max_w;
    let mut depth = width / aspect;
    if depth > max_d {
        depth = max_d;
        width = depth * aspect;
    }
    (width.max(0.02), depth.max(0.02))
}

/// String runs across the view, letter height points back toward the center.
fn label_yaw(theta: f32) -> f32 {
    (-theta.cos()).atan2(-theta.sin())
}
fn spawn_flat_label(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    images: &mut ResMut<Assets<Image>>,
    text: &str,
    center: Vec3,
    yaw: f32,
    width: f32,
    depth: f32,
    rgba: [u8; 4],
) {
    if text.is_empty() || width < 0.04 || depth < 0.02 {
        return;
    }
    let image = name_image(text, rgba);
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::new(Vec3::Y, Vec2::new(width * 0.5, depth * 0.5)))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(images.add(image)),
            base_color: Color::WHITE,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            ..default()
        })),
        Transform {
            translation: center,
            rotation: Quat::from_rotation_y(yaw),
            ..default()
        },
        MapPiece,
    ));
}

fn name_image(text: &str, rgba: [u8; 4]) -> Image {
    let chars: Vec<char> = text.chars().take(40).collect();
    let n = chars.len().max(1) as u32;
    let scale = 4u32;
    let cell = 8 * scale;
    let width = n * cell;
    let height = cell;
    let mut data = vec![0u8; (width * height * 4) as usize];
    for (i, ch) in chars.iter().enumerate() {
        let glyph = font8x8::BASIC_FONTS.get(*ch).unwrap_or([0u8; 8]);
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..8u32 {
                if bits & (1 << col) == 0 {
                    continue;
                }
                for sy in 0..scale {
                    for sx in 0..scale {
                        let x = i as u32 * cell + col * scale + sx;
                        let y = row as u32 * scale + sy;
                        let idx = ((y * width + x) * 4) as usize;
                        data[idx..idx + 4].copy_from_slice(&rgba);
                    }
                }
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::nearest();
    image
}

fn spawn_arc(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    images: &mut ResMut<Assets<Image>>,
    arc: &layout::ArcGeom,
) {
    let (mesh, mut translation, mut scale) = arc_mesh(arc);
    grown(&mut translation, &mut scale, arc.grow, arc.anchor);
    let color = if arc.branch {
        // geometry.c branch_color. Dark red, not the salmon of a new file.
        Color::srgb(0.5, 0.0, 0.0)
    } else {
        Color::srgb(0.22, 0.42, 0.72)
    };
    let mut entity = commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: color,
            unlit: true,
            cull_mode: None,
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
    if arc.label && !arc.name.is_empty() && arc.grow >= 0.98 {
        // treev_apply_label, platform case: curved along the inner edge.
        spawn_curved_label(commands, meshes, materials, images, arc);
    }
}

/// Platform name, one glyph per step along the inner edge.
/// text_draw_curved in the original. The letters lie flat and follow the arc.
fn spawn_curved_label(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    images: &mut ResMut<Assets<Image>>,
    arc: &layout::ArcGeom,
) {
    let chars: Vec<char> = arc.name.chars().take(24).collect();
    let n = chars.len();
    if n == 0 {
        return;
    }
    let span = (arc.theta1 - arc.theta0).abs().max(0.05);
    let usable = span * 0.85;
    let wire_top = arc.y + arc.height * (48.0 / 320.0);
    let glyph = 0.16_f32.min(arc.r0 * usable / n as f32).max(0.04);
    let r = (arc.r0 - glyph * 0.65).max(0.25);
    let step = (glyph / r).min(usable / n as f32);
    let mid = (arc.theta0 + arc.theta1) * 0.5;
    let start = mid - step * (n as f32 - 1.0) * 0.5;
    for (i, ch) in chars.into_iter().enumerate() {
        if ch == ' ' {
            continue;
        }
        let t = start + step * i as f32;
        let pos = Vec3::new(r * t.cos(), wire_top + 0.03, r * t.sin());
        spawn_flat_label(
            commands,
            meshes,
            materials,
            images,
            &ch.to_string(),
            pos,
            label_yaw(t),
            glyph,
            glyph,
            [255, 255, 255, 255],
        );
    }
}

fn grown(translation: &mut Vec3, scale: &mut Vec3, grow: f32, anchor: [f32; 3]) {
    if grow >= 0.999 {
        return;
    }
    let s = grow.max(0.02);
    let a = Vec3::new(anchor[0], anchor[1], anchor[2]);
    *translation = a + (*translation - a) * s;
    *scale *= s;
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

fn spawn_arc_cursor(
    commands: &mut Commands,
    cube: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
    arc: &layout::ArcGeom,
) {
    let bar = layout::cursor_bar();
    let dr = (arc.r1 - arc.r0).abs() * bar;
    let dth = (arc.theta1 - arc.theta0).abs() * bar;
    let dy = arc.height.max(0.01) * bar;
    let thick = ((arc.r1 - arc.r0).abs() * 0.015).clamp(0.012, 0.05);
    for (r, sgn_r) in [(arc.r0, 1.0_f32), (arc.r1, -1.0)] {
        for (th, sgn_th) in [(arc.theta0, 1.0_f32), (arc.theta1, -1.0)] {
            for (y, sgn_y) in [(arc.y, 1.0_f32), (arc.y + arc.height, -1.0)] {
                let (s, c) = th.sin_cos();
                let p = Vec3::new(r * c, y, r * s);
                let radial = Vec3::new(c, 0.0, s) * (dr * sgn_r);
                let (s2, c2) = (th + dth * sgn_th).sin_cos();
                let tangent = Vec3::new(r * c2, y, r * s2);
                spawn_segment(commands, cube.clone(), mat.clone(), p, p + radial, thick);
                spawn_segment(commands, cube.clone(), mat.clone(), p, tangent, thick);
                spawn_segment(commands, cube.clone(), mat.clone(), p, p + Vec3::new(0.0, dy * sgn_y, 0.0), thick);
            }
        }
    }
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
    // Directory, file, and symlink keep the colors this port already uses.
    // The rest are color.c default_nodetype_colors.
    match kind {
        node::NodeType::Directory => Color::srgb(0.3, 0.6, 1.0),
        node::NodeType::File => Color::srgb(0.9, 0.8, 0.3),
        node::NodeType::Symlink => Color::srgb(1.0, 0.5, 0.8),
        node::NodeType::Fifo => Color::srgb(0.0, 1.0, 0.0),
        node::NodeType::Socket => Color::srgb(1.0, 0.5, 0.0),
        node::NodeType::Char => Color::srgb(0.0, 1.0, 1.0),
        node::NodeType::Block => Color::srgb(0.3, 0.63, 1.0),
        node::NodeType::Unknown => Color::srgb(1.0, 0.0, 0.0),
    }
}

/// color.c wpattern_color, the built-in groups. Directories stay a type color.
/// `*.jpg` does not match `.jpg` or `*.JPG`.
fn wild_color(name: &str, kind: &node::NodeType) -> Color {
    if matches!(kind, node::NodeType::Directory) {
        return color_for_kind(kind);
    }
    const GROUPS: &[(&str, &[&str])] = &[
        ("#FF3333", &["*.arj", "*.gz", "*.lzh", "*.tar", "*.tgz", "*.z", "*.zip", "*.Z"]),
        ("#FF33FF", &["*.gif", "*.jpg", "*.png", "*.ppm", "*.tga", "*.tif", "*.xpm"]),
        ("#FFFFFF", &["*.au", "*.mov", "*.mp3", "*.mpg", "*.wav"]),
    ];
    for (hex, pats) in GROUPS {
        if pats.iter().any(|pat| wild_match(pat, name)) {
            return hex_color(hex);
        }
    }
    Color::srgb(1.0, 1.0, 0.627)
}

fn hex_color(hex: &str) -> Color {
    let n = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0);
    Color::srgb(
        ((n >> 16) & 255) as f32 / 255.0,
        ((n >> 8) & 255) as f32 / 255.0,
        (n & 255) as f32 / 255.0,
    )
}

fn wild_match(pat: &str, name: &str) -> bool {
    fn go(p: &[u8], n: &[u8], leading: bool) -> bool {
        if p.is_empty() {
            return n.is_empty();
        }
        match p[0] {
            b'*' => {
                if leading && n.first() == Some(&b'.') {
                    return false;
                }
                let rest = &p[1..];
                if go(rest, n, false) {
                    return true;
                }
                for i in 1..=n.len() {
                    if go(rest, &n[i..], false) {
                        return true;
                    }
                }
                false
            }
            b'?' => {
                if n.is_empty() || (leading && n[0] == b'.') {
                    return false;
                }
                go(&p[1..], &n[1..], false)
            }
            c => {
                if n.is_empty() || n[0] != c {
                    return false;
                }
                go(&p[1..], &n[1..], false)
            }
        }
    }
    go(pat.as_bytes(), name.as_bytes(), true)
}

/// color.c rainbow_color(1 - x). Old files sit at the blue end, new ones at red.
fn age_color(age: f32) -> Color {
    let hue = (1.0 - age.clamp(0.0, 1.0)) * 270.0;
    Color::hsl(hue, 0.78, 0.55)
}

fn click_expand(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    pickables: Query<(&Pickable, &Transform)>,
    mut state: ResMut<AppState>,
    mut egui: EguiContexts,
) {
    if pointer_over_ui(egui.ctx_mut()) {
        return;
    }

    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }

    let Ok(window) = windows.get_single() else { return; };
    let Some(cursor) = window.cursor_position() else { return; };
    let Ok((camera, cam_transform)) = cameras.get_single() else { return; };

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

    let Some((_, path, _, _)) = best else { return; };
    let from_path = state
        .selected
        .clone()
        .unwrap_or_else(|| state.root.path.clone());
    let (from_pos, from_span) = live_place(&pickables, &from_path);
    state.fly_from = Some(from_path);
    state.fly_from_pos = from_pos;
    state.fly_from_span = from_span;
    let open_dir = is_expanded_dir(&state.root, &path);
    state.selected = Some(path.clone());
    state.fly_to = Some(path.clone());
    state.dirty = true;
    if !open_dir {
        start_deploy(&mut state, &path, true);
    }
}

fn is_expanded_dir(node: &node::Node, path: &Path) -> bool {
    if node.path == path {
        return node.is_dir() && node.expanded;
    }
    node.children.iter().any(|child| is_expanded_dir(child, path))
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

struct DeployAnim {
    path: PathBuf,
    from: f32,
    to: f32,
    elapsed: f32,
}

fn start_deploy(state: &mut AppState, path: &Path, open: bool) {
    let to = if open { 1.0 } else { 0.0 };
    if !matches!(state.mode, layout::LayoutMode::TreeV) {
        set_expanded(&mut state.root, path, open);
        set_deployment(&mut state.root, path, to);
        state.dirty = true;
        return;
    }
    let current = deployment_of(&state.root, path).unwrap_or(0.0);
    if open {
        set_expanded(&mut state.root, path, true);
    }
    state.deploys.retain(|d| d.path != path);
    if (current - to).abs() < 0.02 {
        set_deployment(&mut state.root, path, to);
        set_expanded(&mut state.root, path, open);
        state.dirty = true;
        return;
    }
    state.deploys.push(DeployAnim {
        path: path.to_path_buf(),
        from: current,
        to,
        elapsed: 0.0,
    });
    state.dirty = true;
}

fn step_deploy(time: Res<Time>, mut state: ResMut<AppState>) {
    if state.deploys.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    let mut i = 0;
    while i < state.deploys.len() {
        state.deploys[i].elapsed += dt;
        let elapsed = state.deploys[i].elapsed;
        let from = state.deploys[i].from;
        let to = state.deploys[i].to;
        let path = state.deploys[i].path.clone();
        let p = (elapsed / 0.5).clamp(0.0, 1.0);
        // colexp.c: open is inverse quadratic, close is quadratic. Half a second.
        let eased = if to >= from {
            1.0 - (1.0 - p) * (1.0 - p)
        } else {
            p * p
        };
        let v = if p >= 1.0 { to } else { from + (to - from) * eased };
        set_deployment(&mut state.root, &path, v);
        if p >= 1.0 {
            if to < 0.5 {
                set_expanded(&mut state.root, &path, false);
            }
            state.deploys.remove(i);
        } else {
            i += 1;
        }
    }
    state.dirty = true;
}

fn deployment_of(node: &node::Node, path: &Path) -> Option<f32> {
    if node.path == path {
        return Some(node.deployment);
    }
    for child in &node.children {
        if let Some(v) = deployment_of(child, path) {
            return Some(v);
        }
    }
    None
}

fn set_deployment(node: &mut node::Node, path: &Path, value: f32) -> bool {
    if node.path == path {
        node.deployment = value;
        return true;
    }
    for child in &mut node.children {
        if set_deployment(child, path, value) {
            return true;
        }
    }
    false
}

fn set_expanded(node: &mut node::Node, path: &Path, open: bool) -> bool {
    if node.path == path {
        if node.is_dir() {
            node.expanded = open;
            return true;
        }
        return false;
    }
    for child in &mut node.children {
        if set_expanded(child, path, open) {
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
    mut images: ResMut<Assets<Image>>,
    existing: Query<Entity, With<MapPiece>>,
) {
    if !state.dirty {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    spawn_geometry(&mut commands, &mut state, &mut meshes, &mut materials, &mut images);
    state.dirty = false;
}

/// After the folder has opened and the meshes exist, glide to it.
fn fly_to_opened(
    mut state: ResMut<AppState>,
    mut orbits: Query<&mut OrbitCamera>,
    projections: Query<&Projection>,
) {
    if state.dirty {
        return;
    }
    let Some(path) = state.fly_to.clone() else { return; };
    let Some((translation, scale)) = state.fly_to_place else { return; };
    if let Some((from_pos, from_scale)) = state.fly_from_place {
        if let Ok(mut orbit) = orbits.get_single_mut() {
            let new_span = from_scale.x.max(from_scale.z).max(0.05);
            let old_len = state.fly_from_pos.length();
            let new_len = from_pos.length();
            let ratio = if old_len > 0.5 && new_len > 0.5 {
                new_len / old_len
            } else {
                new_span / state.fly_from_span.max(0.05)
            };
            orbit.focus = from_pos;
            orbit.distance = (orbit.distance * ratio).clamp(1.5, 400.0);
        }
    }
    state.fly_to = None;
    state.fly_from = None;
    let is_dir = is_dir_path(&state.root, &path);
    let fov = match projections.get_single() {
        Ok(Projection::Perspective(p)) => p.fov,
        _ => std::f32::consts::FRAC_PI_4,
    };
    if let Ok(mut orbit) = orbits.get_single_mut() {
        begin_fly(&mut orbit, translation, scale, is_dir, fov);
    }
}

fn live_place(pickables: &Query<(&Pickable, &Transform)>, path: &Path) -> (Vec3, f32) {
    let mut best: Option<(f32, Vec3, f32)> = None;
    for (pick, transform) in pickables {
        if pick.path != path {
            continue;
        }
        let span = transform.scale.x.max(transform.scale.z);
        let area = transform.scale.x * transform.scale.z;
        if best.as_ref().map_or(true, |(a, _, _)| area > *a) {
            best = Some((area, transform.translation, span));
        }
    }
    best.map(|(_, pos, span)| (pos, span.max(0.05))).unwrap_or((Vec3::ZERO, 1.0))
}

fn note_place(state: &mut AppState, path: &Path, translation: Vec3, scale: Vec3) {
    let area = scale.x * scale.z;
    if state.fly_from.as_deref() == Some(path.as_ref()) && area >= state.fly_from_area {
        state.fly_from_area = area;
        state.fly_from_place = Some((translation, scale));
    }
    if state.fly_to.as_deref() == Some(path.as_ref()) && area >= state.fly_to_area {
        state.fly_to_area = area;
        state.fly_to_place = Some((translation, scale));
    }
}

fn arc_focus(arc: &layout::ArcGeom) -> (Vec3, Vec3) {
    let mid = (arc.theta0 + arc.theta1) * 0.5;
    let span = (arc.theta1 - arc.theta0).abs().max(0.02);
    let r = (arc.r0 + arc.r1) * 0.5;
    let translation = Vec3::new(r * mid.cos(), arc.y + arc.height * 0.5, r * mid.sin());
    let scale = Vec3::new(
        (r * span).max(0.05),
        arc.height.max(0.05),
        (arc.r1 - arc.r0).abs().max(0.05),
    );
    (translation, scale)
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
    if pointer_over_ui(egui.ctx_mut()) {
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

struct FileRow {
    name: String,
    kind: &'static str,
    size: String,
    path: PathBuf,
    is_dir: bool,
    expanded: bool,
    has_sub: bool,
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

fn file_rows(dir: &node::Node) -> Vec<FileRow> {
    let rows: Vec<FileRow> = dir
        .children
        .iter()
        .map(|child| {
            let kind = match child.kind {
                node::NodeType::Directory => "dir",
                node::NodeType::File => "file",
                node::NodeType::Symlink => "link",
                node::NodeType::Fifo => "fifo",
                node::NodeType::Socket => "sock",
                node::NodeType::Char => "char",
                node::NodeType::Block => "block",
                node::NodeType::Unknown => "?",
            };
            let bytes = if child.is_dir() { child.subtree_size } else { child.size };
            FileRow {
                name: child.name.clone(),
                kind,
                size: human_size(bytes),
                path: child.path.clone(),
                is_dir: child.is_dir(),
                expanded: child.expanded,
                has_sub: child.children.iter().any(|c| c.is_dir()),
            }
        })
        .collect();
    rows
}

/// True once the side panel has been drawn this frame and the pointer is on it.
/// Checked after `side_panel`, because egui only knows the panel after that.
fn pointer_over_ui(ctx: &egui::Context) -> bool {
    ctx.is_pointer_over_area() || ctx.wants_pointer_input()
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
    pickables: Query<(&Pickable, &Transform)>,
) {
    let mut changed = false;
    let bird_now = orbits.get_single().map(|o| o.birdseye).unwrap_or(false);
    let mut bird_on = bird_now;
    let listed = list_dir(&state.root, state.selected.as_deref());
    let list_name = listed.name.clone();
    let rows = file_rows(listed);
    let count = node_count(rows.len());
    let selected_now = state.selected.clone();
    let mut select: Option<PathBuf> = None;
    let mut toggle: Option<PathBuf> = None;
    let mut menu: Option<MenuCmd> = None;

    egui::SidePanel::left("tree_panel")
        .resizable(true)
        .default_width(280.0)
        .show(contexts.ctx_mut(), |ui| {
            ui.horizontal(|ui| {
                let mode_label = match state.mode {
                    layout::LayoutMode::MapV => "MapV",
                    layout::LayoutMode::TreeV => "TreeV",
                };
                if ui.button(mode_label).clicked() {
                    state.mode = match state.mode {
                        layout::LayoutMode::MapV => layout::LayoutMode::TreeV,
                        layout::LayoutMode::TreeV => layout::LayoutMode::MapV,
                    };
                    state.frame_pending = true;
                    changed = true;
                }
                if ui.button(match state.paint {
                    Paint::Type => "Type",
                    Paint::Age => "Age",
                    Paint::Wild => "Wild",
                }).clicked() {
                    state.paint = match state.paint {
                        Paint::Type => Paint::Age,
                        Paint::Age => Paint::Wild,
                        Paint::Wild => Paint::Type,
                    };
                    changed = true;
                }
                if ui.button("Back").clicked() && !state.history.is_empty() {
                    if let Some(prev) = state.history.pop() {
                        state.backtracking = true;
                        queue_fly(&mut state, &pickables, prev.clone());
                        state.selected = Some(prev);
                        state.dirty = true;
                    }
                }
                if ui.button("Root").clicked() {
                    let path = state.root.path.clone();
                    queue_fly(&mut state, &pickables, path.clone());
                    state.selected = Some(path);
                    state.dirty = true;
                }
                if ui.button("Up").clicked() {
                    if let Some(path) = state.selected.clone().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
                        if find_node(&state.root, &path).is_some() {
                            queue_fly(&mut state, &pickables, path.clone());
                            state.selected = Some(path);
                            state.dirty = true;
                        }
                    }
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
            });

            let avail = ui.available_height();
            let tree_h = (avail * 0.55).clamp(100.0, avail - 96.0);
            ui.label(egui::RichText::new("Directory tree").small());
            egui::ScrollArea::vertical()
                .id_salt("dir_tree")
                .max_height(tree_h)
                .show(ui, |ui| {
                    show_dirs(
                        ui,
                        &mut state.root,
                        selected_now.as_deref(),
                        0,
                        &mut select,
                        &mut toggle,
                        &mut menu,
                    );
                });

            ui.separator();
            ui.label(egui::RichText::new(format!("{list_name}  {count}")).small());
            egui::ScrollArea::vertical()
                .id_salt("file_list")
                .max_height((avail - tree_h).max(80.0))
                .show(ui, |ui| {
                    for row in &rows {
                        let text = format!("{:<4}  {:<8}  {}", row.kind, row.size, row.name);
                        let on = state.selected.as_ref() == Some(&row.path);
                        let resp = ui.selectable_label(on, text);
                        if resp.clicked() {
                            select = Some(row.path.clone());
                        }
                        resp.context_menu(|ui| {
                            fill_menu(ui, &row.path, row.is_dir, row.expanded, row.has_sub, &mut menu);
                        });
                    }
                });
            ui.separator();
            ui.label("Change root");
            ui.text_edit_singleline(&mut state.root_input);
            if ui.button("Scan").clicked() {
                match scan::scan(Path::new(&state.root_input)) {
                    Ok(root) => {
                        state.root_input = root.path.display().to_string();
                        state.root = root;
                        state.selected = None;
                        state.hovered = None;
                        state.history.clear();
                        state.deploys.clear();
                        state.props = None;
                        state.root_error.clear();
                        state.frame_pending = true;
                        state.dirty = true;
                    }
                    Err(err) => state.root_error = err.to_string(),
                }
            }
            if !state.root_error.is_empty() {
                ui.label(egui::RichText::new(&state.root_error).small().color(egui::Color32::LIGHT_RED));
            }
        });

    if let Some(cmd) = menu {
        apply_menu(&mut state, &pickables, cmd);
    } else if let Some(path) = toggle {
        let was_open = is_expanded_dir(&state.root, &path);
        if !was_open {
            queue_fly(&mut state, &pickables, path.clone());
        }
        start_deploy(&mut state, &path, !was_open);
        state.selected = Some(path);
    } else if let Some(path) = select {
        queue_fly(&mut state, &pickables, path.clone());
        let open = is_expanded_dir(&state.root, &path);
        if !open {
            start_deploy(&mut state, &path, true);
        }
        state.selected = Some(path);
        state.dirty = true;
    }
    if changed {
        state.dirty = true;
    }
    show_props(contexts.ctx_mut(), &mut state);
}

enum MenuCmd {
    Expand(PathBuf),
    ExpandAll(PathBuf),
    Collapse(PathBuf),
    Look(PathBuf),
    Props(PathBuf),
}

/// dialog.c context_menu. Right click on a row, not on the view.
fn fill_menu(
    ui: &mut egui::Ui,
    path: &Path,
    is_dir: bool,
    expanded: bool,
    has_sub: bool,
    menu: &mut Option<MenuCmd>,
) {
    if is_dir {
        if expanded {
            if ui.button("Collapse").clicked() {
                *menu = Some(MenuCmd::Collapse(path.to_path_buf()));
                ui.close_menu();
            }
        } else {
            if ui.button("Expand").clicked() {
                *menu = Some(MenuCmd::Expand(path.to_path_buf()));
                ui.close_menu();
            }
            if has_sub && ui.button("Expand all").clicked() {
                *menu = Some(MenuCmd::ExpandAll(path.to_path_buf()));
                ui.close_menu();
            }
        }
    }
    if ui.button("Look at").clicked() {
        *menu = Some(MenuCmd::Look(path.to_path_buf()));
        ui.close_menu();
    }
    if ui.button("Properties").clicked() {
        *menu = Some(MenuCmd::Props(path.to_path_buf()));
        ui.close_menu();
    }
}

fn apply_menu(state: &mut AppState, pickables: &Query<(&Pickable, &Transform)>, cmd: MenuCmd) {
    match cmd {
        MenuCmd::Expand(path) => {
            queue_fly(state, pickables, path.clone());
            start_deploy(state, &path, true);
            state.selected = Some(path);
        }
        MenuCmd::ExpandAll(path) => {
            queue_fly(state, pickables, path.clone());
            expand_all(&mut state.root, &path, false);
            state.selected = Some(path);
            state.dirty = true;
        }
        MenuCmd::Collapse(path) => {
            collapse_all(&mut state.root, &path, false);
            start_deploy(state, &path, false);
            state.selected = Some(path);
        }
        MenuCmd::Look(path) => {
            queue_fly(state, pickables, path.clone());
            state.selected = Some(path);
            state.dirty = true;
        }
        MenuCmd::Props(path) => state.props = Some(path),
    }
}

fn expand_all(node: &mut node::Node, path: &Path, inside: bool) {
    let here = inside || node.path == path;
    if here && node.is_dir() {
        node.expanded = true;
        node.deployment = 1.0;
    }
    if here {
        for child in &mut node.children {
            expand_all(child, path, true);
        }
    } else {
        for child in &mut node.children {
            expand_all(child, path, false);
        }
    }
}

fn collapse_all(node: &mut node::Node, path: &Path, inside: bool) {
    let here = inside || node.path == path;
    if here && inside && node.is_dir() {
        node.expanded = false;
        node.deployment = 0.0;
    }
    if here {
        for child in &mut node.children {
            collapse_all(child, path, true);
        }
    } else {
        for child in &mut node.children {
            collapse_all(child, path, false);
        }
    }
}

fn show_props(ctx: &egui::Context, state: &mut AppState) {
    let Some(path) = state.props.clone() else { return };
    let snap = find_node(&state.root, &path).map(|node| {
        (
            node.name.clone(),
            node.kind.clone(),
            node.size,
            node.subtree_size,
            node.children.len(),
            node.is_dir(),
        )
    });
    let Some((name, kind, size, subtree, kids, is_dir)) = snap else {
        state.props = None;
        return;
    };
    let meta = std::fs::symlink_metadata(&path).ok();
    let link = if matches!(kind, node::NodeType::Symlink) {
        std::fs::read_link(&path).ok()
    } else {
        None
    };
    let mut open = true;
    egui::Window::new("Properties")
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(&name).strong());
            ui.label(kind_name(&kind));
            ui.label(path.display().to_string());
            if is_dir {
                ui.label(format!("Total size  {}", human_size(subtree)));
                ui.label(format!("Contains  {}", node_count(kids)));
            } else {
                ui.label(format!("Size  {}", human_size(size)));
                if let Some(meta) = meta.as_ref() {
                    use std::os::unix::fs::MetadataExt;
                    ui.label(format!("Allocation  {}", human_size(meta.blocks() * 512)));
                }
            }
            if let Some(meta) = meta.as_ref() {
                use std::os::unix::fs::MetadataExt;
                ui.label(format!("Owner  uid {}", meta.uid()));
                ui.label(format!("Group  gid {}", meta.gid()));
            }
            if let Some(target) = &link {
                ui.label(format!("Points to  {}", target.display()));
            }
        });
    if !open {
        state.props = None;
    }
}

fn kind_name(kind: &node::NodeType) -> &'static str {
    match kind {
        node::NodeType::Directory => "Directory",
        node::NodeType::File => "File",
        node::NodeType::Symlink => "Symlink",
        node::NodeType::Fifo => "Named pipe",
        node::NodeType::Socket => "Socket",
        node::NodeType::Char => "Character device",
        node::NodeType::Block => "Block device",
        node::NodeType::Unknown => "Unknown",
    }
}

fn queue_fly(state: &mut AppState, pickables: &Query<(&Pickable, &Transform)>, path: PathBuf) {
    let from_path = state.selected.clone().unwrap_or_else(|| state.root.path.clone());
    if from_path != path {
        if state.backtracking {
            state.backtracking = false;
        } else if state.history.last() != Some(&from_path) {
            state.history.push(from_path.clone());
        }
    }
    let (from_pos, from_span) = live_place(pickables, &from_path);
    state.fly_from = Some(from_path);
    state.fly_from_pos = from_pos;
    state.fly_from_span = from_span;
    state.fly_to = Some(path);
}

fn show_dirs(
    ui: &mut egui::Ui,
    node: &mut node::Node,
    selected: Option<&Path>,
    depth: usize,
    select: &mut Option<PathBuf>,
    toggle: &mut Option<PathBuf>,
    menu: &mut Option<MenuCmd>,
) {
    if !node.is_dir() {
        return;
    }
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 14.0);
        let has_sub = node.children.iter().any(|child| child.is_dir());
        let marker = if !has_sub {
            " "
        } else if node.expanded {
            "−"
        } else {
            "+"
        };
        if ui.small_button(marker).clicked() && has_sub {
            *toggle = Some(node.path.clone());
        }
        let on = selected == Some(node.path.as_path());
        let resp = ui.selectable_label(on, &node.name);
        if resp.clicked() {
            *select = Some(node.path.clone());
        }
        let path = node.path.clone();
        let expanded = node.expanded;
        resp.context_menu(|ui| {
            fill_menu(ui, &path, true, expanded, has_sub, menu);
        });
    });
    if node.expanded {
        for child in &mut node.children {
            show_dirs(ui, child, selected, depth + 1, select, toggle, menu);
        }
    }
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
        node::NodeType::Fifo => "fifo",
        node::NodeType::Socket => "sock",
        node::NodeType::Char => "char",
        node::NodeType::Block => "block",
        node::NodeType::Unknown => "?",
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
