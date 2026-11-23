use std::time::Duration;

use drawen_core::{Color, Constraints, InputEvent, Insets, Point, Rect, Size};
use drawen_render::{PaintContext, TextLayout, TextStyle};
use drawen_ui::{
    Accessibility, AccessibilityRole, EventContext, EventStatus, LayoutContext, LayoutResult,
    OverlayOptions, OverlayPlacement, StyleTarget, UiEnvironment, Widget, place_overlay,
};

const DEFAULT_DELAY: Duration = Duration::from_millis(500);
const DEFAULT_PADDING: Insets = Insets::new(5.0, 8.0, 5.0, 8.0);
const TRANSITION_EPSILON: f32 = 0.000_1;

/// A hover hint attached to a single anchor child.
///
/// The anchor is laid out normally. When the pointer rests over the anchor for
/// [`Self::delay`], the tooltip fades in above or below it. The bubble is
/// painted through the runtime's floating pass, so it is not clipped by
/// toolbars or covered by later siblings. It is deliberately outside hit
/// testing: moving the pointer onto the bubble dismisses it.
///
/// ```text
/// tooltip
/// └── button          (the anchor)
/// ```
pub struct Tooltip {
    text: String,
    placement: OverlayPlacement,
    delay: Duration,
    padding: Insets,
    hovered: bool,
    showing: bool,
    opacity: f32,
    delay_remaining: Duration,
    animation: Duration,
    reduced_motion: bool,
    anchor_size: Size,
    anchor_rect: Rect,
    viewport: Option<Rect>,
    bubble_size: Size,
    text_layout: Option<TextLayout>,
    text_color: Color,
    radius: f32,
}

impl Tooltip {
    /// Creates a tooltip that displays `text`.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            placement: OverlayPlacement::top(),
            delay: DEFAULT_DELAY,
            padding: DEFAULT_PADDING,
            hovered: false,
            showing: false,
            opacity: 0.0,
            delay_remaining: Duration::ZERO,
            animation: Duration::ZERO,
            reduced_motion: false,
            anchor_size: Size::ZERO,
            anchor_rect: Rect::ZERO,
            viewport: None,
            bubble_size: Size::ZERO,
            text_layout: None,
            text_color: Color::BLACK,
            radius: 0.0,
        }
    }

    /// Returns this tooltip with a preferred placement.
    #[must_use]
    pub const fn with_placement(mut self, placement: OverlayPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Returns this tooltip with a custom hover delay.
    #[must_use]
    pub const fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Returns this tooltip with custom bubble padding.
    #[must_use]
    pub const fn with_padding(mut self, padding: Insets) -> Self {
        self.padding = padding;
        self
    }

    /// Returns the tooltip text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns whether the bubble is currently visible.
    #[must_use]
    pub fn is_showing(&self) -> bool {
        self.showing || self.opacity > TRANSITION_EPSILON
    }

    fn anchor_contains(&self, point: Point) -> bool {
        Rect::new(Point::ZERO, self.anchor_size).contains(point)
    }

    fn start(&mut self, context: &mut EventContext<'_>) {
        if self.hovered {
            return;
        }
        self.hovered = true;
        if self.text.is_empty() {
            return;
        }
        if self.showing {
            context.request_redraw();
            return;
        }
        if self.delay.is_zero() {
            self.showing = true;
        } else {
            self.delay_remaining = self.delay;
        }
        context.request_redraw();
    }

    fn stop(&mut self, context: &mut EventContext<'_>) {
        if !self.hovered && !self.showing && self.opacity <= TRANSITION_EPSILON {
            return;
        }
        self.hovered = false;
        self.showing = false;
        self.delay_remaining = Duration::ZERO;
        if self.reduced_motion || self.animation.is_zero() {
            self.opacity = 0.0;
        }
        context.request_redraw();
    }
}

impl Widget for Tooltip {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let style = context.style();
        let environment = context.environment();
        let theme = environment.theme();
        let font = environment.font_for(&style);
        self.text_color = style.foreground_or(theme.foreground());
        let text_style = TextStyle::new_with_font(font, self.text_color);
        let text_layout = environment
            .text_shaper()
            .layout(&self.text, &text_style, None);
        self.bubble_size = size(
            text_layout.size().width() + self.padding.horizontal(),
            text_layout.size().height() + self.padding.vertical(),
        );
        self.text_layout = Some(text_layout);
        self.animation = theme.animation_duration();
        self.reduced_motion = !environment.animations_enabled() || self.animation.is_zero();
        self.radius = style
            .corner_radius
            .unwrap_or_else(|| theme.corner_radius() * 0.75);

        let mut children = Vec::new();
        if context.child_count() > 0 {
            let (anchor, anchor_layout) = context
                .layout_child(0, constraints)
                .expect("runtime child index is valid");
            self.anchor_size = anchor_layout.size();
            children.push(drawen_ui::ChildLayout::new(anchor, Point::ZERO));
        } else {
            self.anchor_size = Size::ZERO;
        }

        if self.reduced_motion && self.hovered {
            self.showing = true;
            self.opacity = 1.0;
            self.delay_remaining = Duration::ZERO;
        }
        self.anchor_rect = Rect::new(context.origin(), self.anchor_size);
        self.viewport = context.viewport();

        LayoutResult::with_children(self.anchor_size, children)
    }

    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("tooltip")
    }

    fn paint(&self, _: &mut PaintContext<'_>, _: &UiEnvironment, _: Rect) {}

    fn paint_floating(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, _: Rect) {
        if self.opacity <= TRANSITION_EPSILON {
            return;
        }
        let Some(text) = self.text_layout.as_ref() else {
            return;
        };
        let origin = place_overlay(
            self.anchor_rect,
            self.bubble_size,
            self.viewport,
            OverlayOptions::new(self.placement).with_margin(6.0),
        );
        let bubble = Rect::new(origin, self.bubble_size);
        let theme = environment.theme();
        let opacity = self.opacity.clamp(0.0, 1.0);
        let shadow = theme.shadow();
        context.fill_rounded_rect(
            Rect::new(
                Point::new(bubble.origin.x, bubble.origin.y + 2.0),
                bubble.size,
            ),
            self.radius + 1.0,
            shadow.with_alpha(shadow.alpha * opacity * 0.55),
        );
        let background = theme.raised_surface();
        context.fill_rounded_rect(
            bubble,
            self.radius,
            background.with_alpha(background.alpha * opacity),
        );
        let border = theme.border();
        context.stroke_rounded_rect(
            bubble,
            self.radius,
            1.0,
            border.with_alpha(border.alpha * opacity),
        );
        let color = self.text_color;
        let text = text
            .clone()
            .with_color(color.with_alpha(color.alpha * opacity));
        context.draw_text(
            text,
            Point::new(
                bubble.origin.x + self.padding.left,
                bubble.origin.y + self.padding.top,
            ),
        );
    }

    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        match event {
            InputEvent::PointerEnter(pointer) if self.anchor_contains(pointer.position) => {
                self.start(context);
                EventStatus::Handled
            }
            InputEvent::PointerLeave(pointer) if !self.anchor_contains(pointer.position) => {
                self.stop(context);
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn advance_animation(&mut self, delta: Duration, settle: bool) -> bool {
        let mut changed = false;
        if !self.delay_remaining.is_zero() {
            self.delay_remaining = if settle {
                Duration::ZERO
            } else {
                self.delay_remaining.saturating_sub(delta)
            };
            if self.delay_remaining.is_zero() {
                self.showing = true;
                changed = true;
            }
        }
        let target = if self.showing { 1.0 } else { 0.0 };
        let step = if settle || self.reduced_motion || self.animation.is_zero() {
            1.0
        } else {
            (delta.as_secs_f32() / self.animation.as_secs_f32()).clamp(0.0, 1.0)
        };
        changed |= approach(&mut self.opacity, target, step);
        changed
    }

    fn is_animating(&self) -> bool {
        !self.delay_remaining.is_zero()
            || (self.opacity - if self.showing { 1.0 } else { 0.0 }).abs() > TRANSITION_EPSILON
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }

    fn accessibility(&self) -> Accessibility {
        let mut accessibility =
            Accessibility::new(AccessibilityRole::Tooltip).with_label(self.text.clone());
        accessibility.hidden = !self.is_showing();
        accessibility
    }
}

// Exact comparisons are intentional: values only move between exact targets
// or by clamped arithmetic, so equality marks a settled fade.
#[allow(clippy::float_cmp)]
fn approach(current: &mut f32, target: f32, step: f32) -> bool {
    if (*current - target).abs() <= TRANSITION_EPSILON {
        if *current == target {
            return false;
        }
        *current = target;
        return true;
    }
    let next = if step >= 1.0 {
        target
    } else if *current < target {
        (*current + step).min(target)
    } else {
        (*current - step).max(target)
    };
    if next == *current {
        return false;
    }
    *current = next;
    true
}

fn size(width: f32, height: f32) -> Size {
    Size::new(width, height).expect("tooltip sizes are non-negative and not NaN")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use drawen_core::{Modifiers, PointerButtons, PointerEvent, PointerId, Size};
    use drawen_render::{DisplayCommand, DisplayList, PaintContext};
    use drawen_ui::{LightTheme, Theme, UiRuntime};

    use super::*;
    use crate::Button;

    fn size(width: f32, height: f32) -> Size {
        Size::new(width, height).expect("valid test size")
    }

    fn pointer(position: Point) -> InputEvent {
        InputEvent::PointerMove(PointerEvent {
            pointer_id: PointerId(1),
            position,
            button: None,
            buttons: PointerButtons::NONE,
            modifiers: Modifiers::NONE,
        })
    }

    #[test]
    fn shows_after_the_hover_delay_and_hides_on_leave() {
        let mut runtime = UiRuntime::new(
            Tooltip::new("Refresh the list")
                .with_delay(Duration::from_millis(100))
                .with_placement(OverlayPlacement::bottom()),
        );
        let root = runtime.root();
        runtime
            .append_child(
                root,
                Button::new().with_padding(drawen_core::Insets::all(10.0)),
            )
            .expect("anchor");
        runtime
            .layout(Constraints::loose(size(320.0, 240.0)))
            .expect("layout succeeds");

        let _ = runtime.dispatch_event(&pointer(Point::new(5.0, 5.0)));
        runtime.advance_animations(Duration::from_millis(50));
        runtime.advance_animations(Duration::from_millis(60));
        runtime.advance_animations(Duration::from_millis(200));
        let mut display_list = DisplayList::new();
        runtime
            .paint(&mut PaintContext::new(&mut display_list))
            .expect("tooltip paint succeeds");
        assert!(display_list.commands().iter().any(|command| matches!(
            command,
            DisplayCommand::FillRoundedRect { color, .. } if *color == LightTheme.raised_surface()
        )));

        let _ = runtime.dispatch_event(&pointer(Point::new(200.0, 200.0)));
        runtime.advance_animations(Duration::from_millis(200));
        let mut display_list = DisplayList::new();
        runtime
            .paint(&mut PaintContext::new(&mut display_list))
            .expect("tooltip paint succeeds");
        assert!(!display_list.commands().iter().any(|command| matches!(
            command,
            DisplayCommand::FillRoundedRect { color, .. } if *color == LightTheme.raised_surface()
        )));
    }
}
