use std::time::{Duration, Instant};

use drawen_core::{
    Color, Constraints, FocusChanged, InputEvent, Insets, Key, Modifiers, NamedKey, Point,
    PointerButton, PointerId, Rect, Size, WheelDelta,
};
use drawen_render::{PaintContext, TextLayout, TextStyle};
use drawen_ui::{
    Accessibility, AccessibilityAction, AccessibilityActions, AccessibilityRole,
    AccessibilityState, DssStyle, EventContext, EventPhase, EventStatus, LayoutContext,
    LayoutResult, Property, Signal, StyleTarget, UiEnvironment, Widget,
};

use crate::text_editor::TextEditor;

const DEFAULT_WIDTH: f32 = 120.0;
const HORIZONTAL_PADDING: f32 = 8.0;
const VERTICAL_PADDING: f32 = 6.0;
const BORDER_WIDTH: f32 = 1.0;
const STEPPER_WIDTH: f32 = 20.0;
const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(400);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stepper {
    Up,
    Down,
}

/// A single-line numeric entry control with increment and decrement steppers.
///
/// The control edits text through the same grapheme-aware editor as
/// [`crate::TextField`], but only accepts numeric characters and commits a
/// parsed, clamped number. The value is a [`Property<f64>`]: bind application
/// state with `runtime.bind(&input.value(), &state.value)` and read the scalar
/// with `input.value().get()`. [`Self::changed`] fires only when a user edit,
/// step, or commit changes the committed number.
///
/// `ArrowUp` and `ArrowDown` step by [`Self::step`]; the stacked arrows on the right
/// do the same with the pointer. Enter commits the typed text, Escape reverts to
/// the last committed value, and losing focus commits. Invalid or out-of-range
/// text is clamped to `[minimum, maximum]` on commit; unparseable text reverts.
pub struct NumberInput {
    editor: TextEditor,
    value: Property<f64>,
    committed: f64,
    changed: Signal<f64>,
    minimum: f64,
    maximum: f64,
    step: f64,
    focused: bool,
    text_layout: TextLayout,
    text_style: TextStyle,
    padding: Insets,
    viewport: Size,
    scroll_x: f32,
    dragging: bool,
    pressed_stepper: Option<Stepper>,
    pressed_pointer: Option<PointerId>,
    last_click: Option<(Instant, Point)>,
}

impl NumberInput {
    /// Creates a number input valued at zero with an unbounded range and a step
    /// of one.
    #[must_use]
    pub fn new() -> Self {
        Self::build(0.0, f64::NEG_INFINITY, f64::INFINITY, 1.0)
    }

    fn build(value: f64, minimum: f64, maximum: f64, step: f64) -> Self {
        let minimum = if minimum.is_nan() {
            f64::NEG_INFINITY
        } else {
            minimum
        };
        let maximum = if maximum.is_nan() {
            f64::INFINITY
        } else {
            maximum
        };
        let (minimum, maximum) = if minimum <= maximum {
            (minimum, maximum)
        } else {
            (maximum, minimum)
        };
        let step = if step.is_finite() && step > 0.0 {
            step
        } else {
            1.0
        };
        let value = normalize_value(value, minimum, maximum);
        let editor = TextEditor::with_text(false, format_number(value));
        let property = Property::normalized(value, move |candidate| {
            normalize_value(candidate, minimum, maximum)
        });
        Self {
            editor,
            value: property,
            committed: value,
            changed: Signal::new(),
            minimum,
            maximum,
            step,
            focused: false,
            text_layout: empty_layout(),
            text_style: TextStyle::new(1.0, Color::BLACK),
            padding: default_padding(),
            viewport: Size::ZERO,
            scroll_x: 0.0,
            dragging: false,
            pressed_stepper: None,
            pressed_pointer: None,
            last_click: None,
        }
    }

    /// Returns this input with the value clamped to `[minimum, maximum]`.
    ///
    /// A reversed range is swapped and NaN bounds become infinite. The current
    /// value is re-clamped into the new range.
    #[must_use]
    pub fn with_range(self, minimum: f64, maximum: f64) -> Self {
        let value = self.value.get();
        Self::build(value, minimum, maximum, self.step)
    }

    /// Returns this input with an explicit positive step.
    #[must_use]
    pub fn with_step(mut self, step: f64) -> Self {
        self.step = if step.is_finite() && step > 0.0 {
            step
        } else {
            1.0
        };
        self
    }

    /// Returns this input with `value` as its initial number, clamped to the range.
    #[must_use]
    pub fn with_value(self, value: f64) -> Self {
        Self::build(value, self.minimum, self.maximum, self.step)
    }

    /// Returns the value property shared with this input.
    ///
    /// Every write is clamped to the range, including `UiRuntime::bind` copies.
    /// The property's `changed` signal fires on every write; [`Self::changed`]
    /// fires only on user interaction. Read the scalar with `input.value().get()`.
    pub fn value(&self) -> Property<f64> {
        self.value.clone()
    }

    /// Returns a signal emitted when a user edit, step, or commit changes the
    /// committed number.
    #[must_use]
    pub fn changed(&self) -> Signal<f64> {
        self.changed.clone()
    }

    /// Returns the inclusive lower bound.
    #[must_use]
    pub const fn minimum(&self) -> f64 {
        self.minimum
    }

    /// Returns the inclusive upper bound.
    #[must_use]
    pub const fn maximum(&self) -> f64 {
        self.maximum
    }

    /// Returns the step used by the arrows and stepper buttons.
    #[must_use]
    pub const fn step(&self) -> f64 {
        self.step
    }

    /// Replaces the committed number and the displayed text.
    ///
    /// Emits `value().changed()` when the clamped number differs, but not
    /// [`Self::changed`]; use that signal for user-initiated changes.
    pub fn set_value(&mut self, value: f64) {
        let value = normalize_value(value, self.minimum, self.maximum);
        self.committed = value;
        self.editor.set_text(format_number(value));
        self.value.set(value);
        self.scroll_x = 0.0;
    }

    /// Applies an external property write (a binding copy or direct `set`) to the
    /// editor. User typing leaves the property equal to `committed`, so this is a
    /// no-op mid-edit and preserves the in-progress text and caret.
    fn reconcile_value(&mut self) {
        let stored = self.value.get();
        if (stored - self.committed).abs() > f64::EPSILON {
            self.committed = stored;
            self.editor.set_text(format_number(stored));
            self.scroll_x = 0.0;
        }
    }

    fn text_width(&self) -> f32 {
        (self.viewport.width() - STEPPER_WIDTH - self.padding.horizontal()).max(1.0)
    }

    fn relayout(&mut self, environment: &UiEnvironment) {
        self.text_layout =
            environment
                .text_shaper()
                .layout(self.editor.text(), &self.text_style, None);
    }

    fn ensure_caret_visible(&mut self) {
        let caret = self.text_layout.caret_rect(self.editor.focus(), 1.0).origin.x;
        let width = self.text_width();
        if caret < self.scroll_x {
            self.scroll_x = caret;
        } else if caret + 1.0 > self.scroll_x + width {
            self.scroll_x = caret + 1.0 - width;
        }
        self.scroll_x = self.scroll_x.max(0.0);
    }

    fn hit_test_caret(&self, position: Point) -> usize {
        self.text_layout.byte_at_point(Point::new(
            position.x - self.padding.left + self.scroll_x,
            position.y - self.padding.top,
        ))
    }

    fn stepper_at(&self, position: Point) -> Option<Stepper> {
        let left = self.viewport.width() - STEPPER_WIDTH;
        if position.x < left
            || position.x > self.viewport.width()
            || position.y < 0.0
            || position.y > self.viewport.height()
        {
            return None;
        }
        Some(if position.y < self.viewport.height() / 2.0 {
            Stepper::Up
        } else {
            Stepper::Down
        })
    }

    /// Applies a text-only edit (typing, deletion, clipboard, undo). The committed
    /// number is unchanged until the next commit or step.
    fn apply_text_edit(
        &mut self,
        context: &mut EventContext<'_>,
        edit: impl FnOnce(&mut TextEditor) -> bool,
    ) -> bool {
        let before = self.editor.text().to_owned();
        if !edit(&mut self.editor) {
            return false;
        }
        if before == self.editor.text() {
            context.request_redraw();
        } else {
            context.request_layout();
        }
        true
    }

    /// Commits `value` as the new number, reformatting the text and notifying
    /// user listeners when it differs from the previous committed number.
    fn apply_value(&mut self, value: f64, context: &mut EventContext<'_>, user: bool) {
        let before = self.committed;
        let value = normalize_value(value, self.minimum, self.maximum);
        self.committed = value;
        self.editor.set_text(format_number(value));
        self.value.set(value);
        self.scroll_x = 0.0;
        if user && (value - before).abs() > f64::EPSILON {
            self.changed.emit(&value);
        }
        context.request_layout();
    }

    /// Parses the in-progress text and commits it, reverting to the last
    /// committed number when the text is not a finite number.
    fn commit(&mut self, context: &mut EventContext<'_>) {
        let parsed = self
            .editor
            .text()
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite());
        let value = parsed.unwrap_or(self.committed);
        self.apply_value(value, context, true);
    }

    /// Steps from the parsed in-progress text (or the committed number) by
    /// `direction * step`, clamped to the range.
    fn step_by(&mut self, direction: f64, context: &mut EventContext<'_>) {
        let base = self
            .editor
            .text()
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .unwrap_or(self.committed);
        self.apply_value(base + direction * self.step, context, true);
    }

    fn copy(&self, context: &EventContext<'_>) -> bool {
        let Some(selection) = self.editor.selected_text() else {
            return false;
        };
        context.write_clipboard(selection);
        true
    }

    fn cut(&mut self, context: &mut EventContext<'_>) -> bool {
        if !self.copy(context) {
            return false;
        }
        self.apply_text_edit(context, TextEditor::delete_selection)
    }

    fn paste(&mut self, context: &mut EventContext<'_>) -> bool {
        let Some(text) = context.read_clipboard() else {
            return false;
        };
        let filtered = filter_numeric(&text);
        self.apply_text_edit(context, |editor| editor.insert(&filtered))
    }
}

impl Default for NumberInput {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for NumberInput {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        self.reconcile_value();
        let style = context.style();
        let environment = context.environment();
        let font = environment.font_for(&style);
        self.text_style = TextStyle::new_with_font(
            font.clone(),
            style.foreground_or(environment.theme().foreground()),
        );
        self.padding = style.padding_or(default_padding());
        self.relayout(environment);
        let font_size = font.pixel_size(environment.logical_dpi());
        let width = (self.text_layout.size().width()
            + self.padding.horizontal()
            + STEPPER_WIDTH)
            .max(DEFAULT_WIDTH);
        let height = (self.text_layout.size().height() + self.padding.vertical())
            .max(font_size + self.padding.vertical());
        let size = constraints.constrain(size(width, height));
        self.viewport = size;
        self.ensure_caret_visible();
        LayoutResult::new(size)
    }

    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("number-input")
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        self.paint_styled(context, environment, bounds, &DssStyle::default());
    }

    fn paint_styled(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        bounds: Rect,
        style: &DssStyle,
    ) {
        let theme = environment.theme();
        let radius = style.corner_radius_or(theme.corner_radius());
        let foreground = style.foreground_or(theme.foreground());
        let accent = style.accent_or(theme.accent());
        let transition = context.transition();
        context.fill_rounded_rect(bounds, radius, style.background_or(theme.surface()));
        let border = style.border.map_or_else(
            || transition.focus_color(theme.border(), accent),
            |border| border.color,
        );
        let border_width = style.border.map_or(BORDER_WIDTH, |border| border.width);
        context.stroke_rounded_rect(bounds, radius, border_width, border);

        let text_area = Rect::new(
            Point::new(bounds.origin.x + self.padding.left, bounds.origin.y + self.padding.top),
            size(
                self.text_width(),
                (self.viewport.height() - self.padding.vertical()).max(1.0),
            ),
        );
        let origin = Point::new(text_area.origin.x - self.scroll_x, text_area.origin.y);
        context.with_clip(text_area, |context| {
            let layout = if self.text_layout.color() == foreground {
                self.text_layout.clone()
            } else {
                self.text_layout.clone().with_color(foreground)
            };
            if self.editor.has_selection() {
                for rect in layout.selection_rects(self.editor.selection()) {
                    context.fill_rect(
                        Rect::new(
                            Point::new(rect.origin.x + origin.x, rect.origin.y + origin.y),
                            rect.size,
                        ),
                        theme.selection(),
                    );
                }
            }
            context.draw_text(layout.clone(), origin);
            if self.focused {
                let caret_rect = layout.caret_rect(self.editor.focus(), 1.0);
                context.fill_rect(
                    Rect::new(
                        Point::new(caret_rect.origin.x + origin.x, text_area.origin.y),
                        size(1.0, text_area.size.height().max(caret_rect.size.height())),
                    ),
                    accent,
                );
            }
        });

        // Stepper column: a divider, two stacked buttons, and their arrows.
        let stepper_x = bounds.origin.x + self.viewport.width() - STEPPER_WIDTH;
        let mid_y = bounds.origin.y + self.viewport.height() / 2.0;
        context.fill_rect(
            Rect::new(
                Point::new(stepper_x, bounds.origin.y),
                size(BORDER_WIDTH, self.viewport.height()),
            ),
            border,
        );
        let up = Rect::new(
            Point::new(stepper_x + BORDER_WIDTH, bounds.origin.y),
            size(
                STEPPER_WIDTH - BORDER_WIDTH,
                self.viewport.height() / 2.0,
            ),
        );
        let down = Rect::new(
            Point::new(stepper_x + BORDER_WIDTH, mid_y),
            size(
                STEPPER_WIDTH - BORDER_WIDTH,
                self.viewport.height() / 2.0,
            ),
        );
        if self.pressed_stepper == Some(Stepper::Up) {
            context.fill_rect(up, theme.control_pressed_background());
        }
        if self.pressed_stepper == Some(Stepper::Down) {
            context.fill_rect(down, theme.control_pressed_background());
        }
        context.fill_rect(
            Rect::new(Point::new(stepper_x, mid_y), size(STEPPER_WIDTH, BORDER_WIDTH)),
            border,
        );
        paint_arrow(
            context,
            Point::new(
                up.origin.x + (up.size.width() - 8.0) / 2.0,
                up.origin.y + (up.size.height() - 4.0) / 2.0,
            ),
            true,
            foreground,
        );
        paint_arrow(
            context,
            Point::new(
                down.origin.x + (down.size.width() - 8.0) / 2.0,
                down.origin.y + (down.size.height() - 4.0) / 2.0,
            ),
            false,
            foreground,
        );
    }

    #[allow(clippy::too_many_lines)]
    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        if context.phase() != EventPhase::Target {
            return EventStatus::Ignored;
        }
        self.reconcile_value();
        match event {
            InputEvent::PointerDown(pointer) if pointer.button == Some(PointerButton::Primary) => {
                context.request_focus();
                context.capture_pointer(pointer.pointer_id);
                if let Some(stepper) = self.stepper_at(pointer.position) {
                    self.pressed_stepper = Some(stepper);
                    self.pressed_pointer = Some(pointer.pointer_id);
                    context.request_redraw();
                    return EventStatus::Handled;
                }
                let now = Instant::now();
                let double_click = self.last_click.is_some_and(|(time, last)| {
                    now.duration_since(time) <= DOUBLE_CLICK_INTERVAL
                        && (last.x - pointer.position.x).abs() <= 4.0
                        && (last.y - pointer.position.y).abs() <= 4.0
                });
                self.last_click = Some((now, pointer.position));
                if double_click {
                    self.editor.select_all();
                } else if pointer.modifiers.contains(Modifiers::SHIFT) {
                    self.editor
                        .move_to(self.hit_test_caret(pointer.position), true);
                } else {
                    self.editor.set_caret(self.hit_test_caret(pointer.position));
                }
                self.dragging = true;
                context.request_redraw();
                EventStatus::Handled
            }
            InputEvent::PointerMove(pointer) => {
                if self.dragging && pointer.buttons.contains_button(PointerButton::Primary) {
                    self.editor
                        .move_to(self.hit_test_caret(pointer.position), true);
                    context.request_redraw();
                    return EventStatus::Handled;
                }
                EventStatus::Ignored
            }
            InputEvent::PointerUp(pointer) => {
                if self.pressed_pointer == Some(pointer.pointer_id) {
                    let pressed = self.pressed_stepper.take();
                    self.pressed_pointer = None;
                    context.release_pointer(pointer.pointer_id);
                    if let Some(stepper) = pressed
                        && self.stepper_at(pointer.position) == Some(stepper)
                    {
                        self.step_by(
                            if stepper == Stepper::Up { 1.0 } else { -1.0 },
                            context,
                        );
                    }
                    context.request_redraw();
                    return EventStatus::Handled;
                }
                if self.dragging {
                    self.dragging = false;
                    context.release_pointer(pointer.pointer_id);
                    return EventStatus::Handled;
                }
                EventStatus::Ignored
            }
            InputEvent::PointerCancel(pointer) => {
                self.dragging = false;
                self.pressed_stepper = None;
                self.pressed_pointer = None;
                context.release_pointer(pointer.pointer_id);
                context.request_redraw();
                EventStatus::Handled
            }
            InputEvent::TextInput(text) => {
                let filtered = filter_numeric(text);
                self.apply_text_edit(context, |editor| editor.insert(&filtered));
                EventStatus::Handled
            }
            InputEvent::Wheel(wheel) => {
                let delta = match wheel.delta {
                    WheelDelta::Pixels(delta) => delta,
                    WheelDelta::Lines(delta) => Point::new(delta.x * 24.0, delta.y * 24.0),
                };
                let total_width = self.text_layout.size().width() + self.padding.horizontal();
                if total_width <= self.text_width() {
                    return EventStatus::Ignored;
                }
                self.scroll_x = (self.scroll_x - delta.y - delta.x).max(0.0);
                context.request_redraw();
                EventStatus::Handled
            }
            InputEvent::KeyDown(key) => self.handle_key(context, key),
            InputEvent::FocusChanged(FocusChanged { focused }) => {
                self.focused = *focused == Some(context.current_target());
                if !self.focused {
                    self.dragging = false;
                    self.commit(context);
                }
                context.request_redraw();
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn accepts_focus(&self) -> bool {
        true
    }

    fn accessibility(&self) -> Accessibility {
        let mut accessibility = Accessibility::new(AccessibilityRole::TextField)
            .with_value(self.editor.text())
            .with_states(AccessibilityState::FOCUSABLE)
            .with_actions(
                AccessibilityActions::FOCUS
                    | AccessibilityActions::SET_VALUE
                    | AccessibilityActions::SELECT,
            );
        if self.editor.has_selection() {
            accessibility =
                accessibility.with_text_selection(self.editor.anchor(), self.editor.focus());
        }
        accessibility
    }

    fn accessibility_action(&mut self, action: &AccessibilityAction) -> bool {
        match action {
            AccessibilityAction::SetValue(value) => {
                if let Ok(parsed) = value.trim().parse::<f64>() {
                    self.set_value(parsed);
                }
                true
            }
            AccessibilityAction::Select => {
                self.editor.select_all();
                true
            }
            _ => false,
        }
    }
}

impl NumberInput {
    fn handle_key(
        &mut self,
        context: &mut EventContext<'_>,
        key: &drawen_core::KeyEvent,
    ) -> EventStatus {
        let extend = key.modifiers.contains(Modifiers::SHIFT);
        let command =
            key.modifiers.contains(Modifiers::CONTROL) || key.modifiers.contains(Modifiers::META);
        match &key.key {
            Key::Named(NamedKey::ArrowUp) => {
                self.step_by(1.0, context);
                EventStatus::Handled
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.step_by(-1.0, context);
                EventStatus::Handled
            }
            Key::Named(NamedKey::Enter) => {
                self.commit(context);
                EventStatus::Handled
            }
            Key::Named(NamedKey::Escape) => {
                let committed = self.committed;
                self.apply_value(committed, context, false);
                EventStatus::Handled
            }
            Key::Named(NamedKey::Backspace) => {
                self.apply_text_edit(context, |editor| editor.backspace(command));
                EventStatus::Handled
            }
            Key::Named(NamedKey::Delete) => {
                self.apply_text_edit(context, |editor| editor.delete(command));
                EventStatus::Handled
            }
            Key::Named(NamedKey::ArrowLeft) => {
                self.editor.move_horizontal(false, command, extend);
                self.ensure_caret_visible();
                context.request_redraw();
                EventStatus::Handled
            }
            Key::Named(NamedKey::ArrowRight) => {
                self.editor.move_horizontal(true, command, extend);
                self.ensure_caret_visible();
                context.request_redraw();
                EventStatus::Handled
            }
            Key::Named(NamedKey::Home) => {
                self.editor.move_to(0, extend);
                self.ensure_caret_visible();
                context.request_redraw();
                EventStatus::Handled
            }
            Key::Named(NamedKey::End) => {
                self.editor.move_to(self.editor.text().len(), extend);
                self.ensure_caret_visible();
                context.request_redraw();
                EventStatus::Handled
            }
            Key::Named(NamedKey::Tab) => EventStatus::Ignored,
            _ if command => {
                let handled = match &key.key {
                    Key::Character(character) if character.eq_ignore_ascii_case("a") => {
                        self.editor.select_all();
                        true
                    }
                    Key::Character(character) if character.eq_ignore_ascii_case("c") => {
                        self.copy(context)
                    }
                    Key::Character(character) if character.eq_ignore_ascii_case("x") => {
                        self.cut(context)
                    }
                    Key::Character(character) if character.eq_ignore_ascii_case("v") => {
                        self.paste(context)
                    }
                    Key::Character(character) if character.eq_ignore_ascii_case("z") => {
                        if extend {
                            self.apply_text_edit(context, TextEditor::redo)
                        } else {
                            self.apply_text_edit(context, TextEditor::undo)
                        }
                    }
                    Key::Character(character) if character.eq_ignore_ascii_case("y") => {
                        self.apply_text_edit(context, TextEditor::redo)
                    }
                    _ => false,
                };
                if handled {
                    self.ensure_caret_visible();
                    context.request_redraw();
                    EventStatus::Handled
                } else {
                    EventStatus::Ignored
                }
            }
            _ => EventStatus::Ignored,
        }
    }
}

fn normalize_value(value: f64, minimum: f64, maximum: f64) -> f64 {
    let value = if value.is_nan() { 0.0 } else { value };
    let value = value.clamp(minimum, maximum);
    if value == 0.0 { 0.0 } else { value }
}

fn format_number(value: f64) -> String {
    format!("{value}")
}

fn filter_numeric(input: &str) -> String {
    input
        .chars()
        .filter(|character| {
            character.is_ascii_digit() || matches!(character, '-' | '.' | '+')
        })
        .collect()
}

fn paint_arrow(context: &mut PaintContext<'_>, origin: Point, up: bool, color: Color) {
    for row in 0u8..4 {
        let width = if up {
            2.0 + 2.0 * f32::from(row)
        } else {
            8.0 - 2.0 * f32::from(row)
        };
        context.fill_rect(
            Rect::new(
                Point::new(origin.x + (8.0 - width) / 2.0, origin.y + f32::from(row)),
                size(width, 1.0),
            ),
            color,
        );
    }
}

fn default_padding() -> Insets {
    Insets::new(
        VERTICAL_PADDING,
        HORIZONTAL_PADDING,
        VERTICAL_PADDING,
        HORIZONTAL_PADDING,
    )
}

fn size(width: f32, height: f32) -> Size {
    Size::new(width, height).expect("number input sizes are non-negative and not NaN")
}

fn empty_layout() -> TextLayout {
    drawen_render::FontdueTextShaper::ubuntu_light().layout(
        "",
        &TextStyle::new(1.0, Color::BLACK),
        None,
    )
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // tests assert exact expected numbers by design
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use drawen_core::{
        Constraints, InputEvent, Key, KeyCode, Modifiers, NamedKey, Point, PointerButton,
        PointerButtons, PointerEvent, PointerId, Size,
    };
    use drawen_ui::{Property, UiRuntime};

    use super::NumberInput;

    fn viewport() -> Constraints {
        Constraints::tight(Size::new(200.0, 40.0).expect("valid size")).expect("valid constraints")
    }

    fn key(key: NamedKey) -> InputEvent {
        InputEvent::KeyDown(drawen_core::KeyEvent {
            key: Key::Named(key),
            code: KeyCode::Unidentified,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
    }

    fn pointer(position: Point, up: bool) -> InputEvent {
        let event = PointerEvent {
            pointer_id: PointerId(1),
            position,
            button: Some(PointerButton::Primary),
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

    fn stepper_point(runtime: &UiRuntime, up: bool) -> Point {
        let bounds = runtime.bounds(runtime.root()).expect("input has bounds");
        Point::new(
            bounds.origin.x + bounds.size.width() - 8.0,
            bounds.origin.y + if up { 6.0 } else { bounds.size.height() - 6.0 },
        )
    }

    #[test]
    fn arrow_keys_step_and_clamp_to_the_range() {
        let input = NumberInput::new().with_range(0.0, 3.0).with_value(2.0);
        let value = input.value();
        let changes = Rc::new(RefCell::new(Vec::new()));
        let _subscription = input.changed().subscribe({
            let changes = Rc::clone(&changes);
            move |number| changes.borrow_mut().push(*number)
        });
        let mut runtime = UiRuntime::new(input);
        runtime.layout(viewport()).expect("layout succeeds");
        runtime.focus_widget(runtime.root()).expect("input focuses");

        let _ = runtime.dispatch_event(&key(NamedKey::ArrowUp));
        assert_eq!(value.get(), 3.0);
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowUp));
        assert_eq!(value.get(), 3.0);
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowDown));
        assert_eq!(value.get(), 2.0);
        assert_eq!(*changes.borrow(), vec![3.0, 2.0]);
    }

    #[test]
    fn stepper_buttons_increment_and_decrement() {
        let input = NumberInput::new().with_step(0.5).with_value(1.0);
        let value = input.value();
        let mut runtime = UiRuntime::new(input);
        runtime.layout(viewport()).expect("layout succeeds");

        let up = stepper_point(&runtime, true);
        let _ = runtime.dispatch_event(&pointer(up, false));
        let _ = runtime.dispatch_event(&pointer(up, true));
        assert_eq!(value.get(), 1.5);
        runtime.layout(viewport()).expect("relayout after step");

        let down = stepper_point(&runtime, false);
        let _ = runtime.dispatch_event(&pointer(down, false));
        let _ = runtime.dispatch_event(&pointer(down, true));
        assert_eq!(value.get(), 1.0);
    }

    #[test]
    fn typing_commits_on_enter_and_filters_non_numeric_input() {
        let input = NumberInput::new().with_range(-10.0, 10.0);
        let value = input.value();
        let mut runtime = UiRuntime::new(input);
        runtime.layout(viewport()).expect("layout succeeds");
        runtime.focus_widget(runtime.root()).expect("input focuses");
        let _ = runtime.dispatch_event(&key(NamedKey::End));

        let _ = runtime.dispatch_event(&InputEvent::TextInput("12abc.5".into()));
        // Non-numeric characters are dropped; the number is not committed yet.
        assert_eq!(value.get(), 0.0);
        let _ = runtime.dispatch_event(&key(NamedKey::Enter));
        assert_eq!(value.get(), 10.0); // 12.5 clamped to the maximum
    }

    #[test]
    fn invalid_text_reverts_on_commit() {
        let input = NumberInput::new().with_value(7.0);
        let value = input.value();
        let mut runtime = UiRuntime::new(input);
        runtime.layout(viewport()).expect("layout succeeds");
        runtime.focus_widget(runtime.root()).expect("input focuses");

        let _ = runtime.dispatch_event(&InputEvent::TextInput("-".into()));
        let _ = runtime.dispatch_event(&key(NamedKey::Enter));
        assert_eq!(value.get(), 7.0);
    }

    #[test]
    fn bound_value_follows_application_state() {
        let state = Property::new(5.0_f64);
        let input = NumberInput::new().with_range(0.0, 100.0).with_value(5.0);
        let value = input.value();
        let mut runtime = UiRuntime::new(input);
        let _binding = runtime.bind(&value, &state);
        runtime.layout(viewport()).expect("layout succeeds");
        assert_eq!(value.get(), 5.0);

        state.set(42.0);
        assert_eq!(value.get(), 42.0);
        state.set(500.0);
        assert_eq!(value.get(), 100.0); // clamped by the property normalizer
    }

    #[test]
    fn set_value_emits_property_changed_but_not_user_changed() {
        let mut input = NumberInput::new();
        let value = input.value();
        let property_changes = Rc::new(RefCell::new(0));
        let user_changes = Rc::new(RefCell::new(0));
        let _property = value.subscribe({
            let property_changes = Rc::clone(&property_changes);
            move |_| *property_changes.borrow_mut() += 1
        });
        let _user = input.changed().subscribe({
            let user_changes = Rc::clone(&user_changes);
            move |_| *user_changes.borrow_mut() += 1
        });
        input.set_value(9.0);
        assert_eq!(value.get(), 9.0);
        assert_eq!(*property_changes.borrow(), 1);
        assert_eq!(*user_changes.borrow(), 0);
    }
}