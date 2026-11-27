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
| `last_text_cache` | Text-layout cache hit/miss counters. |

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
