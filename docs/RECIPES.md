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
- For a direct state-to-widget link, recipe 13 replaces the dirty flag with
  `Property<T>` and `UiRuntime::bind`.

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

### Variable-height chat history

For a messenger timeline, rows wrap to the viewport width and keep different
heights. Enable measured rows and keep a handle for scroll commands:

```rust
use std::{cell::RefCell, rc::Rc};
use drawen::{VirtualDataSource, VirtualList};

#[derive(Clone)]
struct Messages(Rc<RefCell<Vec<(u64, String)>>>);

impl VirtualDataSource for Messages {
    fn row_count(&self) -> usize {
        self.0.borrow().len()
    }
    fn row_key(&self, row: usize) -> u64 {
        self.0.borrow()[row].0 // stable id, not the index
    }
    fn row_text(&self, row: usize) -> String {
        self.0.borrow()[row].1.clone()
    }
}

let messages = Messages(Rc::new(RefCell::new(initial)));
let list = VirtualList::new(messages.clone(), 28.0).with_variable_heights(true);
let list_handle = list.handle();
let list = runtime.append_child(runtime.root(), list)?;
```

When older history arrives from the network, prepend it through the shared
source and anchor the reading position in the same UI-thread pass:

```rust
messages.0.borrow_mut().splice(0..0, older);
list_handle.anchor_prepended(older_count);
runtime.invalidate_layout();
```

The offset grows by the prepended extent, selection indices shift with the
data, and measured heights survive because they are keyed by `row_key`. Use
`list_handle.scroll_to_bottom()` after appending a new message while the user
is already at the bottom. Rows are shaped only for the visible window; the
construction height is the estimate for rows that have not been shaped yet.

### Autocomplete and mentions

`Autocomplete` is an editable combobox: typing filters options into a floating
list, `Enter` or a click commits one. Append its popup as the single child:

```rust
use drawen::Autocomplete;

let field = Autocomplete::new(["alice", "bob", "carol"])
    .with_placeholder("Search people");
let selected = field.selected();
let popup = field.popup(); // obtain before moving the field into the runtime
let field = runtime.append_child(runtime.root(), field)?;
runtime.append_child(field, popup)?;
let _subscription = selected.subscribe(|index| println!("picked {index}"));
```

For mention-style completion inside a composition flow, anchor the popup at
the caret with `.with_caret_anchor(true)` and replace the match predicate with
`.with_filter(|query, option| ...)`. The field handles IME preedit, and the
platform positions the candidate window at its caret automatically.

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
and a nested menu always stacks above the modal that contains it.

Stacking orders the layer but does not own it. The runtime also records which
overlay opened each surface, so closing a modal or popover cascades to the
overlays opened from inside it: they close in the same pass and their owners'
open flags are synced, so nothing reappears when the surface is reopened.
Closing an overlay returns keyboard focus to the widget that held it when the
overlay opened; if that opener was removed or hidden in the meantime, focus
falls back to the root instead of a dead handle. Overlay content stays a normal
retained subtree, so bubbling, focus, and accessibility keep working.

Placement resolves to a side and a budget. `place_overlay` returns a
`ResolvedOverlay` carrying the origin, the side actually used after flipping,
and the largest size that fits on that side without covering the anchor. When
the measured content is bigger than that budget, the runtime re-measures it with
capped constraints, so a long menu shrinks and scrolls instead of overflowing.
The resolved side is also what anchor-facing decorations, such as the tooltip
nib, follow after a flip.

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
onto the bubble dismisses it. A small nib points at the anchor and follows the
resolved side after a flip. Near a window edge the bubble shrinks to the space
available and clips its text rather than covering the anchor. Reduced motion
removes the fade.

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
paints its own surface, and handles pointer and keyboard navigation. When the
overlay layer caps its height, the list scrolls: the wheel and the arrow keys
keep the active row visible, and clicks land on the scrolled row under the
pointer:

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
- Oversized content shrinks to the space available on the resolved side and
  scrolls there; do not force a fixed height that would cover the anchor.
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


## 13. Property bindings

`Signal<T>` notifies; `Property<T>` stores. A property is a value cell with a
change signal: clones share one value, and `set` emits `changed` only when the
value actually differs. `UiRuntime::bind` copies a source property into a
widget property and keeps it in sync:

```rust
use drawen::{Binding, Button, Property};

let is_valid = Property::new(false);
let button = Button::new();
let enabled = button.enabled();
let button = runtime.append_child(runtime.root(), button)?;

let _binding: Binding = runtime.bind(&enabled, &is_valid);

// Later, from a signal handler or any UI-thread code:
is_valid.set(true); // the button enables and a redraw is requested
```

The runnable version is
[`bound_app.rs`](../crates/drawen/examples/bound_app.rs): widget signals write
into state properties, property subscriptions derive validity, and the binding
drives `Button::enabled`.

Rules that keep bindings thin:

- A binding is one-way, synchronous, and UI-thread-local. It copies the current
  source value once at `bind`, then on every source change. There is no
  dependency graph, no queued delivery, and no two-way magic: for the reverse
  direction, subscribe to the widget signal and write the state property
  explicitly, exactly as in recipe 3.
- Keep every `Binding` alive like a `Subscription`; dropping it disconnects the
  copy immediately. The target keeps its last value.
- Equal writes are silent, so chains and cycles of bindings converge instead of
  looping. Do not build observer pyramids on top of this; derive state in plain
  Rust subscriptions.
- Bound and watched properties wake the runtime themselves: the change drains
  into `take_redraw_request`, `tick`, and `paint`. An event loop whose painted
  state is all properties can drop the manual dirty flag from recipe 3.
- Drawen does not auto-track dependencies: reading `prop.get()` in `paint` or
  `layout` subscribes nothing. Use `runtime.watch(&property)` for state a widget
  reads at paint time without a widget property to bind. Use `bind_layout` /
  `watch_layout` when the value participates in layout; layout damage implies
  paint damage, so `watch_layout` is also correct for a value that affects both
  geometry and appearance, and the next paint re-runs layout automatically.
- Deduplication is exact `PartialEq`. A float property never dedups `NaN` and
  treats `0.0`/`-0.0` as equal; keep its domain sanitized with
  `Property::normalized` (as `Slider::value` does) rather than relying on
  equality to reject bad values.
- Bindable widget properties: `Button::enabled()`, `Checkbox::checked()`,
  `Switch::checked()`, and `Expander::expanded()` return `Property<bool>`;
  `Slider::value()`, `ProgressBar::progress()`, `ImageViewer::zoom()`,
  `Splitter::ratio()`, `ScrollView::offset()`, and `ListView::offset()` return
  `Property<f32>`; `Spinner::progress()` returns `Property<Option<f32>>` (`None`
  is indeterminate); `TabBar::selected()`, `SegmentedControl::selected()`, and
  `Dropdown::selected()` return `Property<Option<usize>>`;
  `RadioGroup::selected()` returns `Property<Option<T>>`; `TextField::text()`,
  `TextEdit::text()`, `Autocomplete::query()`, and `CommandPalette::query()`
  return `Property<String>`. A disabled button ignores activation, skips focus,
  and matches `button:disabled` in DSS.

### Binding toggles and sliders

The same shape drives every bindable control. For a checkbox, bind the checked
property and keep the user-only `toggled()` signal for the reverse direction:

```rust
use drawen::{Checkbox, Property};

let agreed = Property::new(false);
let checkbox = Checkbox::new();
let checked = checkbox.checked();
let toggled = checkbox.toggled();
let checkbox = runtime.append_child(runtime.root(), checkbox)?;

let _binding = runtime.bind(&checked, &agreed); // state -> widget
let _subscription = toggled.subscribe(move |value| {
    agreed.set(*value); // widget -> state
});
```

The two directions form a cycle that converges: a user toggle writes `agreed`,
the binding copies the same value back into `checked`, and the equal write is
silent. `Slider::value()` works the same way and clamps every copied write to
`[minimum, maximum]`, so an out-of-range source cannot push the thumb off the
track:

```rust
use drawen::{Property, Slider};

let volume = Property::new(0.5_f32);
let slider = Slider::new(0.0, 1.0);
let value = slider.value();
let slider = runtime.append_child(runtime.root(), slider)?;

let _binding = runtime.bind(&value, &volume);
volume.set(2.0); // clamped to 1.0
```

Read a scalar with `.get()`: `checkbox.is_checked()`, `slider.value().get()`.
A property's `changed()` fires on every write; the widget's `toggled()` /
`changed()` signal fires only on user interaction.

### Binding progress, expanders, and selection

`ProgressBar::progress()` is a passive clamped float, exactly like the slider
but without a user signal: bind it with `runtime.bind(&bar.progress(),
&state.fraction)` and every copy stays in `0.0..=1.0`.

Choose `bind` versus `bind_layout` by whether the value changes geometry.
`Expander::expanded()` and `TabBar::selected()` change what is laid out (the
body section, the active page), so bind them with `bind_layout`; the next paint
re-measures the tree. `SegmentedControl::selected()` and `Dropdown::selected()`
only repaint, so plain `bind` is enough:

```rust
use drawen::{Expander, Property, TabBar};

let open = Property::new(false);
let expander = Expander::new("Advanced");
let expanded = expander.expanded();
let expander = runtime.append_child(runtime.root(), expander)?;
let _binding = runtime.bind_layout(&expanded, &open); // geometry -> bind_layout

let tab = Property::new(Some(0_usize));
let tabs = TabBar::new(["Home", "Settings"]);
let selected = tabs.selected();
let tabs = runtime.append_child(runtime.root(), tabs)?;
let _binding = runtime.bind_layout(&selected, &tab);
```

Selection properties normalize against the fixed item count: a binding copy of
an out-of-range index resolves to `None` (no selection) rather than panicking,
so a stale source index cannot select a nonexistent tab. The direct setters
(`set_selected`) still panic on an invalid index, exactly as `Slider::set_value`
asserts finiteness while its normalizer stays lenient for binding copies.

### Binding spinners, radio groups, zoom, and split ratios

`Spinner::progress()` is a clamped `Property<Option<f32>>`: `None` means
indeterminate and `Some` is clamped to `0.0..=1.0`, so bind a task fraction with
`runtime.bind(&spinner.progress(), &state.fraction)`. The animation phase is
internal animation state, not a bindable property.

`RadioGroup::selected()` is a generic `Property<Option<T>>` shared by every
button in the group, so one binding drives the whole set:

```rust
use drawen::{Property, RadioGroup};

let choice = Property::new(Some("small"));
let group = RadioGroup::new();
let _binding = runtime.bind(&group.selected(), &choice);
```

Unlike the widget user signals, `RadioGroup::changed()` fires on every
`set_selected` that stores a new value, whether from a click or a programmatic
call; `clear()` stays silent on it. The property's own `changed()` fires on every
write, including binding copies.

`ImageViewer::zoom()` and `Splitter::ratio()` change geometry (the drawn image
scale and the pane sizes), so bind them with `bind_layout`; the next paint
re-measures and, for the viewer, re-clamps the pan offset. Both normalize every
copied write to their configured range. Configure the viewer's range with
`with_zoom_range` **before** binding `zoom()`: the builder rebuilds the property
with the new bounds.

```rust
use drawen::{ImageViewer, Property, SplitAxis, Splitter};

let zoom = Property::new(1.0_f32);
let viewer = ImageViewer::new(image).with_zoom_range(0.5, 8.0);
let _binding = runtime.bind_layout(&viewer.zoom(), &zoom);

let ratio = Property::new(0.5_f32);
let splitter = Splitter::new(SplitAxis::Horizontal, 0.5);
let _binding = runtime.bind_layout(&splitter.ratio(), &ratio);
```

### Binding text and scroll offsets

`TextField::text()`, `TextEdit::text()`, `Autocomplete::query()`, and
`CommandPalette::query()` are `Property<String>`. Text changes the control's
size (or the filtered rows), so bind with `bind_layout`:

```rust
use drawen::{Property, TextField};

let name = Property::new(String::new());
let field = TextField::new();
let text = field.text();
let field = runtime.append_child(runtime.root(), field)?;
let _binding = runtime.bind_layout(&text, &name);
```

The editor stays the source of truth for the caret and undo history. A user edit
syncs the property from the editor and keeps the caret; an external write (a
binding copy or a direct `set`) is reconciled into the editor at the next layout
or event, which replaces the text and resets the caret. Single-line properties
strip line breaks; `TextEdit` keeps them and folds CRLF to LF. Every write is
normalized, so a binding copy cannot inject a newline into a single-line field.
Read the scalar with `field.text().get()`. The property's `changed()` fires on
every write; the widget's user `changed()` / `query_changed()` fires only on user
edits (and, for the combobox, commits).

Because user edits update the property, two opposite bindings give two-way text
state without a special API: the reverse binding carries each edit into the
state property, and the forward copy of the same value is deduplicated, so
reconciliation is a no-op and the caret survives:

```rust
let _to_field = runtime.bind_layout(&text, &name); // state -> field
let _to_name = runtime.bind(&name, &text); // field -> state
```

`ScrollView::offset()` and `ListView::offset()` are `Property<f32>` bound with
`bind_layout` (the offset positions the child or the visible rows). Their upper
bound depends on the measured content and viewport, which are only known at
layout, so the property normalizer keeps the offset finite and non-negative and
layout re-applies the dynamic upper bound, writing the clamped value back only
when it differs. A binding copy beyond the content is clamped on the next paint:

```rust
use drawen::{Property, ScrollView};

let scroll = Property::new(0.0_f32);
let view = ScrollView::vertical();
let offset = view.offset();
let view = runtime.append_child(runtime.root(), view)?;
let _binding = runtime.bind_layout(&offset, &scroll);
scroll.set(500.0); // clamped to content - viewport on the next layout
```

### Migrating from 0.3

The property conversion is a breaking-change batch for 0.4.0:

| 0.3 | 0.4 |
| --- | --- |
| `Checkbox::is_checked` / `Switch::is_checked` were `const fn` | still return `bool`, but are no longer `const` |
| `Slider::value() -> f32` (a `const fn`) | `Slider::value() -> Property<f32>`; read with `slider.value().get()` |
| `ProgressBar::progress() -> f32` (a `const fn`) | `ProgressBar::progress() -> Property<f32>`; read with `.progress().get()`; `new()` also loses `const` |
| `Expander::is_expanded` was a `const fn` | still returns `bool`, but is no longer `const`; `with_expanded` loses `const` |
| `TabBar::selected` / `SegmentedControl::selected` / `Dropdown::selected` were `const fn -> Option<usize>` | return `Property<Option<usize>>`; read with `.selected().get()` |
| `Spinner::progress() -> Option<f32>` (a `const fn`) | `Spinner::progress() -> Property<Option<f32>>`; read with `.progress().get()` |
| `RadioGroup::selected() -> Option<T>` | `RadioGroup::selected() -> Property<Option<T>>`; read with `.selected().get()` |
| `ImageViewer::zoom() -> f32` (a `const fn`) | `ImageViewer::zoom() -> Property<f32>`; read with `.zoom().get()` |
| `Splitter::ratio() -> f32` (a `const fn`) | `Splitter::ratio() -> Property<f32>`; read with `.ratio().get()` |
| `TextField::text()` / `TextEdit::text()` / `Autocomplete::query()` / `CommandPalette::query()` were `&str` | return `Property<String>`; read with `.text().get()` / `.query().get()` |
| `ScrollView::offset()` / `ListView::offset()` were `const fn -> f32` | return `Property<f32>`; read with `.offset().get()`; `ScrollView::new` / `vertical` / `horizontal` lose `const` |
| `Checkbox::set_checked` / `Switch::set_checked` emitted nothing | emit `checked().changed()` on a real change (still not `toggled()`) |
| `Slider::set_value` emitted nothing | emits `value().changed()` on a real change (still not the user `changed()`) |
| `ProgressBar::set_progress` / `Expander::set_expanded` / the selection `set_selected` emitted nothing | emit the property's `changed()` on a real change (still not the user signal) |
| `Spinner::set_progress` / `ImageViewer::set_zoom` / `ImageViewer::reset_view` emitted nothing | emit the property's `changed()` on a real change (still not the user signal) |
| `TextField::set_text` / `TextEdit::set_text` / `Autocomplete::set_query` / `ScrollView::set_offset` / `ListView::set_offset` emitted nothing | emit the property's `changed()` on a real change (still not the user signal); `CommandPalette` gains a `set_query` |

`Slider::minimum()` and `Slider::maximum()` stay `const fn -> f32`, as do
`ImageViewer::minimum_zoom()`, `ImageViewer::maximum_zoom()`, the viewer's pan
and checkerboard accessors, and `ListView::item_extent()`. The user-interaction
signals (`toggled()`, `changed()`, `activated()`, `zoom_changed()`, `resized()`,
`query_changed()`, `selected()`, `executed()`) are unchanged, so existing
subscription code keeps compiling; only `const` contexts and scalar reads
(`.value()`, `.progress()`, `.selected()`, `.zoom()`, `.ratio()`, `.text()`,
`.query()`, `.offset()`) need updating to `.get()`. Setter signatures are
unchanged. `RadioGroup::changed()` keeps its 0.3 semantics (fires on every
`set_selected`, silent on `clear`); only the `selected()` return type changed.
