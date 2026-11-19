# Changelog

Drawen follows [Semantic Versioning](https://semver.org/). Calendar dates appear
in the headings because the version itself must stay valid semver; see
[`docs/RELEASING.md`](docs/RELEASING.md) for the full policy.

## Unreleased

### Window presentation

- Added `WindowPresentation`, a normalized snapshot of the visible, focused,
  minimized, and occluded flags, plus the `WindowApplication::presentation_changed`
  hook. Adapters now tell an application when the whole presentation state
  changes instead of only forwarding individual events.
- Hidden windows no longer present frames on the 16 ms animation cadence. The
  winit adapter caps them at the 100 ms background interval and calls
  `WindowApplication::tick` directly when the platform stops delivering redraws,
  so audio and media clocks keep running. Restoring requests one full frame.
- Visible but unfocused windows fall back to a 30 fps cap. Applications can opt
  out with `WindowApplication::inactive_frame_interval`. Windows does not report
  full occlusion through `winit`, so focus loss is the practical signal there.
- `UiRuntime::set_presented` freezes interaction transitions and purely visual
  widget animation while hidden; `UiRuntime::tick` advances the runtime without
  building a display list. `Video` and `AudioPlayer` opt into background ticks,
  while `AnimatedImage` and other visual widgets freeze.
- `MediaDirector` now includes `WindowAction::RequestRedraw` in the window action
  when a window is restored, so the documented single-director snippet repaints
  the first visible frame without checking the separate `redraw` flag.

### Player widgets

- `Video` is now a configurable YouTube-style player: overlay title and subtitle,
  center play badge while paused, hover time preview on the seek bar, play/pause,
  back and forward seek, elapsed/remaining/both time labels, volume and mute,
  rate cycling, fullscreen and loop requests, and auto-hiding controls. Existing
  constructors and transport methods keep working. `VideoPlayer` is an alias.
- Added `AudioPlayer`, a music-player widget with cover art, title and subtitle,
  seek bar, transport, volume and mute, rate, repeat, shuffle, and
  previous/next signals. `AudioPlayerLayout::Inline` and `Compact` cover full and
  single-row surfaces.
- Both players share `PlayerTheme`, `PlayerBehavior`, `PlayerTimeDisplay`,
  `VideoControls`, `AudioControls`, and `RepeatMode`, re-exported by `drawen`.
  Players emit typed signals and never spawn threads, timers, or platform calls.
- `MediaSession` gained default volume and mute methods; `PaleoSession` and
  `AudioPaleoSession` implement them.

## 0.2.0 - 2026-11-17

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
- Fills now resolve the clip row interval before looking for the shape edges,
  and a row that the shape covers from clip edge to clip edge is filled without
  any search. A clipped row no longer runs the seed and both binary searches
  across the full shape bounds, so the search cost follows the visible span.
- Added `examples/fill_benchmark.rs` and `BASELINE.md` with the benchmark
  machine, method, median, and p99.

### Widgets

- `Styled<W>` now forwards every `Widget` hook, including `accessibility`,
  `accessibility_action`, `tab_index`, `advance_animation`, and `is_animating`.
  Wrapping a button or animated media widget no longer drops its semantics,
  assistive-technology actions, tab order, or animation scheduling.
- Added `styled_forwards_every_widget_hook` to prevent regressions in the
  wrapper's transparent forwarding.

### Tasks

- Added the `drawen-tasks` crate with the sanctioned boundary for off-thread
  work: a fixed-size `TaskPool`, cancellable `TaskHandle`s, `TaskContext`
  progress reporting and cooperative cancellation, and a `UiDispatcher` that
  moves owned results back to the UI thread and wakes the event loop.
- `drawen::tasks` re-exports the pool, handles, cancellation token, dispatcher,
  and error types, so applications do not depend on the crate directly.

### Documentation

- Added the onboarding path in `docs/AI_GUIDE.md`, `docs/TUTORIAL.md`,
  `docs/RECIPES.md`, and `docs/THREADING.md`, with canonical
  `minimal_window` and `stateful_app` examples.
- Expanded `README.md` with the crate map, application boundaries, and the
  threading rules.

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
- Continuous integration (`.github/workflows/ci.yml`) now builds every feature
  combination of the facade with `cargo hack --feature-powerset`, checks that
  the examples still compile without default features, and runs the test suite
  on Linux, Windows, and macOS.
- `crates/drawen/tests/feature_gates.rs` turns the packaging rules into tests:
  `--no-default-features` may not pull `wgpu`, Servo, Symphonia, Paleo, `image`,
  or `cpal`; the defaults must enable the renderers and media while leaving
  audio and the director opt-in; the GPU and media stacks stay independent; and
  the workspace must resolve a single `wgpu` version.

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
