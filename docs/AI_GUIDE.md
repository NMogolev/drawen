# Ask AI safely

Drawen is unusual enough that a model will guess the wrong API unless the prompt
sets boundaries. This page is the short, copyable contract. It is written for
both humans asking a model and agents editing a Drawen codebase.

## The one architecture

```text
application state -> widgets -> layout -> input -> paint -> DisplayList -> renderer
```

- Application code depends on the `drawen` facade. Add
  `drawen-platform-winit` only for a native window.
- Do not import `drawen-ui`, `drawen-widgets`, `drawen-core`, or
  `drawen-render` in ordinary application code. Use the facade re-exports.
- `UiRuntime` owns the retained widget tree. Build it with `append_child`; do not
  keep live widget values outside the runtime after moving them in.
- The tree, widgets, signals, and subscriptions are **single-threaded** and
  UI-thread-local. They are not `Send`/`Sync` and must not be touched from a
  worker thread. Run owned work on `drawen::tasks::TaskPool` and deliver owned
  data to the UI thread through `drawen::tasks::UiDispatcher`; mutate state in
  the event loop.
- Widgets do not call platform APIs, open windows, spawn threads, or start
  timers. They emit typed signals and use `EventContext` requests. The
  application or `drawen-platform-winit` performs the platform action.
- The native event loop is the one from `minimal_window`:
  `Resized -> layout`, `Input -> dispatch_event`, widget request -> redraw,
  `redraw -> paint`, `CloseRequested -> Exit`.

## Facts models commonly get wrong

| Wrong guess | What Drawen actually has |
| --- | --- |
| `Button::new("Save")` | `Button::new()` then append a `Text` child |
| `button.on_click(...)` | `button.activated().subscribe(...)`, keep the `Subscription` |
| `Text::new("hello")` | `Text::styled("hello", TextStyle::new(px, color))`, or `Text::new(prepared_layout)` |
| `use drawen::TextStyle` | `TextStyle` is in `drawen::render`; `TextLayout` and shapers are there too |
| `text.set_text("...")` | Rebuild the label from application state, or use `TextField::set_text` for editable text |
| React hooks, `use_state`, virtual DOM | Plain Rust state; retained widgets are updated explicitly |
| `Widget::new().build()` | Widgets are values appended with `UiRuntime::append_child` |
| CSS sibling selectors, animations | DSS combinators, structural states, and typed properties only |
| `signal.connect` from any thread | `Signal::subscribe` on the UI thread, returns `drop`-based `Subscription` |
| A `Signal<T>` that stores a value | `Signal` is value-less; `Property<T>` stores a value and emits `changed` |
| `button.enabled = state.is_valid` | `runtime.bind(&button.enabled(), &state.is_valid)`, keep the `Binding` |
| `checkbox.checked = state.agreed` | `runtime.bind(&checkbox.checked(), &state.agreed)`; `Switch::checked()` too |
| `slider.value = state.volume` | `runtime.bind(&slider.value(), &state.volume)`; copies are clamped to the range |
| Reading a slider value | `slider.value().get()` (`value()` returns `Property<f32>`); `minimum()`/`maximum()` stay `f32` |
| `progress.value = state.fraction` | `runtime.bind(&bar.progress(), &state.fraction)`; copies are clamped to `0.0..=1.0` |
| `expander.open = state.open` | `runtime.bind_layout(&expander.expanded(), &state.open)` — expanding changes geometry |
| `tabs.selected = state.tab` | `runtime.bind_layout(&tabs.selected(), &state.tab)`; `SegmentedControl`/`Dropdown` use `bind` |
| Reading a selection | `tabs.selected().get()` returns `Option<usize>`; an out-of-range binding copy resolves to `None` |
| `spinner.progress = state.fraction` | `runtime.bind(&spinner.progress(), &state.fraction)`; `Property<Option<f32>>`, `None` is indeterminate |
| `radio_group.selected = state.choice` | `runtime.bind(&group.selected(), &state.choice)`; `Property<Option<T>>`, `changed()` fires on every `set_selected` |
| `viewer.zoom = state.zoom` | `runtime.bind_layout(&viewer.zoom(), &state.zoom)` — zoom changes geometry; call `with_zoom_range` before binding |
| `splitter.ratio = state.ratio` | `runtime.bind_layout(&splitter.ratio(), &state.ratio)` — the ratio sets the pane sizes |
| `field.text = state.name` | `runtime.bind_layout(&field.text(), &state.name)`; `TextEdit::text()`, `Autocomplete::query()`, `CommandPalette::query()` too. User edits keep the caret; an external write resets it |
| Two-way text state | Two opposite `bind`s converge (equal writes are silent); no `bind_from` or reactive graph |
| `view.offset = state.scroll` | `runtime.bind_layout(&view.offset(), &state.scroll)`; `ListView`, `VirtualList`, `VirtualTable` too. Kept `>= 0`; the content upper bound is clamped at layout. `VirtualListHandle` commands write the same property |
| `std::thread::spawn` that captures a widget | `drawen::tasks::TaskPool` + `UiDispatcher`; workers own plain data |
| An audio widget with its own timer | `AudioPlayer` + `MediaSession`; audio is part of the media session, not a second clock |
| `drawen::MediaDirector` by default | Enable the `director` feature; `AudioPaleoSession` needs `audio` |
| `Video` after `--no-default-features` | Enable `media`; `drawen-widgets` also gates media widgets behind `media` |
| A dropdown that expands inside its parent | `Popover`; the runtime owns an anchored floating overlay layer |
| `tooltip.attach(widget)` | `Tooltip` is a one-child container; append the anchor as its child |
| `popover.set_content(...)` | `Popover` has exactly two children: anchor first, floating content second |
| `menu.add_item(...)` | `Menu::new([MenuEntry::item("...")])`, placed in a `Popover` content slot |
| Hand-built context menu | `Popover` with `PopoverTrigger::Secondary` plus a `Menu` |
| A centered modal dialog | `Modal` + `Dialog`; the runtime adds the scrim, input blocking, and focus trap |
| `dialog.show()` that also lays out | `ModalHandle::open()`, then `runtime.invalidate_layout()`; focus after the next layout |
| A searchable command palette | `Modal` + `CommandPalette::new([PaletteCommand::new("...")])`; `executed(index)` dismisses the modal |
| Transient corner notifications | `ToastHost` + `ToastHandle::show(Toast::new("..."))`; cards paint above the tree and expire on their own |
| A primary action plus a dropdown | `SplitButton::new("Save")` with one `Menu` child; `activated()` runs the action, the arrow floats the menu |
| Copyable read-only text | `Text::styled("...", style).selectable()`; drag or double-click to select, Ctrl+C copies, the text is never edited |
| A numeric field with steppers | `NumberInput::new().with_range(min, max).with_step(s)`; `value()` is a `Property<f64>`, arrows and steppers step, Enter commits |
| An interval with two thumbs | `RangeSlider::with_value(min, max, lo, hi)`; `value()` is a `Property<(f32, f32)>`, thumbs stop at each other, arrows select and step |
| A hierarchical tree with selection | `TreeView::new().with_roots(vec![TreeNode::new(id, label).child(...)])`; `selected()` is a `Property<Option<String>>` of node ids, twisties expand, Enter/double-click activates |
| A calendar date picker | `DatePicker::with_month(y, m).with_value(Date::new(y, m, d)?)`; `selected()` is a `Property<Option<Date>>`, header arrows and PageUp/PageDown page months, no platform clock |
| An editable combobox / mention picker | `Autocomplete::new([...])` with its `popup()` appended as the single child; `selected(index)` commits, `with_caret_anchor(true)` for mentions |
| A chat timeline with wrapped messages | `VirtualList::new(source, estimate).with_variable_heights(true)`; stable `row_key`, `VirtualListHandle::anchor_prepended(count)` when history loads above |

## Q&A

### "Make me a window with a button."

Copy [`minimal_window.rs`](../crates/drawen/examples/minimal_window.rs). The
essential shape is:

```rust
let mut runtime = UiRuntime::new(root);
let button = runtime.append_child(runtime.root(), Button::new())?;
runtime.append_child(button, Text::styled("Save", TextStyle::new(16.0, Color::BLACK)))?;
runtime.layout(Constraints::tight(size)?)?;
```

Then implement `WindowApplication`: `window_options`, `window_event`, and
`redraw`. Do not add a second event loop or a background thread that mutates the
runtime.

### "Why doesn't `Button::new()` take text?"

`Button` is a clickable single-child surface. The child is arbitrary content:
`Text`, `Row`, an icon, a spinner, or a composed widget. This is composition,
not an omission. Put labels in the child; put padding, border, hover, pressed,
and focus styling on the button.

### "Add application state and update the UI."

Use `Rc<RefCell<State>>` for shared model state, a widget signal to mutate it,
and one redraw path. The canonical runnable example is
[`stateful_app.rs`](../crates/drawen/examples/stateful_app.rs). A state signal
handler should set a dirty flag; the event loop calls `invalidate_paint()` and
returns `WindowAction::RequestRedraw`. Call `invalidate_layout()` only when
geometry changes.

Do not invent a reactive dependency graph, hooks, or a global store. A
`Signal<T>` is a synchronous notification channel, not an observable framework.
For a direct state-to-widget link, use `Property<T>` and `UiRuntime::bind`: a
one-way synchronous copy that invalidates paint, not a dependency graph. Drawen
does **not** track dependencies automatically: reading `prop.get()` inside
`paint` or `layout` subscribes nothing. You explicitly `watch` / `watch_layout`
a property (or `bind` it) to declare that it affects the output. See
[RECIPES.md](RECIPES.md#13-property-bindings) and
[`bound_app.rs`](../crates/drawen/examples/bound_app.rs).

### "Bind a widget property to application state."

Store the state in a `Property<T>` and bind the widget's property to it before
or after appending the widget:

```rust
let is_valid = Property::new(false);
let button = Button::new();
let enabled = button.enabled();
let button = runtime.append_child(runtime.root(), button)?;
let binding = runtime.bind(&enabled, &is_valid);
// keep `binding` alive; later, from any UI-thread code:
is_valid.set(true);
```

`bind` copies the current value once, then follows every source change and
wakes redraws through `take_redraw_request`. Keep the returned `Binding` like a
`Subscription`. Equal writes are silent, so chains and cycles converge. There is
no single two-way binding API: for the reverse direction (widget to state),
subscribe to the widget signal and write the property explicitly, or — for text
properties, which user edits already update — add a second opposite `bind`; the
cycle converges and keeps the caret. Use `bind_layout` when the target
participates in layout, and `runtime.watch(&property)` for state a custom widget
reads at paint time.

### "Can I update the UI from another thread?"

No. The widget tree is UI-thread-only. Run decoding, parsing, or computation on
a `drawen::tasks::TaskPool` worker, then deliver owned data through a
`drawen::tasks::UiDispatcher` and apply it from `window_event` (the native
adapter wakes the loop through `WindowApplication::attach_waker`),
`next_frame_in`, or a widget animation tick. Never hold a `UiRuntime`, `Widget`,
`Signal`, or `Subscription` across threads.

### "Let an LLM agent see and drive the app without screenshots."

Enable the facade's `agent` feature (or `agent-serve` for the TCP endpoint)
and use `drawen::agent`. `Agent::observe` renders the runtime's semantic tree
as compact text or JSON; `Agent::execute` applies typed commands through the
same accessibility and input paths a human uses, and every reply carries a
fresh snapshot. Snapshots are bounded (`total`/`truncated`); observe subtrees
with `{"op":"observe","target":<id>}` when the tree is large, and use
`advance`/`wait_until_idle` to settle animations before judging a result.
`AgentServer::bind` serves JSON lines on loopback; drain it on the UI thread
once per event-loop turn, exactly like a `UiDispatcher`. Do not screenshot the
window or synthesize OS input for this — the semantic tree is the observation
channel. See [AGENT.md](AGENT.md).

### "Add a stylesheet."

Use `UiEnvironment::from_dss(theme, source)` and
`UiRuntime::with_environment`. Attach identity with
`Styled::new(widget).with_id(...)` and `.with_class(...)`. The runtime adds
`:hover` and `:focus`. DSS is not full CSS: no sibling combinators, media
queries, keyframes, or animations. See
[RECIPES.md](RECIPES.md#4-a-stylesheet-with-dss) and [DSS.md](DSS.md).

### "Add a tooltip or a dropdown menu."

Do not expand the menu inside the parent. `Tooltip` and `Popover` use the
runtime's anchored overlay layer, which flips and clamps against the window
edges and paints above the rest of the tree.

```rust
let tooltip = runtime.append_child(runtime.root(), Tooltip::new("Refresh"))?;
let button = runtime.append_child(tooltip, Button::new())?;

let menu = Menu::new([
    MenuEntry::item("Duplicate").with_shortcut("Ctrl+D"),
    MenuEntry::separator(),
    MenuEntry::item("Move to trash").disabled(),
]);
let popover = runtime.append_child(
    runtime.root(),
    Popover::new().with_trigger(PopoverTrigger::Primary),
)?;
let anchor = runtime.append_child(popover, Button::new())?;
let panel = runtime.append_child(popover, menu)?; // child 1 floats
```

`Tooltip` takes one child (the anchor). `Popover` takes exactly two: anchor,
then floating content. `Menu` supplies data-driven rows and emits
`selected(index)`; selecting a row also dismisses the surrounding overlay. Light
dismissal (outside primary click, Escape, focus loss) is on by default. See
[RECIPES.md](RECIPES.md#8-tooltips-popovers-and-floating-menus).

### "Make a modal dialog."

Use `Modal` as a zero-size host and `Dialog` as the visible panel. The runtime
centers the panel, paints a themed scrim behind it, blocks pointer input to the
tree behind it, and traps sequential focus inside it.

```rust
let modal = Modal::new();
let handle = modal.handle();
let modal = runtime.append_child(runtime.root(), modal)?;
let dialog = runtime.append_child(modal, Dialog::new())?;
let content = runtime.append_child(dialog, Column::new())?;
// title, body, and an action row of buttons inside `content`
```

`handle.open()` / `handle.close()` change the state and emit `opened()` /
`closed()`, but do not lay out. Call `runtime.invalidate_layout()` afterwards,
and move focus into the dialog only after the next layout pass. Escape, a click
on the scrim, or focus loss closes a light modal; pass
`.with_dismiss(DismissPolicy::None)` to require an explicit button. When the
modal closes, focus returns to the widget that held it when the modal opened.
See [RECIPES.md](RECIPES.md#9-modal-dialogs).

### "Add a command palette."

Use `CommandPalette` as the single child of a `Modal`. It is data-driven like
`Menu`: it owns the query field and the filtered rows, so append no children.

```rust
let palette = CommandPalette::new([
    PaletteCommand::new("Open file").with_hint("Ctrl+O"),
    PaletteCommand::new("Save all"),
]);
let executed = palette.executed();
let palette = runtime.append_child(modal, palette)?;
```

Focus the palette after opening and laying out the modal. Typing filters by
case-insensitive substring, arrows move the active row, and Enter or a row
click emits `executed(index)` and dismisses the modal. Do not handle Escape in
application code; the runtime's light dismissal closes the modal. See
[RECIPES.md](RECIPES.md#10-a-command-palette).

### "Show a toast notification."

Append one `ToastHost` anywhere in the tree; it is zero-size and paints its
cards above everything through the floating paint pass. Show notifications
through its handle:

```rust
let host = ToastHost::new();
let handle = host.handle();
let host = runtime.append_child(runtime.root(), host)?;
// later, from a signal handler:
let _ = handle.show(Toast::new("Saved").with_tone(ToastTone::Success));
```

Invalidate paint afterwards, not layout. Toasts fade in, expire after their
duration, and remove themselves; `next_frame_in()` schedules the frames while
any toast is queued. Durations keep elapsing while the window is hidden, so
stale toasts do not pile up and appear on restore. They are non-interactive —
do not put buttons in them. See [RECIPES.md](RECIPES.md#11-toast-notifications).

### "Add a split button with a dropdown."

`SplitButton::new("Save")` takes the primary label as a string, not a `Text`
child. Append exactly one child: the floating `Menu`.

```rust
let split = SplitButton::new("Save");
let activated = split.activated();
let opened = split.opened();
let split = runtime.append_child(parent, split)?;
let menu = Menu::new([MenuEntry::item("Save as…"), MenuEntry::item("Save all")]);
let selected = menu.selected();
let menu = runtime.append_child(split, menu)?;
```

The left segment emits `activated()`; the arrow toggles the menu, which floats
below the button through the overlay layer with light dismissal. On `opened`,
invalidate layout and focus the menu after the next pass. Enter and Space always
activate the primary segment; `ArrowDown` opens the menu. Do not close the menu
from `selected` — the split button dismisses itself.
See [RECIPES.md](RECIPES.md#12-a-split-button).

### "Let the user copy text from a label."

Plain `Text` is decorative. Call `.selectable()` to opt it into read-only
selection:

```rust
let label = Text::styled("Build 2026.11.27", TextStyle::new(14.0, color)).selectable();
let label = runtime.append_child(parent, label)?;
```

The reader can drag a range, double-click a word, Shift-click to extend, press
Ctrl+C to copy, Ctrl+A to select everything, and Escape to clear. The selection
paints with the theme selection color; the text is never edited and the widget
reports a read-only, focusable label to accessibility. Use a `TextEdit` instead
when the user must change the text.

### "Add a numeric input with up/down steppers."

`NumberInput` edits a number, not free text. Give it a range and a step, then
bind its value property to state:

```rust
let input = NumberInput::new().with_range(0.0, 100.0).with_step(1.0).with_value(10.0);
let value = input.value();
let changed = input.changed();
let input = runtime.append_child(parent, input)?;
let _binding = runtime.bind(&value, &state.amount);
```

Only numeric characters are accepted. ArrowUp/ArrowDown and the stacked arrows
on the right step by `step`, clamped to the range. Enter commits the typed text,
Escape reverts, and losing focus commits; unparseable text reverts to the last
committed number. `value()` is a `Property<f64>` (every write is clamped), while
`changed()` fires only on user interaction. See
[RECIPES.md](RECIPES.md#15-a-number-input).

### "Let the user pick a range with two thumbs."

`RangeSlider` keeps the interval in one ordered property:

```rust
let slider = RangeSlider::with_value(0.0, 100.0, 20.0, 80.0);
let value = slider.value(); // Property<(f32, f32)>, lower <= upper always
let slider = runtime.append_child(parent, slider)?;
let _binding = runtime.bind(&value, &state.span);
```

The pointer grabs the nearest thumb and stops it at the other thumb;
`ArrowDown`/`ArrowUp` select a thumb and `ArrowLeft`/`ArrowRight` step it.
Binding copies clamp both ends and swap a reversed pair, so state can never
push an inverted interval into the widget. See
[RECIPES.md](RECIPES.md#16-a-range-slider).

### "Show a file tree the user can expand and select."

`TreeView` flattens the branches whose ancestors are expanded and addresses
selection by stable node id:

```rust
let tree = TreeView::new().with_roots(vec![
    TreeNode::new("src", "Source").child(TreeNode::new("main", "main.rs")),
]);
let selected = tree.selected(); // Property<Option<String>>
let tree = runtime.append_child(parent, tree)?;
let _binding = runtime.bind(&selected, &state.current_node);
```

Click a twisty to expand or collapse (`toggled()`), click a row to select
(`changed()`), double-click or press Enter to open (`activated()`). Arrow keys
navigate, expand, and collapse; ids survive structure changes, so bind the id
property rather than row indices. See
[RECIPES.md](RECIPES.md#17-a-tree-view).

### "Let the user pick a date from a calendar."

`DatePicker` shows one month at a time and never reads the platform clock, so
give it the initial date yourself:

```rust
let picker = DatePicker::with_month(2026, 11)
    .with_value(Date::new(2026, 11, 27).expect("valid date"));
let selected = picker.selected(); // Property<Option<Date>>
let picker = runtime.append_child(parent, picker)?;
let _binding = runtime.bind(&selected, &state.deadline);
```

Click a day or press Enter on the cursor to select (`changed()`); header arrows
and PageUp/PageDown page months; arrow keys walk days and weeks across month
boundaries. `Date::new` validates leap years and formats as `YYYY-MM-DD`. For a
dropdown field, host the picker in a `Popover`. See
[RECIPES.md](RECIPES.md#18-a-date-picker).

### "Copy from my native window never reaches the OS clipboard."

The runtime starts with an in-memory clipboard. The winit adapter delivers an
OS-backed clipboard once through `WindowApplication::attach_clipboard`; forward
it to the runtime:

```rust
fn attach_clipboard(&mut self, clipboard: Box<dyn drawen::Clipboard>) {
    self.runtime.set_clipboard(clipboard);
}
```

Without that override, cut/copy/paste only touch the in-memory clipboard, so
pasting into another application sees nothing. Headless runtimes and tests keep
the in-memory clipboard on purpose. See
[RECIPES.md](RECIPES.md#14-copyable-text).

### "Play this MP4 and turn on the sound."

Choose the backend explicitly:

```rust
let video = Video::open_paleo("clip.mp4")?;        // "media"; pure Rust: H.264/HEVC/VP9/AV1, no audio
let video = Video::open_paleo_audio("clip.mp4")?;  // "audio" (implies "media")
let video = Video::open("clip.mp4")?;              // "media"; external ffmpeg/ffprobe, no audio
let audio = AudioPlayer::open("song.flac")?;       // "audio"; music player with cover art
```

Only `open_paleo_audio` has an audio-mastered clock. Audio continues only while
the event loop ticks the session; return `runtime.next_frame_in()` from
`WindowApplication::next_frame_in`. Forward `WindowPresentation` to
`runtime.set_presented` from `presentation_changed` and return
`runtime.tick(delta)` from `tick`, so a hidden window advances media without
painting. Use `drawen-director` with the `director` feature for
minimize/occlude/close policy. Background playback after the last window closes
requires `MediaDirector::with_background_playback(true)`; it is deliberately not
the default.

### "Why is my window frozen or my audio stopped?"

Common causes:

- `UiRuntime::layout` is not called after a resize, so hit testing and painting
  use stale geometry.
- A `Subscription` was dropped immediately, so the click handler never runs.
- A `Binding` was dropped immediately, so the widget property stopped following
  its source property.
- The event loop ignores `take_redraw_request()` or never returns a frame
  interval from `next_frame_in()`.
- `presentation_changed` does not forward to `UiRuntime::set_presented`, so the
  runtime keeps scheduling visible frames while the window is hidden. With the
  forwarding in place, `next_frame_in()` returns the background interval only
  for media widgets and queued toasts, and `None` for purely visual animation.
- A visual loop (sticker, spinner) sits outside the scroll viewport, inside a
  collapsed branch, or under a modal scrim. Freezing is a property of the tree:
  the runtime stops purely visual animation per widget visibility, not just per
  window, and resumes it when the widget is visible again. Media clocks and
  toast lifetimes keep running through `Widget::advances_while_hidden`; do not
  "fix" an offscreen loop by forcing frames.
- A long decode or file read runs inside `paint`; paint must only record display
  commands. Move blocking work out of the UI thread.
- A worker thread tries to mutate the runtime; that is not supported.

## Prompt template

Paste this before asking a model for Drawen code:

```text
Use Drawen 0.3. Application code depends on the `drawen` facade, plus
`drawen-platform-winit` for a native window. Do not use drawen-ui or
drawen-widgets directly.

The widget tree, signals, and subscriptions are UI-thread-only. Widgets do not
spawn threads or timers and do not call platform APIs. Build with
`UiRuntime::append_child`, lay out with `Constraints`, dispatch `InputEvent`s,
paint to a `DisplayList`, and let `WindowApplication` return `WindowAction`s.

`Button::new()` takes no label: append a `Text` child. Signals use
`.subscribe(...)` and the returned `Subscription` must be kept alive. State is
plain Rust (`Rc<RefCell<_>>`); after a state change call `invalidate_paint()` and
return `WindowAction::RequestRedraw`. `Property<T>` stores state with a change
signal; `runtime.bind(&target, &source)` copies source changes into a widget
property and wakes redraws, and the returned `Binding` must be kept alive. DSS
has no sibling combinators, animations, or full browser CSS.

Copy the event loop and media policy from:
- crates/drawen/examples/minimal_window.rs
- crates/drawen/examples/stateful_app.rs
- crates/drawen/examples/bound_app.rs
- docs/RECIPES.md
- docs/MEDIA.md
```

## Canonical files to copy

- [`minimal_window.rs`](../crates/drawen/examples/minimal_window.rs) — native
  window, button, resize, close, redraw.
- [`stateful_app.rs`](../crates/drawen/examples/stateful_app.rs) — state,
  signal subscription, dirty paint, lifecycle.
- [`bound_app.rs`](../crates/drawen/examples/bound_app.rs) — property cells,
  `UiRuntime::bind`, watched state, no dirty flag.
- [`hello_drawen.rs`](../crates/drawen/examples/hello_drawen.rs) — a more
  complete single-window application.
- [`workspace.rs`](../crates/drawen/examples/workspace.rs) — docking, tabs,
  persistence.
- [`background_tasks.rs`](../crates/drawen/examples/background_tasks.rs) —
  worker pool, progress, and UI delivery.
- [`RECIPES.md`](RECIPES.md) — the thirteen application patterns.
- [`MEDIA.md`](MEDIA.md) — sessions, clocks, backends, lifecycle policy.
- [`THREADING.md`](THREADING.md) — task pools, cancellation, and the UI bridge.
