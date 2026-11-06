mod node;
mod scan;
mod layout;

use std::path::{PathBuf, Path};
use clap::Parser;
use bevy::prelude::*;

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
}

#[derive(Component)]
struct Pickable {
    path: PathBuf,
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
                .insert_resource(AppState { root, mode })
                .add_systems(Startup, setup)
                .add_systems(Update, (click_expand, rebuild_layout))
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
        Transform::from_xyz(0.0, 12.0, 18.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn((DirectionalLight {
        illuminance: 15_000.0,
        ..default()
    }, Transform::from_xyz(4.0, 8.0, 4.0)));

    spawn_boxes(&mut commands, &state, &mut meshes, &mut materials);
}

fn spawn_boxes(
    commands: &mut Commands,
    state: &AppState,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
) {
    let boxes = layout::layout(&state.root, state.mode);
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));

    for geom in boxes {
        let color = match geom.kind {
            node::NodeType::Directory => Color::srgb(0.2, 0.5, 0.9),
            node::NodeType::File => Color::srgb(0.8, 0.7, 0.2),
            node::NodeType::Symlink => Color::srgb(0.9, 0.4, 0.7),
            node::NodeType::Other => Color::srgb(0.5, 0.5, 0.5),
        };

        commands.spawn((
            Mesh3d(cube.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                ..default()
            })),
            Transform {
                translation: Vec3::new(geom.x, geom.z, geom.y),
                scale: Vec3::new(geom.width.max(0.01), geom.height.max(0.01), geom.depth.max(0.01)),
                ..default()
            },
            Pickable { path: geom.path },
        ));
    }
}

fn click_expand(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    pickables: Query<(&Pickable, &Transform)>,
    mut state: ResMut<AppState>,
) {
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
) {
    if !state.is_changed() {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    spawn_boxes(&mut commands, &state, &mut meshes, &mut materials);
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
