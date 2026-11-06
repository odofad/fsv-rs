mod node;
mod scan;

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

fn main() {
    let args = Args::parse();

    match scan::scan(&args.path) {
        Ok(root) => {
            if args.print {
                print_tree(&root, 0);
                return;
            }

            // Phase 0: just open an empty Bevy window for now
            App::new()
                .add_plugins(DefaultPlugins)
                .add_systems(Startup, setup)
                .run();
        }
        Err(e) => {
            eprintln!("scan error: {}", e);
            std::process::exit(1);
        }
    }
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d::default());
    // empty for Phase 0
}

fn print_tree(node: &node::Node, depth: usize) {
    let indent = "  ".repeat(depth);
    let kind = match node.kind {
        node::NodeType::Directory => "dir",
        node::NodeType::File => "file",
        node::NodeType::Symlink => "link",
        node::NodeType::Other => "other",
    };
    // Reference path and expanded so the fields aren't dead in Phase 0
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
