# fsv-rs

Rust port of [fsv](https://fsv.sourceforge.net/) ([mcuelenaere/fsv](https://github.com/mcuelenaere/fsv)). Ubuntu 24.04 and up. Bevy / wgpu, so the window is Vulkan.

The original is LGPL-2.1. This port follows that.

## Plan

The Bevy app stays. Scanner, window, camera, and both layouts stay in the tree. We do not start over.

A function is ported when a small fake tree produces the same radii, angles, and heights as the C function. A screenshot is not the test. Notes are not the test. The numbers are.

GTK, dialogs, and the old OpenGL setup are not ported function by function. Bevy replaces `gui.c`, `dialog.c`, and `ogl.c`.

### 1. Map the picture code

Write an index, one page, from the source. No new drawing in this step.

| C file | Rust | What it decides |
|---|---|---|
| `common.h` | `node.rs` | Node, sizes, per-mode geometry |
| `scanfs.c` | `scan.rs` | The tree |
| `geometry.c` MapV | `layout.rs` | Area is size. Dir height 384. File height 128 |
| `geometry.c` TreeV | `layout.rs` | Platforms, rows, wires, `sqrt(size)` file height |
| `geometry.c` DiscV | later | Incomplete upstream. Last |
| `camera.c` | `main.rs` | Orbit, fly-to, bird's eye, framing |
| `color.c` | `main.rs` | Type, age, wildcards. Dirs ignore age |
| `tmaptext.c` | `main.rs` | Names on the faces |
| `animation.c`, `colexp.c` | later | Open and close motion |

Under each function, the constants it uses and the next function it calls. If a behavior is not on that page, it is not a feature.

Functions to walk, in order:

1. `mapv_init` / the treemap pack
2. `mapv_apply_label`
3. `treev_init_recursive`
4. `treev_reshape` / `treev_build_dir`
5. `treev_gldraw_inbranch` / `treev_gldraw_outbranch`
6. `treev_apply_label`
7. Camera framing (`geometry_treev_max_leaf_height`, bird's eye)
8. `node_type_color` / `time_color`
9. Face labels
10. Expand / collapse motion
11. `discv_init_recursive` (only what the C actually draws)

### 2. Golden numbers, no window

A Rust test, or a tiny `print` mode, over a fixture of three directories and five files. Compare radii, angles, and heights to the formulas. Do this before changing the picture again.

MapV must show area from size, directory height 384, file height 128, in the original units, before the view scale.

TreeV must show platform height 158.2, leaf edge 256, branch width 256, and a wire only where `treev_gldraw_outbranch` runs: from an open directory to a child that is itself open.

### 3. Re-check what we already draw

Same order as the list above. Each function that already has Rust code gets the fixture before any more tweaking. Where the Rust disagrees with the C, the C wins, except the exceptions below.

### 4. Then the rest

Camera framing, color modes, names on the faces, open/close motion. DiscV last. A `.deb` only after the numbers match.

## Exceptions

These stay written down so they are not "fixed" by accident.

- **Tall files.** `leaf.height = sqrt(size)` is the formula. On a modern disk that makes a gigabyte file about a hundred times the platform, and the tree disappears. The port keeps the order of `sqrt` and stops at 2.4 leaf-edges. The fixture records both the raw value and the capped value.
- **Wires.** The C draws a wire only to a directory that has been opened. Showing a wire to every closed folder is a temporary cheat and comes out when TreeV is re-checked.
- **Age colors.** Default in `color.c` is by type. Age is a switch. Directories keep the type color. The one-week window paints an old archive one color, so the rainbow runs from the oldest file in the tree to the newest. That difference is recorded, not hidden.
- **DiscV.** `discv_gldraw_folder` is still empty upstream. We do not invent the missing half.

## Out of scope until the numbers match

More spacing, thicker wires, new camera moves, and packaging.
