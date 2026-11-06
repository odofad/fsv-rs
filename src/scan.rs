use std::collections::HashMap;
use std::path::{Path, PathBuf};
use jwalk::WalkDir;
use crate::node::{Node, NodeType};

pub fn scan(root: &Path) -> Result<Node, std::io::Error> {
    let mut nodes: HashMap<PathBuf, Node> = HashMap::new();

    for entry in WalkDir::new(root).sort(true) {
        let entry = entry?;
        let path = entry.path();

        let kind = if entry.file_type().is_dir() {
            NodeType::Directory
        } else if entry.file_type().is_symlink() {
            NodeType::Symlink
        } else if entry.file_type().is_file() {
            NodeType::File
        } else {
            NodeType::Other
        };

        let metadata = entry.metadata().ok();
        let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
        let mtime = metadata.and_then(|m| m.modified().ok());

        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        let mut node = Node::new(name, path.clone(), kind);
        node.size = size;
        node.mtime = mtime;
        // Collapsed by default; root will be expanded later
        node.expanded = false;

        nodes.insert(path, node);
    }

    let mut children_map: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    let all_paths: Vec<PathBuf> = nodes.keys().cloned().collect();

    for path in &all_paths {
        if let Some(parent_path) = path.parent() {
            if nodes.contains_key(parent_path) {
                children_map
                    .entry(parent_path.to_path_buf())
                    .or_default()
                    .push(path.clone());
            }
        }
    }

    fn build(path: &Path, nodes: &mut HashMap<PathBuf, Node>, children_map: &HashMap<PathBuf, Vec<PathBuf>>) -> Node {
        let mut node = nodes.remove(path).expect("node exists");
        if let Some(child_paths) = children_map.get(path) {
            for child_path in child_paths {
                let child = build(child_path, nodes, children_map);
                node.children.push(child);
            }
        }
        node
    }

    let mut root_node = build(root, &mut nodes, &children_map);
    root_node.expanded = true; // root starts expanded

    compute_subtree_size(&mut root_node);
    sort_children(&mut root_node);

    Ok(root_node)
}

fn sort_children(node: &mut Node) {
    node.children.sort_by(|a, b| {
        let a_dir = a.is_dir();
        let b_dir = b.is_dir();
        match (a_dir, b_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => b.subtree_size.cmp(&a.subtree_size).then(a.name.cmp(&b.name)),
        }
    });
    for child in &mut node.children {
        sort_children(child);
    }
}

fn compute_subtree_size(node: &mut Node) -> u64 {
    let mut total = node.size;
    for child in &mut node.children {
        total += compute_subtree_size(child);
    }
    node.subtree_size = total;
    total
}
