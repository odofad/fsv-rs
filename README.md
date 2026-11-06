# fsv-rs

Rust rewrite of [fsv](https://fsv.sourceforge.net/) (3D File System Visualizer). Target: Ubuntu 24.04+ with Bevy/wgpu (Vulkan). Inspired by the Jurassic Park fsn/fsv tool.

## Project Plan

### Phase 0 – Skeleton
- Cargo project with Bevy
- Basic `Node` struct (size, mtime, type, children, expanded flag)
- Scanner with `jwalk`/`walkdir` that builds the tree and computes subtree sizes
- Text tree dump for verification

### Phase 1 – MapV
- Port MapV treemap packing (area-proportional blocks in rows)
- Render as boxes/meshes in Bevy with free camera
- Basic colours by type or size

### Phase 2 – TreeV
- Radial platform + leaf placement (the classic fsv look)
- Expand/collapse support

### Phase 3 – Interaction
- Click picking
- egui side panel (tree + file list) synced with the 3D view
- Expand/collapse from both UI and 3D

### Phase 4 – Polish
- Colours by type/age
- Labels (billboards or textures)
- Large-tree performance (culling, LOD)
- Basic file operations if needed

### Phase 5 – Packaging
- `cargo-deb` for Ubuntu 24.04+

## Notes
- Bevy/wgpu → Vulkan on Linux
- Windowed desktop app (not fullscreen by default)
- Does not aim for native GNOME/GTK theming

## License
TBD (likely MIT or Apache-2.0)
