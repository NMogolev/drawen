use std::time::Duration;

use drawen_core::{Constraints, InputEvent, Rect};
use drawen_render::PaintContext;

use crate::{
    Accessibility, AccessibilityAction, DssStyle, EventContext, EventStatus, LayoutContext,
    LayoutResult, StyleTarget, UiEnvironment, Widget,
};

/// A transparent widget wrapper that adds DSS ids and classes.
///
/// The wrapper preserves the inner widget's type name and behavior, so a
/// `Styled<Button>` still matches `button` selectors while also matching
/// `#save` and `.primary` rules:
///
/// ```ignore
/// let save = Styled::new(Button::new())
///     .with_id("save")
///     .with_class("primary");
/// ```
pub struct Styled<W> {
    inner: W,
    id: Option<String>,
    classes: Vec<String>,
}

impl<W> Styled<W> {
    /// Wraps `inner` without additional style metadata.
    #[must_use]
    pub const fn new(inner: W) -> Self {
        Self {
            inner,
            id: None,
            classes: Vec::new(),
        }
    }

    /// Assigns the id matched by `#id` selectors.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Adds a class matched by `.class` selectors.
    #[must_use]
    pub fn with_class(mut self, class: impl Into<String>) -> Self {
        let class = class.into();
        if !self.classes.contains(&class) {
            self.classes.push(class);
        }
        self
    }

    /// Returns the wrapped widget.
    #[must_use]
    pub const fn inner(&self) -> &W {
        &self.inner
    }

    /// Returns mutable access to the wrapped widget.
    pub const fn inner_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    /// Returns the wrapped widget.
    #[must_use]
    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W> Default for Styled<W>
where
    W: Default,
{
    fn default() -> Self {
        Self::new(W::default())
    }
}

impl<W> Widget for Styled<W>
where
    W: Widget,
{
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        self.inner.layout(context, constraints)
    }

    fn style_target(&self) -> StyleTarget {
        let mut target = self.inner.style_target();
        if let Some(id) = &self.id {
            target = target.with_id(id.clone());
        }
        for class in &self.classes {
            target = target.with_class(class.clone());
        }
        target
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        self.inner.paint(context, environment, bounds);
    }

    fn paint_styled(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        bounds: Rect,
        style: &DssStyle,
    ) {
        self.inner.paint_styled(context, environment, bounds, style);
    }

    fn advance_animation(&mut self, delta: Duration, settle: bool) -> bool {
        self.inner.advance_animation(delta, settle)
    }

    fn is_animating(&self) -> bool {
        self.inner.is_animating()
    }

    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        self.inner.handle_event(context, event)
    }

    fn accepts_focus(&self) -> bool {
        self.inner.accepts_focus()
    }

    fn accessibility(&self) -> Accessibility {
        self.inner.accessibility()
    }

    fn tab_index(&self) -> Option<i32> {
        self.inner.tab_index()
    }

    fn accessibility_action(&mut self, action: &AccessibilityAction) -> bool {
        self.inner.accessibility_action(action)
    }

    fn participates_in_hit_test(&self) -> bool {
        self.inner.participates_in_hit_test()
    }

    fn clips_children(&self) -> bool {
        self.inner.clips_children()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::rc::Rc;

    use drawen_core::{Clipboard, MemoryClipboard, Size, WidgetId};
    use drawen_render::DisplayList;

    use super::*;
    use crate::event::EventEffects;
    use crate::{AccessibilityRole, AccessibilityState, EventPhase, UiRuntime};

    #[derive(Clone, Default)]
    struct HookCounters {
        layout: Rc<Cell<usize>>,
        paint: Rc<Cell<usize>>,
        paint_styled: Rc<Cell<usize>>,
        advance: Rc<Cell<usize>>,
        event: Rc<Cell<usize>>,
        action: Rc<Cell<usize>>,
    }

    struct HookWidget {
        counters: HookCounters,
        last_action: Rc<RefCell<Option<AccessibilityAction>>>,
    }

    impl HookWidget {
        fn new() -> (Self, HookCounters, Rc<RefCell<Option<AccessibilityAction>>>) {
            let counters = HookCounters::default();
            let last_action = Rc::new(RefCell::new(None));
            (
                Self {
                    counters: counters.clone(),
                    last_action: Rc::clone(&last_action),
                },
                counters,
                last_action,
            )
        }
    }

    impl Widget for HookWidget {
        fn layout(
            &mut self,
            _context: &mut LayoutContext<'_>,
            _constraints: Constraints,
        ) -> LayoutResult {
            self.counters.layout.set(self.counters.layout.get() + 1);
            LayoutResult::new(Size::ZERO)
        }

        fn style_target(&self) -> StyleTarget {
            StyleTarget::new("hook")
        }

        fn paint(
            &self,
            _context: &mut PaintContext<'_>,
            _environment: &UiEnvironment,
            _bounds: Rect,
        ) {
            self.counters.paint.set(self.counters.paint.get() + 1);
        }

        fn paint_styled(
            &self,
            _context: &mut PaintContext<'_>,
            _environment: &UiEnvironment,
            _bounds: Rect,
            _style: &DssStyle,
        ) {
            self.counters
                .paint_styled
                .set(self.counters.paint_styled.get() + 1);
        }

        fn advance_animation(&mut self, _delta: Duration, _settle: bool) -> bool {
            self.counters.advance.set(self.counters.advance.get() + 1);
            true
        }

        fn is_animating(&self) -> bool {
            true
        }

        fn handle_event(
            &mut self,
            _context: &mut EventContext<'_>,
            _event: &InputEvent,
        ) -> EventStatus {
            self.counters.event.set(self.counters.event.get() + 1);
            EventStatus::Handled
        }

        fn accepts_focus(&self) -> bool {
            true
        }

        fn accessibility(&self) -> Accessibility {
            Accessibility::new(AccessibilityRole::Button)
                .with_label("Hook")
                .with_states(AccessibilityState::FOCUSABLE)
        }

        fn tab_index(&self) -> Option<i32> {
            Some(7)
        }

        fn accessibility_action(&mut self, action: &AccessibilityAction) -> bool {
            self.counters.action.set(self.counters.action.get() + 1);
            *self.last_action.borrow_mut() = Some(action.clone());
            true
        }

        fn participates_in_hit_test(&self) -> bool {
            false
        }

        fn clips_children(&self) -> bool {
            true
        }
    }

    #[test]
    fn styled_forwards_every_widget_hook() {
        let (widget, counters, last_action) = HookWidget::new();
        let mut styled = Styled::new(widget).with_id("save").with_class("primary");

        assert_eq!(styled.style_target().type_name(), Some("hook"));
        assert_eq!(styled.style_target().id(), Some("save"));
        assert_eq!(styled.style_target().classes(), ["primary"]);

        let mut display_list = DisplayList::new();
        styled.paint(
            &mut PaintContext::new(&mut display_list),
            &UiEnvironment::default(),
            Rect::ZERO,
        );
        assert_eq!(counters.paint.get(), 1);

        styled.paint_styled(
            &mut PaintContext::new(&mut display_list),
            &UiEnvironment::default(),
            Rect::ZERO,
            &DssStyle::default(),
        );
        assert_eq!(counters.paint_styled.get(), 1);

        assert!(styled.advance_animation(Duration::ZERO, false));
        assert_eq!(counters.advance.get(), 1);
        assert!(styled.is_animating());
        assert!(styled.accepts_focus());

        let accessibility = styled.accessibility();
        assert_eq!(accessibility.role, AccessibilityRole::Button);
        assert_eq!(accessibility.label.as_deref(), Some("Hook"));
        assert!(accessibility.states.contains(AccessibilityState::FOCUSABLE));
        assert_eq!(styled.tab_index(), Some(7));
        assert!(!styled.participates_in_hit_test());
        assert!(styled.clips_children());

        assert!(styled.accessibility_action(&AccessibilityAction::Click));
        assert_eq!(counters.action.get(), 1);
        assert_eq!(*last_action.borrow(), Some(AccessibilityAction::Click));

        let clipboard: Rc<RefCell<Box<dyn Clipboard>>> =
            Rc::new(RefCell::new(Box::new(MemoryClipboard::new())));
        let mut effects = EventEffects {
            propagation_stopped: false,
            pointer_capture: HashMap::new(),
            focused: None,
            focus_request: None,
            layout_requested: false,
            redraw_requested: false,
            window_commands: Vec::new(),
        };
        let id = WidgetId::new(0, 0);
        let mut event_context =
            EventContext::new(EventPhase::Target, id, id, &mut effects, &clipboard);
        assert_eq!(
            styled.handle_event(&mut event_context, &InputEvent::WindowFocusChanged(true)),
            EventStatus::Handled
        );
        assert_eq!(counters.event.get(), 1);

        let mut runtime = UiRuntime::new(styled);
        runtime
            .layout(Constraints::loose(
                Size::new(10.0, 10.0).expect("test size is finite"),
            ))
            .expect("layout succeeds");
        assert_eq!(counters.layout.get(), 1);
    }
}
