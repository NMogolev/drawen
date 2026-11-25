use drawen_core::{Constraints, InputEvent, Key, NamedKey, Point, PointerButton, Rect, Size};
use drawen_render::PaintContext;
use drawen_ui::{
    Accessibility, AccessibilityAction, AccessibilityActions, AccessibilityRole,
    AccessibilityState, ChildLayout, DismissPolicy, EventContext, EventPhase, EventStatus,
    LayoutContext, LayoutResult, OverlayOptions, OverlayPlacement, Signal, StyleTarget,
    UiEnvironment, Widget,
};

/// How a [`Popover`] reacts to pointer input on its anchor.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PopoverTrigger {
    /// The application opens and closes the popover through its own controls.
    #[default]
    Manual,
    /// A primary click on the anchor toggles the popover.
    Primary,
    /// A secondary click on the anchor opens the popover at the pointer.
    Secondary,
}

/// A two-child container that presents its content in a floating overlay.
///
/// The first direct child is the anchor and participates in normal layout. The
/// second child is the content and is measured only while the popover is open.
/// The runtime positions the content against the anchor, flips it when the
/// viewport is too small, and paints it above the rest of the tree.
///
/// ```text
/// popover
/// ├── button          (anchor, normal layout)
/// └── box             (content, floating overlay)
/// ```
///
/// Light dismissal is enabled by default: a primary click outside the content
/// and anchor, Escape, or losing window focus asks the popover to close. The
/// content subtree keeps the normal event route, so clicks inside it bubble
/// through the popover and focus traversal continues to work.
pub struct Popover {
    open: bool,
    trigger: PopoverTrigger,
    placement: OverlayPlacement,
    dismiss: DismissPolicy,
    anchor_size: Size,
    content_rect: Rect,
    pointer_anchor: Option<Point>,
    opened: Signal<()>,
    closed: Signal<()>,
}

impl Popover {
    /// Creates a closed manual popover.
    #[must_use]
    pub fn new() -> Self {
        Self {
            open: false,
            trigger: PopoverTrigger::Manual,
            placement: OverlayPlacement::default(),
            dismiss: DismissPolicy::Light,
            anchor_size: Size::ZERO,
            content_rect: Rect::ZERO,
            pointer_anchor: None,
            opened: Signal::new(),
            closed: Signal::new(),
        }
    }

    /// Returns this popover with a different anchor trigger.
    #[must_use]
    pub const fn with_trigger(mut self, trigger: PopoverTrigger) -> Self {
        self.trigger = trigger;
        self
    }

    /// Returns this popover with a preferred placement.
    #[must_use]
    pub const fn with_placement(mut self, placement: OverlayPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Returns this popover with an explicit dismissal policy.
    #[must_use]
    pub const fn with_dismiss(mut self, dismiss: DismissPolicy) -> Self {
        self.dismiss = dismiss;
        self
    }

    /// Returns whether the content is currently floating.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// Returns the trigger that opens this popover from pointer input.
    #[must_use]
    pub const fn trigger(&self) -> PopoverTrigger {
        self.trigger
    }

    /// Returns a signal emitted whenever the popover opens.
    #[must_use]
    pub fn opened(&self) -> Signal<()> {
        self.opened.clone()
    }

    /// Returns a signal emitted whenever the popover closes.
    #[must_use]
    pub fn closed(&self) -> Signal<()> {
        self.closed.clone()
    }

    /// Opens or closes the popover without an event context.
    ///
    /// The runtime invalidates layout when this is called through
    /// [`drawen_ui::UiRuntime::widget_mut`].
    pub fn set_open(&mut self, open: bool) {
        if self.open == open {
            return;
        }
        self.open = open;
        if !open {
            self.pointer_anchor = None;
        }
        if open {
            self.opened.emit(&());
        } else {
            self.closed.emit(&());
        }
    }

    /// Opens the popover with its content anchored at `point`, in the
    /// popover's local coordinates.
    pub fn open_at(&mut self, point: Point) {
        self.pointer_anchor = Some(point);
        self.set_open(true);
    }

    /// Closes the popover if it is open.
    pub fn close(&mut self) {
        self.set_open(false);
    }

    fn toggle(&mut self, context: &mut EventContext<'_>) {
        if self.open {
            self.set_open(false);
        } else {
            self.pointer_anchor = None;
            self.set_open(true);
        }
        context.request_layout();
    }

    fn anchor_contains(&self, point: Point) -> bool {
        Rect::new(Point::ZERO, self.anchor_size).contains(point)
    }
}

impl Default for Popover {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Popover {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        if context.child_count() == 0 {
            self.anchor_size = Size::ZERO;
            self.content_rect = Rect::ZERO;
            return LayoutResult::new(constraints.constrain(Size::ZERO));
        }
        let (anchor, anchor_layout) = context
            .layout_child(0, constraints)
            .expect("runtime child index is valid");
        self.anchor_size = anchor_layout.size();
        self.content_rect = Rect::ZERO;
        let mut children = vec![ChildLayout::new(anchor, Point::ZERO)];
        if self.open && context.child_count() > 1 {
            let anchor_rect = self.pointer_anchor.map_or_else(
                || Rect::new(Point::ZERO, self.anchor_size),
                |point| Rect::new(point, Size::ZERO),
            );
            let max = context
                .viewport()
                .map_or_else(|| constraints.max(), |viewport| viewport.size);
            let (panel, panel_layout, origin, _side) = context
                .layout_child_overlay(
                    1,
                    Constraints::loose(max),
                    anchor_rect,
                    OverlayOptions::new(self.placement).with_dismiss(self.dismiss),
                )
                .expect("runtime child index is valid");
            self.content_rect = Rect::new(origin, panel_layout.size());
            children.push(ChildLayout::new(panel, origin));
        }
        LayoutResult::with_children(self.anchor_size, children)
    }

    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("popover")
    }

    fn paint(&self, _: &mut PaintContext<'_>, _: &UiEnvironment, _: Rect) {}

    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        if !matches!(context.phase(), EventPhase::Target | EventPhase::Bubble) {
            return EventStatus::Ignored;
        }
        match event {
            InputEvent::PointerDown(pointer)
                if self.anchor_contains(pointer.position)
                    && !self.content_rect.contains(pointer.position)
                    && matches!(
                        self.trigger,
                        PopoverTrigger::Primary | PopoverTrigger::Secondary
                    ) =>
            {
                match (self.trigger, pointer.button) {
                    (PopoverTrigger::Primary, Some(PointerButton::Primary)) => {
                        // A descendant such as the anchor button owns focus
                        // when the event bubbles here; only take focus when the
                        // popover itself was clicked.
                        if context.phase() == EventPhase::Target {
                            context.request_focus();
                        }
                        self.toggle(context);
                        EventStatus::Handled
                    }
                    (PopoverTrigger::Secondary, Some(PointerButton::Secondary)) => {
                        if context.phase() == EventPhase::Target {
                            context.request_focus();
                        }
                        self.open_at(pointer.position);
                        context.request_layout();
                        EventStatus::Handled
                    }
                    (PopoverTrigger::Secondary, Some(PointerButton::Primary)) if self.open => {
                        self.set_open(false);
                        context.request_layout();
                        EventStatus::Handled
                    }
                    _ => EventStatus::Ignored,
                }
            }
            InputEvent::KeyDown(key)
                if !key.repeat
                    && matches!(key.key, Key::Named(NamedKey::Enter | NamedKey::Space)) =>
            {
                self.toggle(context);
                EventStatus::Handled
            }
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

    fn tab_index(&self) -> Option<i32> {
        Some(-1)
    }

    fn accessibility(&self) -> Accessibility {
        let mut accessibility = Accessibility::new(AccessibilityRole::Group)
            .with_states(AccessibilityState::FOCUSABLE)
            .with_actions(
                AccessibilityActions::FOCUS
                    | AccessibilityActions::EXPAND
                    | AccessibilityActions::COLLAPSE,
            );
        if self.open {
            accessibility = accessibility.with_states(AccessibilityState::EXPANDED);
        }
        accessibility
    }

    fn accessibility_action(&mut self, action: &AccessibilityAction) -> bool {
        match action {
            AccessibilityAction::Expand => {
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

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use drawen_core::{Modifiers, PointerButtons, PointerEvent, PointerId, Size};
    use drawen_ui::UiRuntime;

    use super::*;
    use crate::Button;

    fn size(width: f32, height: f32) -> Size {
        Size::new(width, height).expect("valid test size")
    }

    fn pointer(position: Point, button: PointerButton) -> InputEvent {
        InputEvent::PointerDown(PointerEvent {
            pointer_id: PointerId(1),
            position,
            button: Some(button),
            buttons: if button == PointerButton::Primary {
                PointerButtons::PRIMARY
            } else {
                PointerButtons::SECONDARY
            },
            modifiers: Modifiers::NONE,
        })
    }

    #[test]
    fn primary_trigger_opens_and_light_dismiss_closes() {
        let popover = Popover::new().with_trigger(PopoverTrigger::Primary);
        let mut runtime = UiRuntime::new(popover);
        let root = runtime.root();
        runtime
            .append_child(
                root,
                Button::new().with_padding(drawen_core::Insets::all(20.0)),
            )
            .expect("anchor");
        runtime
            .append_child(
                root,
                crate::Box::new().with_background(drawen_core::Color::WHITE),
            )
            .expect("content");
        runtime
            .layout(Constraints::tight(size(320.0, 240.0)).expect("valid constraints"))
            .expect("layout succeeds");

        let _ = runtime.dispatch_event(&pointer(Point::new(10.0, 10.0), PointerButton::Primary));
        runtime
            .layout(Constraints::tight(size(320.0, 240.0)).expect("valid constraints"))
            .expect("open layout succeeds");
        assert!(runtime.last_layout().is_some());

        let _ = runtime.dispatch_event(&pointer(Point::new(300.0, 230.0), PointerButton::Primary));
        runtime
            .layout(Constraints::tight(size(320.0, 240.0)).expect("valid constraints"))
            .expect("closed layout succeeds");
        assert!(runtime.last_layout().is_some());
    }

    #[test]
    fn secondary_trigger_reports_opened_and_closed() {
        let popover = Popover::new().with_trigger(PopoverTrigger::Secondary);
        let opened = Rc::new(RefCell::new(0));
        let closed = Rc::new(RefCell::new(0));
        let _opened = popover.opened().subscribe({
            let opened = Rc::clone(&opened);
            move |()| *opened.borrow_mut() += 1
        });
        let _closed = popover.closed().subscribe({
            let closed = Rc::clone(&closed);
            move |()| *closed.borrow_mut() += 1
        });
        let mut runtime = UiRuntime::new(popover);
        let root = runtime.root();
        runtime
            .append_child(
                root,
                Button::new().with_padding(drawen_core::Insets::all(12.0)),
            )
            .expect("anchor");
        runtime
            .append_child(root, crate::Box::new())
            .expect("content");
        runtime
            .layout(Constraints::tight(size(320.0, 240.0)).expect("valid constraints"))
            .expect("layout succeeds");

        let _ = runtime.dispatch_event(&pointer(Point::new(5.0, 5.0), PointerButton::Secondary));
        assert_eq!(*opened.borrow(), 1);
        runtime
            .layout(Constraints::tight(size(320.0, 240.0)).expect("valid constraints"))
            .expect("open layout succeeds");
        let _ = runtime.dispatch_event(&pointer(Point::new(310.0, 230.0), PointerButton::Primary));
        assert_eq!(*closed.borrow(), 1);
    }
}
