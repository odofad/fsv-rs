use std::path::Path;
use jwalk::WalkDir;
use crate::node::{Node, NodeType};

pub fn scan(root: &Path) -> Result<Node, std::io::Error> {
    let mut root_node = Node::new(
        root.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        root.to_path_buf(),
        NodeType::Directory,
    );

    // Simple recursive-style build via jwalk (depth-first collection)
    // For Phase 0 we build a flat-ish tree then nest it properly
    let mut nodes: Vec<(usize, Node)> = Vec::new();

    for entry in WalkDir::new(root).sort(true) {
        let entry = entry?;
        let path = entry.path();
        let depth = entry.depth();

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

        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();

        let mut node = Node::new(name, path.clone(), kind);
        node.size = size;
        node.mtime = mtime;

        nodes.push((depth, node));
    }

    // Rebuild into proper tree (simple parent-pointer approach)
    // For a first cut we just return the root and attach children by depth
    // (proper tree building can be refined)
    if !nodes.is_empty() {
        root_node = nodes.remove(0).1;
        // TODO: properly nest by depth for Phase 0 verification
        // For now just collect all as flat children of root for testing
        for (_, node) in nodes {
            if node.path.parent() == Some(&root_node.path) {
                root_node.children.push(node);
            }
        }
    }

    // Compute subtree sizes (bottom-up)
    compute_subtree_size(&mut root_node);

    Ok(root_node)
}

fn compute_subtree_size(node: &mut Node) -> u64 {
    let mut total = node.size;
    for child in &mut node.children {
        total += compute_subtree_size(child);
    }
    node.subtree_size = total;
    total
}
