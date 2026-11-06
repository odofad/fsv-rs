use crate::node::{MapGeom, Node, NodeType};

/// MapV constants from fsv geometry.c.
const DIR_HEIGHT: f64 = 384.0;
const LEAF_HEIGHT: f64 = 128.0;
const BORDER_PROPORTION: f64 = 0.01;
const ROOT_ASPECT: f64 = 1.2;
const DIR_SLANT: f64 = 0.032;
const GOLDEN: f64 = 1.618033988749895;

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
            // Wide streets. A block keeps about a third of its cell.
            gap: 0.66,
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
    /// Expanded directory pad. Drawn dark so the blocks on it read separately.
    pub floor: bool,
    /// Top-face inset, as a fraction of width and depth. From mapv_side_slant_ratios.
    pub slant_x: f32,
    pub slant_y: f32,
    /// Folder outline. Collapsed directories only.
    pub folder: bool,
    /// Name laid on the top face.
    pub label: bool,
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
    #[allow(dead_code)]
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

fn side_slant(kind: &NodeType) -> f64 {
    match kind {
        NodeType::Directory => 0.032,
        NodeType::File => 0.064,
        NodeType::Symlink => 0.333,
        NodeType::Other => 0.0,
    }
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

struct Block {
    index: usize,
    area: f64,
    bytes: f64,
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

/// Squarified MapV layout.
///
/// fsv's `mapv_init_recursive` always lays a row across the full width and
/// only looks at the newest block. A small folder already in that row is
/// then crushed when a larger sibling deepens it. The author left a note
/// ("write layout optimization routine sometime") and never did. This is
/// that pass: largest first, row along the short side, and a block is not
/// added when it makes the row's worst aspect worse. Border, slant inset
/// and heights stay the original numbers.
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
    let cx = (parent.c0x + parent.c1x) * 0.5;
    let cy = (parent.c0y + parent.c1y) * 0.5;

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
    for block in &mut blocks {
        block.area *= scale_factor;
    }
    blocks.sort_by(|a, b| b.area.total_cmp(&a.area).then(a.index.cmp(&b.index)));

    let mut placed = Vec::with_capacity(blocks.len());
    squarify(
        &blocks,
        cx - 0.5 * dims_x,
        cy - 0.5 * dims_y,
        cx + 0.5 * dims_x,
        cy + 0.5 * dims_y,
        scale_factor,
        dir,
        &mut placed,
    );
    placed
}

fn row_worst(blocks: &[Block], side: f64) -> f64 {
    let sum: f64 = blocks.iter().map(|b| b.area).sum();
    if sum <= 0.0 || side <= 0.0 {
        return f64::MAX;
    }
    let thick = sum / side;
    if thick <= 0.0 {
        return f64::MAX;
    }
    let mut worst = 1.0;
    for block in blocks {
        if block.area <= 0.0 {
            continue;
        }
        let len = block.area / thick;
        let aspect = (len / thick).max(thick / len);
        if aspect > worst {
            worst = aspect;
        }
    }
    worst
}

fn squarify(
    mut blocks: &[Block],
    mut x0: f64,
    mut y0: f64,
    x1: f64,
    y1: f64,
    scale: f64,
    dir: &Node,
    out: &mut Vec<Placed>,
) {
    while !blocks.is_empty() {
        let w = x1 - x0;
        let h = y1 - y0;
        if w <= 1.0 || h <= 1.0 {
            break;
        }
        // Strip runs along the short side, same rule as a squarified treemap.
        let side = w.min(h);
        let along_y = w >= h;
        let mut row_len = 1;
        let mut prev = row_worst(&blocks[..1], side);
        while row_len < blocks.len() {
            let next = row_worst(&blocks[..=row_len], side);
            if next <= prev {
                prev = next;
                row_len += 1;
            } else {
                break;
            }
        }
        let row_area: f64 = blocks[..row_len].iter().map(|b| b.area).sum();
        let room = if along_y { w } else { h };
        let thick = (row_area / side).clamp(0.0, room);
        if thick <= 1.0 {
            break;
        }
        if along_y {
            let mut cursor = y0;
            for block in &blocks[..row_len] {
                let len = if thick > 0.0 { block.area / thick } else { 0.0 };
                let end = (cursor + len).min(y1);
                place_block(block, x0, cursor, x0 + thick, end, scale, dir, out);
                cursor = end;
            }
            x0 += thick;
        } else {
            let mut cursor = x0;
            for block in &blocks[..row_len] {
                let len = if thick > 0.0 { block.area / thick } else { 0.0 };
                let end = (cursor + len).min(x1);
                place_block(block, cursor, y0, end, y0 + thick, scale, dir, out);
                cursor = end;
            }
            y0 += thick;
        }
        blocks = &blocks[row_len..];
    }
}

fn place_block(
    block: &Block,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    scale: f64,
    dir: &Node,
    out: &mut Vec<Placed>,
) {
    let bw = x1 - x0;
    let bd = y1 - y0;
    if bw < 1.0 || bd < 1.0 {
        return;
    }
    let node_area = scale * block.bytes;
    let kdim = bw + bd;
    let inner = (bw * bd) - node_area;
    let disc = kdim * kdim - 4.0 * inner;
    let mut border = if disc > 0.0 {
        0.25 * (kdim - disc.sqrt())
    } else {
        0.0
    };
    if !border.is_finite() || border < 0.0 {
        border = 0.0;
    }
    border = border.min((bw.min(bd) * 0.45).max(0.0));
    if bw - 2.0 * border < 1.0 || bd - 2.0 * border < 1.0 {
        border = border.min(bw.min(bd) * 0.1);
    }

    let child = &dir.children[block.index];
    let height = if child.is_dir() {
        DIR_HEIGHT
    } else {
        LEAF_HEIGHT
    };
    out.push(Placed {
        index: block.index,
        geom: MapGeom {
            c0x: x0 + border,
            c0y: y0 + border,
            c1x: x1 - border,
            c1y: y1 - border,
            height,
        },
    });
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
    // Children are packed into this rectangle. Shrinking an open directory
    // would leave them hanging off the sides.
    let open_dir = node.is_dir() && node.expanded;
    let draw_gap = if is_base || open_dir { 0.0 } else { gap };

    let full_w = geom.c1x - geom.c0x;
    let full_d = geom.c1y - geom.c0y;
    let k = side_slant(&node.kind);
    // Same inset the packer used, in original units. Do not recompute it
    // from the stretched world height or the top face moves out from under
    // the children.
    let slant_x = (geom.height.min(k * full_w) / full_w).clamp(0.0, 0.45) as f32;
    let slant_y = (geom.height.min(k * full_d) / full_d).clamp(0.0, 0.45) as f32;

    let mut c0x = geom.c0x;
    let mut c0y = geom.c0y;
    let mut c1x = geom.c1x;
    let mut c1y = geom.c1y;
    if draw_gap > 0.0 {
        let ix = (full_w * draw_gap * 0.5).min(full_w * 0.46);
        let iy = (full_d * draw_gap * 0.5).min(full_d * 0.46);
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
    let collapsed_dir = node.is_dir() && !node.expanded;
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
        floor: is_base || open_dir,
        slant_x,
        slant_y,
        folder: collapsed_dir && !is_base,
        label: !is_base && !node.name.is_empty() && (collapsed_dir || !node.is_dir()),
    });
}

/// Folder outline on the top face, in unit-box space (-0.5..0.5).
/// Port of mapv_gldraw_folder. `ox`/`oy` are the slant insets.
pub fn folder_outline(ox: f32, oy: f32) -> Vec<[f32; 2]> {
    let hw = 0.5 - ox;
    let hd = 0.5 - oy;
    let dims_x = hw * 2.0;
    let dims_y = hd * 2.0;
    let border = 0.0625 * dims_x.min(dims_y);
    let c0x = -hw + border;
    let c0y = -hd + border;
    let c1x = hw - border;
    let c1y = hd - border;
    let tab_x = c1x - (GOLDEN as f32 - 1.0) * (c1x - c0x);
    let tab_y = c1y - border;
    vec![
        [c0x, c0y],
        [c0x, tab_y],
        [c0x + border, c1y],
        [tab_x - border, c1y],
        [tab_x, tab_y],
        [c1x, tab_y],
        [c1x, c0y],
        [c0x, c0y],
    ]
}

/// Corner-bracket length, as a fraction of the box. From mapv_gldraw_cursor.
pub fn cursor_bar() -> f32 {
    let t = GOLDEN as f32 - 1.0;
    t * t * t * t
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
        floor: false,
        slant_x: 0.0,
        slant_y: 0.0,
        folder: false,
        label: false,
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
