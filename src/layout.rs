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
    pub path: std::path::PathBuf, // for picking
}

pub enum LayoutMode {
    MapV,
    TreeV,
}

pub fn layout(root: &Node, mode: LayoutMode) -> Vec<BoxGeom> {
    match mode {
        LayoutMode::MapV => mapv_layout(root),
        LayoutMode::TreeV => treev_layout(root),
    }
}

pub fn mapv_layout(root: &Node) -> Vec<BoxGeom> {
    let mut boxes = Vec::new();
    if root.subtree_size == 0 {
        return boxes;
    }

    let root_w = 12.0;
    let root_d = 10.0;

    layout_recursive(
        root,
        -root_w / 2.0,
        -root_d / 2.0,
        root_w,
        root_d,
        0.0,
        root.subtree_size as f32,
        &mut boxes,
    );

    boxes
}

fn layout_recursive(
    node: &Node,
    x: f32,
    y: f32,
    width: f32,
    depth: f32,
    z: f32,
    parent_total: f32,
    out: &mut Vec<BoxGeom>,
) {
    if width < 0.08 || depth < 0.08 {
        return;
    }

    let rel = (node.subtree_size.max(1) as f32 / parent_total.max(1.0)).clamp(0.0, 1.0);
    let base = if node.is_dir() { 0.15 } else { 0.06 };
    let height = base + rel * 0.25;

    out.push(BoxGeom {
        x: x + width / 2.0,
        y: y + depth / 2.0,
        z: z + height / 2.0,
        width,
        depth,
        height,
        kind: node.kind.clone(),
        name: node.name.clone(),
        path: node.path.clone(),
    });

    // Only recurse into children if expanded
    if !node.is_dir() || !node.expanded || node.children.is_empty() {
        return;
    }

    let total = node.subtree_size.max(1) as f32;
    let pad = 0.04;
    let inner_x = x + pad;
    let inner_y = y + pad;
    let inner_w = (width - 2.0 * pad).max(0.1);
    let inner_d = (depth - 2.0 * pad).max(0.1);

    let mut remaining = node.children.as_slice();
    let mut cursor_y = inner_y;
    let mut remaining_d = inner_d;

    while !remaining.is_empty() && remaining_d > 0.1 {
        let mut row_area = 0.0f32;
        let mut row_count = 0;
        let mut best_aspect = f32::MAX;

        for i in 1..=remaining.len() {
            let area: f32 = remaining[..i]
                .iter()
                .map(|c| (c.subtree_size.max(1) as f32) / total * inner_w * inner_d)
                .sum();
            let row_d = (area / inner_w).max(0.1);
            let aspect = (inner_w / (area / row_d)).max(row_d / inner_w);
            if aspect < best_aspect || row_count == 0 {
                best_aspect = aspect;
                row_area = area;
                row_count = i;
            } else {
                break;
            }
        }

        row_count = row_count.max(1);
        let row = &remaining[..row_count];
        let row_d = (row_area / inner_w).min(remaining_d).max(0.1);

        let mut cursor_x = inner_x;
        for child in row {
            let frac = (child.subtree_size.max(1) as f32) / total;
            let child_area = frac * inner_w * inner_d;
            let child_w = (child_area / row_d).max(0.08);

            layout_recursive(
                child,
                cursor_x,
                cursor_y,
                child_w.min(inner_w - (cursor_x - inner_x)),
                row_d,
                z + height,
                total,
                out,
            );

            cursor_x += child_w;
        }

        cursor_y += row_d;
        remaining_d -= row_d;
        remaining = &remaining[row_count..];
    }
}

pub fn treev_layout(root: &Node) -> Vec<BoxGeom> {
    let mut boxes = Vec::new();
    if root.subtree_size == 0 {
        return boxes;
    }

    treev_recursive(root, 0.0, 0.0, 0.0, 2.5, 0.0, &mut boxes);
    boxes
}

fn treev_recursive(
    node: &Node,
    cx: f32,
    cy: f32,
    z: f32,
    radius: f32,
    parent_angle: f32,
    out: &mut Vec<BoxGeom>,
) {
    let height = if node.is_dir() { 0.2 } else { 0.1 };
    let size = radius * 0.6;

    out.push(BoxGeom {
        x: cx,
        y: cy,
        z: z + height / 2.0,
        width: size,
        depth: size,
        height,
        kind: node.kind.clone(),
        name: node.name.clone(),
        path: node.path.clone(),
    });

    // Only show children if expanded
    if !node.is_dir() || !node.expanded || node.children.is_empty() {
        return;
    }

    let n = node.children.len() as f32;
    let angle_step = std::f32::consts::TAU / n.max(1.0);
    let child_radius = radius * 0.55;
    let dist = radius * 1.4;

    for (i, child) in node.children.iter().enumerate() {
        let angle = parent_angle + i as f32 * angle_step;
        let child_cx = cx + dist * angle.cos();
        let child_cy = cy + dist * angle.sin();
        treev_recursive(child, child_cx, child_cy, z + height, child_radius, angle, out);
    }
}
