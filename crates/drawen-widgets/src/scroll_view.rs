use drawen_core::{Constraints, InputEvent, Point, Size, WheelDelta};
use drawen_ui::{
    ChildLayout, EventContext, EventPhase, EventStatus, LayoutContext, LayoutResult, Property,
    StyleTarget, Widget,
};

/// Scroll direction supported by [`ScrollView`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScrollAxis {
    /// Scroll horizontally.
    Horizontal,
    /// Scroll vertically.
    #[default]
    Vertical,
}

/// A clipped single-child viewport scrollable with the pointer wheel.
///
/// The scroll offset is a [`Property`]: bind application state to it with
/// `runtime.bind_layout(&view.offset(), &state.offset)`. The offset changes the
/// child's position, so use `bind_layout`. The stored value is kept non-negative;
/// the upper bound depends on the measured content and viewport and is applied on
/// the next layout. Read the scalar with `view.offset().get()`.
pub struct ScrollView {
    axis: ScrollAxis,
    offset: Property<f32>,
    viewport: f32,
    content: f32,
}

impl ScrollView {
    /// Creates a scroll view along `axis`.
    #[must_use]
    pub fn new(axis: ScrollAxis) -> Self {
        Self {
            axis,
            offset: Property::normalized(0.0, normalize_offset),
            viewport: 0.0,
            content: 0.0,
        }
    }

    /// Creates a vertically scrolling view.
    #[must_use]
    pub fn vertical() -> Self {
        Self::new(ScrollAxis::Vertical)
    }

    /// Creates a horizontally scrolling view.
    #[must_use]
    pub fn horizontal() -> Self {
        Self::new(ScrollAxis::Horizontal)
    }

    /// Returns the scroll offset property shared with this view.
    ///
    /// The stored value is non-negative; the upper bound depends on the measured
    /// content and viewport and is applied on the next layout. Bind state with
    /// `runtime.bind_layout(&view.offset(), &state.offset)` and read the scalar
    /// with `view.offset().get()`. The property's `changed` signal fires on every
    /// write, including wheel scrolling.
    pub fn offset(&self) -> Property<f32> {
        self.offset.clone()
    }

    /// Sets the scroll offset, clamping it to the known content bounds.
    ///
    /// Emits `offset().changed()` when the clamped offset differs. The upper bound
    /// is re-applied on the next layout once content is measured.
    pub fn set_offset(&mut self, offset: f32) {
        let clamped = offset.clamp(0.0, self.max_offset());
        self.offset.set(clamped);
    }

    fn max_offset(&self) -> f32 {
        (self.content - self.viewport).max(0.0)
    }
}

/// Keeps a scroll offset finite and non-negative; the dynamic upper bound is
/// applied at layout once the content and viewport are measured.
fn normalize_offset(offset: f32) -> f32 {
    if offset.is_finite() {
        offset.max(0.0)
    } else {
        0.0
    }
}

impl Widget for ScrollView {
    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("scroll-view")
    }

    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let max = constraints.max();
        let viewport = constraints.constrain(size(
            if max.width().is_finite() {
                max.width()
            } else {
                constraints.min().width()
            },
            if max.height().is_finite() {
                max.height()
            } else {
                constraints.min().height()
            },
        ));
        self.viewport = axis_length(viewport, self.axis);
        let child_constraints = match self.axis {
            ScrollAxis::Horizontal => Constraints::loose(size(f32::INFINITY, viewport.height())),
            ScrollAxis::Vertical => Constraints::loose(size(viewport.width(), f32::INFINITY)),
        };
        let mut children = Vec::new();
        if context.child_count() == 1 {
            let (id, layout) = context
                .layout_child(0, child_constraints)
                .expect("runtime child index is valid");
            self.content = axis_length(layout.size(), self.axis);
            self.set_offset(self.offset.get());
            let offset = self.offset.get();
            let origin = match self.axis {
                ScrollAxis::Horizontal => Point::new(-offset, 0.0),
                ScrollAxis::Vertical => Point::new(0.0, -offset),
            };
            children.push(ChildLayout::new(id, origin));
        } else {
            self.content = 0.0;
            self.offset.set(0.0);
        }
        LayoutResult::with_children(viewport, children)
    }

    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        if context.phase() == EventPhase::Capture {
            return EventStatus::Ignored;
        }
        let InputEvent::Wheel(wheel) = event else {
            return EventStatus::Ignored;
        };
        let delta = match wheel.delta {
            WheelDelta::Pixels(delta) => axis_coordinate(delta, self.axis),
            WheelDelta::Lines(delta) => axis_coordinate(delta, self.axis) * 40.0,
        };
        let old = self.offset.get();
        self.set_offset(old - delta);
        if (self.offset.get() - old).abs() <= f32::EPSILON {
            return EventStatus::Ignored;
        }
        context.request_layout();
        EventStatus::Handled
    }

    fn clips_children(&self) -> bool {
        true
    }
}

fn axis_length(size: Size, axis: ScrollAxis) -> f32 {
    match axis {
        ScrollAxis::Horizontal => size.width(),
        ScrollAxis::Vertical => size.height(),
    }
}

fn axis_coordinate(point: Point, axis: ScrollAxis) -> f32 {
    match axis {
        ScrollAxis::Horizontal => point.x,
        ScrollAxis::Vertical => point.y,
    }
}

fn size(width: f32, height: f32) -> Size {
    Size::new(width, height).expect("scroll layout sizes are non-negative and not NaN")
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use drawen_core::{Modifiers, Rect, WheelEvent};
    use drawen_render::{DisplayList, PaintContext};
    use drawen_ui::UiRuntime;

    use crate::SizedBox;

    use super::*;

    #[test]
    fn wheel_scrolling_relayouts_children_before_the_next_paint() {
        let mut runtime = UiRuntime::new(ScrollView::vertical());
        let child = runtime
            .append_child(runtime.root(), SizedBox::new(Some(100.0), Some(100.0)))
            .expect("scroll view exists");
        let viewport = Constraints::tight(size(100.0, 20.0)).expect("viewport is finite");
        runtime.layout(viewport).expect("initial layout succeeds");
        assert_eq!(
            runtime.bounds(child),
            Some(Rect::new(Point::ZERO, size(100.0, 100.0)))
        );

        let wheel = InputEvent::Wheel(WheelEvent {
            position: Point::new(5.0, 5.0),
            delta: WheelDelta::Pixels(Point::new(0.0, -20.0)),
            modifiers: Modifiers::NONE,
        });
        assert_eq!(runtime.dispatch_event(&wheel), EventStatus::Handled);
        assert!(runtime.last_layout().is_none());

        let mut display_list = DisplayList::new();
        runtime
            .paint(&mut PaintContext::new(&mut display_list))
            .expect("deferred layout and paint succeed");
        assert_eq!(
            runtime.bounds(child),
            Some(Rect::new(Point::new(0.0, -20.0), size(100.0, 100.0)))
        );
    }

    #[test]
    fn bound_offset_is_clamped_and_relayouts_child() {
        let state = Property::new(0.0_f32);
        let view = ScrollView::vertical();
        let offset = view.offset();
        let mut runtime = UiRuntime::new(view);
        let child = runtime
            .append_child(runtime.root(), SizedBox::new(Some(100.0), Some(100.0)))
            .expect("child appends");
        let _binding = runtime.bind_layout(&offset, &state);
        let viewport = Constraints::tight(size(100.0, 20.0)).expect("viewport is finite");
        runtime.layout(viewport).expect("initial layout succeeds");

        // A binding copy beyond the content bounds is clamped on the next layout
        // (content 100 - viewport 20 = 80) and repositions the child.
        state.set(500.0);
        let mut list = DisplayList::new();
        runtime
            .paint(&mut PaintContext::new(&mut list))
            .expect("paint after a bound offset change succeeds");
        assert!((offset.get() - 80.0).abs() <= f32::EPSILON);
        assert_eq!(
            runtime.bounds(child),
            Some(Rect::new(Point::new(0.0, -80.0), size(100.0, 100.0)))
        );

        // Negative writes normalize to zero even before the next layout.
        state.set(-10.0);
        assert!(offset.get().abs() <= f32::EPSILON);
    }

    #[test]
    fn wheel_scrolling_emits_the_offset_property() {
        let view = ScrollView::vertical();
        let offset = view.offset();
        let changes = Rc::new(RefCell::new(Vec::new()));
        let _subscription = offset.subscribe({
            let changes = Rc::clone(&changes);
            move |value| changes.borrow_mut().push(*value)
        });
        let mut runtime = UiRuntime::new(view);
        runtime
            .append_child(runtime.root(), SizedBox::new(Some(100.0), Some(100.0)))
            .expect("child appends");
        let viewport = Constraints::tight(size(100.0, 20.0)).expect("viewport is finite");
        runtime.layout(viewport).expect("layout succeeds");

        let wheel = InputEvent::Wheel(WheelEvent {
            position: Point::new(5.0, 5.0),
            delta: WheelDelta::Pixels(Point::new(0.0, -20.0)),
            modifiers: Modifiers::NONE,
        });
        assert_eq!(runtime.dispatch_event(&wheel), EventStatus::Handled);
        assert!((offset.get() - 20.0).abs() <= f32::EPSILON);
        assert_eq!(changes.borrow().len(), 1);
    }
}
