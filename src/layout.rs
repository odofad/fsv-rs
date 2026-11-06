use crate::node::{Node, NodeType};

#[derive(Debug, Clone)]
pub struct BoxGeom {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub width: f32,
    pub depth: f32,
    pub height: f32,
    pub kind: NodeType,
    pub name: String,
    pub path: std::path::PathBuf,
    pub is_base: bool,
}

#[derive(Debug, Clone)]
pub struct LineGeom {
    pub from: [f32; 3],
    pub to: [f32; 3],
}

#[derive(Debug, Clone, Copy)]
pub enum LayoutMode {
    MapV,
    TreeV,
}

pub struct LayoutResult {
    pub boxes: Vec<BoxGeom>,
    pub lines: Vec<LineGeom>,
}

pub fn layout(root: &Node, mode: LayoutMode) -> LayoutResult {
    match mode {
        LayoutMode::MapV => mapv_layout(root),
        LayoutMode::TreeV => treev_layout(root),
    }
}

pub fn mapv_layout(root: &Node) -> LayoutResult {
    let mut boxes = Vec::new();
    let mut lines = Vec::new();
    if root.subtree_size == 0 {
        return LayoutResult { boxes, lines };
    }

    let root_w = 18.0;
    let root_d = 16.0;

    // Root is a large patch
    layout_patch(
        root,
        -root_w / 2.0,
        -root_d / 2.0,
        root_w,
        root_d,
        0.0,
        root.subtree_size as f32,
        true,
        &mut boxes,
        &mut lines,
    );

    LayoutResult { boxes, lines }
}

fn layout_patch(
    node: &Node,
    x: f32,
    y: f32,
    width: f32,
    depth: f32,
    z: f32,
    parent_total: f32,
    is_root: bool,
    boxes: &mut Vec<BoxGeom>,
    lines: &mut Vec<LineGeom>,
) {
    if width < 0.2 || depth < 0.2 {
        return;
    }

    let is_expanded = node.is_dir() && node.expanded && !node.children.is_empty();

    // Folder patch (thin)
    let patch_h = if is_root { 0.06 } else { 0.12 };
    boxes.push(BoxGeom {
        x: x + width / 2.0,
        y: y + depth / 2.0,
        z: z + patch_h / 2.0,
        width,
        depth,
        height: patch_h,
        kind: node.kind.clone(),
        name: node.name.clone(),
        path: node.path.clone(),
        is_base: true,
    });

    if !is_expanded {
        // Collapsed dir or file: raised block on the patch
        let rel = (node.subtree_size.max(1) as f32 / parent_total.max(1.0)).clamp(0.0, 1.0);
        let h = 0.3 + rel * 0.4;
        boxes.push(BoxGeom {
            x: x + width / 2.0,
            y: y + depth / 2.0,
            z: z + patch_h + h / 2.0,
            width: width * 0.7,
            depth: depth * 0.7,
            height: h,
            kind: node.kind.clone(),
            name: node.name.clone(),
            path: node.path.clone(),
            is_base: false,
        });
        return;
    }

    // Cap and sort children
    let mut children: Vec<&Node> = node.children.iter().collect();
    children.sort_by(|a, b| b.subtree_size.cmp(&a.subtree_size));
    let mut visible: Vec<&Node> = Vec::new();
    let mut files = 0;
    const MAX_FILES: usize = 8;
    for c in &children {
        if c.is_dir() {
            visible.push(c);
        } else if files < MAX_FILES {
            visible.push(c);
            files += 1;
        }
    }

    let total = node.subtree_size.max(1) as f32;
    let gap = 0.25; // spacious
    let inner_x = x + gap;
    let inner_y = y + gap;
    let inner_w = (width - 2.0 * gap).max(0.3);
    let inner_d = (depth - 2.0 * gap).max(0.3);

    let child_z = z + patch_h;

    // Simple grid-ish placement with space
    let n = visible.len().max(1) as f32;
    let cols = (n.sqrt().ceil() as usize).max(1);
    let rows = ((n / cols as f32).ceil() as usize).max(1);
    let cell_w = inner_w / cols as f32;
    let cell_d = inner_d / rows as f32;

    for (i, child) in visible.iter().enumerate() {
        let col = i % cols;
        let row = i / cols;
        let cx = inner_x + col as f32 * cell_w + gap * 0.3;
        let cy = inner_y + row as f32 * cell_d + gap * 0.3;
        let cw = (cell_w - gap * 0.6).max(0.2);
        let cd = (cell_d - gap * 0.6).max(0.2);

        // Connection line from parent center to child
        lines.push(LineGeom {
            from: [x + width / 2.0, z + patch_h, y + depth / 2.0],
            to: [cx + cw / 2.0, child_z, cy + cd / 2.0],
        });

        layout_patch(child, cx, cy, cw, cd, child_z, total, false, boxes, lines);
    }
}

pub fn treev_layout(root: &Node) -> LayoutResult {
    let mut boxes = Vec::new();
    let mut lines = Vec::new();
    if root.subtree_size == 0 {
        return LayoutResult { boxes, lines };
    }

    treev_recursive(root, 0.0, 0.0, 0.0, 2.5, 0.0, &mut boxes, &mut lines);
    LayoutResult { boxes, lines }
}

fn treev_recursive(
    node: &Node,
    cx: f32,
    cy: f32,
    z: f32,
    radius: f32,
    parent_angle: f32,
    boxes: &mut Vec<BoxGeom>,
    lines: &mut Vec<LineGeom>,
) {
    let height = if node.is_dir() { 0.2 } else { 0.1 };
    let size = radius * 0.6;

    boxes.push(BoxGeom {
        x: cx,
        y: cy,
        z: z + height / 2.0,
        width: size,
        depth: size,
        height,
        kind: node.kind.clone(),
        name: node.name.clone(),
        path: node.path.clone(),
        is_base: false,
    });

    if !node.is_dir() || !node.expanded || node.children.is_empty() {
        return;
    }

    let n = node.children.len() as f32;
    let angle_step = std::f32::consts::TAU / n.max(1.0);
    let child_radius = radius * 0.55;
    let dist = radius * 1.8; // more space

    for (i, child) in node.children.iter().enumerate() {
        let angle = parent_angle + i as f32 * angle_step;
        let child_cx = cx + dist * angle.cos();
        let child_cy = cy + dist * angle.sin();

        lines.push(LineGeom {
            from: [cx, z + height, cy],
            to: [child_cx, z + height, child_cy],
        });

        treev_recursive(child, child_cx, child_cy, z + height, child_radius, angle, boxes, lines);
    }
}
