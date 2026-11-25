# Changelog

Drawen follows [Semantic Versioning](https://semver.org/). Calendar dates appear
in the headings because the version itself must stay valid semver; see
[`docs/RELEASING.md`](docs/RELEASING.md) for the full policy.

## Unreleased

### Overlays

- `UiRuntime` now owns a floating overlay layer. Widgets request an overlay
  child through `LayoutContext::layout_child_overlay`, and the runtime measures
  it only while visible, flips it to the opposite side when it would leave the
  viewport, clamps it inside a configurable margin, paints it above the rest of
  the tree, and hit tests it first. Overlay children stay normal retained nodes,
  so bubbling, focus, and accessibility are unchanged. Open overlays stack LIFO
  by open time across relayouts, so a surface opened later paints and hit tests
  above earlier ones regardless of tree order, and closing an overlay restores
  focus to the widget that held it when the overlay opened.
- Added `OverlaySide`, `OverlayAlign`, `OverlayPlacement`, `OverlayOptions`,
  `DismissPolicy`, and `place_overlay` for anchor-relative placement. The flip
  decision checks only the axis the requested side extends along, so
  cross-axis overflow is clamped instead of vetoing the flip and covering the
  anchor. Light dismissal closes an overlay on an outside primary click,
  Escape, or window focus loss; a click on a modal scrim dismisses the modal
  even when a still-open surface behind it covers that position.
- Added `Widget::paint_floating`, a post-pass for non-interactive floating
  paint such as tooltip bubbles.
- Added `Popover`, a two-child anchor/content container with
  `PopoverTrigger::{Manual, Primary, Secondary}`, `DismissPolicy`, `opened()` and
  `closed()` signals, and pointer-anchored opening for context menus.
- Added `Tooltip`, a one-child anchor wrapper with a delayed fade, placement,
  reduced-motion handling, and a bubble that is painted above siblings and
  skipped by hit testing.
- Added `Menu` and `MenuEntry`, a data-driven vertical action list with
  separators, shortcuts, disabled rows, pointer and keyboard navigation, and a
  `selected(index)` signal. Place it in a `Popover` content slot for dropdowns
  and pointer-anchored context menus.
- Added `EventContext::request_overlay_dismiss`, so an action inside a menu or
  dialog can close the surrounding overlay without a second pointer event.
- Added `LayoutContext::layout_child_modal`. The runtime centers the child in the
  viewport, paints a themed scrim behind it, blocks pointer input to the tree
  behind it, and confines sequential focus traversal to the modal subtree. A
  modal without light dismissal owns Escape instead of letting it close a
  surface behind it.
- `Theme::scrim()` returns the backdrop color painted behind a modal. The default
  is a 40% black tint; returning a transparent color keeps input blocking and the
  focus trap without dimming.
- Added `Modal`, a zero-size one-child host with a shared `ModalHandle` and
  `opened()` / `closed()` signals, and `Dialog`, a padded, rounded, shadowed
  panel that reports the `Dialog` accessibility role with the modal state.
- Added `CommandPalette` and `PaletteCommand`, a modal search palette with a
  built-in query editor, case-insensitive filtering, up to eight visible rows
  with scrolling, pointer and keyboard navigation, and an `executed(index)`
  signal that dismisses the surrounding modal. Place it as the single child of
  a `Modal` and focus it after opening.
- Added `ToastHost`, `ToastHandle`, `Toast`, `ToastTone`, and `ToastCorner`:
  transient non-interactive notifications stacked in a window corner. The
  zero-size host paints cards through the floating paint pass, fades them in
  and out, and removes them when their duration elapses via
  `Widget::advance_animation`. Durations keep elapsing while the window is
  hidden, so stale toasts expire in the background instead of appearing on
  restore. The queue caps at five toasts; `show`, `dismiss`, and `clear` manage
  it through the shared handle.
- Added `SplitButton`, a primary action joined to a dropdown arrow. The left
  segment emits `activated()`; the arrow toggles a floating `Menu` supplied as
  the widget's single child, anchored below the button through the overlay
  layer with light dismissal. `opened()` / `closed()` report menu transitions,
  Enter and Space activate the primary segment, and `ArrowDown` opens the menu.
- Closing an overlay now cascades to the overlays opened from inside its
  subtree. The runtime records the nearest ancestor overlay root that owns each
  open root, so a popover opened inside a modal closes with the modal instead of
  lingering as a state orphan that reappears on reopen. The cascade runs both on
  the dismissal paths and after a layout that drops a surface closed through its
  own handle, syncing the owner widgets' open flags.
- Focus restore on close now validates the remembered opener. When that widget
  was removed or hidden while the overlay was open, focus falls back to the root
  (if it accepts focus) instead of being cleared or pointed at a dead handle.
- `place_overlay` returns a `ResolvedOverlay` with the placement `origin`, the
  resolved `side` after flipping, and the `available` size that fits on that
  side. When neither side fits, the side with more room wins. The runtime
  re-measures an oversized overlay child with constraints capped to `available`,
  so long content shrinks and scrolls instead of covering its anchor.
- `Menu` scrolls when its height is capped: the wheel and keyboard navigation
  keep the active row visible, and hit testing and paint account for the scroll
  offset. `Tooltip` paints an anchor-facing nib that follows the resolved side
  after a flip, and shrinks its bubble to the available space.

### Fonts

- `FontDatabase::discover_system_fallbacks()` keeps a lazy index of installed
  fonts. When a registered face lacks a character, the shaper parses candidates
  on demand, ranked by script group (Han, kana, Hangul, Arabic, Hebrew, Indic,
  Thai, emoji, or the generic Latin stack) and then by style and weight. At most
  24 lazily parsed faces and one coverage answer per character are retained.
- Added `FontdueTextShaper::multilingual()`, which combines Drawen's bundled
  Ubuntu Light face with that system fallback. Latin, Greek, and Cyrillic keep
  the deterministic bundled typography; other scripts resolve from the platform.
- `UiEnvironment::use_system_fallback_fonts()` installs that shaper on an
  existing runtime. The default environment stays bundled-only so headless tests
  do not depend on installed fonts.
- Faces inside font collections (`.ttc`) are now addressable through
  `FontFace::index()`, `FontFace::from_bytes_with_index()`, and
  `FontDatabase::add_font_with_index()`. Shaping and rasterization pass the
  collection index through instead of assuming face zero.
- Grapheme fallback now requires a face to cover the whole cluster, not just its
  first character, so combining marks and emoji sequences are not split across
  fonts. Controls, variation selectors, and other default-ignorable characters
  are excluded from the coverage check.

### Drawen Style Sheets

- DSS selectors now form a complex chain: descendant and child combinators,
  `:first-child`, `:last-child`, `:only-child`, `:nth-child(An+B)`, `:not(...)`,
  and `:is(...)`. The runtime records nearest-first ancestors and child positions
  during the style pass to match them.
- Accessibility flags mirror into selectors: `disabled`, `checked`/`mixed`, and
  `selected` resolve as `:disabled`, `:checked`, and `:selected`.
- Custom properties (`--token`) and `var()` bring nested fallbacks and cycle
  detection. `calc()` evaluates `px` lengths and unitless numbers.
- Colors accept the CSS Color Level 4 name table plus `hsl()`/`hsla()` and the
  modern space-separated `rgb()`/`hsl()` syntax with `/ alpha`.
- `!important` declarations form a second cascade layer applied after all normal
  declarations.
- `width`, `height`, `min-*`, and `max-*` bound a node by clamping the
  constraints its parent offers. Minimums win over maximums.

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
  and `ToastHost` keeps expiring queued toasts, while `AnimatedImage` and other
  visual widgets freeze.
- `MediaDirector` now includes `WindowAction::RequestRedraw` in the window action
  when a window is restored, so the documented single-director snippet repaints
  the first visible frame without checking the separate `redraw` flag.
- The winit adapter enables IME on the window, so composition (CJK input,
  dead-key preedit) reaches editable widgets through `InputEvent::TextPreedit`
  and `InputEvent::TextInput` instead of being silently disabled.
- The IME candidate window now opens at the caret: `Widget::ime_cursor_area`
  reports a local caret rectangle, `UiRuntime::ime_cursor_area` offsets it by
  the focused node's bounds, and the winit adapter forwards it to
  `Window::set_ime_cursor_area` in physical pixels after every redraw.
  `TextField`, `TextEdit`, `CommandPalette`, and `Autocomplete` report it.

### Widgets

- `VirtualList::with_variable_heights` switches the list to measured, wrapped
  rows: the construction height becomes the estimate for unshaped rows, a
  Fenwick tree keeps prefix offsets in `f64`, and measured heights are retained
  by stable row key across scrolling and data changes. Re-measuring a row above
  the viewport adjusts the scroll offset so the visible content stays put. The
  uniform path is unchanged.
- Added `VirtualListHandle` for scroll commands issued outside event handling:
  `anchor_prepended(count)` keeps the reading position when history is inserted
  above (shifting selection indices with the data), `set_offset`, and
  `scroll_to_bottom`. `VirtualList` also gained the same methods directly for
  use before the widget moves into a runtime.
- Added `Autocomplete` and `AutocompletePopup`, an editable combobox whose
  typing filters options into a floating suggestion list. It supports IME
  preedit, selection and clipboard editing, `ArrowUp`/`ArrowDown` navigation,
  `Enter` and pointer commits, `Escape` and light dismissal, a custom
  `with_filter` predicate, `with_placeholder`, `with_caret_anchor` for
  mention-style completion, and `query_changed` / `selected` signals. Append
  the popup returned by `Autocomplete::popup` as the field's single child.

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
- `UiDispatcher` wake-ups are coalesced: a burst of sends invokes the wake
  callback once instead of once per value, and receiving re-arms the wake so
  values sent during a drain are never stranded without a wake-up.
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
