//! Step 2. A fixture of three directories and five files, and the numbers
//! the original formulas produce. The current layout is printed beside
//! them. Nothing here changes the picture.
//!
//! `cargo test -- --ignored --nocapture` shows the mismatch and fails.
//! `cargo test` only checks that this reference stays put.

use std::path::PathBuf;

use crate::layout::{self, LayoutMode};
use crate::node::{Node, NodeType};

const DIR_H: f64 = 384.0;
const LEAF_H: f64 = 128.0;
const SLANT: f64 = 0.032;
const BORDER: f64 = 0.01;
const ASPECT: f64 = 1.2;

struct Gold {
    name: &'static str,
    c0x: f64,
    c0y: f64,
    c1x: f64,
    c1y: f64,
    height: f64,
}

fn file(name: &str, size: u64) -> Node {
    let mut n = Node::new(name.into(), PathBuf::from(name), NodeType::File);
    n.size = size;
    n.subtree_size = size;
    n.expanded = false;
    n
}

fn dir(name: &str, expanded: bool, children: Vec<Node>) -> Node {
    let mut n = Node::new(name.into(), PathBuf::from(name), NodeType::Directory);
    n.expanded = expanded;
    n.children = children;
    let mut total = n.size;
    for child in &n.children {
        total += child.subtree_size;
    }
    n.subtree_size = total;
    n
}

/// root, docs (open), src (closed), and five files.
/// Scan order: directories first, then larger, then name.
fn fixture() -> Node {
    let docs = dir("docs", true, vec![file("notes", 4000), file("readme", 1000)]);
    let src = dir("src", false, vec![file("main", 9000), file("util", 100)]);
    dir(
        "root",
        true,
        vec![src, docs, file("photo", 160000)],
    )
}

fn c_bytes(node: &Node) -> f64 {
    let own = node.size.max(256) as f64;
    if node.is_dir() {
        // subtree_size includes this node. The C figure does not.
        own + node.subtree_size.saturating_sub(node.size) as f64
    } else {
        own
    }
}

struct Placed {
    name: String,
    c0x: f64,
    c0y: f64,
    c1x: f64,
    c1y: f64,
    height: f64,
    children: Vec<Placed>,
}

/// `mapv_init` + `mapv_init_recursive`. The row walk, not a squarified treemap.
fn golden_map(node: &Node, rect: Option<(f64, f64, f64, f64, f64)>) -> Placed {
    let (c0x, c0y, c1x, c1y, height) = if let Some(r) = rect {
        r
    } else {
        // mapv_init sizes the root from the real byte count, not the
        // 256-byte floor used when packing a child.
        let bytes = node.subtree_size as f64;
        let depth = (bytes / ASPECT).sqrt();
        let width = ASPECT * depth;
        (-0.5 * width, -0.5 * depth, 0.5 * width, 0.5 * depth, DIR_H)
    };
    let mut children = Vec::new();
    if !node.children.is_empty() {
        let mut dims_x = c1x - c0x;
        let mut dims_y = c1y - c0y;
        dims_x -= 2.0 * height.min(SLANT * dims_x);
        dims_y -= 2.0 * height.min(SLANT * dims_y);
        let nominal = (BORDER * (dims_x * dims_y).sqrt()).min(dims_x.min(dims_y) / 3.0);
        dims_x -= nominal;
        dims_y -= nominal;
        let dir_area = dims_x * dims_y;
        let mut blocks: Vec<(usize, f64, f64)> = Vec::new();
        let mut total = 0.0;
        for (i, child) in node.children.iter().enumerate() {
            let bytes = c_bytes(child);
            let area = (bytes.sqrt() + nominal).powi(2);
            total += area;
            blocks.push((i, area, bytes));
        }
        let scale = dir_area / total;
        let mut rows: Vec<Vec<usize>> = Vec::new();
        let mut row: Vec<usize> = Vec::new();
        let mut row_area = 0.0;
        for (n, block) in blocks.iter_mut().enumerate() {
            block.1 *= scale;
            if row.is_empty() {
                row_area = 0.0;
            }
            row.push(n);
            row_area += block.1;
            let by = row_area / dims_x;
            let bx = block.1 / by;
            if bx / by < 1.0 {
                rows.push(std::mem::take(&mut row));
                row_area = 0.0;
            }
        }
        if !row.is_empty() {
            rows.push(row);
        }
        let cx = 0.5 * (c0x + c1x);
        let cy = 0.5 * (c0y + c1y);
        let mut pos_y = cy + 0.5 * dims_y;
        for row in &rows {
            let area: f64 = row.iter().map(|&i| blocks[i].1).sum();
            let by = area / dims_x;
            let mut pos_x = cx + 0.5 * dims_x;
            for &i in row {
                let bx = blocks[i].1 / by;
                let bytes = blocks[i].2;
                let node_area = scale * bytes;
                let k = bx + by;
                let disc = k * k - 4.0 * (blocks[i].1 - node_area);
                let border = if disc > 0.0 {
                    0.25 * (k - disc.sqrt())
                } else {
                    0.0
                };
                let child = &node.children[blocks[i].0];
                let h = if child.is_dir() { DIR_H } else { LEAF_H };
                let rect = (pos_x - bx + border, pos_y - by + border, pos_x - border, pos_y - border, h);
                children.push(golden_map(child, Some(rect)));
                pos_x -= bx;
            }
            pos_y -= by;
        }
    }
    Placed {
        name: node.name.clone(),
        c0x,
        c0y,
        c1x,
        c1y,
        height,
        children,
    }
}

fn expect() -> Vec<Gold> {
    vec![
        Gold { name: "root", c0x: -228.539, c0y: -190.449, c1x: 228.539, c1y: 190.449, height: 384.0 },
        Gold { name: "src", c0x: 188.849, c0y: -175.450, c1x: 211.102, c1y: 175.449, height: 384.0 },
        Gold { name: "main", c0x: 190.136, c0y: -153.819, c1x: 209.814, c1y: 163.645, height: 128.0 },
        Gold { name: "util", c0x: 190.297, c0y: -163.484, c1x: 209.653, c1y: -154.304, height: 128.0 },
        Gold { name: "docs", c0x: 174.839, c0y: -175.643, c1x: 187.326, c1y: 175.643, height: 384.0 },
        Gold { name: "notes", c0x: 175.651, c0y: -97.332, c1x: 186.514, c1y: 163.989, height: 128.0 },
        Gold { name: "readme", c0x: 175.730, c0y: -163.910, c1x: 186.435, c1y: -97.616, height: 128.0 },
        Gold { name: "photo", c0x: -210.178, c0y: -174.525, c1x: 172.391, c1y: 174.526, height: 128.0 },
    ]
}

fn flatten(p: &Placed, out: &mut Vec<Placed>) {
    out.push(Placed {
        name: p.name.clone(),
        c0x: p.c0x,
        c0y: p.c0y,
        c1x: p.c1x,
        c1y: p.c1y,
        height: p.height,
        children: Vec::new(),
    });
    for c in &p.children {
        flatten(c, out);
    }
}

#[test]
fn golden_map_matches_the_original_rows() {
    let tree = fixture();
    let placed = golden_map(&tree, None);
    let mut flat = Vec::new();
    flatten(&placed, &mut flat);
    let want = expect();
    assert_eq!(flat.len(), want.len());
    for (got, exp) in flat.iter().zip(&want) {
        assert_eq!(got.name, exp.name, "order");
        for (label, a, b) in [
            ("c0x", got.c0x, exp.c0x),
            ("c0y", got.c0y, exp.c0y),
            ("c1x", got.c1x, exp.c1x),
            ("c1y", got.c1y, exp.c1y),
            ("h", got.height, exp.height),
        ] {
            assert!(
                (a - b).abs() < 0.02,
                "{name} {label}: formula {a:.3} locked {b:.3}",
                name = got.name
            );
        }
    }
}

#[test]
fn golden_treev_heights_and_wires() {
    // treev_init_recursive: sqrt(max(64, bytes)), platform height is constant.
    let heights = [
        ("readme", 1000f64.sqrt()),
        ("notes", 4000f64.sqrt()),
        ("main", 9000f64.sqrt()),
        ("util", 100f64.max(64.0).sqrt()),
        ("photo", 160000f64.sqrt()),
        ("docs", 5000f64.sqrt()),
        ("src", 9100f64.sqrt()),
        ("root", 174100f64.sqrt()),
    ];
    assert!((heights[0].1 - 31.6228).abs() < 1e-3);
    assert!((heights[4].1 - 400.0).abs() < 1e-6);
    assert!((heights[7].1 - 417.253).abs() < 1e-2);
    let platform_height = 158.2_f64;
    assert!((platform_height - 158.2).abs() < 1e-9);

    // One open child sits on the parent centerline, so the branch arc
    // from first open child to last open child has length 0. The stem
    // is still drawn. A closed directory adds nothing.
    let open = ["docs"];
    let closed = ["src"];
    assert!(open.len() == 1 && closed.len() == 1);
}

fn find<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    if node.name == name {
        return Some(node);
    }
    for child in &node.children {
        if let Some(found) = find(child, name) {
            return Some(found);
        }
    }
    None
}

#[test]
fn current_layout_against_golden() {
    let mut tree = fixture();
    layout::build_mapv(&mut tree);
    let want = expect();
    let mut report = String::new();
    let mut mismatches = 0;
    for exp in &want {
        let Some(node) = find(&tree, exp.name) else {
            mismatches += 1;
            report.push_str(&format!("missing {}\n", exp.name));
            continue;
        };
        let Some(map) = node.map else {
            mismatches += 1;
            report.push_str(&format!("{} has no map\n", exp.name));
            continue;
        };
        let fields = [
            ("c0x", map.c0x, exp.c0x),
            ("c0y", map.c0y, exp.c0y),
            ("c1x", map.c1x, exp.c1x),
            ("c1y", map.c1y, exp.c1y),
            ("h", map.height, exp.height),
        ];
        for (label, got, expv) in fields {
            let delta = got - expv;
            report.push_str(&format!(
                "{:<8} {:<4} {:10.3} {:10.3} {:+10.3}\n",
                exp.name, label, got, expv, delta
            ));
            if delta.abs() > 0.05 {
                mismatches += 1;
            }
        }
    }

    let drawn = layout::layout(&mut tree, LayoutMode::TreeV);
    let names: Vec<&str> = drawn.boxes.iter().map(|b| b.name.as_str()).collect();
    report.push_str(&format!("treev boxes {names:?}\n"));
    let platforms: Vec<&str> = drawn
        .arcs
        .iter()
        .filter(|a| !a.branch)
        .map(|a| a.name.as_str())
        .collect();
    let branches: Vec<&str> = drawn
        .arcs
        .iter()
        .filter(|a| a.branch)
        .map(|a| a.name.as_str())
        .collect();
    report.push_str(&format!("platforms {platforms:?}\nbranches {branches:?}\n"));
    // Closed src is a block. Its files stay hidden. Open docs is a platform.
    for need in ["photo", "src", "notes", "readme"] {
        if !names.contains(&need) {
            mismatches += 1;
            report.push_str(&format!("missing leaf {need}\n"));
        }
    }
    for hidden in ["main", "util"] {
        if names.contains(&hidden) {
            mismatches += 1;
            report.push_str(&format!("closed directory leaked {hidden}\n"));
        }
    }
    if !platforms.contains(&"docs") || platforms.contains(&"src") {
        mismatches += 1;
        report.push_str("platform set is wrong\n");
    }
    if !branches.contains(&"docs") {
        mismatches += 1;
        report.push_str("docs has no incoming wire\n");
    }

    assert!(
        mismatches == 0,
        "{mismatches} MapV fields differ from the original row layout\n{report}"
    );
}
