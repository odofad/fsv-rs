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

    // Fixed size root platform so it always fits in view
    let root_w = 10.0;
    let root_d = 8.0;

    layout_recursive(
        root,
        -root_w / 2.0,
        -root_d / 2.0,
        root_w,
        root_d,
        0.0,
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
    out: &mut Vec<BoxGeom>,
) {
    if width < 0.05 || depth < 0.05 {
        return;
    }

    let height = if node.is_dir() { 0.3 } else { 0.12 };

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
    let mut cursor_x = x + 0.05;
    let mut cursor_y = y + 0.05;
    let inner_w = (width - 0.1).max(0.1);
    let mut row_max_d = 0.0f32;

    for child in &node.children {
        let frac = (child.subtree_size.max(1) as f32) / total;
        let child_area = inner_w * (depth - 0.1).max(0.1) * frac;
        let child_w = (child_area * 1.4).sqrt().max(0.1);
        let child_d = (child_area / child_w).max(0.1);

        if cursor_x + child_w > x + width - 0.05 && cursor_x > x + 0.05 {
            cursor_x = x + 0.05;
            cursor_y += row_max_d + 0.05;
            row_max_d = 0.0;
        }

        if cursor_y + child_d > y + depth - 0.05 {
            break; // no more room
        }

        layout_recursive(
            child,
            cursor_x,
            cursor_y,
            child_w.min(inner_w),
            child_d.min(depth - 0.1),
            z + height,
            out,
        );

        cursor_x += child_w + 0.02;
        row_max_d = row_max_d.max(child_d);
    }
}
