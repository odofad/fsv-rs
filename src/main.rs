mod node;
mod scan;
mod layout;

use std::path::{PathBuf, Path};
use clap::Parser;
use bevy::prelude::*;
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
}

#[derive(Component)]
struct Pickable {
    path: PathBuf,
}

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
                })
                .add_systems(Startup, setup)
                .add_systems(Update, (orbit_camera, click_expand, rebuild_layout, side_panel))
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
    state: Res<AppState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 14.0, 22.0).looking_at(Vec3::ZERO, Vec3::Y),
        OrbitCamera {
            yaw: 0.5,
            pitch: 0.6,
            distance: 24.0,
            focus: Vec3::ZERO,
        },
    ));

    commands.spawn((DirectionalLight {
        illuminance: 18_000.0,
        ..default()
    }, Transform::from_xyz(6.0, 12.0, 6.0)));

    spawn_geometry(&mut commands, &state, &mut meshes, &mut materials);
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
        orbit.pitch = (orbit.pitch - delta.y * 0.005).clamp(-1.4, 1.4);
    }

    if buttons.pressed(MouseButton::Right) || buttons.pressed(MouseButton::Middle) {
        let right = transform.right();
        let up = transform.up();
        orbit.focus += (-right * delta.x + up * delta.y) * 0.025;
    }

    for ev in scroll.read() {
        orbit.distance = (orbit.distance - ev.y * 1.8).clamp(3.0, 100.0);
    }

    let rot = Quat::from_euler(EulerRot::YXZ, orbit.yaw, orbit.pitch, 0.0);
    let offset = rot * Vec3::new(0.0, 0.0, orbit.distance);
    transform.translation = orbit.focus + offset;
    transform.look_at(orbit.focus, Vec3::Y);
}

fn spawn_geometry(
    commands: &mut Commands,
    state: &AppState,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
) {
    let result = layout::layout(&state.root, state.mode);
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let line_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.7, 0.7, 0.8),
        unlit: true,
        ..default()
    });

    for geom in result.boxes {
        let mut color = color_for_kind(&geom.kind);
        if geom.is_base {
            let [r, g, b, a] = color.to_srgba().to_f32_array();
            color = Color::srgba(r * 0.5, g * 0.5, b * 0.5, a);
        }

        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: 0.5,
                ..default()
            })),
            Transform {
                translation: Vec3::new(geom.x, geom.z, geom.y),
                scale: Vec3::new(geom.width.max(0.01), geom.height.max(0.01), geom.depth.max(0.01)),
                ..default()
            },
            Pickable { path: geom.path.clone() },
        ));

        if !geom.is_base && geom.width > 0.5 && geom.depth > 0.5 && !geom.name.is_empty() {
            let label = if geom.name.len() > 14 {
                format!("{}...", &geom.name[..11])
            } else {
                geom.name.clone()
            };
            commands.spawn((
                Text2d::new(label),
                TextFont { font_size: 28.0, ..default() },
                TextColor(Color::WHITE),
                Transform {
                    translation: Vec3::new(geom.x, geom.z + geom.height * 0.55 + 0.2, geom.y),
                    rotation: Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
                    scale: Vec3::splat(0.018),
                    ..default()
                },
            ));
        }
    }

    // Connecting lines (simple thin cubes for now)
    for line in result.lines {
        let from = Vec3::new(line.from[0], line.from[1], line.from[2]);
        let to = Vec3::new(line.to[0], line.to[1], line.to[2]);
        let mid = (from + to) * 0.5;
        let dir = to - from;
        let len = dir.length().max(0.01);
        let rot = Quat::from_rotation_arc(Vec3::Y, dir.normalize_or_zero());

        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(line_mat.clone()),
            Transform {
                translation: mid,
                rotation: rot,
                scale: Vec3::new(0.02, len, 0.02),
                ..default()
            },
        ));
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
        toggle_expanded(&mut state.root, &path);
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
        }
        return true;
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
    state: Res<AppState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<Entity, With<Pickable>>,
    labels: Query<Entity, With<Text2d>>,
) {
    if !state.is_changed() {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    for e in &labels {
        commands.entity(e).despawn();
    }
    spawn_geometry(&mut commands, &state, &mut meshes, &mut materials);
}

fn side_panel(
    mut contexts: EguiContexts,
    mut state: ResMut<AppState>,
) {
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
        }

        ui.separator();
        ui.label("Tree");

        show_node(ui, &mut state.root, 0);
    });
}

fn show_node(ui: &mut egui::Ui, node: &mut node::Node, depth: usize) {
    let indent = "  ".repeat(depth);
    let marker = if node.is_dir() {
        if node.expanded { "[-]" } else { "[+]" }
    } else {
        "   "
    };

    let size_str = human_size(node.subtree_size);
    let label = format!("{}{} {}  ({})", indent, marker, node.name, size_str);

    if ui.button(label).clicked() && node.is_dir() {
        node.expanded = !node.expanded;
    }

    if node.expanded {
        for child in &mut node.children {
            show_node(ui, child, depth + 1);
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
