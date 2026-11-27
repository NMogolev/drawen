use drawen_core::{
    Constraints, InputEvent, Key, NamedKey, Point, PointerButton, PointerId, Rect, Size,
};
use drawen_render::PaintContext;
use drawen_ui::{
    Accessibility, AccessibilityAction, AccessibilityActions, AccessibilityRole,
    AccessibilityState, DssStyle, EventContext, EventPhase, EventStatus, LayoutContext,
    LayoutResult, Property, Signal, StyleTarget, UiEnvironment, Widget,
};

const DEFAULT_WIDTH: f32 = 160.0;
const CONTROL_HEIGHT: f32 = 24.0;
const TRACK_HEIGHT: f32 = 4.0;
const THUMB_RADIUS: f32 = 8.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Thumb {
    Lower,
    Upper,
}

/// A horizontal control for selecting a continuous interval from a finite range.
///
/// Two thumbs share one track: the lower thumb bounds the interval from below
/// and the upper thumb from above. The pair is a clamped [`Property`]: bind
/// application state with `runtime.bind(&slider.value(), &state.span)` and every
/// copied write stays inside `[minimum, maximum]` with `lower <= upper`
/// (a reversed pair is swapped). User interaction additionally emits
/// [`Self::changed`].
///
/// The pointer grabs the nearest thumb and drags it up to the other thumb, so
/// the interval can collapse to a point but never invert. `ArrowDown` and
/// `ArrowUp` select the lower and upper thumb; `ArrowLeft` and `ArrowRight` step
/// the selected thumb; `Home` and `End` send it to the range ends, stopping at
/// the other thumb.
pub struct RangeSlider {
    minimum: f32,
    maximum: f32,
    value: Property<(f32, f32)>,
    changed: Signal<(f32, f32)>,
    active: Thumb,
    dragging: Option<PointerId>,
    size: Size,
}

impl RangeSlider {
    /// Creates a range slider spanning the whole inclusive range.
    ///
    /// # Panics
    ///
    /// Panics when either bound is non-finite or `minimum` exceeds `maximum`.
    #[must_use]
    pub fn new(minimum: f32, maximum: f32) -> Self {
        Self::with_value(minimum, maximum, minimum, maximum)
    }

    /// Creates a range slider with the interval `[lower, upper]`, clamped to the
    /// finite inclusive range.
    ///
    /// # Panics
    ///
    /// Panics when a bound or interval end is non-finite, or when `minimum`
    /// exceeds `maximum`. A reversed interval is swapped.
    #[must_use]
    pub fn with_value(minimum: f32, maximum: f32, lower: f32, upper: f32) -> Self {
        assert_valid_range(minimum, maximum);
        assert!(
            lower.is_finite() && upper.is_finite(),
            "range slider values must be finite"
        );
        Self {
            minimum,
            maximum,
            value: Property::normalized((lower, upper), move |(lower, upper)| {
                normalize_pair(lower, upper, minimum, maximum)
            }),
            changed: Signal::new(),
            active: Thumb::Lower,
            dragging: None,
            size: Size::ZERO,
        }
    }

    /// Returns the minimum selectable value.
    #[must_use]
    pub const fn minimum(&self) -> f32 {
        self.minimum
    }

    /// Returns the maximum selectable value.
    #[must_use]
    pub const fn maximum(&self) -> f32 {
        self.maximum
    }

    /// Returns the current lower end of the interval.
    #[must_use]
    pub fn lower(&self) -> f32 {
        self.value.get().0
    }

    /// Returns the current upper end of the interval.
    #[must_use]
    pub fn upper(&self) -> f32 {
        self.value.get().1
    }

    /// Returns the interval property shared with this slider.
    ///
    /// Every write is clamped to `[minimum, maximum]` and ordered so that
    /// `lower <= upper`, including `UiRuntime::bind` copies; a non-finite end
    /// falls back to the nearest bound. Bind state with
    /// `runtime.bind(&slider.value(), &state.span)` and read the pair with
    /// `slider.value().get()`. The property's `changed` signal fires on every
    /// write; [`Self::changed`] fires only on user interaction.
    pub fn value(&self) -> Property<(f32, f32)> {
        self.value.clone()
    }

    /// Sets the interval, clamped to the finite inclusive range.
    ///
    /// Emits `value().changed()` when the clamped pair differs, but not the
    /// user-only [`Self::changed`] signal. When the property is watched or
    /// bound through the runtime, the change invalidates paint.
    ///
    /// # Panics
    ///
    /// Panics when either end is non-finite.
    pub fn set_value(&mut self, lower: f32, upper: f32) {
        assert!(
            lower.is_finite() && upper.is_finite(),
            "range slider values must be finite"
        );
        self.value.set((lower, upper));
    }

    /// Returns a signal emitted when the user changes the interval.
    #[must_use]
    pub fn changed(&self) -> Signal<(f32, f32)> {
        self.changed.clone()
    }

    fn set_user_value(&mut self, pair: (f32, f32), context: &mut EventContext<'_>) {
        let pair = normalize_pair(pair.0, pair.1, self.minimum, self.maximum);
        let current = self.value.get();
        if (current.0 - pair.0).abs() <= f32::EPSILON && (current.1 - pair.1).abs() <= f32::EPSILON
        {
            return;
        }
        if self.value.set(pair) {
            self.changed.emit(&pair);
            context.request_redraw();
        }
    }

    /// Moves one thumb to `value`, stopping at the other thumb so the interval
    /// never inverts.
    fn move_thumb(&mut self, thumb: Thumb, value: f32, context: &mut EventContext<'_>) {
        let (lower, upper) = self.value.get();
        match thumb {
            Thumb::Lower => {
                self.set_user_value((value.clamp(self.minimum, upper), upper), context);
            }
            Thumb::Upper => {
                self.set_user_value((lower, value.clamp(lower, self.maximum)), context);
            }
        }
    }

    fn value_at(&self, position: Point) -> f32 {
        let track = track_bounds(self.size);
        let ratio = if track.size.width() > 0.0 {
            ((position.x - track.origin.x) / track.size.width()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.minimum + (self.maximum - self.minimum) * ratio
    }

    /// Picks the thumb nearest to the press. Ties go to the lower thumb, except
    /// when both thumbs coincide and the press lands to their right.
    fn thumb_at(&self, position: Point) -> Thumb {
        let track = track_bounds(self.size);
        let (lower, upper) = self.value.get();
        let lower_x = track.origin.x + track.size.width() * self.ratio(lower);
        let upper_x = track.origin.x + track.size.width() * self.ratio(upper);
        if (lower - upper).abs() <= f32::EPSILON && position.x > lower_x {
            return Thumb::Upper;
        }
        if (position.x - lower_x).abs() <= (position.x - upper_x).abs() {
            Thumb::Lower
        } else {
            Thumb::Upper
        }
    }

    fn set_thumb_from_position(&mut self, position: Point, context: &mut EventContext<'_>) {
        let value = self.value_at(position);
        self.move_thumb(self.active, value, context);
    }

    fn keyboard_step(&self) -> f32 {
        (self.maximum - self.minimum) / 100.0
    }

    fn ratio(&self, value: f32) -> f32 {
        let range = self.maximum - self.minimum;
        if range > 0.0 {
            (value - self.minimum) / range
        } else {
            0.0
        }
    }
}

impl Widget for RangeSlider {
    fn layout(&mut self, _: &mut LayoutContext<'_>, constraints: Constraints) -> LayoutResult {
        let max = constraints.max();
        let width = if max.width().is_finite() {
            max.width()
        } else {
            DEFAULT_WIDTH
        };
        self.size = constraints.constrain(size(width, CONTROL_HEIGHT));
        LayoutResult::new(self.size)
    }

    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("range-slider")
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
        let accent = style.accent.unwrap_or_else(|| theme.accent());
        let transition = context.transition();
        let track = offset_rect(track_bounds(bounds.size), bounds.origin);
        let (lower, upper) = self.value.get();
        let lower_x = track.origin.x + track.size.width() * self.ratio(lower);
        let upper_x = track.origin.x + track.size.width() * self.ratio(upper);
        let filled = Rect::new(
            Point::new(lower_x, track.origin.y),
            size((upper_x - lower_x).max(0.0), track.size.height()),
        );
        let base_radius = style
            .corner_radius
            .unwrap_or(THUMB_RADIUS)
            .min(THUMB_RADIUS);
        let thumb_radius =
            base_radius * (1.0 + transition.hover() * 0.06 + transition.press() * 0.1);
        let track_radius = style
            .corner_radius
            .unwrap_or(TRACK_HEIGHT / 2.0)
            .min(track.size.height() / 2.0);
        context.fill_rounded_rect(
            track,
            track_radius,
            style.background.unwrap_or_else(|| {
                transition.control_color(
                    theme.control_background(),
                    theme.control_hover_background(),
                    theme.control_pressed_background(),
                )
            }),
        );
        if let Some(border) = style.border {
            context.stroke_rounded_rect(track, track_radius, border.width, border.color);
        }
        if filled.size.width() > 0.0 {
            context.fill_rounded_rect(filled, track_radius, accent);
        }
        let thumb_color =
            transition.control_color(accent, accent.lighten(0.06), accent.darken(0.06));
        for center in [lower_x, upper_x] {
            let thumb = Rect::new(
                Point::new(
                    center - thumb_radius,
                    bounds.origin.y + (bounds.size.height() - thumb_radius * 2.0) / 2.0,
                ),
                size(thumb_radius * 2.0, thumb_radius * 2.0),
            );
            context.draw_shadow(thumb, thumb_radius, 3.0, theme.shadow());
            context.fill_rounded_rect(thumb, thumb_radius, thumb_color);
            context.stroke_rounded_rect(
                thumb,
                thumb_radius,
                1.0,
                theme.accent_foreground().with_alpha(0.35),
            );
        }
    }

    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        if context.phase() != EventPhase::Target {
            return EventStatus::Ignored;
        }
        match event {
            InputEvent::PointerDown(pointer) if pointer.button == Some(PointerButton::Primary) => {
                self.active = self.thumb_at(pointer.position);
                self.dragging = Some(pointer.pointer_id);
                self.set_thumb_from_position(pointer.position, context);
                context.capture_pointer(pointer.pointer_id);
                context.request_focus();
                EventStatus::Handled
            }
            InputEvent::PointerMove(pointer) if self.dragging == Some(pointer.pointer_id) => {
                self.set_thumb_from_position(pointer.position, context);
                EventStatus::Handled
            }
            InputEvent::PointerUp(pointer) if self.dragging == Some(pointer.pointer_id) => {
                self.set_thumb_from_position(pointer.position, context);
                self.dragging = None;
                context.release_pointer(pointer.pointer_id);
                EventStatus::Handled
            }
            InputEvent::PointerCancel(pointer) if self.dragging == Some(pointer.pointer_id) => {
                self.dragging = None;
                context.release_pointer(pointer.pointer_id);
                EventStatus::Handled
            }
            InputEvent::KeyDown(key) if !key.repeat => match key.key {
                Key::Named(NamedKey::ArrowDown) => {
                    self.active = Thumb::Lower;
                    EventStatus::Handled
                }
                Key::Named(NamedKey::ArrowUp) => {
                    self.active = Thumb::Upper;
                    EventStatus::Handled
                }
                Key::Named(NamedKey::ArrowLeft) => {
                    let value = self.active_value() - self.keyboard_step();
                    self.move_thumb(self.active, value, context);
                    EventStatus::Handled
                }
                Key::Named(NamedKey::ArrowRight) => {
                    let value = self.active_value() + self.keyboard_step();
                    self.move_thumb(self.active, value, context);
                    EventStatus::Handled
                }
                Key::Named(NamedKey::Home) => {
                    self.move_thumb(self.active, self.minimum, context);
                    EventStatus::Handled
                }
                Key::Named(NamedKey::End) => {
                    self.move_thumb(self.active, self.maximum, context);
                    EventStatus::Handled
                }
                _ => EventStatus::Ignored,
            },
            _ => EventStatus::Ignored,
        }
    }

    fn accepts_focus(&self) -> bool {
        true
    }

    fn accessibility(&self) -> Accessibility {
        let (lower, upper) = self.value.get();
        Accessibility::new(AccessibilityRole::Slider)
            .with_value(format!("{lower} - {upper}"))
            .with_numeric_value(
                f64::from(self.active_value()),
                f64::from(self.minimum),
                f64::from(self.maximum),
            )
            .with_states(AccessibilityState::FOCUSABLE)
            .with_actions(
                AccessibilityActions::FOCUS
                    | AccessibilityActions::SET_VALUE
                    | AccessibilityActions::INCREMENT
                    | AccessibilityActions::DECREMENT,
            )
    }

    fn accessibility_action(&mut self, action: &AccessibilityAction) -> bool {
        match action {
            AccessibilityAction::SetValue(value) => {
                let mut ends = value.split(',').map(str::trim).map(str::parse::<f32>);
                let (Some(Ok(first)), second) = (ends.next(), ends.next()) else {
                    return false;
                };
                match second {
                    Some(Ok(second)) => self.set_accessibility_value(first, second),
                    None => {
                        let other = match self.active {
                            Thumb::Lower => self.upper(),
                            Thumb::Upper => self.lower(),
                        };
                        match self.active {
                            Thumb::Lower => self.set_accessibility_value(first, other),
                            Thumb::Upper => self.set_accessibility_value(other, first),
                        }
                    }
                    Some(Err(_)) => return false,
                }
                true
            }
            AccessibilityAction::Increment => {
                let value = self.active_value() + self.keyboard_step();
                self.step_accessibility(value);
                true
            }
            AccessibilityAction::Decrement => {
                let value = self.active_value() - self.keyboard_step();
                self.step_accessibility(value);
                true
            }
            _ => false,
        }
    }
}

impl RangeSlider {
    fn active_value(&self) -> f32 {
        match self.active {
            Thumb::Lower => self.lower(),
            Thumb::Upper => self.upper(),
        }
    }

    fn step_accessibility(&mut self, value: f32) {
        let (lower, upper) = self.value.get();
        let pair = match self.active {
            Thumb::Lower => (value.clamp(self.minimum, upper), upper),
            Thumb::Upper => (lower, value.clamp(lower, self.maximum)),
        };
        if self.value.set(pair) {
            self.changed.emit(&self.value.get());
        }
    }

    fn set_accessibility_value(&mut self, lower: f32, upper: f32) {
        if !lower.is_finite() || !upper.is_finite() {
            return;
        }
        if self.value.set((lower, upper)) {
            self.changed.emit(&self.value.get());
        }
    }
}

fn normalize_pair(lower: f32, upper: f32, minimum: f32, maximum: f32) -> (f32, f32) {
    let lower = if lower.is_finite() {
        lower.clamp(minimum, maximum)
    } else {
        minimum
    };
    let upper = if upper.is_finite() {
        upper.clamp(minimum, maximum)
    } else {
        maximum
    };
    if lower <= upper {
        (lower, upper)
    } else {
        (upper, lower)
    }
}

fn assert_valid_range(minimum: f32, maximum: f32) {
    assert!(
        minimum.is_finite() && maximum.is_finite() && minimum <= maximum,
        "range slider range must be finite and ordered"
    );
}

fn track_bounds(bounds_size: Size) -> Rect {
    let width = (bounds_size.width() - THUMB_RADIUS * 2.0).max(0.0);
    Rect::new(
        Point::new(
            THUMB_RADIUS.min(bounds_size.width() / 2.0),
            (bounds_size.height() - TRACK_HEIGHT) / 2.0,
        ),
        size(width, TRACK_HEIGHT.min(bounds_size.height())),
    )
}

fn offset_rect(rect: Rect, offset: Point) -> Rect {
    Rect::new(
        Point::new(rect.origin.x + offset.x, rect.origin.y + offset.y),
        rect.size,
    )
}

fn size(width: f32, height: f32) -> Size {
    Size::new(width, height).expect("range slider sizes are non-negative and not NaN")
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // tests assert exact expected numbers by design
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use drawen_core::{KeyCode, Modifiers, PointerButtons, PointerEvent};
    use drawen_ui::UiRuntime;

    use super::*;

    #[test]
    fn dragging_the_nearest_thumb_stops_at_the_other_thumb() {
        let slider = RangeSlider::with_value(0.0, 100.0, 20.0, 80.0);
        let value = slider.value();
        let changes = Rc::new(RefCell::new(Vec::new()));
        let _subscription = slider.changed().subscribe({
            let changes = Rc::clone(&changes);
            move |pair| changes.borrow_mut().push(*pair)
        });
        let mut runtime = UiRuntime::new(slider);
        runtime
            .layout(Constraints::tight(size(116.0, CONTROL_HEIGHT)).expect("valid constraints"))
            .expect("range slider layout succeeds");

        // Track spans x = 8..108, so the value equals x - 8; the lower thumb
        // sits at 28 and the upper at 88.
        let _ = runtime.dispatch_event(&pointer(InputEventKind::Down, 8.0));
        let _ = runtime.dispatch_event(&pointer(InputEventKind::Move, 108.0));
        let _ = runtime.dispatch_event(&pointer(InputEventKind::Up, 120.0));
        assert_eq!(value.get(), (80.0, 80.0));
        assert_eq!(*changes.borrow(), vec![(0.0, 80.0), (80.0, 80.0)]);
    }

    #[test]
    fn coincident_thumbs_pick_the_upper_one_when_pressed_on_the_right() {
        let slider = RangeSlider::with_value(0.0, 100.0, 50.0, 50.0);
        let value = slider.value();
        let mut runtime = UiRuntime::new(slider);
        runtime
            .layout(Constraints::tight(size(116.0, CONTROL_HEIGHT)).expect("valid constraints"))
            .expect("range slider layout succeeds");

        let _ = runtime.dispatch_event(&pointer(InputEventKind::Down, 83.0));
        let _ = runtime.dispatch_event(&pointer(InputEventKind::Up, 83.0));
        assert_eq!(value.get(), (50.0, 75.0));
    }

    #[test]
    fn keyboard_selects_and_steps_each_thumb_within_the_interval() {
        let slider = RangeSlider::with_value(0.0, 100.0, 20.0, 80.0);
        let value = slider.value();
        let mut runtime = UiRuntime::new(slider);
        runtime
            .layout(Constraints::tight(size(100.0, CONTROL_HEIGHT)).expect("valid constraints"))
            .expect("range slider layout succeeds");
        runtime
            .focus_widget(runtime.root())
            .expect("range slider focuses");

        // The lower thumb is selected by default and steps by one percent.
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowLeft));
        assert_eq!(value.get(), (19.0, 80.0));
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowUp));
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowRight));
        assert_eq!(value.get(), (19.0, 81.0));
        // Home/End stop at the other thumb instead of inverting the interval.
        let _ = runtime.dispatch_event(&key(NamedKey::Home));
        assert_eq!(value.get(), (19.0, 19.0));
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowDown));
        let _ = runtime.dispatch_event(&key(NamedKey::End));
        assert_eq!(value.get(), (19.0, 19.0));
    }

    #[test]
    fn bound_pair_is_clamped_and_ordered() {
        let span = Property::new((10.0_f32, 40.0_f32));
        let slider = RangeSlider::new(0.0, 100.0);
        let value = slider.value();
        let mut runtime = UiRuntime::new(slider);
        let _binding = runtime.bind(&value, &span);
        runtime
            .layout(Constraints::tight(size(100.0, CONTROL_HEIGHT)).expect("valid constraints"))
            .expect("range slider layout succeeds");
        assert_eq!(value.get(), (10.0, 40.0));

        span.set((-5.0, 500.0));
        assert_eq!(value.get(), (0.0, 100.0));
        span.set((70.0, 30.0));
        assert_eq!(value.get(), (30.0, 70.0));
    }

    #[test]
    fn set_value_emits_property_changed_but_not_user_changed() {
        let slider = RangeSlider::new(0.0, 100.0);
        let value = slider.value();
        let property_changes = Rc::new(RefCell::new(0));
        let user_changes = Rc::new(RefCell::new(0));
        let _property = value.subscribe({
            let property_changes = Rc::clone(&property_changes);
            move |_| *property_changes.borrow_mut() += 1
        });
        let _user = slider.changed().subscribe({
            let user_changes = Rc::clone(&user_changes);
            move |_| *user_changes.borrow_mut() += 1
        });
        let mut runtime = UiRuntime::new(slider);
        runtime
            .layout(Constraints::tight(size(100.0, CONTROL_HEIGHT)).expect("valid constraints"))
            .expect("range slider layout succeeds");

        value.set((25.0, 75.0));
        assert_eq!(*property_changes.borrow(), 1);
        assert_eq!(*user_changes.borrow(), 0);

        let _ = runtime.dispatch_event(&pointer(InputEventKind::Down, 8.0));
        let _ = runtime.dispatch_event(&pointer(InputEventKind::Up, 8.0));
        assert_eq!(*property_changes.borrow(), 2);
        assert_eq!(*user_changes.borrow(), 1);
    }

    #[derive(Clone, Copy)]
    enum InputEventKind {
        Down,
        Move,
        Up,
    }

    fn pointer(kind: InputEventKind, x: f32) -> InputEvent {
        let event = PointerEvent {
            pointer_id: PointerId(1),
            position: Point::new(x, 12.0),
            button: Some(PointerButton::Primary),
            buttons: match kind {
                InputEventKind::Down | InputEventKind::Move => PointerButtons::PRIMARY,
                InputEventKind::Up => PointerButtons::NONE,
            },
            modifiers: Modifiers::NONE,
        };
        match kind {
            InputEventKind::Down => InputEvent::PointerDown(event),
            InputEventKind::Move => InputEvent::PointerMove(event),
            InputEventKind::Up => InputEvent::PointerUp(event),
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
}
