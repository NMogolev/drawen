# Common recipes

These are the canonical application patterns. They assume a `UiRuntime` named
`runtime` and that errors are propagated with `?` from a function returning
`Result<(), Box<dyn std::error::Error>>`.

Use the `drawen` facade in application code. The only extra dependency a normal
native application needs is `drawen-platform-winit` for the window adapter.

## 1. A button with text

`Button` is a clickable single-child surface, not a text control. Compose the
label as a child:

```rust
use drawen::{Button, Color, Subscription, Text, render::TextStyle};

let button = Button::new();
let _clicks: Subscription = button.activated().subscribe(|()| println!("Save"));

let button = runtime.append_child(runtime.root(), button)?;
runtime.append_child(
    button,
    Text::styled("Save", TextStyle::new(14.0, Color::BLACK)),
)?;
```

Why this shape:

- `Button::new()` deliberately takes no text, so the same control can contain a
  `Text`, a `Row` with an icon, a `Spinner`, or a composed custom widget.
- The button paints the surface, padding, border, and interaction states. The
  child paints its content.
- `activated()` is a typed `Signal<()>`. Keep the returned `Subscription` alive
  for as long as the handler should run.

For a button that should be styled by DSS, wrap it without changing the child:

```rust
let save = Styled::new(Button::new())
    .with_id("save")
    .with_class("primary");
```

## 2. A form with `TextField`

`TextField` owns its editable text. `changed()` emits the current value after
user input; it does not fire for `set_text()`.

```rust
use std::{cell::RefCell, rc::Rc};

use drawen::{
    Button, Color, Column, Subscription, Text, TextField, render::TextStyle,
};

let field = TextField::with_text("");
let submitted_value = Rc::new(RefCell::new(String::new()));

let _changed: Subscription = field.changed().subscribe({
    let submitted_value = Rc::clone(&submitted_value);
    move |text| *submitted_value.borrow_mut() = text.clone()
});

let submit = Button::new();
let _submit: Subscription = submit.activated().subscribe({
    let submitted_value = Rc::clone(&submitted_value);
    move |()| println!("submit: {}", submitted_value.borrow())
});

let form = runtime.append_child(runtime.root(), Column::new())?;
let field = runtime.append_child(form, field)?;
let submit = runtime.append_child(form, submit)?;
runtime.append_child(
    submit,
    Text::styled("Submit", TextStyle::new(14.0, Color::BLACK)),
)?;
```

The window adapter translates keyboard layout and input methods into
`InputEvent::TextInput`. Click the field before typing; the runtime routes text
and key events to the focused widget. Clipboard operations use the runtime
clipboard, which `drawen-platform-winit` fills with the OS clipboard.

If the form also needs validation, keep the value in the same application state
struct as the rest of the screen (recipe 3) instead of a separate `String`.

## 3. Application state, signals, and one redraw path

State is ordinary Rust. There is no hook runtime, virtual DOM, or global store.
The canonical shape is:

1. application state lives in `Rc<RefCell<State>>` (or in the application
   struct when only one place needs it);
2. widgets emit typed signals;
3. a signal handler mutates state and marks the application dirty;
4. the event loop turns that dirty flag into a paint invalidation and a redraw
   request;
5. `redraw()` paints the current state.

The runnable version is
[`stateful_app.rs`](../crates/drawen/examples/stateful_app.rs). Its core:

```rust
use std::{cell::{Cell, RefCell}, rc::Rc};

#[derive(Default)]
struct AppState {
    count: u32,
}

let state = Rc::new(RefCell::new(AppState::default()));
let dirty = Rc::new(Cell::new(false));

let button = Button::new();
let _subscription = button.activated().subscribe({
    let state = Rc::clone(&state);
    let dirty = Rc::clone(&dirty);
    move |()| {
        state.borrow_mut().count += 1;
        dirty.set(true);
    }
});
```

In `WindowApplication::window_event`:

```rust
WindowEvent::Input(event) => {
    let _ = self.runtime.dispatch_event(&event);
    let state_changed = self.dirty.replace(false);
    let widget_changed = self.runtime.take_redraw_request();
    if state_changed {
        self.runtime.invalidate_paint();
    }
    if state_changed || widget_changed {
        WindowAction::RequestRedraw
    } else {
        WindowAction::None
    }
}
```

Rules that keep this predictable:

- Call `invalidate_paint()` for a visual state change; call `invalidate_layout()`
  (usually through a mutable widget accessor) only when geometry changes.
- Build the `DisplayList` from `UiRuntime::paint` in `redraw()`. Do not draw from
  the event handler.
- Keep every `Subscription` in the application or in a `Vec<Subscription>` so it
  is not dropped immediately.
- Signals are synchronous and UI-thread-local. Do not use them from a worker
  thread; send data to the UI thread first.

## 4. A stylesheet with DSS

DSS resolves typed values on retained nodes. It uses a semantic theme as a
fallback, so a stylesheet does not need to redefine every color.

```rust
use drawen::{
    Box as DrawenBox, Button, Color, DarkTheme, Styled, Text, UiEnvironment, UiRuntime,
    render::TextStyle,
};

let environment = UiEnvironment::from_dss(
    DarkTheme,
    r#"
        :root {
            background: #101820;
            color: #f1f3f5;
            accent-color: #2edcb3;
            font-size: 15px;
            border-radius: 6px;
        }

        button {
            background: #243447;
            padding: 6px 10px;
            font-weight: 600;
        }

        button:hover { background: #34516b; }
        button:pressed { background: #182838; }
        button:focus { border: 2px solid #2edcb3; }
        #save { background: #1f7a5a; }
        .primary { border-radius: 8px; }
    "#,
)?;

let mut root = DrawenBox::new();
root.set_background(Some(Color::BLACK));
let mut runtime = UiRuntime::with_environment(root, environment);
let save = Styled::new(Button::new())
    .with_id("save")
    .with_class("primary");
let save = runtime.append_child(runtime.root(), save)?;
runtime.append_child(
    save,
    Text::styled("Save", TextStyle::new(15.0, Color::WHITE)),
)?;
```

What DSS supports:

- type, id, class, and state selectors (`button`, `#save`, `.primary`,
  `:hover`, `:pressed`, `:focus`, `:disabled`, `:root`);
- descendant and child combinators (`toolbar button`, `toolbar > button`);
- structural pseudo-classes (`:first-child`, `:last-child`, `:only-child`,
  `:nth-child(2n+1)`) and `:not()`/`:is()`;
- `background`, `color`, `accent-color`, `border`, `padding`, `gap`,
  `font-family`, `font-size`, `font-weight`, `font-style`, `border-radius`,
  `width`, `height`, `min-*`, and `max-*`;
- custom properties with `var()`, `calc()`, `!important`, and the CSS Color
  Level 4 palette including `hsl()`;
- CSS specificity and source order;
- inheritance for text properties and `accent-color`.

What it does not support: sibling combinators, percentages, media queries,
keyframes, animations, or full browser CSS. The runtime owns `:hover` and
`:focus` and adds those states to the node's target automatically. See
[DSS.md](DSS.md).

## 5. Scrolling and virtual lists

For a small, fully retained list, put a `Column` inside a `ScrollView` and give
the viewport a bounded height:

```rust
use drawen::{Color, Column, ScrollView, SizedBox, Text, render::TextStyle};

let viewport = runtime.append_child(
    runtime.root(),
    SizedBox::new(None, Some(320.0)),
)?;
let scroll = runtime.append_child(viewport, ScrollView::vertical())?;
let content = runtime.append_child(scroll, Column::new())?;
for index in 0..100 {
    runtime.append_child(
        content,
        Text::styled(format!("Row {index}"), TextStyle::new(14.0, Color::BLACK)),
    )?;
}
```

For a large data set, use `VirtualList`. It owns its row data, shapes only the
visible rows, and caches row visuals by stable key:

```rust
use drawen::{VirtualDataSource, VirtualList};

struct Rows(Vec<String>);

impl VirtualDataSource for Rows {
    fn row_count(&self) -> usize {
        self.0.len()
    }

    fn row_key(&self, row: usize) -> u64 {
        row as u64
    }

    fn row_text(&self, row: usize) -> String {
        self.0[row].clone()
    }
}

let list = VirtualList::new(
    Rows((0..100_000).map(|index| format!("Row {index}")).collect()),
    28.0,
);
let _selection = list.selection_changed().subscribe(|rows| println!("selected: {rows:?}"));
let list = runtime.append_child(runtime.root(), list)?;
```

`ListView` is the middle ground: the runtime owns each row widget, and the view
lays out only the visible range for a fixed row extent. Use `VirtualList` when
row count is large or row data is expensive to shape; use `ListView` when each
row is a real composed widget.

## 6. A docking workspace

`WorkspaceLayout` is serializable layout data; `DockArea` is the widget that
presents it. The panel widgets are still ordinary runtime children, appended in
the same order as the registered panel IDs.

```rust
use std::{cell::RefCell, rc::Rc};

use drawen::{
    Color, DockArea, DockAxis, LayoutNode, PanelId, Text, WorkspaceLayout, render::TextStyle,
};

let layout = WorkspaceLayout::new(LayoutNode::split(
    DockAxis::Horizontal,
    0.6,
    LayoutNode::Panel { id: PanelId::from("files") },
    LayoutNode::Panel { id: PanelId::from("editor") },
))?;
let layout = Rc::new(RefCell::new(layout));

let mut dock = DockArea::new(Rc::clone(&layout));
dock.register_panel(PanelId::from("files"))?;
dock.register_panel(PanelId::from("editor"))?;

let dock = runtime.append_child(runtime.root(), dock)?;
runtime.append_child(
    dock,
    Text::styled("Files", TextStyle::new(14.0, Color::BLACK)),
)?;
runtime.append_child(
    dock,
    Text::styled("Editor", TextStyle::new(14.0, Color::BLACK)),
)?;
```

Persist `WorkspaceLayout` as JSON at the application boundary, not the live
widget tree. Tabs, floating panels, MDI documents, split resizing, and restore
validation are covered in [DOCKING.md](DOCKING.md) and the `workspace` example.

## 7. Video and audio

Both players share one configuration vocabulary: `PlayerTheme`,
`PlayerBehavior`, and a plain control set. Open media through the backend that
matches the file:

```rust
use drawen::{Color, PlayerBehavior, PlayerTheme, Video, VideoControls};

// Pure Rust, in-process. H.264 video only; no audio.
let video = Video::open_paleo("clip.mp4")?
    .with_title("A short film")
    .with_subtitle("Paleo · 1080p")
    .with_control_set(VideoControls::new())
    .with_theme(PlayerTheme::new().with_accent(Color::rgba8(255, 45, 85, 255)))
    .with_behavior(PlayerBehavior::new().with_auto_hide(None));
let video = runtime.append_child(runtime.root(), video)?;

// External ffmpeg/ffprobe. Broad codecs, but no audio track.
// let video = Video::open("clip.mp4")?;
```

The overlay behaves like a web player: hovering shows it, inactivity hides it
after `PlayerBehavior::auto_hide` (three seconds by default), the center badge
pauses, and the seek bar shows a hover time preview. Set `auto_hide: None` to pin
the controls, or start from `VideoControls::none()` / `minimal()`.

For files with audio, enable the `audio` feature and open the audio-mastered
Paleo session:

```text
cargo add drawen --features audio,director
```

```rust
// H.264 video (when present) plus decoded audio through the default output.
let video = Video::open_paleo_audio("clip.mp4")?;
let video = runtime.append_child(runtime.root(), video)?;
```

The audio player is a separate widget over the same `MediaSession` contract:

```rust
use drawen::{AudioControls, AudioPlayer, AudioPlayerLayout, PlayerTheme, RepeatMode};

let audio = AudioPlayer::open("song.flac")?
    .with_title("Night drive")
    .with_subtitle("Paleo · FLAC")
    .with_layout(AudioPlayerLayout::Inline)
    .with_control_set(AudioControls::new())
    .with_repeat(RepeatMode::All)
    .with_theme(PlayerTheme::new().with_accent(Color::rgba8(46, 196, 182, 255)));

// Take the signals before moving the widget into the tree.
let previous = audio.previous_requested();
let next = audio.next_requested();
let audio = runtime.append_child(runtime.root(), audio)?;
previous.subscribe(|()| { /* load the previous track */ });
next.subscribe(|()| { /* load the next track */ });
```

`AudioPlayerLayout::Compact` collapses to a single row for tight surfaces.
Previous and next are requests: subscribe to the signals and rebuild the player
with the new session, so the widget never invents playlist policy. `RepeatMode`
and shuffle are also signals rather than implicit playlist mutations.

Both players present frames and forward play/pause/seek/rate intent to their
`MediaSession`. They do not own a timer. Return `runtime.next_frame_in()` from
`WindowApplication::next_frame_in` so the event loop ticks playback while the
session is active. Forward presentation changes and the hidden tick as well:

```rust
fn presentation_changed(&mut self, presentation: WindowPresentation) -> WindowAction {
    self.runtime.set_presented(presentation.is_presented());
    WindowAction::None
}

fn tick(&mut self, delta: std::time::Duration) -> bool {
    self.runtime.tick(delta)
}

fn next_frame_in(&self) -> Option<std::time::Duration> {
    self.runtime.next_frame_in()
}
```

A hidden runtime freezes visual transitions and animated images, but keeps
`Video` and `AudioPlayer` sessions on the low-frequency background cadence.

Audio is the master clock in `AudioPaleoSession`. If the window is minimized or
occluded, use `drawen-director` to stop video presentation while keeping audio,
and opt into background playback explicitly. See [MEDIA.md](MEDIA.md),
[PALEO.md](PALEO.md), and the background-playback section in the README.

## 8. Tooltips, popovers, and floating menus

Floating surfaces do not expand in place. `UiRuntime` owns an overlay layer:
a widget requests placement through `LayoutContext::layout_child_overlay`, and
the runtime measures the content only while it is visible, flips it to the
opposite side when it would leave the window along that side's axis, clamps it
inside the viewport margin, paints it above the rest of the tree, and hit tests
it first. Open overlays stack LIFO by open time: the surface opened last paints
on top and receives hits first, regardless of where its owner sits in the tree,
and a nested menu always stacks above the modal that contains it. Closing an
overlay returns keyboard focus to the widget that held it when the overlay
opened. Overlay content stays a normal retained subtree, so bubbling, focus, and
accessibility keep working.

`Tooltip` wraps one anchor and adds a non-interactive hover hint:

```rust
use std::time::Duration;
use drawen::{Button, Color, OverlayPlacement, Text, Tooltip, TextStyle};

let tooltip = Tooltip::new("Refresh")
    .with_placement(OverlayPlacement::top())
    .with_delay(Duration::from_millis(400));
let tooltip = runtime.append_child(runtime.root(), tooltip)?;
let button = runtime.append_child(tooltip, Button::new())?;
runtime.append_child(button, Text::styled("Refresh", TextStyle::new(14.0, Color::BLACK)))?;
```

The bubble is painted through the runtime's floating pass, so toolbars and later
siblings cannot cover it, and it is skipped by hit testing: moving the pointer
onto the bubble dismisses it. Reduced motion removes the fade.

`Popover` has exactly two children: the anchor and the floating content.

```rust
use drawen::{Button, Color, OverlayPlacement, Popover, PopoverTrigger, Text, TextStyle};

let popover = Popover::new()
    .with_trigger(PopoverTrigger::Primary)
    .with_placement(OverlayPlacement::bottom());
let popover = runtime.append_child(runtime.root(), popover)?;
let anchor = runtime.append_child(popover, Button::new())?;
runtime.append_child(anchor, Text::styled("Options", TextStyle::new(14.0, Color::BLACK)))?;

// Child 1 is the floating panel, built like any other subtree.
let panel = runtime.append_child(popover, /* your menu or form */)?;
```

Light dismissal is on by default: a primary click outside the content and its
anchor, Escape, or losing window focus asks the popover to close. Use
`.with_dismiss(DismissPolicy::None)` for surfaces the application closes itself,
such as a modal dialog or a command palette. `PopoverTrigger::Secondary` opens
the panel at the pointer, which is the usual context-menu shape.
`PopoverTrigger::Manual` is the default: the application calls `set_open`,
`open_at`, or `close` and reacts to the `opened()` and `closed()` signals.

`Menu` supplies the rows for a dropdown or context menu. It is data-driven,
paints its own surface, and handles pointer and keyboard navigation:

```rust
use drawen::{Button, Menu, MenuEntry, Popover, PopoverTrigger};

let menu = Menu::new([
    MenuEntry::item("Duplicate").with_shortcut("Ctrl+D"),
    MenuEntry::item("Rename").with_shortcut("F2"),
    MenuEntry::separator(),
    MenuEntry::item("Move to trash").with_shortcut("Del"),
]);
let selected = menu.selected();
let popover = Popover::new().with_trigger(PopoverTrigger::Primary);
let popover = runtime.append_child(runtime.root(), popover)?;
let anchor = runtime.append_child(popover, Button::new())?;
let menu = runtime.append_child(popover, menu)?;

selected.subscribe(|index| { /* run the action */ });
// After opened() fires and dispatch_event returns, move focus into the menu
// with runtime.focus_widget(menu) so the arrow keys reach it.
```

Use `PopoverTrigger::Secondary` for a context menu: the same menu opens at the
pointer instead of the anchor's edge. Selecting a row emits `selected(index)` and
asks the runtime to dismiss the surrounding overlay, so the application does not
close it manually. Separators and disabled rows are skipped by keyboard
navigation and never emit.

Rules that keep overlays predictable:

- Placement is a request, not a coordinate. Choose a side and an alignment with
  `OverlayPlacement` and let the runtime flip and clamp.
- Overlay content is still a normal subtree. Do not also position it absolutely.
- A closed popover does not lay out its panel, so hidden menus cost no geometry
  work.
- Keep overlay trees shallow. They are painted last and hit tested first, but
  they are not a second window.

## 9. Modal dialogs

A modal is the same overlay layer with three extra guarantees: the runtime paints
a themed scrim behind it, blocks pointer input to the tree behind it, and keeps
sequential focus inside it. Build it from two widgets: `Modal` is an invisible
one-child host, and `Dialog` is the visible panel.

```rust
use drawen::{Button, Column, Dialog, Modal, Row, Text, TextStyle};

let modal = Modal::new();
let handle = modal.handle();
let modal = runtime.append_child(runtime.root(), modal)?;

let dialog = runtime.append_child(modal, Dialog::new())?;
let content = runtime.append_child(dialog, Column::new())?;
let title = runtime.append_child(content, Text::styled("Delete project?", TextStyle::new(18.0, ink)))?;
// ... body text and an action row with Cancel and Delete buttons ...
let cancel = runtime.append_child(actions, Button::new())?;
```

Open and close it through the handle. Changing the handle does not lay out the
panel, so invalidate layout once per change:

```rust
handle.open();
runtime.invalidate_layout();
// On the next frame, after layout, move focus into the dialog:
let _ = runtime.focus_widget(cancel);
```

`Dialog` is a single-child surface with padding, rounded corners, a border, and a
soft shadow. It reports the `Dialog` accessibility role with the modal state.
The `Modal` host itself is zero-size and skips hit testing, so it does not affect
the surrounding layout.

Light dismissal is on by default: Escape, a primary click on the scrim, or
losing window focus closes the modal and emits `closed()`. Use
`.with_dismiss(DismissPolicy::None)` for a dialog that only its own buttons may
close. When a light modal closes itself, the runtime invalidates layout and
returns focus to the widget that held it when the modal opened; closing through
the handle restores focus on the next layout pass. The application only needs
to react to `closed()`.

Rules for modal dialogs:

- Put the `Modal` in the tree once and toggle the handle, rather than adding and
  removing nodes.
- Lay out after opening before focusing a control inside it. A closed modal does
  not lay out its panel, so focus metadata is stale until the next pass.
- Prefer a `Modal -> Dialog -> Column` structure. The `Dialog` is the panel users
  see; the `Modal` only supplies centering, the scrim, and the focus trap.
- Do not nest a second modal inside a modal. Open a popover or menu from inside
  the dialog instead; the runtime still dismisses those with the dialog.

## 10. A command palette

A command palette is a modal search surface: a query field above a filtered
list of commands. `CommandPalette` is data-driven like `Menu` — it owns its
query editor and rows, so it needs no child widgets. Place it directly as the
single child of a `Modal`:

```rust
use drawen::{CommandPalette, Modal, PaletteCommand};

let modal = Modal::new();
let handle = modal.handle();
let modal = runtime.append_child(runtime.root(), modal)?;

let palette = CommandPalette::new([
    PaletteCommand::new("Open file").with_hint("Ctrl+O"),
    PaletteCommand::new("Save all").with_hint("Ctrl+S"),
    PaletteCommand::new("Copy path"),
]);
let executed = palette.executed();
let palette = runtime.append_child(modal, palette)?;
```

Open it like any modal, then focus the palette after the next layout pass:

```rust
handle.open();
runtime.invalidate_layout();
// On the next frame, after layout:
let _ = runtime.focus_widget(palette);
```

Typing filters commands by case-insensitive substring; the first match is
active immediately. Arrow keys move the active row, Enter executes it, and a
primary click on a row executes it directly. The query field supports the usual
single-line editing: caret movement, shift selection, clipboard, and undo.

`executed()` emits the command's index in the original list, then the palette
asks the runtime to dismiss the surrounding overlay — which closes a
light-dismiss `Modal`. Escape and scrim clicks close the palette through the
same modal dismissal; the palette itself does not handle Escape.

```rust
let _subscription = executed.subscribe(move |index| {
    // Run the command, set a dirty flag, request one redraw.
});
```

Rules for command palettes:

- Keep the command list fixed at construction. The palette filters and scrolls
  the rows itself; it shows up to eight rows and scrolls the rest.
- React to `executed()` with plain state changes; the modal closes itself, so
  do not call `handle.close()` again from the handler.
- Use `matched()` and `active_command()` for read-only inspection, and
  `query()` for the current filter text.
- Style it through the `command-palette` style target; padding, background,
  border, corner radius, foreground, and accent resolve from DSS or the theme.

## 11. Toast notifications

Toasts are transient, non-interactive notifications stacked in a window corner.
`ToastHost` is a zero-size widget that paints its cards through the floating
paint pass, so they appear above the tree and any open overlays without
affecting layout, hit testing, or focus. A shared `ToastHandle` shows and
dismisses them:

```rust
use drawen::{Toast, ToastHost, ToastTone};

let host = ToastHost::new();            // bottom-right corner by default
let handle = host.handle();
let host = runtime.append_child(runtime.root(), host)?;

// From a signal handler or window_event:
let _ = handle.show(Toast::new("Project saved").with_tone(ToastTone::Success));
// ... then invalidate paint and request one redraw.
```

Each toast fades in, stays for its duration (four seconds by default, override
with `.with_duration(...)`), fades out, and removes itself. Timed dismissal runs
through `Widget::advance_animation`; while any toast is queued,
`runtime.next_frame_in()` keeps scheduling frames, so the standard
`tick`/`next_frame_in` wiring in `WindowApplication` is all the timing support
an application needs. Durations keep elapsing while the window is hidden or
minimized, so stale notifications expire in the background instead of piling up
and appearing all at once on restore.

The queue keeps at most five toasts and drops the oldest when a sixth arrives.
`handle.dismiss(id)` removes one early, `handle.clear()` removes all, and
`shown()` emits the identifier of each new toast. Use
`.with_corner(ToastCorner::TopRight)` (or `TopLeft` / `BottomLeft`) to move the
stack; top corners keep the newest toast at the top edge, bottom corners at the
bottom edge.

Rules for toasts:

- Show toasts from plain state changes; do not append a widget per notification.
  The host owns the queue and the cards.
- Toasts are non-interactive like tooltip bubbles. Put actions in a dialog or
  alert instead; a toast should only report status.
- Invalidate paint, not layout, after `show` / `dismiss` / `clear`. The host
  computes card geometry at paint time from the live queue.
- Keep messages short. Cards wrap at 360 logical pixels and never take focus.

## 12. A split button

A `SplitButton` joins a primary action to a dropdown arrow. The left segment
runs the default action and emits `activated()`; the right segment toggles a
floating menu supplied as the widget's single child, usually a `Menu`:

```rust
use drawen::{Menu, MenuEntry, SplitButton};

let split = SplitButton::new("Save");
let activated = split.activated();
let opened = split.opened();
let split = runtime.append_child(parent, split)?;

let menu = Menu::new([
    MenuEntry::item("Save as…").with_shortcut("Ctrl+Shift+S"),
    MenuEntry::item("Save all"),
]);
let selected = menu.selected();
let menu = runtime.append_child(split, menu)?;
```

The menu floats through the same overlay layer as `Popover`, anchored below the
button and aligned to its trailing edge. It is measured only while open. Light
dismissal is on by default: an outside primary click, Escape, or window focus
loss closes the menu, and selecting a row dismisses it through the same path.

Wire the signals like any other overlay:

- `activated()` fires on a completed primary-segment click, Enter, or Space.
- `opened()` / `closed()` fire on every menu transition. On `opened`, invalidate
  layout and focus the menu after the next pass so arrow keys work immediately.
- The child menu's `selected(index)` reports the chosen row; the split button
  closes itself, so do not call `set_open(false)` from the handler.

```rust
subscriptions.push(opened.subscribe({
    let pending_layout = Rc::clone(&pending_layout);
    let pending_focus = Rc::clone(&pending_focus);
    move |()| {
        pending_layout.set(true);
        pending_focus.set(Some(menu));
    }
}));
```

Rules for split buttons:

- Give the widget exactly one child: the floating menu. The primary label is a
  string passed to `new`, not a `Text` child.
- `ArrowDown` opens the menu from the keyboard; Enter and Space always activate
  the primary segment, never the arrow.
- Style it through the `split-button` style target; padding, background, border,
  corner radius, and foreground resolve from DSS or the theme. The arrow segment
  shares the surface and is separated by a one-pixel divider.
- Call `set_open` only from plain state changes and pair it with
  `invalidate_layout`; pointer and keyboard paths request layout themselves.

