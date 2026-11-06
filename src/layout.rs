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

    // Root platform
    let root_area = (root.subtree_size as f32).sqrt() * 0.01;
    let root_w = root_area * 1.2;
    let root_d = root_area / 1.2;

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
    if width <= 0.0 || depth <= 0.0 {
        return;
    }

    let height = if node.is_dir() { 0.4 } else { 0.15 };

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

    // Simple row packing for children
    let total = node.subtree_size as f32;
    let mut cursor_x = x + 0.02 * width;
    let inner_w = width * 0.96;
    let inner_d = depth * 0.96;
    let row_y = y + 0.02 * depth;

    let mut row_height = 0.0f32;
    let mut row_used = 0.0f32;

    for child in &node.children {
        let frac = (child.subtree_size.max(1) as f32) / total;
        let child_area = inner_w * inner_d * frac;
        let child_w = (child_area * 1.2).sqrt();
        let child_d = child_area / child_w.max(0.01);

        if row_used + child_w > inner_w && row_used > 0.0 {
            // new row
            cursor_x = x + 0.02 * width;
            row_height += row_d_max(&node.children) * 0.5; // rough
            row_used = 0.0;
        }

        layout_recursive(
            child,
            cursor_x,
            row_y + row_height,
            child_w.min(inner_w - row_used),
            child_d.min(inner_d),
            z + height,
            out,
        );

        cursor_x += child_w;
        row_used += child_w;
    }
}

fn row_d_max(children: &[Node]) -> f32 {
    children.iter().map(|c| (c.subtree_size as f32).sqrt() * 0.001).fold(0.0, f32::max)
}
