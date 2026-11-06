mod node;
mod scan;
mod layout;

use std::path::{PathBuf, Path};
use clap::Parser;
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

#[derive(Component)]
struct OrbitCamera {
    yaw: f32,
    pitch: f32,
    distance: f32,
    focus: Vec3,
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
                    dirty: true,
                })
                .add_systems(Startup, setup)
                .add_systems(Update, (orbit_camera, click_expand, side_panel, rebuild_layout))
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
        Transform::from_xyz(0.0, 22.0, 26.0).looking_at(Vec3::ZERO, Vec3::Y),
        OrbitCamera {
            yaw: 0.35,
            pitch: 0.95,
            distance: 32.0,
            focus: Vec3::ZERO,
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

fn orbit_camera(
    mut cameras: Query<(&mut Transform, &mut OrbitCamera)>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut scroll: EventReader<bevy::input::mouse::MouseWheel>,
    mut motion: EventReader<bevy::input::mouse::MouseMotion>,
    mut egui: EguiContexts,
) {
    if egui.ctx_mut().is_pointer_over_area() {
        return;
    }

    let Ok((mut transform, mut orbit)) = cameras.get_single_mut() else { return; };

    let mut delta = Vec2::ZERO;
    for ev in motion.read() {
        delta += ev.delta;
    }

    if buttons.pressed(MouseButton::Left) {
        orbit.yaw -= delta.x * 0.005;
        orbit.pitch = (orbit.pitch - delta.y * 0.005).clamp(-1.2, 1.45);
    }

    if buttons.pressed(MouseButton::Right) || buttons.pressed(MouseButton::Middle) {
        let right = transform.right();
        let up = transform.up();
        orbit.focus += (-right * delta.x + up * delta.y) * 0.02;
    }

    for ev in scroll.read() {
        orbit.distance = (orbit.distance - ev.y * 1.6).clamp(2.0, 120.0);
    }

    let rot = Quat::from_euler(EulerRot::YXZ, orbit.yaw, orbit.pitch, 0.0);
    let offset = rot * Vec3::new(0.0, 0.0, orbit.distance);
    transform.translation = orbit.focus + offset;
    transform.look_at(orbit.focus, Vec3::Y);
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

    for geom in result.boxes {
        let mut color = color_for_kind(&geom.kind);
        if geom.is_base {
            let [r, g, b, a] = color.to_srgba().to_f32_array();
            color = Color::srgba(r * 0.45, g * 0.45, b * 0.45, a);
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
                scale,
                ..default()
            },
            Pickable { path: geom.path.clone() },
            MapPiece,
        ));

        if geom.folder {
            let pts = layout::folder_outline(geom.slant_x, geom.slant_y);
            let y = translation.y + scale.y * 0.5 + 0.012;
            let thick = (scale.x.min(scale.z) * 0.012).clamp(0.008, 0.04);
            for w in pts.windows(2) {
                let a = Vec3::new(translation.x + w[0][0] * scale.x, y, translation.z + w[0][1] * scale.z);
                let b = Vec3::new(translation.x + w[1][0] * scale.x, y, translation.z + w[1][1] * scale.z);
                spawn_segment(commands, cube.clone(), folder_mat.clone(), a, b, thick);
            }
        }

        if geom.label && !geom.name.is_empty() {
            spawn_label(commands, &geom, translation, scale);
        }

        if state.selected.as_ref() == Some(&geom.path) {
            spawn_cursor(commands, cube.clone(), cursor_mat.clone(), translation, scale);
        }
    }

    for line in result.lines {
        let from = Vec3::new(line.from[0], line.from[1], line.from[2]);
        let to = Vec3::new(line.to[0], line.to[1], line.to[2]);
        spawn_segment(commands, cube.clone(), line_mat.clone(), from, to, 0.02);
    }
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

fn spawn_label(commands: &mut Commands, geom: &layout::BoxGeom, translation: Vec3, scale: Vec3) {
    let shown: String = geom.name.chars().take(22).collect();
    let n = shown.chars().count().max(1) as f32;
    let top_w = scale.x * (1.0 - 2.0 * geom.slant_x) * 0.8125;
    let top_d = scale.z * (1.0 - 2.0 * geom.slant_y) * 0.382;
    if top_w < 0.05 || top_d < 0.03 {
        return;
    }
    let font = 40.0;
    let scale_px = (top_w / (n * font * 0.55)).min(top_d / font);
    if scale_px < 0.0008 {
        return;
    }
    commands.spawn((
        Text2d::new(shown),
        TextFont {
            font_size: font,
            ..default()
        },
        TextColor(Color::srgb(0.95, 0.97, 1.0)),
        Transform {
            translation: Vec3::new(translation.x, translation.y + scale.y * 0.5 + 0.02, translation.z),
            rotation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            scale: Vec3::splat(scale_px),
        },
        MapPiece,
    ));
}

fn spawn_cursor(
    commands: &mut Commands,
    cube: Handle<Mesh>,
    mat: Handle<StandardMaterial>,
    translation: Vec3,
    scale: Vec3,
) {
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
        let p = Vec3::new(px, py, pz);
        spawn_segment(commands, cube.clone(), mat.clone(), p, p + Vec3::new(dx, 0.0, 0.0), thick);
        spawn_segment(commands, cube.clone(), mat.clone(), p, p + Vec3::new(0.0, dy, 0.0), thick);
        spawn_segment(commands, cube.clone(), mat.clone(), p, p + Vec3::new(0.0, 0.0, dz), thick);
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

fn click_expand(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform)>,
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

    if let Some((_, path)) = best {
        state.selected = Some(path.clone());
        if toggle_expanded(&mut state.root, &path) {
            state.dirty = true;
        } else {
            state.dirty = true;
        }
    }
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

fn side_panel(
    mut contexts: EguiContexts,
    mut state: ResMut<AppState>,
) {
    let mut changed = false;
    egui::SidePanel::left("tree_panel").show(contexts.ctx_mut(), |ui| {
        ui.heading("fsv-rs");
        ui.separator();

        let mode_label = match state.mode {
            layout::LayoutMode::MapV => "MapV",
            layout::LayoutMode::TreeV => "TreeV",
        };
        ui.label(format!("Mode: {}", mode_label));

        if ui.button("Toggle MapV / TreeV").clicked() {
            state.mode = match state.mode {
                layout::LayoutMode::MapV => layout::LayoutMode::TreeV,
                layout::LayoutMode::TreeV => layout::LayoutMode::MapV,
            };
            changed = true;
        }

        ui.separator();
        ui.label("Tree");

        changed |= show_node(ui, &mut state.root, 0);
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
