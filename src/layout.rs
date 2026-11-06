use crate::node::{Node, NodeType};

/// MapV constants from fsv geometry.c.
/// Area is proportional to size. Height is a fixed short prism
/// (directories 384, leaves 128), not a stack of slabs.
const DIR_HEIGHT: f64 = 384.0;
const LEAF_HEIGHT: f64 = 128.0;
const BORDER_PROPORTION: f64 = 0.01;
const ROOT_ASPECT: f64 = 1.2;
const DIR_SLANT: f64 = 0.032;
/// World width of the root, so the camera in main.rs frames the city.
const TARGET_WIDTH: f64 = 20.0;
/// Keep directory prisms visible when the real filesystem is huge.
/// Ratio of directory height to file height stays 3:1, same as fsv.
const MIN_DIR_HEIGHT: f64 = 0.32;

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
    pub span: f32,
}

pub fn layout(root: &Node, mode: LayoutMode) -> LayoutResult {
    match mode {
        LayoutMode::MapV => mapv_layout(root),
        LayoutMode::TreeV => treev_layout(root),
    }
}

fn represented_bytes(node: &Node) -> f64 {
    // fsv: max(256, own size) + subtree-without-self for directories.
    // Our subtree_size already includes own size, which matches that total.
    (node.subtree_size.max(node.size).max(256)) as f64
}

pub fn mapv_layout(root: &Node) -> LayoutResult {
    let mut boxes = Vec::new();
    let bytes = represented_bytes(root).max(1.0);
    let root_d = (bytes / ROOT_ASPECT).sqrt();
    let root_w = ROOT_ASPECT * root_d;
    let xy_scale = TARGET_WIDTH / root_w.max(1.0);
    let dir_h_world = DIR_HEIGHT * xy_scale;
    let z_boost = if dir_h_world < MIN_DIR_HEIGHT {
        MIN_DIR_HEIGHT / dir_h_world
    } else {
        1.0
    };
    let z_scale = xy_scale * z_boost;

    let c0x = -0.5 * root_w;
    let c0y = -0.5 * root_d;
    let c1x = 0.5 * root_w;
    let c1y = 0.5 * root_d;

    emit_box(
        root,
        c0x,
        c0y,
        c1x,
        c1y,
        0.0,
        DIR_HEIGHT,
        true,
        xy_scale,
        z_scale,
        &mut boxes,
    );

    if root.expanded {
        place_children(
            root,
            c0x,
            c0y,
            c1x,
            c1y,
            DIR_HEIGHT,
            0.0,
            xy_scale,
            z_scale,
            &mut boxes,
        );
    }

    LayoutResult {
        boxes,
        lines: Vec::new(),
        span: TARGET_WIDTH as f32,
    }
}

fn emit_box(
    node: &Node,
    c0x: f64,
    c0y: f64,
    c1x: f64,
    c1y: f64,
    z0: f64,
    height: f64,
    is_base: bool,
    xy_scale: f64,
    z_scale: f64,
    out: &mut Vec<BoxGeom>,
) {
    let w = (c1x - c0x).max(0.0);
    let d = (c1y - c0y).max(0.0);
    if w < 1e-6 || d < 1e-6 || height <= 0.0 {
        return;
    }
    out.push(BoxGeom {
        x: ((c0x + c1x) * 0.5 * xy_scale) as f32,
        y: ((c0y + c1y) * 0.5 * xy_scale) as f32,
        z: ((z0 + height * 0.5) * z_scale) as f32,
        width: (w * xy_scale) as f32,
        depth: (d * xy_scale) as f32,
        height: (height * z_scale) as f32,
        kind: node.kind.clone(),
        name: node.name.clone(),
        path: node.path.clone(),
        is_base,
    });
}

/// Port of mapv_init_recursive: pack every child into the directory's top face.
fn place_children(
    dir: &Node,
    c0x: f64,
    c0y: f64,
    c1x: f64,
    c1y: f64,
    dir_height: f64,
    z0: f64,
    xy_scale: f64,
    z_scale: f64,
    out: &mut Vec<BoxGeom>,
) {
    if dir.children.is_empty() {
        return;
    }

    let mut dims_x = c1x - c0x;
    let mut dims_y = c1y - c0y;
    let inset_x = dir_height.min(DIR_SLANT * dims_x);
    let inset_y = dir_height.min(DIR_SLANT * dims_y);
    dims_x -= 2.0 * inset_x;
    dims_y -= 2.0 * inset_y;
    if dims_x <= 1.0 || dims_y <= 1.0 {
        return;
    }

    let nominal_border = (BORDER_PROPORTION * (dims_x * dims_y).sqrt())
        .min(dims_x.min(dims_y) / 3.0);
    dims_x -= nominal_border;
    dims_y -= nominal_border;
    if dims_x <= 1.0 || dims_y <= 1.0 {
        return;
    }
    let dir_area = dims_x * dims_y;

    struct Block {
        index: usize,
        area: f64,
        bytes: f64,
    }

    let mut blocks: Vec<Block> = Vec::with_capacity(dir.children.len());
    let mut total_area = 0.0;
    for (index, child) in dir.children.iter().enumerate() {
        let bytes = represented_bytes(child);
        let k = bytes.sqrt() + nominal_border;
        let area = k * k;
        total_area += area;
        blocks.push(Block { index, area, bytes });
    }
    if total_area <= 0.0 {
        return;
    }
    let scale_factor = dir_area / total_area;

    // Second pass: scale, then cut a new row once a block is taller than it is wide.
    struct Row {
        first: usize,
        area: f64,
    }
    let mut rows: Vec<Row> = Vec::new();
    let mut row_first: Option<usize> = None;
    let mut row_area = 0.0;
    for (i, block) in blocks.iter_mut().enumerate() {
        block.area *= scale_factor;
        if row_first.is_none() {
            row_first = Some(i);
            row_area = 0.0;
            rows.push(Row { first: i, area: 0.0 });
        }
        row_area += block.area;
        if let Some(row) = rows.last_mut() {
            row.area = row_area;
        }
        let row_depth = row_area / dims_x;
        let block_w = if row_depth > 0.0 { block.area / row_depth } else { 0.0 };
        if row_depth > 0.0 && block_w / row_depth < 1.0 {
            row_first = None;
        }
    }

    // Lay rows from the right/rear corner of the top face, same as fsv.
    let start_x = (c0x + c1x) * 0.5 + 0.5 * dims_x;
    let start_y = (c0y + c1y) * 0.5 + 0.5 * dims_y;
    let child_z0 = z0 + dir_height;
    let mut pos_y = start_y;
    let mut block_i = 0;

    for (ri, row) in rows.iter().enumerate() {
        let row_depth = row.area / dims_x;
        if row_depth <= 0.0 {
            continue;
        }
        let mut pos_x = start_x;
        let next_first = rows.get(ri + 1).map(|r| r.first);

        while block_i < blocks.len() && Some(block_i) != next_first {
            let block = &blocks[block_i];
            let bw = block.area / row_depth;
            let node_area = scale_factor * block.bytes;
            let kdim = bw + row_depth;
            let inner = block.area - node_area;
            let disc = kdim * kdim - 4.0 * inner;
            let mut border = if disc > 0.0 {
                0.25 * (kdim - disc.sqrt())
            } else {
                0.0
            };
            if !border.is_finite() || border < 0.0 {
                border = 0.0;
            }
            let max_border = (bw.min(row_depth) * 0.45).max(0.0);
            border = border.min(max_border);

            let child = &dir.children[block.index];
            let cc0x = pos_x - bw + border;
            let cc0y = pos_y - row_depth + border;
            let cc1x = pos_x - border;
            let cc1y = pos_y - border;
            let height = if child.is_dir() { DIR_HEIGHT } else { LEAF_HEIGHT };

            emit_box(
                child,
                cc0x,
                cc0y,
                cc1x,
                cc1y,
                child_z0,
                height,
                false,
                xy_scale,
                z_scale,
                out,
            );

            if child.is_dir() && child.expanded {
                place_children(
                    child,
                    cc0x,
                    cc0y,
                    cc1x,
                    cc1y,
                    height,
                    child_z0,
                    xy_scale,
                    z_scale,
                    out,
                );
            }

            pos_x -= bw;
            block_i += 1;
        }

        pos_y -= row_depth;
    }
}

pub fn treev_layout(root: &Node) -> LayoutResult {
    let mut boxes = Vec::new();
    let mut lines = Vec::new();
    if root.subtree_size == 0 {
        return LayoutResult {
            boxes,
            lines,
            span: 10.0,
        };
    }

    treev_recursive(root, 0.0, 0.0, 0.0, 2.5, 0.0, &mut boxes, &mut lines);
    LayoutResult {
        boxes,
        lines,
        span: 16.0,
    }
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
        is_base: node.is_dir(),
    });

    if !node.is_dir() || !node.expanded || node.children.is_empty() {
        return;
    }

    let n = node.children.len().max(1) as f32;
    let angle_step = std::f32::consts::TAU / n;
    let child_radius = radius * 0.55;
    let dist = radius * 1.8;

    for (i, child) in node.children.iter().enumerate() {
        let angle = parent_angle + i as f32 * angle_step;
        let child_cx = cx + dist * angle.cos();
        let child_cy = cy + dist * angle.sin();

        lines.push(LineGeom {
            from: [cx, z + height, cy],
            to: [child_cx, z + height, child_cy],
        });

        treev_recursive(
            child,
            child_cx,
            child_cy,
            z + height,
            child_radius,
            angle,
            boxes,
            lines,
        );
    }
}
