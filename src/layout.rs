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

    // Height scales with relative size so big nodes stand out
    let rel = (node.subtree_size.max(1) as f32 / parent_total.max(1.0)).clamp(0.0, 1.0);
    let base = if node.is_dir() { 0.2 } else { 0.08 };
    let height = base + rel * 0.8;

    out.push(BoxGeom {
        x: x + width / 2.0,
        y: y + depth / 2.0,
        z: z + height / 2.0,
        width,
        depth,
        height,
        kind: node.kind.clone(),
        name: node.name.clone(),
    });

    if !node.is_dir() || node.children.is_empty() {
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
