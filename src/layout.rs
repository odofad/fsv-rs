use crate::node::{MapGeom, Node, NodeType};

/// MapV constants from fsv geometry.c.
const DIR_HEIGHT: f64 = 384.0;
const LEAF_HEIGHT: f64 = 128.0;
const BORDER_PROPORTION: f64 = 0.01;
const ROOT_ASPECT: f64 = 1.2;
const DIR_SLANT: f64 = 0.032;

/// How the renderer turns original units into the world.
/// None of this is written back onto the node.
#[derive(Debug, Clone, Copy)]
pub struct View {
    /// World width of the root rectangle.
    pub target_width: f64,
    /// If the true directory height would be thinner than this, stretch Z only.
    /// `0.0` keeps the original proportion.
    pub min_dir_height: f64,
    /// Fraction of each drawn block left empty. `0.0` is the original, blocks touching.
    pub gap: f64,
}

impl Default for View {
    fn default() -> Self {
        Self {
            target_width: 20.0,
            min_dir_height: 0.32,
            gap: 0.40,
        }
    }
}

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

pub fn layout(root: &mut Node, mode: LayoutMode) -> LayoutResult {
    match mode {
        LayoutMode::MapV => {
            build_mapv(root);
            emit_mapv(root, &View::default())
        }
        LayoutMode::TreeV => treev_layout(root),
    }
}

fn represented_bytes(node: &Node) -> f64 {
    // fsv: max(256, own size) + subtree-without-self for directories.
    // Our subtree_size already includes own size.
    (node.subtree_size.max(node.size).max(256)) as f64
}

/// Fill `node.map` for the root and every descendant. Original units only.
pub fn build_mapv(root: &mut Node) {
    let bytes = represented_bytes(root).max(1.0);
    let root_d = (bytes / ROOT_ASPECT).sqrt();
    let root_w = ROOT_ASPECT * root_d;
    root.map = Some(MapGeom {
        c0x: -0.5 * root_w,
        c0y: -0.5 * root_d,
        c1x: 0.5 * root_w,
        c1y: 0.5 * root_d,
        height: DIR_HEIGHT,
    });
    pack_children(root);
}

struct Placed {
    index: usize,
    geom: MapGeom,
}

fn pack_children(dir: &mut Node) {
    let placed = pack_rects(dir);
    for placed in placed {
        let child = &mut dir.children[placed.index];
        child.map = Some(placed.geom);
        if child.is_dir() {
            pack_children(child);
        }
    }
}

/// Port of mapv_init_recursive. Reads the parent rect, writes nothing.
fn pack_rects(dir: &Node) -> Vec<Placed> {
    let Some(parent) = dir.map else {
        return Vec::new();
    };
    if dir.children.is_empty() {
        return Vec::new();
    }

    let mut dims_x = parent.c1x - parent.c0x;
    let mut dims_y = parent.c1y - parent.c0y;
    let inset_x = parent.height.min(DIR_SLANT * dims_x);
    let inset_y = parent.height.min(DIR_SLANT * dims_y);
    dims_x -= 2.0 * inset_x;
    dims_y -= 2.0 * inset_y;
    if dims_x <= 1.0 || dims_y <= 1.0 {
        return Vec::new();
    }

    let nominal_border = (BORDER_PROPORTION * (dims_x * dims_y).sqrt())
        .min(dims_x.min(dims_y) / 3.0);
    dims_x -= nominal_border;
    dims_y -= nominal_border;
    if dims_x <= 1.0 || dims_y <= 1.0 {
        return Vec::new();
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
        return Vec::new();
    }
    let scale_factor = dir_area / total_area;

    struct Row {
        first: usize,
        area: f64,
    }
    let mut rows: Vec<Row> = Vec::new();
    let mut row_open = false;
    let mut row_area = 0.0;
    for (i, block) in blocks.iter_mut().enumerate() {
        block.area *= scale_factor;
        if !row_open {
            row_open = true;
            row_area = 0.0;
            rows.push(Row { first: i, area: 0.0 });
        }
        row_area += block.area;
        if let Some(row) = rows.last_mut() {
            row.area = row_area;
        }
        let row_depth = row_area / dims_x;
        let block_w = if row_depth > 0.0 {
            block.area / row_depth
        } else {
            0.0
        };
        if row_depth > 0.0 && block_w / row_depth < 1.0 {
            row_open = false;
        }
    }

    let start_x = (parent.c0x + parent.c1x) * 0.5 + 0.5 * dims_x;
    let start_y = (parent.c0y + parent.c1y) * 0.5 + 0.5 * dims_y;
    let mut pos_y = start_y;
    let mut block_i = 0;
    let mut placed = Vec::with_capacity(blocks.len());

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
            border = border.min((bw.min(row_depth) * 0.45).max(0.0));

            let child = &dir.children[block.index];
            let height = if child.is_dir() {
                DIR_HEIGHT
            } else {
                LEAF_HEIGHT
            };
            placed.push(Placed {
                index: block.index,
                geom: MapGeom {
                    c0x: pos_x - bw + border,
                    c0y: pos_y - row_depth + border,
                    c1x: pos_x - border,
                    c1y: pos_y - border,
                    height,
                },
            });

            pos_x -= bw;
            block_i += 1;
        }

        pos_y -= row_depth;
    }

    placed
}

fn emit_mapv(root: &Node, view: &View) -> LayoutResult {
    let mut boxes = Vec::new();
    let Some(root_geom) = root.map else {
        return LayoutResult {
            boxes,
            lines: Vec::new(),
            span: view.target_width as f32,
        };
    };

    let root_w = (root_geom.c1x - root_geom.c0x).max(1.0);
    let xy = view.target_width / root_w;
    let mut z_scale = xy;
    if view.min_dir_height > 0.0 {
        let dir_h = DIR_HEIGHT * xy;
        if dir_h < view.min_dir_height {
            z_scale = view.min_dir_height / DIR_HEIGHT;
        }
    }

    push_box(root, 0.0, true, xy, z_scale, 0.0, &mut boxes);
    emit_children(root, 0.0, xy, z_scale, view.gap, &mut boxes);

    LayoutResult {
        boxes,
        lines: Vec::new(),
        span: view.target_width as f32,
    }
}

fn emit_children(
    dir: &Node,
    z0: f64,
    xy: f64,
    z_scale: f64,
    gap: f64,
    out: &mut Vec<BoxGeom>,
) {
    let Some(geom) = dir.map else {
        return;
    };
    if !dir.expanded {
        return;
    }
    let child_z = z0 + geom.height;
    for child in &dir.children {
        push_box(child, child_z, false, xy, z_scale, gap, out);
        if child.is_dir() {
            emit_children(child, child_z, xy, z_scale, gap, out);
        }
    }
}

fn push_box(
    node: &Node,
    z0: f64,
    is_base: bool,
    xy: f64,
    z_scale: f64,
    gap: f64,
    out: &mut Vec<BoxGeom>,
) {
    let Some(geom) = node.map else {
        return;
    };
    let mut c0x = geom.c0x;
    let mut c0y = geom.c0y;
    let mut c1x = geom.c1x;
    let mut c1y = geom.c1y;
    if !is_base && gap > 0.0 {
        let w = c1x - c0x;
        let d = c1y - c0y;
        let ix = (w * gap * 0.5).min(w * 0.46);
        let iy = (d * gap * 0.5).min(d * 0.46);
        c0x += ix;
        c1x -= ix;
        c0y += iy;
        c1y -= iy;
    }
    let w = c1x - c0x;
    let d = c1y - c0y;
    if w < 1e-6 || d < 1e-6 || geom.height <= 0.0 {
        return;
    }
    out.push(BoxGeom {
        x: ((c0x + c1x) * 0.5 * xy) as f32,
        y: ((c0y + c1y) * 0.5 * xy) as f32,
        z: ((z0 + geom.height * 0.5) * z_scale) as f32,
        width: (w * xy) as f32,
        depth: (d * xy) as f32,
        height: (geom.height * z_scale) as f32,
        kind: node.kind.clone(),
        name: node.name.clone(),
        path: node.path.clone(),
        is_base,
    });
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
    let dist = radius * 2.6;

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
