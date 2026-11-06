# fsv-rs

Rust rewrite of [fsv](https://fsv.sourceforge.net/) (the [mcuelenaere/fsv](https://github.com/mcuelenaere/fsv) sources). Ubuntu 24.04 and up. Bevy / wgpu, so the window is Vulkan.

The original is LGPL-2.1. This port follows that.

## What we are copying

The behaviour, not the toolkit. GTK 1.2, Glade, XPM icons and OpenGL display lists stay behind. The layout math, the camera, the colours and the way you move through a tree come across.

fsv is three views over one scanned tree:

| Mode | What you see | Original |
|---|---|---|
| MapV | A city. Area is bytes. Directories are taller than files (384 vs 128). Children sit on the parent's top face. | `mapv_init_recursive` in `geometry.c` |
| TreeV | Platforms in arcs around a parent, files standing on the platform, branches between platforms. | `treev_reshape_platform`, `treev_arrange_recursive` |
| DiscV | Discs. Marked work in progress in the original. Last, or dropped. | `DiscVGeomParams` |

One rule that already bit us: layout code writes the original geometry (`c0`, `c1`, `height`, arc, theta). The renderer applies one view scale, and any extra gap is a view setting. Do not rewrite the packer to chase a screenshot.

## Original pieces and where they go

| Original | Job | Rust | Now |
|---|---|---|---|
| `scanfs.c` | Walk, sort dirs-first then size, subtree sizes | `scan.rs` | Works. Missing fifo/socket/devices, uid, perms, atime/ctime |
| `common.h` | Node types, `NodeDesc`, `DirNodeDesc` | `node.rs` | Partial. `expanded` stands in for `deployment` |
| `geometry.c` MapV | Row packing, fixed heights, z = sum of ancestors | `layout.rs` | Packer is in. Gap is a view hack. No slant, no folder tab |
| `geometry.c` TreeV | Arc platforms, leaves, branches | `layout.rs` | Placeholder radial cubes |
| `color.c` | By type, by time, by wildcard | not started | Hard-coded blue/yellow |
| `camera.c` | Look-at a node, dolly, revolve, bird's eye | orbit camera | Free orbit. Left drag fights click-to-expand |
| `colexp.c` | Expand, collapse, recursive, animated 0..1 | boolean toggle | No morph |
| `dirtree.c`, `filelist.c` | Tree and the files in the current directory | egui tree | No file list, selection does not fly the camera |
| `tmaptext.c` | Name fitted to the top face | `Text2d` | Rough |
| `viewport.c` | Pick a face, highlight, status text | ray vs AABB | Selects and toggles in one click |
| `dialog.c`, `window.c` | Change root, colour setup, status bar | not started | |
| `animation.c` | Morphs for deploy and camera pans | not started | |

## Interaction we are aiming at

This is how the original feels, and how the mouse should work here:

- Left click selects a node and the camera flies to it.
- Double click expands or collapses a directory. The tree does the same.
- Right drag revolves. Scroll dollies.
- Bird's eye is a toggle, not the default.
- A collapsed directory is one block (folder mark on top in MapV, a leaf in TreeV).
- An expanded directory shows its children and nothing deeper.
- Status bar shows the name, the size, and the counts.
- Colours switch: file type, age (rainbow / heat / two-colour), or wildcard.

## Order of work

1. **Geometry record.** Store MapV `c0/c1/height` on the node in original units. Draw from that. View scale and the extra gutter live in the renderer only.
2. **MapV mesh.** Slanted prism (side ratios from `mapv_side_slant_ratios`), folder outline when collapsed, label fitted to the top face, highlight frame on the selected node.
3. **Camera and mouse.** Split select, fly-to, expand, revolve, dolly, bird's eye. Stop using one button for all of it.
4. **TreeV for real.** Port the arc platform, the leaf rows, and the branches. A collapsed directory is a leaf.
5. **Panels.** File list of the selected directory, status bar, change root, up, back. Tree and 3D stay on the same node.
6. **Colour.** Type, timestamp spectrum, wildcard groups. Read straight from `color.c`.
7. **Motion.** `deployment` animates 0 to 1. Camera pans instead of cutting.
8. **DiscV,** only if it is still worth it. The original never finished it.
9. **`.deb`** for Ubuntu 24.04+.

## Already decided

- Windowed, not fullscreen.
- Not a GNOME-styled app. egui on the side, the view is the 3D.
- Root starts expanded. Everything under it starts collapsed.
