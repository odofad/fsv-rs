# Picture map

Index of the original picture code in [mcuelenaere/fsv](https://github.com/mcuelenaere/fsv) (`src/`, LGPL-2.1). This is the checklist for the port. A behavior that is not on this page is not a feature.

GTK, dialogs, and the old OpenGL setup (`gui.c`, `dialog.c`, `ogl.c`) are not mapped. Bevy replaces them.

## Data

`common.h`

- `NodeDesc`: type, name, `size`, `size_alloc`, owner, perms, atime/mtime/ctime, color, five geometry slots.
- `DirNodeDesc` adds three more geometry slots, `deployment` (0 collapsed, 1 expanded), and `subtree.size` plus a count per node type. Subtree size does not include the directory node itself.
- `scanfs` walks with `scandir`, then `compare_node`: directories first, then larger to smaller (a directory's size is its own size plus `subtree.size`), then A to Z.

`geometry.h` stores per-mode numbers in those slots:

| Mode | Fields |
|---|---|
| MapV | corners `c0`, `c1`, `height` |
| TreeV leaf | `distance`, `theta`, `height` |
| TreeV platform | `theta`, `depth`, `arc_width`, `height`, `subtree_arc_width` |
| DiscV | `radius`, `theta`, derived `pos`. Marked work in progress |

## 1. MapV pack

`mapv_init` then `mapv_init_recursive`.

Constants: root aspect 1.2, border proportion 0.01, directory height 384, file height 128. Side slant by type: directory 0.032, file 0.064, symlink 0.333, fifo and socket 0, char and block 0.25.

Root footprint is `sqrt(subtree.size / 1.2)` by `1.2` times that. The root node's own height is 0. The first real directory is height 384.

Children are packed onto the parent's top face, after the slant inset. A node counts as `max(256, size)`, plus subtree size if it is a directory. Block area is `(sqrt(that) + border)^2`, then scaled so the blocks fill the face. Rows run along the wide side. A block that comes out taller than it is wide ends the row. The third pass is an empty comment: the optimizer was never written. Placement starts at the right/rear corner and walks the already-sorted list, so the largest blocks land first.

`mapv_apply_label` sits on the top face, inset by the same slant. A directory label is at z = 0 of that face. A file label is at the file height. Label box is 0.8125 of the face width and `(2 - phi)` of the depth. `phi` is 1.618.

Call: `mapv_init` -> `mapv_init_recursive` -> same for each child directory.

## 2. TreeV platforms

`treev_init` -> `treev_init_recursive` -> `treev_arrange` -> `treev_arrange_recursive` -> `treev_reshape_platform`.

Constants, original units:

| Name | Value |
|---|---|
| leaf edge | 256 |
| platform height | 158.2 |
| branch width | 256 |
| gap between platforms (depth) | 2048 |
| gap across an arc (width) | 512 |
| minimum core radius | 8192 |
| core grow factor | 1.25 |
| arc of the whole tree | 90 to 225 degrees |
| curve step | 5 degrees |

`treev_init_recursive` sets every platform height to 158.2 and every leaf height to `sqrt(max(64, size))`, with subtree size added for a directory. The multiplier is 1. There is no cap.

`treev_reshape_platform` estimates area from the child count (`1.5 * edge * ceil(sqrt(n)) + 0.5 * edge`, squared), then solves a cubic so the outer edge length equals the depth. Depth is then rounded up to a whole number of leaf rows. Arc width has a floor: the inner edge must be at least two leaf edges plus the 512 spacer.

`treev_arrange` grows or shrinks the core radius (start 8192, step 1.25) until the root subtree arc sits between 90 and 225 degrees. A collapsed directory contributes no arc. An expanded directory contributes the max of its own arc and its subtree arc. Children are spread around the parent's centerline.

## 3. TreeV leaves and wires

`treev_build_dir` lays children from the inner edge outward, walking the child list backwards, so the smallest land on the inside. A row holds `floor((arc_length - 128) / 384)` nodes. Centers are 384 apart (1.5 edges). The first center is one edge out from the inner radius. After the rows, platform depth is rewritten to the real outer edge. That is the depth that matters. The reshape value is only an estimate.

`treev_draw_recursive` returns true only when that directory is fully expanded. The parent records the first and last child that returned true.

Wires (`treev_gldraw_inbranch`, `treev_gldraw_outbranch`) run only for a directory that is itself fully expanded:

- one radial stem, cartesian width 256, from halfway across the incoming gap to the inner edge
- if at least one child is fully expanded, one arc from that first child's angle to that last child's angle, centered in the outgoing gap, radial thickness 256, plus a square corner of one branch-width

A wire does not go to a closed folder. It is one arc spanning every open child, not one wire per child.

`treev_apply_label`: a leaf name is on the top face, rotated with the leaf. A platform name is curved just inside the inner edge, at z = 0.

Collapsed directory: drawn as a leaf, plus the folder glyph. Expanded: platform plus leaf children. In between: the leaf shrinks and the platform grows from that leaf's position, scaled by `deployment`.

## 4. Camera

`camera.c`. Field of view is 60 degrees. Near clip is half the distance. Far clip is 128 times the near clip.

Distance comes from `field_distance`: the target should fill the view. MapV uses the root width, or the stacked expanded height, whichever is larger. First view is theta 270, phi 0, distance 4 times that. TreeV uses twice the outer radius of the laid-out tree. First view is theta 0, phi 90 (looking down), distance 2 times the fit, aimed at the middle of the root platform.

`camera_look_at` flies to a node. `camera_birdseye_view` saves the camera, goes up, and restores it. `camera_dolly` and `camera_revolve` are the manual moves. Scrollbars exist. DiscV scrollbar code is empty.

TreeV leaf yaw is a small fraction of where the leaf sits in its parent's arc. Platform yaw is `-0.125 * (theta - 90)`.

## 5. Color

`color.c`. Default mode is by node type. Directories always keep the type color, in every mode.

Age uses mtime and a window of one week ending now. Rainbow, 1024 steps. Older than the window is the first color at half brightness. Newer than now is the last color at half brightness. An archive older than a week is one color, not a spread from oldest file to newest.

Wildcards are a third mode. A directory still uses the type color. An unmatched file falls back to the type color.

## 6. Names and motion

`tmaptext.c` draws three ways: straight (MapV), straight but rotated (TreeV leaves), curved (TreeV platform names). The glyph aspect is fixed. The label box from geometry is the maximum size, and the text is fitted inside it.

`colexp` drives `deployment` between 0 and 1. Each step calls `geometry_colexp_in_progress`, which marks TreeV branches stale and asks for a rearrange. `animation.c` is the timer: `morph` eases a double toward a target. Open and close are that ease, not an instant swap.

## 7. DiscV

`discv_init_recursive` does assign radii and angles. `discv_gldraw_node` draws a disc. `discv_gldraw_folder` is empty. `discv_build_dir` forces deployment to 1 and is marked to be fixed. Not in scope until MapV and TreeV numbers match, and then only the part that actually draws.

## What the Rust already has, against this map

These are notes for step 3. They are not fixes.

- MapV in Rust is a squarified treemap. The C packer is the row walk above, and its optimizer was never written. Area is `sqrt(size)`, not the size itself, and every node is at least 256 bytes.
- Heights 384 and 128 match the constants. The slant ratios and the border formula do not, yet.
- TreeV leaf height in the C is raw `sqrt`, no cap. The 2.4-edge cap stays an exception, and the fixture must record both numbers.
- TreeV depth must come from the row walk in `treev_build_dir`, then the arc from the cubic. Estimating depth and also inventing a second spacing is how the picture drifted.
- Wires are one arc over the open children, width 256 in cartesian units, and only when those children are open.
