use drawen_core::{
    Color, Constraints, InputEvent, Insets, Key, NamedKey, Point, PointerButton, PointerId, Rect,
    Size,
};
use drawen_render::{PaintContext, TextLayout, TextStyle};
use drawen_ui::{
    Accessibility, AccessibilityAction, AccessibilityActions, AccessibilityRole,
    AccessibilityState, ChildLayout, DismissPolicy, EventContext, EventPhase, EventStatus,
    LayoutContext, LayoutResult, OverlayAlign, OverlayOptions, OverlayPlacement, OverlaySide,
    PseudoClass, Signal, StyleTarget, UiEnvironment, Widget,
};

const DIVIDER_WIDTH: f32 = 1.0;
const ARROW_MARKER_WIDTH: f32 = 6.0;
const ARROW_MARKER_HEIGHT: f32 = 4.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Segment {
    Primary,
    Arrow,
}

/// A primary action button joined to a dropdown arrow.
///
/// The left segment runs the default action and emits [`Self::activated`].
/// The right segment toggles a floating menu supplied as the widget's single
/// child, usually a [`crate::Menu`]:
///
/// ```text
/// split-button
/// └── menu            (floating content, measured only while open)
/// ```
///
/// ```rust,ignore
/// let split = SplitButton::new("Save");
/// let activated = split.activated();
/// let opened = split.opened();
/// let split = runtime.append_child(parent, split)?;
/// let menu = Menu::new([MenuEntry::item("Save as…"), MenuEntry::item("Save all")]);
/// let selected = menu.selected();
/// let menu = runtime.append_child(split, menu)?;
/// // On `opened`, invalidate layout, then focus the menu after the next pass.
/// ```
///
/// The menu floats through the runtime overlay layer, anchored below the
/// button and aligned to its trailing edge. Light dismissal is on by default:
/// an outside primary click, Escape, or window focus loss closes the menu, and
/// selecting a menu row dismisses it through the same path. Enter and Space
/// activate the primary segment; `ArrowDown` opens the menu.
pub struct SplitButton {
    label: String,
    open: bool,
    has_content: bool,
    pressed: Option<Segment>,
    pressed_pointer: Option<PointerId>,
    hover: Option<Segment>,
    activated: Signal<()>,
    opened: Signal<()>,
    closed: Signal<()>,
    size: Size,
    arrow_x: f32,
    padding: Insets,
    label_layout: Option<TextLayout>,
    idle_color: Color,
    hover_color: Color,
    pressed_color: Color,
    border_color: Color,
    text_color: Color,
    radius: f32,
}

impl SplitButton {
    /// Creates a split button whose primary segment shows `label`.
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            open: false,
            has_content: false,
            pressed: None,
            pressed_pointer: None,
            hover: None,
            activated: Signal::new(),
            opened: Signal::new(),
            closed: Signal::new(),
            size: Size::ZERO,
            arrow_x: 0.0,
            padding: Insets::ZERO,
            label_layout: None,
            idle_color: Color::WHITE,
            hover_color: Color::WHITE,
            pressed_color: Color::WHITE,
            border_color: Color::TRANSPARENT,
            text_color: Color::BLACK,
            radius: 0.0,
        }
    }

    /// Returns the primary segment's label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns whether the floating menu is open.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// Returns a signal emitted after a completed primary-segment click or
    /// keyboard activation.
    #[must_use]
    pub fn activated(&self) -> Signal<()> {
        self.activated.clone()
    }

    /// Returns a signal emitted whenever the menu opens.
    #[must_use]
    pub fn opened(&self) -> Signal<()> {
        self.opened.clone()
    }

    /// Returns a signal emitted whenever the menu closes.
    #[must_use]
    pub fn closed(&self) -> Signal<()> {
        self.closed.clone()
    }

    /// Opens or closes the menu and emits the matching signal.
    ///
    /// This does not request a layout pass; pair it with
    /// [`drawen_ui::UiRuntime::invalidate_layout`] when called outside event
    /// handling.
    pub fn set_open(&mut self, open: bool) {
        if self.open == open {
            return;
        }
        self.open = open;
        if open {
            self.opened.emit(&());
        } else {
            self.closed.emit(&());
        }
    }

    fn segment_at(&self, point: Point) -> Option<Segment> {
        if !self.size.contains(point) {
            return None;
        }
        Some(if point.x >= self.arrow_x {
            Segment::Arrow
        } else {
            Segment::Primary
        })
    }

    fn segment_color(&self, segment: Segment) -> Color {
        if self.pressed == Some(segment) {
            self.pressed_color
        } else if self.hover == Some(segment) {
            self.hover_color
        } else {
            self.idle_color
        }
    }

    fn toggle_menu(&mut self, context: &mut EventContext<'_>) {
        if !self.has_content {
            return;
        }
        self.set_open(!self.open);
        context.request_layout();
    }
}

impl Widget for SplitButton {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let style = context.style();
        let environment = context.environment();
        let theme = environment.theme();
        let font = environment.font_for(&style);
        self.padding = style.padding.unwrap_or_else(|| theme.control_padding());
        self.text_color = style.foreground_or(theme.foreground());
        self.idle_color = style
            .background
            .unwrap_or_else(|| theme.control_background());
        self.hover_color = theme.control_hover_background();
        self.pressed_color = theme.control_pressed_background();
        self.border_color = style
            .border
            .map_or_else(|| theme.border(), |border| border.color);
        self.radius = style.corner_radius.unwrap_or_else(|| theme.corner_radius());

        let label_layout = environment.text_shaper().layout(
            &self.label,
            &TextStyle::new_with_font(font.clone(), self.text_color),
            None,
        );
        let font_size = font.pixel_size(environment.logical_dpi());
        let height =
            (label_layout.size().height().max(font_size) + self.padding.vertical()).max(0.0);
        let primary_width = label_layout.size().width() + self.padding.horizontal();
        let arrow_width = height;
        self.label_layout = Some(label_layout);
        self.size =
            constraints.constrain(size(primary_width + DIVIDER_WIDTH + arrow_width, height));
        self.arrow_x = primary_width + DIVIDER_WIDTH;
        self.has_content = context.child_count() > 0;

        let mut children = Vec::new();
        if self.open && self.has_content {
            let max = context
                .viewport()
                .map_or_else(|| constraints.max(), |viewport| viewport.size);
            let (panel, _, origin, _side) = context
                .layout_child_overlay(
                    0,
                    Constraints::loose(max),
                    Rect::new(Point::ZERO, self.size),
                    OverlayOptions::new(OverlayPlacement::new(
                        OverlaySide::Bottom,
                        OverlayAlign::End,
                    ))
                    .with_dismiss(DismissPolicy::Light),
                )
                .expect("runtime child index is valid");
            children.push(ChildLayout::new(panel, origin));
        }
        LayoutResult::with_children(self.size, children)
    }

    fn style_target(&self) -> StyleTarget {
        let target = StyleTarget::new("split-button");
        if self.pressed.is_some() {
            target.with_state(PseudoClass::Pressed)
        } else {
            target
        }
    }

    fn paint(&self, context: &mut PaintContext<'_>, _: &UiEnvironment, bounds: Rect) {
        if self.size.width() <= 0.0 || self.size.height() <= 0.0 {
            return;
        }
        let surface = Rect::new(bounds.origin, self.size);
        let primary = self.segment_color(Segment::Primary);
        let arrow = self.segment_color(Segment::Arrow);
        context.fill_rounded_rect(surface, self.radius, primary);
        let arrow_rect = Rect::new(
            Point::new(surface.origin.x + self.arrow_x, surface.origin.y),
            size(
                (self.size.width() - self.arrow_x).max(0.0),
                self.size.height(),
            ),
        );
        context.fill_rounded_rect(arrow_rect, self.radius, arrow);
        // Square off the arrow segment's leading corners so the two segments
        // read as one joined control.
        context.fill_rect(
            Rect::new(
                arrow_rect.origin,
                size(
                    (arrow_rect.size.width() / 2.0).min(self.radius),
                    arrow_rect.size.height(),
                ),
            ),
            arrow,
        );
        context.fill_rect(
            Rect::new(
                Point::new(
                    surface.origin.x + self.arrow_x - DIVIDER_WIDTH,
                    surface.origin.y,
                ),
                size(DIVIDER_WIDTH, self.size.height()),
            ),
            self.border_color,
        );
        context.stroke_rounded_rect(surface, self.radius, 1.0, self.border_color);

        if let Some(text) = &self.label_layout {
            let text_height = text.size().height();
            context.draw_text(
                text.clone(),
                Point::new(
                    surface.origin.x + self.padding.left,
                    surface.origin.y + (self.size.height() - text_height) / 2.0,
                ),
            );
        }
        let marker = Rect::new(
            Point::new(
                arrow_rect.origin.x + (arrow_rect.size.width() - ARROW_MARKER_WIDTH) / 2.0,
                arrow_rect.origin.y + (arrow_rect.size.height() - ARROW_MARKER_HEIGHT) / 2.0,
            ),
            size(ARROW_MARKER_WIDTH, ARROW_MARKER_HEIGHT),
        );
        context.fill_rect(marker, self.text_color);
    }

    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        if context.phase() != EventPhase::Target {
            return EventStatus::Ignored;
        }
        match event {
            InputEvent::PointerDown(pointer) if pointer.button == Some(PointerButton::Primary) => {
                self.pressed = self.segment_at(pointer.position);
                self.pressed_pointer = Some(pointer.pointer_id);
                context.capture_pointer(pointer.pointer_id);
                context.request_focus();
                context.request_redraw();
                EventStatus::Handled
            }
            InputEvent::PointerUp(pointer)
                if pointer.button == Some(PointerButton::Primary)
                    && self.pressed_pointer == Some(pointer.pointer_id) =>
            {
                let released = self.segment_at(pointer.position);
                let activate = self.pressed.is_some() && self.pressed == released;
                let segment = self.pressed.take();
                self.pressed_pointer = None;
                context.release_pointer(pointer.pointer_id);
                context.request_redraw();
                if activate {
                    match segment {
                        Some(Segment::Primary) => self.activated.emit(&()),
                        Some(Segment::Arrow) => self.toggle_menu(context),
                        None => {}
                    }
                }
                EventStatus::Handled
            }
            InputEvent::PointerCancel(pointer)
                if self.pressed_pointer == Some(pointer.pointer_id) =>
            {
                self.pressed = None;
                self.pressed_pointer = None;
                context.release_pointer(pointer.pointer_id);
                context.request_redraw();
                EventStatus::Handled
            }
            InputEvent::PointerMove(pointer) => {
                if self.pressed.is_none() {
                    let hover = self.segment_at(pointer.position);
                    if hover != self.hover {
                        self.hover = hover;
                        context.request_redraw();
                    }
                }
                EventStatus::Ignored
            }
            InputEvent::PointerEnter(pointer) => {
                self.hover = self.segment_at(pointer.position);
                context.request_redraw();
                EventStatus::Handled
            }
            InputEvent::PointerLeave(_) => {
                if self.pressed.is_none() {
                    self.hover = None;
                    context.request_redraw();
                }
                EventStatus::Handled
            }
            InputEvent::KeyDown(key) if !key.repeat => match key.key {
                Key::Named(NamedKey::Enter | NamedKey::Space) => {
                    self.activated.emit(&());
                    context.request_redraw();
                    EventStatus::Handled
                }
                Key::Named(NamedKey::ArrowDown) if !self.open && self.has_content => {
                    self.set_open(true);
                    context.request_layout();
                    EventStatus::Handled
                }
                _ => EventStatus::Ignored,
            },
            _ => EventStatus::Ignored,
        }
    }

    fn dismiss_overlay(&mut self) -> bool {
        if !self.open {
            return false;
        }
        self.set_open(false);
        true
    }

    fn accepts_focus(&self) -> bool {
        true
    }

    fn accessibility(&self) -> Accessibility {
        let mut states = AccessibilityState::FOCUSABLE;
        if self.open {
            states |= AccessibilityState::EXPANDED;
        }
        Accessibility::new(AccessibilityRole::Button)
            .with_label(self.label.clone())
            .with_states(states)
            .with_actions(
                AccessibilityActions::FOCUS
                    | AccessibilityActions::CLICK
                    | AccessibilityActions::EXPAND
                    | AccessibilityActions::COLLAPSE,
            )
    }

    fn accessibility_action(&mut self, action: &AccessibilityAction) -> bool {
        match action {
            AccessibilityAction::Click => {
                self.activated.emit(&());
                true
            }
            AccessibilityAction::Expand if self.has_content => {
                self.set_open(true);
                true
            }
            AccessibilityAction::Collapse => {
                self.set_open(false);
                true
            }
            _ => false,
        }
    }
}

fn size(width: f32, height: f32) -> Size {
    Size::new(width, height).expect("split button sizes are non-negative and not NaN")
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use drawen_core::{
        Constraints, InputEvent, Key, KeyCode, Modifiers, NamedKey, Point, PointerButtons,
        PointerEvent, PointerId, Size,
    };
    use drawen_ui::UiRuntime;

    use super::SplitButton;
    use crate::{Menu, MenuEntry};

    fn size(width: f32, height: f32) -> Size {
        Size::new(width, height).expect("valid test size")
    }

    fn pointer(position: Point, up: bool) -> InputEvent {
        let event = PointerEvent {
            pointer_id: PointerId(1),
            position,
            button: Some(drawen_core::PointerButton::Primary),
            buttons: if up {
                PointerButtons::NONE
            } else {
                PointerButtons::PRIMARY
            },
            modifiers: Modifiers::NONE,
        };
        if up {
            InputEvent::PointerUp(event)
        } else {
            InputEvent::PointerDown(event)
        }
    }

    fn key(key: NamedKey) -> InputEvent {
        InputEvent::KeyDown(drawen_core::KeyEvent {
            key: Key::Named(key),
            code: KeyCode::Unidentified,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
    }

    fn sample() -> (SplitButton, Menu) {
        (
            SplitButton::new("Save"),
            Menu::new([MenuEntry::item("Save as…"), MenuEntry::item("Save all")]),
        )
    }

    fn arrow_point(runtime: &UiRuntime) -> Point {
        let bounds = runtime.bounds(runtime.root()).expect("button has bounds");
        Point::new(
            bounds.origin.x + bounds.size.width() - 5.0,
            bounds.origin.y + bounds.size.height() / 2.0,
        )
    }

    #[test]
    fn primary_click_activates_without_opening_the_menu() {
        let (split, menu) = sample();
        let activations = Rc::new(Cell::new(0));
        let _subscription = split.activated().subscribe({
            let activations = Rc::clone(&activations);
            move |()| activations.set(activations.get() + 1)
        });
        let mut runtime = UiRuntime::new(split);
        runtime
            .append_child(runtime.root(), menu)
            .expect("split button accepts one child");
        runtime
            .layout(Constraints::loose(size(400.0, 300.0)))
            .expect("layout succeeds");

        let point = Point::new(10.0, 10.0);
        let _ = runtime.dispatch_event(&pointer(point, false));
        let _ = runtime.dispatch_event(&pointer(point, true));
        assert_eq!(activations.get(), 1);
        assert_eq!(runtime.focused_widget(), Some(runtime.root()));
    }

    #[test]
    fn arrow_click_toggles_the_floating_menu() {
        let (split, menu) = sample();
        let opened = Rc::new(Cell::new(0));
        let closed = Rc::new(Cell::new(0));
        let _opened = split.opened().subscribe({
            let opened = Rc::clone(&opened);
            move |()| opened.set(opened.get() + 1)
        });
        let _closed = split.closed().subscribe({
            let closed = Rc::clone(&closed);
            move |()| closed.set(closed.get() + 1)
        });
        let mut runtime = UiRuntime::new(split);
        let menu = runtime
            .append_child(runtime.root(), menu)
            .expect("split button accepts one child");
        runtime
            .layout(Constraints::loose(size(400.0, 300.0)))
            .expect("layout succeeds");
        assert_eq!(
            runtime.bounds(menu).expect("menu node exists").size,
            Size::ZERO
        );

        let point = arrow_point(&runtime);
        let _ = runtime.dispatch_event(&pointer(point, false));
        let _ = runtime.dispatch_event(&pointer(point, true));
        assert_eq!(opened.get(), 1);
        runtime
            .layout(Constraints::loose(size(400.0, 300.0)))
            .expect("open layout succeeds");
        let menu_bounds = runtime.bounds(menu).expect("menu is laid out");
        let button_bounds = runtime.bounds(runtime.root()).expect("button has bounds");
        assert!(menu_bounds.origin.y >= button_bounds.bottom());

        let _ = runtime.dispatch_event(&pointer(point, false));
        let _ = runtime.dispatch_event(&pointer(point, true));
        assert_eq!(closed.get(), 1);
    }

    #[test]
    fn selecting_a_menu_row_dismisses_the_menu() {
        let (split, menu) = sample();
        let selected = Rc::new(std::cell::RefCell::new(Vec::new()));
        let _subscription = menu.selected().subscribe({
            let selected = Rc::clone(&selected);
            move |index| selected.borrow_mut().push(*index)
        });
        let closed = Rc::new(Cell::new(0));
        let _closed = split.closed().subscribe({
            let closed = Rc::clone(&closed);
            move |()| closed.set(closed.get() + 1)
        });
        let mut runtime = UiRuntime::new(split);
        let menu = runtime
            .append_child(runtime.root(), menu)
            .expect("split button accepts one child");
        runtime
            .layout(Constraints::loose(size(400.0, 300.0)))
            .expect("layout succeeds");

        let point = arrow_point(&runtime);
        let _ = runtime.dispatch_event(&pointer(point, false));
        let _ = runtime.dispatch_event(&pointer(point, true));
        runtime
            .layout(Constraints::loose(size(400.0, 300.0)))
            .expect("open layout succeeds");

        let bounds = runtime.bounds(menu).expect("menu is laid out");
        let row = Point::new(bounds.origin.x + 20.0, bounds.origin.y + 19.0);
        let _ = runtime.dispatch_event(&pointer(row, false));
        let _ = runtime.dispatch_event(&pointer(row, true));
        assert_eq!(*selected.borrow(), vec![0]);
        assert_eq!(closed.get(), 1);
    }

    #[test]
    fn keyboard_activates_the_primary_segment_and_opens_with_arrow_down() {
        let (split, menu) = sample();
        let activations = Rc::new(Cell::new(0));
        let _subscription = split.activated().subscribe({
            let activations = Rc::clone(&activations);
            move |()| activations.set(activations.get() + 1)
        });
        let opened = Rc::new(Cell::new(0));
        let _opened = split.opened().subscribe({
            let opened = Rc::clone(&opened);
            move |()| opened.set(opened.get() + 1)
        });
        let mut runtime = UiRuntime::new(split);
        runtime
            .append_child(runtime.root(), menu)
            .expect("split button accepts one child");
        runtime
            .layout(Constraints::loose(size(400.0, 300.0)))
            .expect("layout succeeds");
        runtime
            .focus_widget(runtime.root())
            .expect("split button is focusable");

        let _ = runtime.dispatch_event(&key(NamedKey::Enter));
        assert_eq!(activations.get(), 1);
        assert_eq!(opened.get(), 0);
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowDown));
        assert_eq!(opened.get(), 1);
        assert_eq!(activations.get(), 1);
    }

    #[test]
    fn releasing_outside_the_pressed_segment_does_nothing() {
        let (split, menu) = sample();
        let activations = Rc::new(Cell::new(0));
        let _subscription = split.activated().subscribe({
            let activations = Rc::clone(&activations);
            move |()| activations.set(activations.get() + 1)
        });
        let opened = Rc::new(Cell::new(0));
        let _opened = split.opened().subscribe({
            let opened = Rc::clone(&opened);
            move |()| opened.set(opened.get() + 1)
        });
        let mut runtime = UiRuntime::new(split);
        runtime
            .append_child(runtime.root(), menu)
            .expect("split button accepts one child");
        runtime
            .layout(Constraints::loose(size(400.0, 300.0)))
            .expect("layout succeeds");

        let _ = runtime.dispatch_event(&pointer(Point::new(10.0, 10.0), false));
        let _ = runtime.dispatch_event(&pointer(Point::new(300.0, 200.0), true));
        assert_eq!(activations.get(), 0);
        assert_eq!(opened.get(), 0);
    }
}
