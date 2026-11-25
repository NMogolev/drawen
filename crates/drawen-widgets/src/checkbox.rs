use drawen_core::{Constraints, InputEvent, Key, NamedKey, Point, PointerButton, Rect, Size};
use drawen_render::PaintContext;
use drawen_ui::{
    Accessibility, AccessibilityAction, AccessibilityActions, AccessibilityRole,
    AccessibilityState, Border, ChildLayout, DssStyle, EventContext, EventPhase, EventStatus,
    LayoutContext, LayoutResult, Property, Signal, StyleTarget, UiEnvironment, Widget,
};

const CONTROL_SIZE: f32 = 18.0;
const MARK_INSET: f32 = 5.0;

/// A two-state control with an optional runtime-owned label child.
///
/// The checked state is a [`Property`]: bind application state to it with
/// `runtime.bind(&checkbox.checked(), &state.agreed)`, or read and write it
/// directly. User interaction additionally emits [`Self::toggled`].
pub struct Checkbox {
    checked: Property<bool>,
    toggled: Signal<bool>,
    pressed_pointer: Option<drawen_core::PointerId>,
    size: Size,
}

impl Checkbox {
    /// Creates an unchecked checkbox.
    #[must_use]
    pub fn new() -> Self {
        Self::with_checked(false)
    }

    /// Creates a checkbox with the supplied initial state.
    #[must_use]
    pub fn with_checked(checked: bool) -> Self {
        Self {
            checked: Property::new(checked),
            toggled: Signal::new(),
            pressed_pointer: None,
            size: Size::ZERO,
        }
    }

    /// Returns the checked property shared with this checkbox.
    ///
    /// Bind application state to it with
    /// `runtime.bind(&checkbox.checked(), &state.agreed)`; the binding keeps
    /// the checkbox in sync and wakes redraws on every change. The property's
    /// `changed` signal fires on every write, including programmatic ones;
    /// [`Self::toggled`] fires only on user interaction.
    pub fn checked(&self) -> Property<bool> {
        self.checked.clone()
    }

    /// Returns whether this checkbox is checked.
    #[must_use]
    pub fn is_checked(&self) -> bool {
        self.checked.get()
    }

    /// Sets the checked state.
    ///
    /// Emits `checked().changed()` when the value differs, but not
    /// [`Self::toggled`], which is reserved for user interaction. When the
    /// property is watched or bound through the runtime, the change
    /// invalidates paint.
    pub fn set_checked(&mut self, checked: bool) {
        self.checked.set(checked);
    }

    /// Returns a signal emitted after user interaction changes the checked state.
    #[must_use]
    pub fn toggled(&self) -> Signal<bool> {
        self.toggled.clone()
    }

    fn toggle(&mut self, context: &mut EventContext<'_>) {
        self.toggle_state();
        context.request_redraw();
    }

    fn toggle_state(&mut self) {
        let checked = !self.checked.get();
        self.checked.set(checked);
        self.toggled.emit(&checked);
    }
}

impl Default for Checkbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Checkbox {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let gap = context
            .style()
            .spacing
            .unwrap_or_else(|| context.environment().theme().spacing());
        let mut children = Vec::new();
        let label_size = if context.child_count() == 1 {
            let max = constraints.max();
            let child_max = size((max.width() - CONTROL_SIZE - gap).max(0.0), max.height());
            let (id, layout) = context
                .layout_child(0, Constraints::loose(child_max))
                .expect("runtime child index is valid");
            children.push(ChildLayout::new(
                id,
                Point::new(
                    CONTROL_SIZE + gap,
                    (CONTROL_SIZE.max(layout.size().height()) - layout.size().height()) / 2.0,
                ),
            ));
            layout.size()
        } else {
            Size::ZERO
        };
        let size = constraints.constrain(size(
            CONTROL_SIZE
                + if context.child_count() == 1 {
                    gap + label_size.width()
                } else {
                    0.0
                },
            CONTROL_SIZE.max(label_size.height()),
        ));
        self.size = size;
        LayoutResult::with_children(size, children)
    }

    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("checkbox")
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
        let indicator = indicator_bounds(bounds);
        let checked = self.checked.get();
        let theme = environment.theme();
        let accent = style.accent.unwrap_or_else(|| theme.accent());
        let transition = context.transition();
        let background = style.background.unwrap_or_else(|| {
            if checked {
                transition.control_color(accent, accent.lighten(0.08), accent.darken(0.08))
            } else {
                transition.control_color(
                    theme.surface(),
                    theme.control_hover_background(),
                    theme.control_pressed_background(),
                )
            }
        });
        let border = style.border.unwrap_or_else(|| {
            let color = if checked {
                accent
            } else {
                transition.control_color(theme.border(), accent.mix(theme.border(), 0.45), accent)
            };
            Border::new(1.0, color)
        });
        let radius = style
            .corner_radius
            .unwrap_or_else(|| theme.corner_radius())
            .min(CONTROL_SIZE / 2.0);
        context.fill_rounded_rect(indicator, radius, background);
        context.stroke_rounded_rect(indicator, radius, border.width, border.color);
        if checked {
            context.fill_rect(
                indicator.inset(drawen_core::Insets::all(MARK_INSET)),
                style
                    .foreground
                    .unwrap_or_else(|| theme.accent_foreground()),
            );
        }
    }

    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        if context.phase() != EventPhase::Target {
            return EventStatus::Ignored;
        }
        match event {
            InputEvent::PointerDown(pointer) if pointer.button == Some(PointerButton::Primary) => {
                self.pressed_pointer = Some(pointer.pointer_id);
                context.capture_pointer(pointer.pointer_id);
                context.request_focus();
                EventStatus::Handled
            }
            InputEvent::PointerUp(pointer)
                if pointer.button == Some(PointerButton::Primary)
                    && self.pressed_pointer == Some(pointer.pointer_id) =>
            {
                self.pressed_pointer = None;
                context.release_pointer(pointer.pointer_id);
                if self.size.contains(pointer.position) {
                    self.toggle(context);
                }
                EventStatus::Handled
            }
            InputEvent::PointerCancel(pointer)
                if self.pressed_pointer == Some(pointer.pointer_id) =>
            {
                self.pressed_pointer = None;
                context.release_pointer(pointer.pointer_id);
                EventStatus::Handled
            }
            InputEvent::KeyDown(key)
                if !key.repeat
                    && matches!(key.key, Key::Named(NamedKey::Space | NamedKey::Enter)) =>
            {
                self.toggle(context);
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn accepts_focus(&self) -> bool {
        true
    }

    fn accessibility(&self) -> Accessibility {
        let mut accessibility = Accessibility::new(AccessibilityRole::CheckBox)
            .with_states(AccessibilityState::FOCUSABLE)
            .with_actions(
                AccessibilityActions::FOCUS
                    | AccessibilityActions::CLICK
                    | AccessibilityActions::TOGGLE,
            );
        if self.checked.get() {
            accessibility = accessibility.with_states(AccessibilityState::CHECKED);
        }
        accessibility
    }

    fn accessibility_action(&mut self, action: &AccessibilityAction) -> bool {
        if matches!(
            action,
            AccessibilityAction::Click | AccessibilityAction::Toggle
        ) {
            self.toggle_state();
            true
        } else {
            false
        }
    }
}

fn indicator_bounds(bounds: Rect) -> Rect {
    Rect::new(
        Point::new(
            bounds.origin.x,
            bounds.origin.y + (bounds.size.height() - CONTROL_SIZE).max(0.0) / 2.0,
        ),
        size(CONTROL_SIZE, CONTROL_SIZE),
    )
}

fn size(width: f32, height: f32) -> Size {
    Size::new(width, height).expect("checkbox sizes are non-negative and not NaN")
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use drawen_core::{KeyCode, Modifiers, PointerButtons, PointerEvent, PointerId};
    use drawen_ui::UiRuntime;

    use super::*;

    #[test]
    fn toggles_with_pointer_and_keyboard() {
        let checkbox = Checkbox::new();
        let values = Rc::new(RefCell::new(Vec::new()));
        let _subscription = checkbox.toggled().subscribe({
            let values = Rc::clone(&values);
            move |value| values.borrow_mut().push(*value)
        });
        let mut runtime = UiRuntime::new(checkbox);
        runtime
            .layout(Constraints::UNBOUNDED)
            .expect("checkbox layout succeeds");

        let _ = runtime.dispatch_event(&pointer(true));
        let _ = runtime.dispatch_event(&pointer(false));
        let _ = runtime.dispatch_event(&key());

        assert_eq!(*values.borrow(), vec![true, false]);
    }

    #[test]
    fn bound_checked_follows_application_state() {
        let agreed = Property::new(false);
        let checkbox = Checkbox::new();
        let checked = checkbox.checked();
        let mut runtime = UiRuntime::new(checkbox);
        let _binding = runtime.bind(&checked, &agreed);
        runtime
            .layout(Constraints::UNBOUNDED)
            .expect("checkbox layout succeeds");
        assert!(!checked.get());

        agreed.set(true);
        assert!(checked.get());

        agreed.set(false);
        assert!(!checked.get());
    }

    #[test]
    fn set_checked_emits_changed_but_not_toggled() {
        let checkbox = Checkbox::new();
        let checked = checkbox.checked();
        let changed_values = Rc::new(RefCell::new(Vec::new()));
        let toggled_values = Rc::new(RefCell::new(Vec::new()));
        let _changed = checked.subscribe({
            let changed_values = Rc::clone(&changed_values);
            move |value| changed_values.borrow_mut().push(*value)
        });
        let _toggled = checkbox.toggled().subscribe({
            let toggled_values = Rc::clone(&toggled_values);
            move |value| toggled_values.borrow_mut().push(*value)
        });
        let mut runtime = UiRuntime::new(checkbox);
        runtime
            .layout(Constraints::UNBOUNDED)
            .expect("checkbox layout succeeds");

        // A programmatic write emits changed, never toggled.
        checked.set(true);
        assert_eq!(*changed_values.borrow(), vec![true]);
        assert!(toggled_values.borrow().is_empty());

        // User interaction emits both.
        let _ = runtime.dispatch_event(&pointer(true));
        let _ = runtime.dispatch_event(&pointer(false));
        assert_eq!(*changed_values.borrow(), vec![true, false]);
        assert_eq!(*toggled_values.borrow(), vec![false]);
    }

    fn pointer(down: bool) -> InputEvent {
        let event = PointerEvent {
            pointer_id: PointerId(1),
            position: Point::ZERO,
            button: Some(PointerButton::Primary),
            buttons: if down {
                PointerButtons::PRIMARY
            } else {
                PointerButtons::NONE
            },
            modifiers: Modifiers::NONE,
        };
        if down {
            InputEvent::PointerDown(event)
        } else {
            InputEvent::PointerUp(event)
        }
    }

    fn key() -> InputEvent {
        InputEvent::KeyDown(drawen_core::KeyEvent {
            key: Key::Named(NamedKey::Space),
            code: KeyCode::Unidentified,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
    }
}
