# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`bevy_gstreamer` is a Bevy engine plugin that captures rendered frames from a Bevy camera and streams them through a GStreamer pipeline. It reads GPU framebuffer data via a render graph node and pushes RGBA frames into a GStreamer `appsrc`, which feeds into an H.264 encode/RTP/UDP pipeline.

Forked from `bevy_image_export` — the README still references the original project.

## Build & Test Commands

```bash
cargo build                          # build the library
cargo build --example basic          # build the example
cargo test                           # run tests
just build                           # same as cargo build
just test                            # same as cargo test
just build-refactor                  # build with cargo-limit (less noise)
just we-build                        # watch mode: rebuild on file changes
just we-test                         # watch mode: retest on file changes
```

## Dependencies

- **Bevy 0.12** (render, asset, winit subsystems)
- **GStreamer 0.21** bindings (`gstreamer`, `gstreamer-app`, `gstreamer-video`)
- **wgpu 0.17** for GPU buffer readback
- System GStreamer libraries and VA-API H.264 encoder (`vah264enc`) must be installed

## Architecture

The plugin has three layers that form a pipeline from GPU framebuffer to network:

1. **Render asset & GPU readback** (`src/plugin.rs`): `ImageExportSource` is a Bevy `RenderAsset` that allocates a staging buffer. `get_image()` maps this buffer to CPU memory, removes row padding, and returns a `bevy::render::Image`.

2. **Render graph node** (`src/node.rs`): `ImageExportNode` runs after the camera driver in Bevy's render graph. It issues a `copy_texture_to_buffer` command to copy the rendered texture into the staging buffer each frame.

3. **GStreamer integration** (`src/gstreamer.rs`): `NDIExportPlugin` is the Bevy plugin. `NDIExport::new()` constructs a GStreamer pipeline (`appsrc → queue → vah264enc → h264parse → rtph264pay → udpsink`) and spawns a glib main loop thread. The `ndi_send_buffer` system runs in the `Render` schedule, reads back the image, and pushes buffers into the `appsrc`. Flow control uses `need_data`/`enough_data` callbacks with `Arc<Mutex<>>` shared state.

**Data flow:** Camera renders to texture → `ImageExportNode` copies to staging buffer → `ndi_send_buffer` reads back to CPU → pushes into GStreamer `appsrc` → encoded and sent via UDP to `127.0.0.1:5003`.

## Naming Note

Despite "NDI" in type names (`NDIExport`, `NDIExportPlugin`, `NDIExportBundle`), this uses GStreamer with H.264/RTP/UDP — not the NDI protocol. The naming is vestigial.
