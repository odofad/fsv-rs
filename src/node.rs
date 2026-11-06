use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeType {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,              // own size
    pub subtree_size: u64,      // recursive for dirs
    pub mtime: Option<SystemTime>,
    pub kind: NodeType,
    pub children: Vec<Node>,
    pub expanded: bool,
}

impl Node {
    pub fn new(name: String, path: PathBuf, kind: NodeType) -> Self {
        Self {
            name,
            path,
            size: 0,
            subtree_size: 0,
            mtime: None,
            kind,
            children: Vec::new(),
            expanded: true,
        }
    }

    pub fn is_dir(&self) -> bool {
        matches!(self.kind, NodeType::Directory)
    }
}
