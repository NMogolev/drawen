//! Anchored overlays: tooltips, dropdown menus, a modal dialog, a command
//! palette, and toast notifications.
//!
//! `Tooltip` wraps one anchor. `Popover` takes an anchor plus floating content,
//! and `Menu` supplies the rows. `SplitButton` joins a primary action to a
//! dropdown arrow that floats the same kind of menu. `Modal` hosts a centered
//! `Dialog` above a scrim and traps focus inside it; a `CommandPalette` uses the
//! same modal layer for a searchable command list. `ToastHost` paints transient
//! notifications above everything through the floating paint pass. The runtime
//! measures floating and modal content only while it is open, flips anchored
//! content when the window edge is too close, and paints both above the rest of
//! the tree.
//!
//! Run it with `cargo run -p drawen --example overlays`.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use drawen::{
    Border, Box, Button, Color, Column, CommandPalette, Constraints, Dialog, Insets, LayoutContext,
    LayoutResult, Menu, MenuEntry, Modal, OverlayPlacement, Padding, PaletteCommand, Popover,
    PopoverTrigger, Rect, Row, Size, SizedBox, SplitButton, Subscription, Text, Toast, ToastHost,
    ToastTone, Tooltip, UiEnvironment, UiRuntime, Widget, WidgetId,
    platform::{WindowAction, WindowApplication, WindowEvent, WindowOptions, WindowPresentation},
    render::{DisplayList, FontdueTextShaper, PaintContext, TextStyle},
};

fn main() -> Result<(), drawen_platform_winit::RunError> {
    drawen_platform_winit::run(OverlayDemo::new())
}

fn ink() -> Color {
    Color::rgba8(26, 30, 34, 255)
}

fn muted() -> Color {
    Color::rgba8(96, 105, 115, 255)
}

fn line() -> Color {
    Color::rgba8(214, 218, 222, 255)
}

/// Paints the shared status line without owning any state of its own.
struct StatusView {
    status: Rc<RefCell<String>>,
}

impl Widget for StatusView {
    fn layout(&mut self, _: &mut LayoutContext<'_>, constraints: Constraints) -> LayoutResult {
        LayoutResult::new(
            constraints.constrain(Size::new(420.0, 20.0).expect("status size is valid")),
        )
    }

    fn paint(&self, context: &mut PaintContext<'_>, _: &UiEnvironment, bounds: Rect) {
        let text = FontdueTextShaper::ubuntu_light().layout(
            &self.status.borrow(),
            &TextStyle::new(13.0, muted()),
            None,
        );
        context.draw_text(text, bounds.origin);
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

struct OverlayDemo {
    runtime: UiRuntime,
    size: Size,
    dirty: Rc<Cell<bool>>,
    pending_layout: Rc<Cell<bool>>,
    pending_focus: Rc<Cell<Option<WidgetId>>>,
    _subscriptions: Vec<Subscription>,
}

impl OverlayDemo {
    fn new() -> Self {
        let status = Rc::new(RefCell::new("Hover a button, or open a menu.".to_string()));
        let dirty = Rc::new(Cell::new(false));
        let pending_layout = Rc::new(Cell::new(false));
        let pending_focus = Rc::new(Cell::new(None));
        let mut subscriptions = Vec::new();

        let mut runtime = UiRuntime::new(Box::new().with_background(Color::WHITE));
        let root = runtime.root();

        let frame = Box::new().with_padding(Insets::new(28.0, 32.0, 28.0, 32.0));
        let frame = runtime
            .append_child(root, frame)
            .expect("root accepts one child");
        let column = runtime
            .append_child(frame, Column::new())
            .expect("frame accepts one child");

        append_text(&mut runtime, column, "Overlays", 26.0, ink());

        let subtitle = Padding::new(Insets::new(0.0, 0.0, 6.0, 0.0));
        let subtitle = runtime
            .append_child(column, subtitle)
            .expect("column accepts children");
        append_text(
            &mut runtime,
            subtitle,
            "Tooltips, dropdown menus, and pointer-anchored context menus.",
            13.0,
            muted(),
        );

        build_toolbar(&mut runtime, column);
        build_dropdown(
            &mut runtime,
            column,
            &status,
            &dirty,
            &pending_focus,
            &mut subscriptions,
        );
        build_split_button(
            &mut runtime,
            column,
            &status,
            &dirty,
            &pending_layout,
            &pending_focus,
            &mut subscriptions,
        );
        build_context_menu(
            &mut runtime,
            column,
            &status,
            &dirty,
            &pending_focus,
            &mut subscriptions,
        );
        build_dialog(
            &mut runtime,
            column,
            &status,
            &dirty,
            &pending_layout,
            &pending_focus,
            &mut subscriptions,
        );
        build_palette(
            &mut runtime,
            column,
            &status,
            &dirty,
            &pending_layout,
            &pending_focus,
            &mut subscriptions,
        );
        build_toasts(&mut runtime, column, &dirty, &mut subscriptions);

        let status_row = Padding::new(Insets::new(22.0, 0.0, 0.0, 0.0));
        let status_row = runtime
            .append_child(column, status_row)
            .expect("column accepts children");
        runtime
            .append_child(
                status_row,
                StatusView {
                    status: Rc::clone(&status),
                },
            )
            .expect("status row accepts one child");

        let size = Size::new(720.0, 540.0).expect("window size is valid");
        runtime
            .layout(Constraints::tight(size).expect("window constraints are valid"))
            .expect("layout succeeds");

        Self {
            runtime,
            size,
            dirty,
            pending_layout,
            pending_focus,
            _subscriptions: subscriptions,
        }
    }

    fn layout(&mut self, size: Size) {
        self.size = size;
        if let Ok(constraints) = Constraints::tight(size) {
            let _ = self.runtime.layout(constraints);
        }
    }
}

/// Adds a row of buttons, each wrapped in a delayed hover hint.
fn build_toolbar(runtime: &mut UiRuntime, column: WidgetId) {
    let toolbar = Padding::new(Insets::new(18.0, 0.0, 0.0, 0.0));
    let toolbar = runtime
        .append_child(column, toolbar)
        .expect("column accepts children");
    let row = runtime
        .append_child(toolbar, Row::new())
        .expect("toolbar accepts one child");
    for (index, (label, hint)) in [
        ("Refresh", "Reload the current view"),
        ("Share", "Copy a read-only link"),
        ("Delete", "Remove the selection"),
    ]
    .iter()
    .enumerate()
    {
        if index > 0 {
            runtime
                .append_child(row, SizedBox::new(Some(10.0), None))
                .expect("row accepts children");
        }
        let tooltip = Tooltip::new(*hint)
            .with_placement(OverlayPlacement::top())
            .with_delay(Duration::from_millis(350));
        let tooltip = runtime
            .append_child(row, tooltip)
            .expect("row accepts children");
        let button = runtime
            .append_child(tooltip, Button::new())
            .expect("tooltip accepts one anchor");
        append_text(runtime, button, label, 14.0, ink());
    }
}

/// Adds a button whose menu floats below it and moves focus into the menu.
fn build_dropdown(
    runtime: &mut UiRuntime,
    column: WidgetId,
    status: &Rc<RefCell<String>>,
    dirty: &Rc<Cell<bool>>,
    pending_focus: &Rc<Cell<Option<WidgetId>>>,
    subscriptions: &mut Vec<Subscription>,
) {
    let row = Padding::new(Insets::new(18.0, 0.0, 0.0, 0.0));
    let row = runtime
        .append_child(column, row)
        .expect("column accepts children");

    let popover = Popover::new()
        .with_trigger(PopoverTrigger::Primary)
        .with_placement(OverlayPlacement::bottom());
    let opened = popover.opened();
    let closed = popover.closed();
    let popover = runtime
        .append_child(row, popover)
        .expect("row accepts children");

    let anchor = runtime
        .append_child(popover, Button::new())
        .expect("popover accepts its anchor");
    append_text(runtime, anchor, "Open menu", 14.0, ink());

    let menu_widget = Menu::new([
        MenuEntry::item("Duplicate").with_shortcut("Ctrl+D"),
        MenuEntry::item("Rename").with_shortcut("F2"),
        MenuEntry::separator(),
        MenuEntry::item("Move to trash").with_shortcut("Del"),
    ]);
    let selected = menu_widget.selected();
    let menu = runtime
        .append_child(popover, menu_widget)
        .expect("popover accepts its content");

    subscriptions.push(selected.subscribe({
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        move |index| {
            let message = match *index {
                0 => "Duplicated the selection",
                1 => "Rename requested",
                _ => "Moved to trash",
            };
            *status.borrow_mut() = message.to_string();
            dirty.set(true);
        }
    }));
    subscriptions.push(opened.subscribe({
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        let pending_focus = Rc::clone(pending_focus);
        move |()| {
            *status.borrow_mut() = "Menu opened. Use the arrow keys and Enter.".to_string();
            pending_focus.set(Some(menu));
            dirty.set(true);
        }
    }));
    subscriptions.push(closed.subscribe({
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        move |()| {
            *status.borrow_mut() = "Menu closed.".to_string();
            dirty.set(true);
        }
    }));
}

/// Adds a split button: the left segment saves, the arrow floats a menu.
fn build_split_button(
    runtime: &mut UiRuntime,
    column: WidgetId,
    status: &Rc<RefCell<String>>,
    dirty: &Rc<Cell<bool>>,
    pending_layout: &Rc<Cell<bool>>,
    pending_focus: &Rc<Cell<Option<WidgetId>>>,
    subscriptions: &mut Vec<Subscription>,
) {
    let row = Padding::new(Insets::new(18.0, 0.0, 0.0, 0.0));
    let row = runtime
        .append_child(column, row)
        .expect("column accepts children");

    let split_widget = SplitButton::new("Save");
    let activated = split_widget.activated();
    let opened = split_widget.opened();
    let closed = split_widget.closed();
    let split = runtime
        .append_child(row, split_widget)
        .expect("row accepts children");

    let menu_widget = Menu::new([
        MenuEntry::item("Save as…").with_shortcut("Ctrl+Shift+S"),
        MenuEntry::item("Save all").with_shortcut("Ctrl+Alt+S"),
    ]);
    let selected = menu_widget.selected();
    let menu = runtime
        .append_child(split, menu_widget)
        .expect("split button accepts its menu");

    subscriptions.push(activated.subscribe({
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        move |()| {
            *status.borrow_mut() = "Saved the document".to_string();
            dirty.set(true);
        }
    }));
    subscriptions.push(selected.subscribe({
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        move |index| {
            let message = match *index {
                0 => "Save as… requested",
                _ => "Saved all documents",
            };
            *status.borrow_mut() = message.to_string();
            dirty.set(true);
        }
    }));
    subscriptions.push(opened.subscribe({
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        let pending_layout = Rc::clone(pending_layout);
        let pending_focus = Rc::clone(pending_focus);
        move |()| {
            *status.borrow_mut() = "Save menu opened. Use the arrow keys and Enter.".to_string();
            pending_layout.set(true);
            pending_focus.set(Some(menu));
            dirty.set(true);
        }
    }));
    subscriptions.push(closed.subscribe({
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        let pending_layout = Rc::clone(pending_layout);
        move |()| {
            *status.borrow_mut() = "Save menu closed.".to_string();
            pending_layout.set(true);
            dirty.set(true);
        }
    }));
}

/// Adds a panel that opens a menu at the secondary-click position.
fn build_context_menu(
    runtime: &mut UiRuntime,
    column: WidgetId,
    status: &Rc<RefCell<String>>,
    dirty: &Rc<Cell<bool>>,
    pending_focus: &Rc<Cell<Option<WidgetId>>>,
    subscriptions: &mut Vec<Subscription>,
) {
    let row = Padding::new(Insets::new(14.0, 0.0, 0.0, 0.0));
    let row = runtime
        .append_child(column, row)
        .expect("column accepts children");

    let popover = Popover::new()
        .with_trigger(PopoverTrigger::Secondary)
        .with_placement(OverlayPlacement::bottom());
    let opened = popover.opened();
    let closed = popover.closed();
    let popover = runtime
        .append_child(row, popover)
        .expect("row accepts children");

    let anchor = Box::new()
        .with_background(Color::rgba8(246, 247, 249, 255))
        .with_border(Border::new(1.0, line()))
        .with_corner_radius(6.0)
        .with_padding(Insets::new(10.0, 12.0, 10.0, 12.0));
    let anchor = runtime
        .append_child(popover, anchor)
        .expect("popover accepts its anchor");
    append_text(runtime, anchor, "Right-click this panel", 13.0, muted());

    let menu_widget = Menu::new([
        MenuEntry::item("Cut").with_shortcut("Ctrl+X"),
        MenuEntry::item("Copy").with_shortcut("Ctrl+C"),
        MenuEntry::item("Paste").with_shortcut("Ctrl+V"),
        MenuEntry::separator(),
        MenuEntry::item("Select all").with_shortcut("Ctrl+A"),
    ]);
    let selected = menu_widget.selected();
    let menu = runtime
        .append_child(popover, menu_widget)
        .expect("popover accepts its content");

    subscriptions.push(selected.subscribe({
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        move |index| {
            let message = match *index {
                0 => "Cut",
                1 => "Copied",
                2 => "Pasted",
                _ => "Selected all",
            };
            *status.borrow_mut() = message.to_string();
            dirty.set(true);
        }
    }));
    subscriptions.push(opened.subscribe({
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        let pending_focus = Rc::clone(pending_focus);
        move |()| {
            *status.borrow_mut() = "Context menu opened at the pointer.".to_string();
            pending_focus.set(Some(menu));
            dirty.set(true);
        }
    }));
    subscriptions.push(closed.subscribe({
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        move |()| {
            *status.borrow_mut() = "Context menu closed.".to_string();
            dirty.set(true);
        }
    }));
}

/// Adds a button that opens a centered modal dialog.
fn build_dialog(
    runtime: &mut UiRuntime,
    column: WidgetId,
    status: &Rc<RefCell<String>>,
    dirty: &Rc<Cell<bool>>,
    pending_layout: &Rc<Cell<bool>>,
    pending_focus: &Rc<Cell<Option<WidgetId>>>,
    subscriptions: &mut Vec<Subscription>,
) {
    let row = Padding::new(Insets::new(22.0, 0.0, 0.0, 0.0));
    let row = runtime
        .append_child(column, row)
        .expect("column accepts children");

    let trigger = Button::new();
    let activated = trigger.activated();
    let trigger = runtime
        .append_child(row, trigger)
        .expect("row accepts children");
    append_text(runtime, trigger, "Open dialog", 14.0, ink());

    let modal = Modal::new();
    let handle = modal.handle();
    let closed = modal.closed();
    let modal = runtime
        .append_child(column, modal)
        .expect("column accepts children");

    let dialog = runtime
        .append_child(modal, Dialog::new())
        .expect("modal accepts one child");
    let content = runtime
        .append_child(dialog, Column::new())
        .expect("dialog accepts one child");
    append_text(runtime, content, "Delete project?", 18.0, ink());

    let body = Padding::new(Insets::new(8.0, 0.0, 16.0, 0.0));
    let body = runtime
        .append_child(content, body)
        .expect("content accepts children");
    append_text(
        runtime,
        body,
        "This removes every file in the project. This cannot be undone.",
        13.0,
        muted(),
    );

    let actions = runtime
        .append_child(content, Row::new())
        .expect("content accepts children");
    let cancel = Button::new();
    let cancel_activated = cancel.activated();
    let cancel = runtime
        .append_child(actions, cancel)
        .expect("row accepts children");
    append_text(runtime, cancel, "Cancel", 14.0, ink());
    runtime
        .append_child(actions, SizedBox::new(Some(10.0), None))
        .expect("row accepts children");
    let confirm = Button::new();
    let confirm_activated = confirm.activated();
    let confirm = runtime
        .append_child(actions, confirm)
        .expect("row accepts children");
    append_text(runtime, confirm, "Delete", 14.0, ink());

    subscriptions.push(activated.subscribe({
        let handle = handle.clone();
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        let pending_layout = Rc::clone(pending_layout);
        let pending_focus = Rc::clone(pending_focus);
        move |()| {
            handle.open();
            pending_layout.set(true);
            pending_focus.set(Some(cancel));
            *status.borrow_mut() = "Dialog opened. Escape or Cancel closes it.".to_string();
            dirty.set(true);
        }
    }));
    subscriptions.push(cancel_activated.subscribe({
        let handle = handle.clone();
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        let pending_layout = Rc::clone(pending_layout);
        move |()| {
            handle.close();
            pending_layout.set(true);
            *status.borrow_mut() = "Dialog cancelled.".to_string();
            dirty.set(true);
        }
    }));
    subscriptions.push(confirm_activated.subscribe({
        let handle = handle.clone();
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        let pending_layout = Rc::clone(pending_layout);
        move |()| {
            handle.close();
            pending_layout.set(true);
            *status.borrow_mut() = "Project deleted.".to_string();
            dirty.set(true);
        }
    }));
    subscriptions.push(closed.subscribe({
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        let pending_layout = Rc::clone(pending_layout);
        move |()| {
            // Escape closes the modal inside the runtime, which already
            // invalidates layout. Setting the flag is harmless for that path
            // and required for programmatic closes.
            pending_layout.set(true);
            *status.borrow_mut() = "Dialog closed.".to_string();
            dirty.set(true);
        }
    }));
}

/// Adds a button that opens a modal command palette.
fn build_palette(
    runtime: &mut UiRuntime,
    column: WidgetId,
    status: &Rc<RefCell<String>>,
    dirty: &Rc<Cell<bool>>,
    pending_layout: &Rc<Cell<bool>>,
    pending_focus: &Rc<Cell<Option<WidgetId>>>,
    subscriptions: &mut Vec<Subscription>,
) {
    let row = Padding::new(Insets::new(14.0, 0.0, 0.0, 0.0));
    let row = runtime
        .append_child(column, row)
        .expect("column accepts children");

    let trigger = Button::new();
    let activated = trigger.activated();
    let trigger = runtime
        .append_child(row, trigger)
        .expect("row accepts children");
    append_text(runtime, trigger, "Open palette", 14.0, ink());

    let modal = Modal::new();
    let handle = modal.handle();
    let closed = modal.closed();
    let modal = runtime
        .append_child(column, modal)
        .expect("column accepts children");

    let commands = [
        PaletteCommand::new("Open file").with_hint("Ctrl+O"),
        PaletteCommand::new("Save all").with_hint("Ctrl+S"),
        PaletteCommand::new("Copy path").with_hint("Ctrl+Shift+C"),
        PaletteCommand::new("Toggle terminal").with_hint("Ctrl+`"),
        PaletteCommand::new("Format document").with_hint("Shift+Alt+F"),
    ];
    let labels: Vec<String> = commands
        .iter()
        .map(|command| command.label().to_string())
        .collect();
    let palette_widget = CommandPalette::new(commands);
    let executed = palette_widget.executed();
    let palette = runtime
        .append_child(modal, palette_widget)
        .expect("modal accepts one child");

    // Executing a command dismisses the modal, so remember that a command ran
    // and let the close handler keep the execution message on screen.
    let ran = Rc::new(Cell::new(false));
    subscriptions.push(activated.subscribe({
        let handle = handle.clone();
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        let pending_layout = Rc::clone(pending_layout);
        let pending_focus = Rc::clone(pending_focus);
        move |()| {
            handle.open();
            pending_layout.set(true);
            pending_focus.set(Some(palette));
            *status.borrow_mut() = "Palette opened. Type to filter, Enter to run.".to_string();
            dirty.set(true);
        }
    }));
    subscriptions.push(executed.subscribe({
        let labels = labels.clone();
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        let ran = Rc::clone(&ran);
        move |index| {
            *status.borrow_mut() = format!("Ran \"{}\"", labels[*index]);
            ran.set(true);
            dirty.set(true);
        }
    }));
    subscriptions.push(closed.subscribe({
        let status = Rc::clone(status);
        let dirty = Rc::clone(dirty);
        let pending_layout = Rc::clone(pending_layout);
        let ran = Rc::clone(&ran);
        move |()| {
            pending_layout.set(true);
            if !ran.replace(false) {
                *status.borrow_mut() = "Palette closed.".to_string();
            }
            dirty.set(true);
        }
    }));
}

/// Adds a button that shows transient toast notifications.
fn build_toasts(
    runtime: &mut UiRuntime,
    column: WidgetId,
    dirty: &Rc<Cell<bool>>,
    subscriptions: &mut Vec<Subscription>,
) {
    let row = Padding::new(Insets::new(14.0, 0.0, 0.0, 0.0));
    let row = runtime
        .append_child(column, row)
        .expect("column accepts children");

    let trigger = Button::new();
    let activated = trigger.activated();
    let trigger = runtime
        .append_child(row, trigger)
        .expect("row accepts children");
    append_text(runtime, trigger, "Show toast", 14.0, ink());

    // The host is zero-size and paints its cards above the whole tree, so it
    // can live anywhere in the layout without affecting it.
    let host = ToastHost::new();
    let handle = host.handle();
    runtime
        .append_child(column, host)
        .expect("column accepts children");

    subscriptions.push(activated.subscribe({
        let handle = handle.clone();
        let dirty = Rc::clone(dirty);
        move |()| {
            let _ = handle.show(Toast::new("Project saved").with_tone(ToastTone::Success));
            dirty.set(true);
        }
    }));
}

fn append_text(runtime: &mut UiRuntime, parent: WidgetId, text: &str, size: f32, color: Color) {
    runtime
        .append_child(parent, Text::styled(text, TextStyle::new(size, color)))
        .expect("parent accepts a text child");
}

impl WindowApplication for OverlayDemo {
    fn window_options(&self) -> WindowOptions {
        WindowOptions::new("Drawen overlays", self.size)
    }

    fn window_event(&mut self, event: WindowEvent) -> WindowAction {
        match event {
            WindowEvent::CloseRequested => WindowAction::Exit,
            WindowEvent::Resized(size) => {
                self.layout(size);
                WindowAction::RequestRedraw
            }
            WindowEvent::Input(event) => {
                let _ = self.runtime.dispatch_event(&event);
                if self.pending_layout.replace(false) {
                    self.runtime.invalidate_layout();
                    self.layout(self.size);
                }
                if let Some(widget) = self.pending_focus.take() {
                    let _ = self.runtime.focus_widget(widget);
                }
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
            WindowEvent::Visible(_) | WindowEvent::Minimized(_) | WindowEvent::Occluded(_) => {
                WindowAction::None
            }
            WindowEvent::Maximized(_) | WindowEvent::Fullscreen(_) => WindowAction::RequestRedraw,
            WindowEvent::RedrawRequested => WindowAction::None,
        }
    }

    fn presentation_changed(&mut self, presentation: WindowPresentation) -> WindowAction {
        self.runtime.set_presented(presentation.is_presented());
        WindowAction::None
    }

    fn redraw(&mut self) -> DisplayList {
        let mut display_list = DisplayList::new();
        let _ = self
            .runtime
            .paint(&mut PaintContext::new(&mut display_list));
        display_list
    }

    fn tick(&mut self, delta: Duration) -> bool {
        self.runtime.tick(delta)
    }

    fn next_frame_in(&self) -> Option<Duration> {
        self.runtime.next_frame_in()
    }
}
