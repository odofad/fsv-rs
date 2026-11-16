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
            min_dir_height: 1.15,
            // A street, not a vacant lot. A block keeps about four fifths of its cell.
            gap: 0.22,
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
    /// Flat X. An opened directory leaves this on its parent.
    pub x_mark: bool,
    /// Yaw around the up axis, radians. MapV leaves this at 0.
    pub yaw: f32,
    /// File age, 0 oldest to 1 newest. Directories stay None: color.c
    /// keeps their type color even in timestamp mode.
    pub age: Option<f32>,
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

/// Annular sector. TreeV platforms and the dark red branches.
#[derive(Debug, Clone)]
pub struct ArcGeom {
    pub r0: f32,
    pub r1: f32,
    pub theta0: f32,
    pub theta1: f32,
    pub y: f32,
    pub height: f32,
    pub path: std::path::PathBuf,
    pub name: String,
    pub branch: bool,
    pub label: bool,
}

pub struct LayoutResult {
    pub boxes: Vec<BoxGeom>,
    pub lines: Vec<LineGeom>,
    pub arcs: Vec<ArcGeom>,
    /// Camera distance for the first TreeV view. MapV ignores it.
    pub span: f32,
    /// World point the TreeV camera looks at. MapV leaves this at the origin.
    pub focus: [f32; 3],
}

pub fn layout(root: &mut Node, mode: LayoutMode) -> LayoutResult {
    let mut oldest = u64::MAX;
    let mut newest = 0u64;
    mtime_span(root, &mut oldest, &mut newest);
    if oldest == u64::MAX {
        oldest = 0;
    }
    let span = (oldest, newest);
    match mode {
        LayoutMode::MapV => {
            build_mapv(root);
            emit_mapv(root, &View::default(), span)
        }
        LayoutMode::TreeV => treev_layout(root, span),
    }
}

fn mtime_span(node: &Node, oldest: &mut u64, newest: &mut u64) {
    if !node.is_dir() {
        if let Some(t) = node.mtime {
            if let Ok(d) = t.duration_since(std::time::UNIX_EPOCH) {
                let s = d.as_secs();
                *oldest = (*oldest).min(s);
                *newest = (*newest).max(s);
            }
        }
    }
    for child in &node.children {
        mtime_span(child, oldest, newest);
    }
}

/// color.c time_color: directories are not aged. Files run from the
/// oldest mtime in the tree to the newest. The original default window
/// is one week, which paints an archive all one color.
fn file_age(node: &Node, oldest: u64, newest: u64) -> Option<f32> {
    if node.is_dir() {
        return None;
    }
    let t = node.mtime?;
    let s = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    if newest <= oldest {
        return Some(1.0);
    }
    Some(((s.saturating_sub(oldest)) as f32 / (newest - oldest) as f32).clamp(0.0, 1.0))
}

fn represented_bytes(node: &Node) -> f64 {
    // TreeV leaf height. subtree_size already includes this node.
    (node.subtree_size.max(node.size).max(64)) as f64
}

/// MapV block size. A child is at least 256 bytes. A directory adds the
/// subtree without counting itself twice.
fn map_child_bytes(node: &Node) -> f64 {
    let own = node.size.max(256) as f64;
    if node.is_dir() {
        own + node.subtree_size.saturating_sub(node.size) as f64
    } else {
        own
    }
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
    // mapv_init sizes the root from the real byte count. The 256-byte floor
    // applies to children only.
    let bytes = (root.subtree_size.max(1)) as f64;
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

/// `mapv_init_recursive`. Rows across the full width, children in scan order.
/// The C source leaves the "optimize the layout" pass empty.
fn pack_rects(dir: &Node) -> Vec<Placed> {
    let Some(parent) = dir.map else {
        return Vec::new();
    };
    if dir.children.is_empty() {
        return Vec::new();
    }

    let mut dims_x = parent.c1x - parent.c0x;
    let mut dims_y = parent.c1y - parent.c0y;
    dims_x -= 2.0 * parent.height.min(DIR_SLANT * dims_x);
    dims_y -= 2.0 * parent.height.min(DIR_SLANT * dims_y);
    if dims_x <= 1.0 || dims_y <= 1.0 {
        return Vec::new();
    }
    let nominal = (BORDER_PROPORTION * (dims_x * dims_y).sqrt()).min(dims_x.min(dims_y) / 3.0);
    dims_x -= nominal;
    dims_y -= nominal;
    if dims_x <= 1.0 || dims_y <= 1.0 {
        return Vec::new();
    }
    let dir_area = dims_x * dims_y;

    let mut blocks: Vec<Block> = Vec::with_capacity(dir.children.len());
    let mut total_area = 0.0;
    for (index, child) in dir.children.iter().enumerate() {
        let bytes = map_child_bytes(child);
        let area = (bytes.sqrt() + nominal).powi(2);
        total_area += area;
        blocks.push(Block { index, area, bytes });
    }
    if total_area <= 0.0 {
        return Vec::new();
    }
    let scale = dir_area / total_area;
    for block in &mut blocks {
        block.area *= scale;
    }

    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut row: Vec<usize> = Vec::new();
    let mut row_area = 0.0;
    for (n, block) in blocks.iter().enumerate() {
        if row.is_empty() {
            row_area = 0.0;
        }
        row.push(n);
        row_area += block.area;
        let by = row_area / dims_x;
        let bx = block.area / by;
        if bx / by < 1.0 {
            rows.push(std::mem::take(&mut row));
            row_area = 0.0;
        }
    }
    if !row.is_empty() {
        rows.push(row);
    }

    let cx = (parent.c0x + parent.c1x) * 0.5;
    let cy = (parent.c0y + parent.c1y) * 0.5;
    let mut pos_y = cy + 0.5 * dims_y;
    let mut placed = Vec::with_capacity(blocks.len());
    for row in &rows {
        let area: f64 = row.iter().map(|&i| blocks[i].area).sum();
        let by = area / dims_x;
        let mut pos_x = cx + 0.5 * dims_x;
        for &i in row {
            let block = &blocks[i];
            let bx = block.area / by;
            let node_area = scale * block.bytes;
            let k = bx + by;
            let disc = k * k - 4.0 * (block.area - node_area);
            let border = if disc > 0.0 {
                0.25 * (k - disc.sqrt())
            } else {
                0.0
            };
            let child = &dir.children[block.index];
            let height = if child.is_dir() { DIR_HEIGHT } else { LEAF_HEIGHT };
            placed.push(Placed {
                index: block.index,
                geom: MapGeom {
                    c0x: pos_x - bx + border,
                    c0y: pos_y - by + border,
                    c1x: pos_x - border,
                    c1y: pos_y - border,
                    height,
                },
            });
            pos_x -= bx;
        }
        pos_y -= by;
    }
    placed
}

fn emit_mapv(root: &Node, view: &View, span: (u64, u64)) -> LayoutResult {
    let mut boxes = Vec::new();
    let Some(root_geom) = root.map else {
        return LayoutResult {
            boxes,
            lines: Vec::new(),
            arcs: Vec::new(),
            span: view.target_width as f32,
            focus: [0.0, 0.0, 0.0],
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

    push_box(root, 0.0, true, xy, z_scale, 0.0, span, &mut boxes);
    emit_children(root, 0.0, xy, z_scale, view.gap, span, &mut boxes);

    LayoutResult {
        boxes,
        lines: Vec::new(),
        arcs: Vec::new(),
        span: view.target_width as f32,
        focus: [0.0, 0.0, 0.0],
    }
}

fn emit_children(
    dir: &Node,
    z0: f64,
    xy: f64,
    z_scale: f64,
    gap: f64,
    span: (u64, u64),
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
        push_box(child, child_z, false, xy, z_scale, gap, span, out);
        if child.is_dir() {
            emit_children(child, child_z, xy, z_scale, gap, span, out);
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
    span: (u64, u64),
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
        x_mark: false,
        yaw: 0.0,
        age: file_age(node, span.0, span.1),
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

pub fn treev_layout(root: &Node, span: (u64, u64)) -> LayoutResult {
    // geometry.c treev_init / treev_arrange / treev_build_dir.
    const SPACING_D: f64 = 2048.0;
    const MIN_CORE: f64 = 8192.0;
    const GROW: f64 = 1.25;
    const MIN_ARC: f64 = 90.0;
    const MAX_ARC: f64 = 225.0;

    let mut core = MIN_CORE;
    for _ in 0..24 {
        let (arc, _, sub) = tv_measure(root, core + SPACING_D);
        let width = arc.max(sub);
        if width > MAX_ARC {
            core *= GROW;
        } else if width < MIN_ARC && core > MIN_CORE {
            core = (core / GROW).max(MIN_CORE);
            if core <= MIN_CORE {
                break;
            }
        } else {
            break;
        }
    }

    let mut boxes = Vec::new();
    let mut arcs = Vec::new();
    let mut max_r = core + SPACING_D;
    if root.expanded {
        tv_emit(
            root,
            core + SPACING_D,
            90.0,
            0.0,
            false,
            &mut boxes,
            &mut arcs,
            &mut max_r,
            span,
        );
    } else {
        tv_leaf(
            root,
            core,
            0.5 * SPACING_D,
            90.0,
            0.0,
            true,
            false,
            &mut boxes,
            &mut max_r,
            span,
        );
    }

    let scale = 18.0 / max_r.max(1.0);
    for b in &mut boxes {
        b.x = (b.x as f64 * scale) as f32;
        b.y = (b.y as f64 * scale) as f32;
        b.z = (b.z as f64 * scale) as f32;
        b.width = (b.width as f64 * scale) as f32;
        b.depth = (b.depth as f64 * scale) as f32;
        b.height = (b.height as f64 * scale) as f32;
    }
    for a in &mut arcs {
        a.r0 = (a.r0 as f64 * scale) as f32;
        a.r1 = (a.r1 as f64 * scale) as f32;
        a.y = (a.y as f64 * scale) as f32;
        a.height = (a.height as f64 * scale) as f32;
    }

    let (focus, span) = treev_frame(&arcs, &boxes);
    LayoutResult {
        boxes,
        lines: Vec::new(),
        arcs,
        span,
        focus,
    }
}

/// First TreeV view looks at the root platform, not the empty core.
/// camera.c puts the target on that platform and looks straight down.
fn treev_frame(arcs: &[ArcGeom], boxes: &[BoxGeom]) -> ([f32; 3], f32) {
    let Some(platform) = arcs.iter().find(|arc| !arc.branch) else {
        return ([0.0, 0.0, 0.0], 36.0);
    };
    let r = (platform.r0 + platform.r1) * 0.5;
    let theta = (platform.theta0 + platform.theta1) * 0.5;
    let fx = r * theta.cos();
    let fz = r * theta.sin();
    let fy = platform.y + platform.height * 0.5;
    let mut reach = (platform.r1 - platform.r0).abs();
    reach = reach.max(r * (platform.theta1 - platform.theta0).abs());
    for box_geom in boxes {
        let dx = box_geom.x - fx;
        let dz = box_geom.y - fz;
        let dist = (dx * dx + dz * dz).sqrt() + box_geom.width.max(box_geom.depth);
        reach = reach.max(dist);
    }
    ([fx, fy, fz], reach.max(platform.height).max(1.0))
}

fn tv_half_deg(width: f64, radius: f64) -> f64 {
    (180.0 * width / std::f64::consts::PI) / radius.max(1.0) * 0.5
}

fn raw_arc(
    r0: f64,
    r1: f64,
    theta0: f64,
    theta1: f64,
    y: f64,
    height: f64,
    node: &Node,
    branch: bool,
    label: bool,
) -> ArcGeom {
    ArcGeom {
        r0: r0 as f32,
        r1: r1 as f32,
        theta0: theta0.to_radians() as f32,
        theta1: theta1.to_radians() as f32,
        y: y as f32,
        height: height as f32,
        path: node.path.clone(),
        name: node.name.clone(),
        branch,
        label,
    }
}

/// Arc width of this platform, its depth, and the arc taken by expanded children.
/// A collapsed directory is a leaf on its parent, so it adds no arc.
fn tv_measure(node: &Node, r0: f64) -> (f64, f64, f64) {
    if !node.is_dir() || !node.expanded {
        return (0.0, 0.0, 0.0);
    }
    let (depth, arc) = tv_fit(node.children.len().max(1), r0);
    let child_r = r0 + depth + SPACING_D;
    let mut sub = 0.0;
    for child in &node.children {
        if child.is_dir() && child.expanded {
            let (ca, _, cs) = tv_measure(child, child_r);
            sub += ca.max(cs);
        }
    }
    (arc, depth, sub)
}

/// Nominal arc from the original cubic, then a depth that actually covers the rows.
fn tv_fit(n_children: usize, r0: f64) -> (f64, f64) {
    let (_, arc) = tv_reshape(n_children, r0);
    let depth = tv_rows_depth(n_children, r0, arc);
    (depth, arc)
}

fn tv_reshape(n_children: usize, r0: f64) -> (f64, f64) {
    const PI: f64 = std::f64::consts::PI;
    let n = n_children.max(1) as f64;
    let k = PITCH * n.sqrt().ceil() + EDGE05;
    let area = k * k;
    let mut d = tv_depth(area, r0, SPACING_W);
    if !d.is_finite() || d <= 0.0 {
        d = PITCH;
    }
    let mut depth = d;
    let frac = (depth - EDGE05).rem_euclid(PITCH);
    depth += (PITCH - frac) + EDGE05;
    let theta = 180.0 * (d + SPACING_W) / (PI * (r0 + d));
    let min_arc = (180.0 * (2.0 * LEAF + SPACING_W) / PI) / r0.max(1.0);
    (depth, theta.max(min_arc))
}

/// Same final depth as treev_build_dir: lay the rows, then step back half a block.
fn tv_rows_depth(n: usize, r0: f64, arc_deg: f64) -> f64 {
    const PI: f64 = std::f64::consts::PI;
    if n == 0 {
        return PITCH;
    }
    let mut remaining = n;
    let mut pos_r = r0 + LEAF;
    while remaining > 0 {
        let arc_len = (PI / 180.0) * pos_r * arc_deg - SPACING_W;
        let mut row_n = ((arc_len - EDGE05) / PITCH).floor() as isize;
        if row_n < 1 {
            row_n = 1;
        }
        let take = (row_n as usize).min(remaining);
        remaining -= take;
        pos_r += PITCH;
    }
    (pos_r - EDGE05 - r0).max(PITCH)
}

/// Positive root of the platform cubic in treev_reshape_platform.
fn tv_depth(a: f64, r: f64, w: f64) -> f64 {
    let a2 = a * a;
    let a3 = a * a2;
    let r2 = r * r;
    let r3 = r * r2;
    let r4 = r2 * r2;
    let w2 = w * w;
    let w3 = w2 * w;
    let w4 = w2 * w2;
    let ka = 72.0 * (a * r - w * (a + r)) - 64.0 * r3 + 48.0 * r2 * w - 36.0 * w2
        + 24.0 * r * w2
        - 8.0 * w3;
    let t1 = 72.0 * a * w2 - 132.0 * a * r * w2 - 240.0 * a * w * r3 + 120.0 * a * w2 * r2
        - 24.0 * a2 * w * r
        - 60.0 * w3 * r;
    let t2 = 12.0 * (w2 * r2 + a2 * w2 - w4 * r + w4 * r2 + a * w3 + w3);
    let t3 = 48.0 * (w2 * r4 - w2 * r3 - w3 * r3) + 96.0 * (a3 + w3 * r2);
    let t4 = 192.0 * a * r4 + 156.0 * a2 * r2 + 3.0 * w4 + 144.0 * a2 * w + 264.0 * a * w * r2;
    let inside = t1 + t2 + t3 + t4;
    if inside < 0.0 {
        return -1.0;
    }
    let kb = 12.0 * inside.sqrt();
    let kc = (kb.atan2(ka) / 3.0).cos();
    let kd = ka.hypot(kb).cbrt();
    if kd.abs() < 1e-9 {
        return -1.0;
    }
    (-w - 2.0 * r) / 3.0
        + ((8.0 * r2 - 4.0 * w * r + 2.0 * w2) / 3.0 + 4.0 * a + 2.0 * w) * kc / kd
        + kc * kd / 6.0
}

fn tv_emit(
    node: &Node,
    r0: f64,
    abs_deg: f64,
    z0: f64,
    incoming: bool,
    boxes: &mut Vec<BoxGeom>,
    arcs: &mut Vec<ArcGeom>,
    max_r: &mut f64,
    span: (u64, u64),
) {
    const BRANCH: f64 = 256.0;
    const PLATFORM_H: f64 = 320.0;
    const PI: f64 = std::f64::consts::PI;

    let (depth, arc) = tv_fit(node.children.len().max(1), r0);
    let half = 0.5 * arc;
    arcs.push(raw_arc(
        r0,
        r0 + depth,
        abs_deg - half,
        abs_deg + half,
        z0,
        PLATFORM_H,
        node,
        false,
        true,
    ));
    *max_r = max_r.max(r0 + depth);

    // Incoming stem. The root has no parent, so it does not grow a line
    // back to the empty core ring.
    if incoming {
        let stem_r = r0 - 0.5 * SPACING_D;
        let stem_half = tv_half_deg(BRANCH, (stem_r + r0) * 0.5);
        arcs.push(raw_arc(
            stem_r,
            r0,
            abs_deg - stem_half,
            abs_deg + stem_half,
            z0,
            48.0,
            node,
            true,
            false,
        ));
    }

    // Every child takes a slot, including an open directory. An open
    // directory is not a block: it is the next platform. A closed
    // directory is a block. Files are blocks.
    let kids: Vec<&Node> = node.children.iter().collect();
    let mut remaining = kids.len();
    let mut pos_r = r0 + LEAF;
    let mut idx = kids.len();
    while idx > 0 {
        let arc_len = (PI / 180.0) * pos_r * arc - SPACING_W;
        let mut row_n = ((arc_len - EDGE05) / PITCH).floor() as isize;
        if row_n < 1 {
            row_n = 1;
        }
        let row_n = row_n as usize;
        let take = row_n.min(remaining);
        let inter = (180.0 * PITCH / PI) / pos_r;
        let mut theta = 0.5 * inter * (take as f64 - 1.0);
        for _ in 0..take {
            idx -= 1;
            let child = kids[idx];
            let open_dir = child.is_dir() && child.expanded;
            // An open directory keeps a flat X in its old slot.
            tv_leaf(
                child,
                r0,
                pos_r - r0,
                abs_deg + theta,
                z0 + PLATFORM_H,
                !open_dir,
                open_dir,
                boxes,
                max_r,
                span,
            );
            theta -= inter;
        }
        remaining = remaining.saturating_sub(row_n);
        pos_r += PITCH;
        if remaining == 0 {
            break;
        }
    }

    let child_r = r0 + depth + SPACING_D;
    let mut first: Option<f64> = None;
    let mut last: Option<f64> = None;
    let mut cursor = -0.5
        * node
            .children
            .iter()
            .filter(|c| c.is_dir() && c.expanded)
            .map(|c| {
                let (ca, _, cs) = tv_measure(c, child_r);
                ca.max(cs)
            })
            .sum::<f64>();
    for child in &node.children {
        if !(child.is_dir() && child.expanded) {
            continue;
        }
        let (ca, _, cs) = tv_measure(child, child_r);
        let w = ca.max(cs);
        let rel = cursor + 0.5 * w;
        cursor += w;
        first = Some(first.unwrap_or(rel));
        last = Some(rel);
        tv_emit(
            child,
            child_r,
            abs_deg + rel,
            z0,
            true,
            boxes,
            arcs,
            max_r,
            span,
        );
    }
    if let (Some(a), Some(b)) = (first, last) {
        let r1 = r0 + depth;
        let mid_r = r1 + 0.5 * SPACING_D;
        let half_w = tv_half_deg(BRANCH, mid_r);
        arcs.push(raw_arc(
            r1,
            mid_r,
            abs_deg - half_w,
            abs_deg + half_w,
            z0,
            48.0,
            node,
            true,
            false,
        ));
        let pad = tv_half_deg(BRANCH, mid_r);
        let span_deg = (b - a).abs();
        // treev_gldraw_outbranch skips a zero-length arc. One open child
        // sits on the center line, so only the stem is drawn.
        if span_deg > 1e-3 {
            arcs.push(raw_arc(
                mid_r - 0.5 * BRANCH,
                mid_r + 0.5 * BRANCH,
                abs_deg + a.min(b) - pad,
                abs_deg + a.max(b) + pad,
                z0,
                48.0,
                node,
                true,
                false,
            ));
        }
    }
}

const LEAF: f64 = 256.0;
const EDGE05: f64 = 0.5 * LEAF;
/// Center-to-center. treev_build_dir steps 1.5 leaf edges.
const PITCH: f64 = 1.5 * LEAF;
const SPACING_W: f64 = 512.0;
const SPACING_D: f64 = 2048.0;

fn tv_leaf_height(node: &Node) -> f64 {
    // geometry.c uses sqrt(bytes). On a 1999 disk that stayed near the
    // leaf edge. A few gigabytes is ~100 edges, which hides the platforms.
    // Same order as sqrt, then a slow climb that stops at 2.4 edges.
    const EDGE: f64 = 256.0;
    let bytes = represented_bytes(node).max(64.0);
    let raw = bytes.sqrt();
    if raw <= EDGE {
        raw.max(EDGE * 0.2)
    } else {
        let doublings = (bytes / (EDGE * EDGE)).ln() / std::f64::consts::LN_2;
        (EDGE + doublings * EDGE * 0.18).min(EDGE * 2.4)
    }
}

fn tv_leaf(
    node: &Node,
    r0: f64,
    dist: f64,
    abs_deg: f64,
    z0: f64,
    full: bool,
    x_mark: bool,
    boxes: &mut Vec<BoxGeom>,
    max_r: &mut f64,
    span: (u64, u64),
) {
    const LEAF: f64 = 256.0;
    let edge = if full { LEAF } else { 0.875 * LEAF };
    // geometry.c: leaf.height = sqrt(bytes). The camera frames the tallest
    // one, which is what geometry_treev_max_leaf_height is for.
    let height = if full { tv_leaf_height(node) } else { LEAF / 64.0 };
    let theta = abs_deg.to_radians();
    let r = r0 + dist;
    *max_r = max_r.max(r + 0.5 * edge);
    let collapsed_dir = node.is_dir() && !node.expanded;
    boxes.push(BoxGeom {
        x: (r * theta.cos()) as f32,
        y: (r * theta.sin()) as f32,
        z: (z0 + height * 0.5) as f32,
        width: edge as f32,
        depth: edge as f32,
        height: height as f32,
        kind: node.kind.clone(),
        name: node.name.clone(),
        path: node.path.clone(),
        floor: false,
        slant_x: 0.0,
        slant_y: 0.0,
        folder: collapsed_dir && !x_mark,
        label: full && !x_mark && !node.name.is_empty(),
        x_mark,
        yaw: theta as f32,
        age: file_age(node, span.0, span.1),
    });
}
