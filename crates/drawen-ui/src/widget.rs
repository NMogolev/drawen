use std::time::Duration;

use drawen_core::{Constraints, InputEvent, Rect};
use drawen_render::PaintContext;

use crate::{
    Accessibility, AccessibilityAction, AccessibilityRole, DssStyle, EventContext, EventStatus,
    LayoutContext, LayoutResult, StyleTarget, UiEnvironment,
};

/// A retained UI element whose placement and relationships are owned by the runtime.
///
/// A widget defines its own behavior. Its direct children are retained by
/// [`crate::UiRuntime`], and visible children can be measured through
/// [`LayoutContext`] during layout. Children omitted from the returned
/// [`LayoutResult`] remain retained but are excluded from painting, hit testing,
/// and focus traversal. This permits virtualized containers to measure only the
/// currently visible range.
pub trait Widget {
    /// Lays out this widget and positions its direct runtime-owned children.
    fn layout(&mut self, context: &mut LayoutContext<'_>, constraints: Constraints)
    -> LayoutResult;

    /// Returns the style metadata used to resolve this node's DSS cascade.
    ///
    /// Widgets normally return a [`StyleTarget`] with their type name. The
    /// runtime adds interaction states it owns, such as `:hover` and `:focus`,
    /// before resolving the sheet. Wrap a widget in [`crate::Styled`] when an
    /// application needs to assign an id or classes.
    fn style_target(&self) -> StyleTarget {
        StyleTarget::empty()
    }

    /// Records this widget's own paint operations within its runtime-owned `bounds`.
    ///
    /// The runtime paints descendants after this method returns.
    fn paint(&self, _context: &mut PaintContext<'_>, _environment: &UiEnvironment, _bounds: Rect) {}

    /// Records paint operations that must appear above every normal tree node.
    ///
    /// The runtime calls this after the tree and floating overlay subtrees are
    /// painted, so content can extend beyond `bounds` without being clipped or
    /// covered by later siblings. Tooltips use this hook for their bubble; the
    /// painted region is intentionally outside hit testing and focus traversal.
    fn paint_floating(
        &self,
        _context: &mut PaintContext<'_>,
        _environment: &UiEnvironment,
        _bounds: Rect,
    ) {
    }

    /// Records paint operations using the style resolved for this node.
    ///
    /// The default forwards to [`Widget::paint`] for widgets that only use the
    /// semantic theme. Styled standard widgets override this method so a single
    /// node can consume its resolved `background`, `border`, `padding`,
    /// `border-radius`, and other typed values.
    fn paint_styled(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        bounds: Rect,
        style: &DssStyle,
    ) {
        let _ = style;
        self.paint(context, environment, bounds);
    }

    /// Advances widget-owned, time-driven content by `delta`.
    ///
    /// The runtime calls this before painting every frame. Widgets that play
    /// media, drive procedural effects, or otherwise change visually with time
    /// should update their state here and return whether paint output changed.
    /// `settle` is `true` when the host requested an immediate jump to the
    /// final state, such as when animations are disabled.
    ///
    /// Returning `true` invalidates the retained display list so the next
    /// paint reflects the new state. This method must not emit application
    /// signals; report user-driven changes from [`Widget::handle_event`].
    fn advance_animation(&mut self, _delta: Duration, _settle: bool) -> bool {
        false
    }

    /// Returns whether this widget needs continuous animation frames.
    ///
    /// Platform adapters use this through [`crate::UiRuntime::next_frame_in`]
    /// to schedule the next paint while media is playing.
    fn is_animating(&self) -> bool {
        false
    }

    /// Returns whether animation changes stay inside this widget's bounds.
    ///
    /// When `true`, the runtime damages only the node's bounds plus the
    /// standard effect margin after [`Widget::advance_animation`] reports a
    /// change, so the backend can repaint a small region instead of the whole
    /// window. Keep the default `false` when the widget paints through
    /// [`Widget::paint_floating`], draws shadows or glows larger than the
    /// margin, or otherwise cannot bound its animated output; the runtime then
    /// falls back to full-frame damage, which is always correct.
    fn local_animation_damage(&self) -> bool {
        false
    }

    /// Returns whether this widget must keep advancing while it is not visible.
    ///
    /// The runtime freezes purely visual animations when the window cannot be
    /// presented or when the widget contributes no pixels: scrolled offscreen,
    /// clipped away by a scrolling ancestor, inside a collapsed subtree, or
    /// covered by a modal scrim.
    /// Media widgets return `true` so their audio clocks and session updates keep
    /// running, and stateful widgets such as toast queues return `true` so their
    /// lifetimes keep elapsing. A widget that only changes pixels should leave
    /// the default `false` and resume from the same state when it becomes
    /// visible again.
    fn advances_while_hidden(&self) -> bool {
        false
    }

    /// Returns the caret area for input-method composition, in local coordinates.
    ///
    /// Editable widgets return the caret rectangle so platform adapters can
    /// position the IME candidate window next to the insertion point. The
    /// runtime translates the rectangle to window coordinates for the focused
    /// widget; `None` leaves the candidate window at its previous position.
    fn ime_cursor_area(&self) -> Option<Rect> {
        None
    }

    /// Handles an input event during capture, target, or bubble propagation.
    fn handle_event(
        &mut self,
        _context: &mut EventContext<'_>,
        _event: &InputEvent,
    ) -> EventStatus {
        EventStatus::Ignored
    }

    /// Closes a light-dismissible overlay owned by this widget.
    ///
    /// The runtime calls this when a primary click lands outside the overlay
    /// subtree and its anchor, or when Escape is pressed. Return `true` when
    /// the widget changed state and the runtime should rebuild layout and
    /// paint output.
    fn dismiss_overlay(&mut self) -> bool {
        false
    }

    /// Returns whether this widget may receive keyboard focus.
    fn accepts_focus(&self) -> bool {
        false
    }

    /// Describes this widget to assistive technologies.
    ///
    /// The runtime adds focus state and absolute bounds. Widgets should report
    /// their role, accessible name, value, and interaction states. Returning
    /// the default [`AccessibilityRole::GenericContainer`] keeps the node in
    /// the tree as a structural container.
    fn accessibility(&self) -> Accessibility {
        Accessibility::new(AccessibilityRole::GenericContainer)
    }

    /// Returns this widget's position in the sequential tab order.
    ///
    /// Positive values are visited before zero or unspecified values. A
    /// negative index removes the widget from Tab traversal while leaving it
    /// available to [`crate::UiRuntime::focus_widget`].
    fn tab_index(&self) -> Option<i32> {
        None
    }

    /// Performs an action requested by an assistive technology.
    ///
    /// The runtime validates focus requests itself. Returning `true` means the
    /// widget handled the action and the runtime should invalidate its layout
    /// and paint output.
    fn accessibility_action(&mut self, _action: &AccessibilityAction) -> bool {
        false
    }

    /// Returns whether this widget itself may be selected as a pointer hit-test target.
    ///
    /// Decorative widgets should return `false` so an interactive ancestor can
    /// receive pointer input while still owning the decoration as a child.
    fn participates_in_hit_test(&self) -> bool {
        true
    }

    /// Returns whether the runtime clips all descendant painting to this widget's bounds.
    fn clips_children(&self) -> bool {
        false
    }
}

impl<W> Widget for Box<W>
where
    W: Widget + ?Sized,
{
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        (**self).layout(context, constraints)
    }

    fn style_target(&self) -> StyleTarget {
        (**self).style_target()
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        (**self).paint(context, environment, bounds);
    }

    fn paint_styled(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        bounds: Rect,
        style: &DssStyle,
    ) {
        (**self).paint_styled(context, environment, bounds, style);
    }

    fn paint_floating(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        bounds: Rect,
    ) {
        (**self).paint_floating(context, environment, bounds);
    }

    fn advance_animation(&mut self, delta: Duration, settle: bool) -> bool {
        (**self).advance_animation(delta, settle)
    }

    fn is_animating(&self) -> bool {
        (**self).is_animating()
    }

    fn local_animation_damage(&self) -> bool {
        (**self).local_animation_damage()
    }

    fn advances_while_hidden(&self) -> bool {
        (**self).advances_while_hidden()
    }

    fn ime_cursor_area(&self) -> Option<Rect> {
        (**self).ime_cursor_area()
    }

    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        (**self).handle_event(context, event)
    }

    fn dismiss_overlay(&mut self) -> bool {
        (**self).dismiss_overlay()
    }

    fn accepts_focus(&self) -> bool {
        (**self).accepts_focus()
    }

    fn accessibility(&self) -> Accessibility {
        (**self).accessibility()
    }

    fn tab_index(&self) -> Option<i32> {
        (**self).tab_index()
    }

    fn accessibility_action(&mut self, action: &AccessibilityAction) -> bool {
        (**self).accessibility_action(action)
    }

    fn participates_in_hit_test(&self) -> bool {
        (**self).participates_in_hit_test()
    }

    fn clips_children(&self) -> bool {
        (**self).clips_children()
    }
}
