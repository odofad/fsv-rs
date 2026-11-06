# fsv-rs

Rust rewrite of the old [fsv](https://fsv.sourceforge.net/) 3D file system visualizer. Aimed at Ubuntu 24.04 and up, using Bevy/wgpu so it runs on Vulkan.

## Plan

### Phase 0 – Skeleton
- Basic Bevy project
- Simple node tree (size, mtime, type, children, expanded flag)
- Scanner that walks a directory and builds the tree with subtree sizes
- Text dump so you can check it looks right

### Phase 1 – MapV
- Treemap-style packing (area proportional to size)
- Draw the boxes in Bevy with a free camera
- Some basic colouring

### Phase 2 – TreeV
- The radial platform layout
- Expand and collapse

### Phase 3 – Interaction
- Click to select
- Side panel (tree + file list) that stays in sync with the 3D view

### Phase 4 – Polish
- Better colours, labels, performance on big trees
- Whatever else makes it less rough

### Phase 5 – Packaging
- .deb for Ubuntu 24.04+

## Notes
- Windowed app, not fullscreen
- Not trying to look like a native GNOME app

## License
TBD
