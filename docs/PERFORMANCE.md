# Performance and frame diagnostics

Drawen frames are event-driven: nothing repaints unless an event, a binding,
or an animation actually changes something. This page describes the frame
pipeline, the diagnostics you can read from `UiRuntime`, and the reproducible
benchmark.

## The frame pipeline

```text
event / binding / animation
        │  invalidate only what changed
        ▼
UiRuntime::paint ──▶ retained DisplayList + FrameDamage
        │
        ▼
backend rasterizes only the damage
  GPU: retained offscreen texture, scissored repaint, blit to the surface
  software: region reset + repaint under a damage clip
```

- Input events invalidate paint only when they change something: a handled
  event, a redraw request, or runtime-owned hover/press/focus/overlay state.
  An ignored pointer move reuses the retained display list.
- `runtime.invalidate_paint_for(widget_id)` and `runtime.invalidate_rect(rect)`
  record local damage. The recorded region is the union of old and new bounds
  plus a 24px effect margin (shadows, focus rings, strokes), so effects that
  overhang a widget never leave stale pixels.
- Bounded animations opt into local damage through
  `Widget::local_animation_damage`; floating surfaces (tooltips, toasts,
  menus, modals) keep full-frame damage because they paint above the tree.
- Animation advancement is O(active): interaction transitions visit only nodes
  whose hover/press/focus target changed, and ancestor visibility is resolved
  only for animating nodes. A static widget costs a single `is_animating`
  check, so a large tree with a few live animations does not pay a clip walk
  per node each frame.
- Equal property writes record no damage, and state changes within one event
  turn coalesce into a single repaint.

## Reading diagnostics

`runtime.frame_diagnostics()` (alias `stats()`) returns a cheap copy of the
latest frame's counters:

| Field | Meaning |
| --- | --- |
| `layout_passes` / `paint_passes` | Completed passes; cache hits do not count. |
| `last_layout_time` / `last_paint_time` | Duration of the most recent real pass. |
| `last_layout_reused` / `last_paint_reused` | Whether the last request hit the cache. |
| `last_paint_command_count` | Commands emitted by the last paint. |
| `last_display_stats` | Per-command counters (fills, text runs, images, …). |
| `last_damage` | `Some(rect)` for a rebuilt frame, `None` for a reused one. |
| `last_text_cache` | Text-layout cache hit/miss counters and retained bytes. |

`runtime.frame_damage()` turns the same snapshot into the backend-ready
`FrameDamage` value: `Unchanged` for a reused list, `Region(rect)` for a
localized rebuild, and `Full` whenever no region is known. Forward it from
`WindowApplication::frame_damage` (the canonical examples do) so the adapter
can pass it to the backend.

## Backends and damage

- **GPU (`drawen-wgpu`)**: swapchain images rotate, so the frame is rasterized
  into a persistent offscreen texture. `Region` loads the retained pixels,
  resets the region to transparency, and repaints only commands whose
  conservative bounds intersect it, with every scissor restricted to the
  region; a fullscreen blit presents the result. `Unchanged` skips
  rasterization and blits the retained frame. Resize drops the retained
  texture, forcing one full frame. Composed text bitmaps are cached in a
  16 MiB LRU keyed by `TextLayout::identity`, color, and scale, so unchanged
  labels skip CPU glyph compositing on every rasterized frame.

  GPU allocations are persistent or recycled, never per-frame: vertex buffers
  grow on demand and are rewritten in place; the glyph atlas starts at 512×512
  and doubles only when a frame's text no longer fits (up to 2048×2048);
  raster images live in a 64 MiB LRU, and evicted textures wait in a 32 MiB
  reuse pool so dynamic content (3D views, video) re-uploads into the same
  allocation instead of recreating a texture every frame. A texture retired
  during a frame is only reused after the next frame boundary, so earlier
  draws in the same frame keep sampling the old pixels.
  `GpuRenderer::set_cache_budgets` tunes the three byte budgets, and
  `GpuRenderer::cache_stats` reports entries, retained bytes, pool occupancy,
  and the current atlas size for profiling.
- **Software (`drawen-software`)**: `render_with_damage` resets the reported
  pixels and repaints them under a clip aligned to the cleared pixel span.
  Commands outside the region are skipped entirely. This is the backend the
  headless benchmark uses, so its numbers isolate rasterization cost.

Command culling is conservative: bounds include stroke width, shadow falloff,
antialiasing slack, glyph overhang proportional to line height, and the active
transform. A command is skipped only when its bounds cannot touch the region.

Per-frame UI work is kept proportional to what actually changes: the floating
paint pass iterates a registry of widgets that opt in through
`Widget::paints_floating` rather than every node, and unchanged frames reuse
the retained display list without repainting.

## Reproducible benchmark

```sh
cargo run --release -p drawen --example frame_benchmark
```

The harness reports median, p99, and worst of 240 measured frames after
warmup, never just an average. Sections:

- **unchanged retained frame** — paint cost when nothing changed (must reuse
  the retained list and report no damage).
- **software fill** — raw fill rate at 1080p and 4K.
- **software damage** — a dense grid (~2600 commands) repainted fully, with a
  single damaged region, and unchanged.
- **runtime local damage** — the same scene driven end to end:
  `invalidate_paint` + full render versus `invalidate_rect` +
  `frame_damage()` + damage render.
- **virtual list scroll** — one million rows; visible text commands must scale
  with the viewport, not the row count.

Reference numbers (Windows, release build, one developer machine — treat as
directional, not contractual):

```text
software damage 1920x1080 (2592 commands)
  full repaint          median 21952 us
  one damaged region    median   135 us   (~160x faster)
  unchanged frame       median     0 us

runtime local damage (grid scene, 1280x800)
  full invalidate       median 10893 us
  invalidate_rect       median   144 us   (~75x faster)

virtual list scroll (1,000,000 rows)
  median 41 us, 32 visible text commands
```

### Measuring the GPU path

The headless benchmark intentionally excludes GPU rasterization: it needs a
native surface and a present loop. To compare GPU frames, run a windowed
example in release and read `frame_diagnostics()` per frame (paint time and
command count), or capture a frame with your platform's GPU profiler
(RenderDoc, PIX, Xcode). With local damage the paint pass still rebuilds the
display list; the saving is in rasterization and CPU text composition, so
compare frames where only a small region changes (a button hover) against
frames that invalidate everything.

## Choosing the right tool

- Large data sets: use `VirtualList`. It keeps paint work proportional to the
  viewport; no paint cache can do that for you.
- Subtree/layer caches: add them only when diagnostics show a confirmed need.
  The retained display list plus local damage already removes most repaint
  work, and a cache that is rarely hit only adds memory and invalidation bugs.
- Continuous animation: return a frame interval from `next_frame_in` and let
  bounded animations finish; do not force frames for purely visual effects
  while the window is hidden or offscreen.

## Frugal by default

The 0.5 memory audit found the same failure pattern behind every weakness: a
mechanism allocated for the worst case up front, grew without a budget, or
copied data it could have shared. Treat that as the standing lesson. New
mechanisms — in the library and in applications — must be frugal out of the
box, without asking the user to configure anything:

- **Lazy until first use.** Do not scan, decode, shape, or allocate until an
  event actually needs it: the system font scan waits for the first uncovered
  character, showcase pages mount on first activation. Laziness must not
  become visible lag — do the work at the moment the user asks for the
  content, never on a speculative timer.
- **Size to demand, not to the worst case.** Start small and grow on need:
  the glyph atlas starts at 512×512, vertex buffers grow with headroom.
  Growth may be one-way when shrinking would only churn allocations.
- **Every cache has a budget and a receipt.** A byte budget plus an entry cap
  plus eviction, and a stats accessor reporting retained bytes
  (`TextLayoutCacheStats`, `GlyphRasterCacheStats`, `SystemFallbackStats`,
  `GpuCacheStats`). An unbounded map keyed by user-controlled data is a leak.
- **Share, do not copy.** Immutable heavy data lives behind `Arc` with a
  stable identity (`RasterImage`, `TextLayout`); constructors accept
  `impl Into<Arc<_>>` so sharing is available without being mandatory.
- **GPU resources are persistent or recycled.** Never allocate a buffer or
  texture per frame; reuse a retired allocation only after a frame boundary,
  so earlier draws in the same frame keep their pixels.
- **Idle means zero.** Nothing schedules frames when nothing changes; the
  showcase idle test guards this. A regression that wakes an idle window is
  a bug, not a tuning issue.

Frugality never costs convenience: defaults must work untuned, budgets are an
optimization knob rather than a requirement, and public APIs keep their
ergonomic shapes — `TextShaper::layout` still returns `TextLayout` by value;
the sharing happens inside. Measure first, and do not add a cache that is
rarely hit — it only adds memory and invalidation bugs.
