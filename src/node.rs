use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeType {
    Directory,
    File,
    Symlink,
    Other,
}

/// MapV rectangle in the original fsv units.
/// x/y come from sqrt(bytes). Height is 384 for a directory and 128 for a file.
/// The renderer scales this. Do not bake a gap or a camera scale in here.
#[derive(Debug, Clone, Copy)]
pub struct MapGeom {
    pub c0x: f64,
    pub c0y: f64,
    pub c1x: f64,
    pub c1y: f64,
    pub height: f64,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub subtree_size: u64,
    pub mtime: Option<SystemTime>,
    pub kind: NodeType,
    pub children: Vec<Node>,
    pub expanded: bool,
    pub map: Option<MapGeom>,
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
            map: None,
        }
    }

    pub fn is_dir(&self) -> bool {
        matches!(self.kind, NodeType::Directory)
    }
}
