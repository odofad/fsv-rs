mod node;
mod scan;
mod layout;

use std::path::PathBuf;
use clap::Parser;
use bevy::prelude::*;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Directory to scan
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Print the tree and exit (no window)
    #[arg(long)]
    print: bool,
}

#[derive(Resource)]
struct Layout(Vec<layout::BoxGeom>);

fn main() {
    let args = Args::parse();

    match scan::scan(&args.path) {
        Ok(root) => {
            if args.print {
                print_tree(&root, 0);
                return;
            }

            let boxes = layout::mapv_layout(&root);

            App::new()
                .add_plugins(DefaultPlugins)
                .insert_resource(Layout(boxes))
                .add_systems(Startup, setup)
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
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    layout: Res<Layout>,
) {
    // Camera looking at the origin from a sensible distance
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 12.0, 18.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Light
    commands.spawn((DirectionalLight {
        illuminance: 15_000.0,
        ..default()
    }, Transform::from_xyz(4.0, 8.0, 4.0)));

    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));

    for geom in &layout.0 {
        let _ = &geom.name; // silence unused warning for now
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
        ));
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
    let _ = (&node.path, node.expanded);
    println!(
        "{}{} [{}] size={} subtree={}",
        indent,
        node.name,
        kind,
        node.size,
        node.subtree_size
    );
    for child in &node.children {
        print_tree(child, depth + 1);
    }
}
