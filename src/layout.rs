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

#[derive(Debug, Clone, Copy)]
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

    let root_w = 14.0;
    let root_d = 12.0;

    layout_recursive(
        root,
        -root_w / 2.0,
        -root_d / 2.0,
        root_w,
        root_d,
        0.0,
        root.subtree_size as f32,
        true,
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
    is_root: bool,
    out: &mut Vec<BoxGeom>,
) {
    if width < 0.1 || depth < 0.1 {
        return;
    }

    let rel = (node.subtree_size.max(1) as f32 / parent_total.max(1.0)).clamp(0.0, 1.0);

    let is_expanded_dir = node.is_dir() && node.expanded && !node.children.is_empty();
    let is_base = is_root || is_expanded_dir;

    // Very thin bases; individual blocks get real height
    let height = if is_base {
        0.04
    } else {
        0.4 + rel * 0.5
    };

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
        is_base,
    });

    if !is_expanded_dir {
        return;
    }

    // Cap leaves for readability
    let mut children: Vec<&Node> = node.children.iter().collect();
    children.sort_by(|a, b| b.subtree_size.cmp(&a.subtree_size));
    let mut visible: Vec<&Node> = Vec::new();
    let mut file_count = 0;
    const MAX_FILES: usize = 8;
    for c in &children {
        if c.is_dir() {
            visible.push(c);
        } else if file_count < MAX_FILES {
            visible.push(c);
            file_count += 1;
        }
    }

    let total = node.subtree_size.max(1) as f32;
    let gap = 0.12; // bigger gaps
    let inner_x = x + gap;
    let inner_y = y + gap;
    let inner_w = (width - 2.0 * gap).max(0.15);
    let inner_d = (depth - 2.0 * gap).max(0.15);

    let child_z = z + height + 0.02; // sit clearly on top

    let mut remaining = visible.as_slice();
    let mut cursor_x = inner_x;
    let mut cursor_y = inner_y;
    let mut remain_w = inner_w;
    let mut remain_d = inner_d;
    let mut horizontal = remain_w >= remain_d;

    while !remaining.is_empty() && remain_w > 0.1 && remain_d > 0.1 {
        let side = if horizontal { remain_w } else { remain_d };
        let other = if horizontal { remain_d } else { remain_w };

        let mut best_count = 1;
        let mut best_aspect = f32::MAX;
        let mut accum = 0.0f32;

        for i in 1..=remaining.len() {
            accum += (remaining[i - 1].subtree_size.max(1) as f32) / total;
            let area = accum * remain_w * remain_d;
            let row_thick = (area / side).max(0.1);
            let aspect = (side / row_thick).max(row_thick / side);
            if aspect < best_aspect {
                best_aspect = aspect;
                best_count = i;
            } else if i > 2 {
                break;
            }
        }

        let row = &remaining[..best_count];
        let row_frac: f32 = row.iter().map(|c| (c.subtree_size.max(1) as f32) / total).sum();
        let row_area = row_frac * remain_w * remain_d;
        let thick = (row_area / side).min(other).max(0.12);

        let mut along = 0.0f32;
        for child in row {
            let frac = (child.subtree_size.max(1) as f32) / total;
            let child_len = (frac / row_frac.max(0.0001) * side).max(0.12);

            let usable = (child_len - gap).max(0.1);

            let (cx, cy, cw, cd) = if horizontal {
                (cursor_x + along + gap * 0.5, cursor_y, usable, (thick - gap).max(0.1))
            } else {
                (cursor_x, cursor_y + along + gap * 0.5, (thick - gap).max(0.1), usable)
            };

            layout_recursive(child, cx, cy, cw, cd, child_z, total, false, out);
            along += child_len;
        }

        if horizontal {
            cursor_y += thick;
            remain_d -= thick;
        } else {
            cursor_x += thick;
            remain_w -= thick;
        }

        remaining = &remaining[best_count..];
        horizontal = remain_w >= remain_d;
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
        is_base: false,
    });

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
