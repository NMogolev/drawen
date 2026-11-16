# Changelog

Drawen follows [Semantic Versioning](https://semver.org/). Calendar dates appear
in the headings because the version itself must stay valid semver; see
[`docs/RELEASING.md`](docs/RELEASING.md) for the full policy.

## 0.2.0 - 2026-11-15

### GPU renderer

- Paint commands now execute in display-list order. Shapes, text, raster images,
  and external textures are recorded into one ordered operation stream, and the
  render pass switches pipeline and bindings in that order. Adjacent shapes with
  the same scissor still merge into one draw.
- Adjacent text commands with the same scissor merge into one draw within an
  uninterrupted text run. A shape, image, or external texture is an ordering
  barrier and starts a new run.
- Raster images no longer share the glyph atlas. Each image owns a
  linear-filtered `Rgba8Unorm` texture and is cached by its immutable
  `Arc<RasterImage>` pointer in a 64 MiB LRU. Images above the budget use a
  single retained oversized slot instead of being re-uploaded every frame.
- Raster images use a dedicated straight-alpha shader. Translucent images are no
  longer brightened by the premultiplied browser-surface shader.
- Images larger than `max_texture_dimension_2d` are skipped instead of tripping
  a wgpu validation error, so one oversized asset cannot fail the frame.
- The render pass caches the active pipeline, vertex buffer, bind group, and
  scissor, and skips redundant state changes.
- Overlapping glyph coverage in the CPU atlas composites with source-over alpha
  instead of `max(alpha)`.

### Software renderer

- The fill path uses scanline spans with analytic clip intervals and an
  opaque-destination integer blend, while keeping bit-identical float
  source-over for translucent destinations.
- Deterministic randomized differential tests compare the fast path against a
  per-pixel reference across 500 scenes; they caught a non-monotonic binary
  search that is now fixed.
- Added `examples/fill_benchmark.rs` and `BASELINE.md` with the benchmark
  machine, method, median, and p99.

### Widgets

- `Styled<W>` now forwards every `Widget` hook, including `accessibility`,
  `accessibility_action`, `tab_index`, `advance_animation`, and `is_animating`.
  Wrapping a button or animated media widget no longer drops its semantics,
  assistive-technology actions, tab order, or animation scheduling.
- Added `styled_forwards_every_widget_hook` to prevent regressions in the
  wrapper's transparent forwarding.

### Packaging

- `drawen` now exposes `gpu`, `browser`, and `media` as separate features.
  `drawen-browser` and `drawen-wgpu` are optional, so
  `--no-default-features` produces a headless build without `wgpu`, DXGI/D3D12,
  Servo interop, or Paleo. The defaults keep `gpu`, `browser`, and `media`
  enabled; `audio` and `director` remain opt-in.
- `drawen-widgets` gates `Video`, `AnimatedImage`, `PaleoSession`, and the media
  contracts behind its own `media` feature instead of compiling Paleo and
  Symphonia unconditionally. The `audio` feature implies `media`.
- `drawen` re-exports its `wgpu` version as `drawen::gpu::wgpu` and
  `drawen::browser::wgpu` so applications can name device and texture types
  without a mismatched direct dependency.

### Dependencies

- Updated `wgpu` from 29.0.4 to 30.0.2. All internal crates now use caret
  ranges instead of the exact `=29.0.4` pin, and the Servo interop crates pass
  the explicit initial texture state required by wgpu 30.

### Breaking changes

- `drawen_wgpu::GpuRenderer::render` now takes `&mut self`.
- `drawen_widgets::Image::new`, `Image::source`, and the animated-image accessors
  are no longer `const`; `AnimationFrame::image` returns an `Arc`-backed
  reference.
- Raster image payloads in display lists are shared as `Arc<RasterImage>`.
- `drawen-widgets` no longer enables media widgets by default. Direct users of
  `Video`, `AnimatedImage`, or `PaleoSession` must enable the `media` feature;
  the `drawen` facade enables it from its default feature set.
- Browser surfaces now require wgpu 30. The public signatures are unchanged,
  but applications with a direct `wgpu` dependency must use version 30.

## 0.1.0 - 2026-11-14

Initial published crate set: core geometry, render contracts, retained UI,
widgets, software and GPU renderers, task pool, platform abstraction, winit
adapter, docking workspace, media director, Servo browser integration, and the
Paleo media stack.
