# Drawen agent notes

Before generating Drawen application code, read
[`docs/AI_GUIDE.md`](docs/AI_GUIDE.md) and copy one of the canonical examples.

Non-negotiables:

- Application code uses the `drawen` facade; add `drawen-platform-winit` for a
  native window. Do not import `drawen-ui`, `drawen-widgets`, `drawen-core`, or
  `drawen-render` in ordinary app code.
- The widget tree, `Signal`, `Subscription`, and `UiRuntime` are UI-thread-only.
  Never move them to a worker thread.
- Widgets do not spawn threads or timers and do not call platform APIs. Use
  typed signals, `EventContext` redraw/layout requests,
  `Widget::advance_animation`, and `UiRuntime::next_frame_in`.
- `Button::new()` takes no label. Append a `Text` child.
- State is plain Rust. On a state change, invalidate paint and return
  `WindowAction::RequestRedraw`; invalidate layout only when geometry changes.
- `Property<T>` stores state with a change signal; `UiRuntime::bind(&target,
  &source)` is a thin one-way copy that wakes redraws. Keep the `Binding`
  alive. No dependency graph, no two-way magic.
- DSS is a typed subset: type/id/class/state selectors, descendant and child
  combinators, structural pseudo-classes, `var()`/`calc()`, and `!important`.
  No sibling combinators, animations, or full browser CSS.
- Media backends are explicit: `Video::open_paleo` (pure Rust: H.264, HEVC,
  VP9, AV1; no audio), `Video::open_paleo_audio` (feature `audio`),
  `Video::open` (external FFmpeg, no audio). Background playback after the
  last window closes requires `MediaDirector::with_background_playback(true)`.
- 3D content goes through `drawen::cubex`: build a `Scene` from indexed meshes
  and a `Camera`, then present it with `CubeXView`. The rasterizer is
  deterministic and headless; the view caches a `RasterImage` and stays on the
  UI thread. Do not add a game loop or a second scene model inside a widget.
- Forward `WindowPresentation` to `UiRuntime::set_presented` from
  `presentation_changed` and implement `tick` with `UiRuntime::tick`, so a
  hidden window advances media clocks without painting. Purely visual
  animation freezes while hidden or offscreen; do not force frames for it.
- Floating surfaces (`Tooltip`, `Popover`, `Menu`, `Modal`, `ToastHost`) use
  the runtime's overlay layer. Do not expand them inside their parent or
  paint them by hand above siblings.
- Off-thread work goes through `drawen::tasks` (`TaskPool` + `UiDispatcher`).
  Workers own plain data and never capture widgets, signals, or subscriptions;
  the UI thread drains delivered values in `window_event`.

Canonical files:

- `crates/drawen/examples/minimal_window.rs`
- `crates/drawen/examples/stateful_app.rs`
- `crates/drawen/examples/bound_app.rs`
- `crates/drawen/examples/cubex_showcase.rs`
- `docs/RECIPES.md`
- `docs/MEDIA.md`
- `docs/CUBEX.md`
- `docs/THREADING.md`
