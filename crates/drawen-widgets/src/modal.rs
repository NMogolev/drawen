use std::{cell::Cell, rc::Rc};

use drawen_core::{Constraints, Size};
use drawen_ui::{
    ChildLayout, DismissPolicy, LayoutContext, LayoutResult, Signal, StyleTarget, Widget,
};

/// Shared open state for a [`Modal`].
///
/// A handle lets application code open or close a modal after the widget has
/// been appended to the runtime. Changing the state does not request a layout
/// pass; call [`drawen_ui::UiRuntime::invalidate_layout`] afterwards, or set the
/// application's own dirty flag and invalidate once per event.
#[derive(Clone)]
pub struct ModalHandle {
    open: Rc<Cell<bool>>,
    opened: Signal<()>,
    closed: Signal<()>,
}

impl ModalHandle {
    /// Returns whether the modal is open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open.get()
    }

    /// Opens or closes the modal and emits `opened` or `closed` when the state
    /// actually changes. Returns whether it changed.
    #[must_use]
    pub fn set_open(&self, open: bool) -> bool {
        if self.open.get() == open {
            return false;
        }
        self.open.set(open);
        if open {
            self.opened.emit(&());
        } else {
            self.closed.emit(&());
        }
        true
    }

    /// Opens the modal.
    pub fn open(&self) {
        let _ = self.set_open(true);
    }

    /// Closes the modal.
    pub fn close(&self) {
        let _ = self.set_open(false);
    }

    /// Returns a signal emitted whenever the modal opens.
    #[must_use]
    pub fn opened(&self) -> Signal<()> {
        self.opened.clone()
    }

    /// Returns a signal emitted whenever the modal closes.
    #[must_use]
    pub fn closed(&self) -> Signal<()> {
        self.closed.clone()
    }
}

/// A one-child host that presents its child as a centered modal overlay.
///
/// The child is measured only while the modal is open. The runtime centers it
/// in the window, paints a themed scrim behind it, blocks pointer input to the
/// rest of the tree, and traps sequential focus inside the modal.
///
/// ```text
/// modal
/// └── dialog          (centered panel, modal overlay root)
///     └── column      (title, body, actions)
/// ```
///
/// Use [`Self::handle`] to open and close the modal from application code:
///
/// ```rust,ignore
/// let modal = Modal::new();
/// let handle = modal.handle();
/// let modal = runtime.append_child(root, modal)?;
/// let dialog = runtime.append_child(modal, Dialog::new())?;
/// // ...
/// handle.open();
/// runtime.invalidate_layout();
/// ```
///
/// Light dismissal is enabled by default: a primary click on the scrim, Escape,
/// or losing window focus closes the modal through [`Widget::dismiss_overlay`].
/// Use [`DismissPolicy::None`] for surfaces the application closes itself.
pub struct Modal {
    open: Rc<Cell<bool>>,
    dismiss: DismissPolicy,
    margin: f32,
    opened: Signal<()>,
    closed: Signal<()>,
}

impl Modal {
    /// Creates a closed modal with light dismissal.
    #[must_use]
    pub fn new() -> Self {
        Self {
            open: Rc::new(Cell::new(false)),
            dismiss: DismissPolicy::Light,
            margin: 24.0,
            opened: Signal::new(),
            closed: Signal::new(),
        }
    }

    /// Returns this modal with an initial open state.
    ///
    /// This does not emit `opened`; it configures the first layout.
    #[must_use]
    pub fn with_open(self, open: bool) -> Self {
        self.open.set(open);
        self
    }

    /// Returns this modal with an explicit dismissal policy.
    #[must_use]
    pub const fn with_dismiss(mut self, dismiss: DismissPolicy) -> Self {
        self.dismiss = dismiss;
        self
    }

    /// Returns this modal with a minimum distance from the window edges.
    #[must_use]
    pub const fn with_margin(mut self, margin: f32) -> Self {
        self.margin = margin.max(0.0);
        self
    }

    /// Returns whether the modal is open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open.get()
    }

    /// Returns a cloneable handle that can open and close this modal.
    #[must_use]
    pub fn handle(&self) -> ModalHandle {
        ModalHandle {
            open: Rc::clone(&self.open),
            opened: self.opened.clone(),
            closed: self.closed.clone(),
        }
    }

    /// Returns a signal emitted whenever the modal opens.
    #[must_use]
    pub fn opened(&self) -> Signal<()> {
        self.opened.clone()
    }

    /// Returns a signal emitted whenever the modal closes.
    #[must_use]
    pub fn closed(&self) -> Signal<()> {
        self.closed.clone()
    }

    /// Opens or closes the modal and emits the matching signal.
    pub fn set_open(&mut self, open: bool) {
        if self.open.get() == open {
            return;
        }
        self.open.set(open);
        if open {
            self.opened.emit(&());
        } else {
            self.closed.emit(&());
        }
    }
}

impl Default for Modal {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Modal {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        if context.child_count() == 0 || !self.open.get() {
            return LayoutResult::new(Size::ZERO);
        }
        let max = context.viewport().map_or_else(
            || constraints.max(),
            |viewport| {
                Size::new(
                    (viewport.size.width() - self.margin * 2.0).max(0.0),
                    (viewport.size.height() - self.margin * 2.0).max(0.0),
                )
                .unwrap_or(Size::ZERO)
            },
        );
        let (panel, _, origin) = context
            .layout_child_modal(0, Constraints::loose(max), self.dismiss)
            .expect("runtime child index is valid");
        LayoutResult::with_children(Size::ZERO, vec![ChildLayout::new(panel, origin)])
    }

    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("modal")
    }

    fn dismiss_overlay(&mut self) -> bool {
        if !self.open.get() {
            return false;
        }
        self.set_open(false);
        true
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use drawen_core::{Constraints, Key, KeyCode, Modifiers, NamedKey, Size};
    use drawen_ui::UiRuntime;

    use super::Modal;
    use crate::{Button, Dialog};

    fn size(width: f32, height: f32) -> Size {
        Size::new(width, height).expect("valid test size")
    }

    fn escape() -> drawen_core::InputEvent {
        drawen_core::InputEvent::KeyDown(drawen_core::KeyEvent {
            key: Key::Named(NamedKey::Escape),
            code: KeyCode::Unidentified,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
    }

    fn tab() -> drawen_core::InputEvent {
        drawen_core::InputEvent::KeyDown(drawen_core::KeyEvent {
            key: Key::Named(NamedKey::Tab),
            code: KeyCode::Unidentified,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
    }

    #[test]
    fn opening_emits_once_and_centers_the_panel() {
        let modal = Modal::new();
        let opened = Rc::new(Cell::new(0));
        let closed = Rc::new(Cell::new(0));
        let _opened = modal.opened().subscribe({
            let opened = Rc::clone(&opened);
            move |()| opened.set(opened.get() + 1)
        });
        let _closed = modal.closed().subscribe({
            let closed = Rc::clone(&closed);
            move |()| closed.set(closed.get() + 1)
        });
        let handle = modal.handle();
        let mut runtime = UiRuntime::new(modal);
        let root = runtime.root();
        let panel = runtime.append_child(root, Dialog::new()).expect("panel");
        runtime.append_child(panel, Button::new()).expect("action");
        runtime
            .layout(Constraints::tight(size(400.0, 300.0)).expect("valid constraints"))
            .expect("closed layout succeeds");
        assert_eq!(runtime.tab_order(), [] as [drawen_core::WidgetId; 0]);

        handle.open();
        assert!(handle.is_open());
        assert_eq!(opened.get(), 1);
        handle.open();
        assert_eq!(opened.get(), 1);
        runtime.invalidate_layout();
        runtime
            .layout(Constraints::tight(size(400.0, 300.0)).expect("valid constraints"))
            .expect("open layout succeeds");

        let bounds = runtime.bounds(panel).expect("panel has bounds");
        let center_x = bounds.origin.x + bounds.size.width() / 2.0;
        let center_y = bounds.origin.y + bounds.size.height() / 2.0;
        assert!((center_x - 200.0).abs() < f32::EPSILON);
        assert!((center_y - 150.0).abs() < f32::EPSILON);

        handle.close();
        assert_eq!(closed.get(), 1);
    }

    #[test]
    fn escape_closes_a_light_modal() {
        let modal = Modal::new();
        let handle = modal.handle();
        let mut runtime = UiRuntime::new(modal);
        let root = runtime.root();
        let panel = runtime.append_child(root, Dialog::new()).expect("panel");
        runtime.append_child(panel, Button::new()).expect("action");
        handle.open();
        runtime
            .layout(Constraints::tight(size(400.0, 300.0)).expect("valid constraints"))
            .expect("layout succeeds");

        let _ = runtime.dispatch_event(&escape());
        assert!(!handle.is_open());
    }

    #[test]
    fn tab_is_trapped_inside_the_modal() {
        let modal = Modal::new();
        let handle = modal.handle();
        let mut runtime = UiRuntime::new(crate::Stack::new());
        let root = runtime.root();
        let modal = runtime.append_child(root, modal).expect("modal host");
        let panel = runtime.append_child(modal, Dialog::new()).expect("panel");
        let content = runtime
            .append_child(panel, drawen_ui::Column)
            .expect("content");
        let first = runtime
            .append_child(content, Button::new())
            .expect("first action");
        let second = runtime
            .append_child(content, Button::new())
            .expect("second action");
        let outside = runtime
            .append_child(root, Button::new())
            .expect("outside action");
        handle.open();
        runtime
            .layout(Constraints::tight(size(400.0, 300.0)).expect("valid constraints"))
            .expect("layout succeeds");

        assert_eq!(runtime.tab_order(), vec![first, second]);
        let _ = runtime.dispatch_event(&tab());
        assert_eq!(runtime.focused_widget(), Some(first));
        let _ = runtime.dispatch_event(&tab());
        assert_eq!(runtime.focused_widget(), Some(second));
        let _ = runtime.dispatch_event(&tab());
        assert_eq!(runtime.focused_widget(), Some(first));
        assert_ne!(runtime.focused_widget(), Some(outside));
    }

    fn focus_restore_runtime() -> (UiRuntime, super::ModalHandle, drawen_core::WidgetId) {
        let modal = Modal::new();
        let handle = modal.handle();
        let mut runtime = UiRuntime::new(crate::Stack::new());
        let root = runtime.root();
        let opener = runtime.append_child(root, Button::new()).expect("opener");
        let modal = runtime.append_child(root, modal).expect("modal host");
        let panel = runtime.append_child(modal, Dialog::new()).expect("panel");
        runtime.append_child(panel, Button::new()).expect("action");
        runtime
            .layout(Constraints::tight(size(400.0, 300.0)).expect("valid constraints"))
            .expect("closed layout succeeds");
        runtime.focus_widget(opener).expect("opener is focusable");
        (runtime, handle, opener)
    }

    #[test]
    fn escape_restores_focus_to_the_opener() {
        let (mut runtime, handle, opener) = focus_restore_runtime();
        handle.open();
        runtime.invalidate_layout();
        runtime
            .layout(Constraints::tight(size(400.0, 300.0)).expect("valid constraints"))
            .expect("open layout succeeds");
        assert_eq!(runtime.focused_widget(), Some(opener));

        let _ = runtime.dispatch_event(&tab());
        assert_ne!(runtime.focused_widget(), Some(opener));
        let _ = runtime.dispatch_event(&escape());
        assert!(!handle.is_open());
        assert_eq!(
            runtime.focused_widget(),
            Some(opener),
            "closing the modal must return focus to the widget that opened it"
        );
    }

    #[test]
    fn closing_through_the_handle_restores_focus_after_layout() {
        let (mut runtime, handle, opener) = focus_restore_runtime();
        handle.open();
        runtime.invalidate_layout();
        runtime
            .layout(Constraints::tight(size(400.0, 300.0)).expect("valid constraints"))
            .expect("open layout succeeds");
        let _ = runtime.dispatch_event(&tab());
        assert_ne!(runtime.focused_widget(), Some(opener));

        handle.close();
        runtime.invalidate_layout();
        runtime
            .layout(Constraints::tight(size(400.0, 300.0)).expect("valid constraints"))
            .expect("closed layout succeeds");
        assert_eq!(runtime.focused_widget(), Some(opener));
    }

    #[test]
    fn nested_overlay_restores_focus_step_by_step() {
        use std::{cell::Cell, rc::Rc};

        use crate::{Popover, PopoverTrigger};

        let modal = Modal::new();
        let handle = modal.handle();
        let popover = Popover::new().with_trigger(PopoverTrigger::Primary);
        let popover_closed = Rc::new(Cell::new(0));
        let _closed = popover.closed().subscribe({
            let popover_closed = Rc::clone(&popover_closed);
            move |()| popover_closed.set(popover_closed.get() + 1)
        });
        let mut runtime = UiRuntime::new(crate::Stack::new());
        let root = runtime.root();
        let opener = runtime.append_child(root, Button::new()).expect("opener");
        let modal = runtime.append_child(root, modal).expect("modal host");
        let panel = runtime.append_child(modal, Dialog::new()).expect("panel");
        let column = runtime
            .append_child(panel, drawen_ui::Column)
            .expect("column");
        let popover = runtime.append_child(column, popover).expect("popover");
        let anchor = runtime
            .append_child(
                popover,
                Button::new().with_padding(drawen_core::Insets::all(12.0)),
            )
            .expect("anchor");
        let content = runtime
            .append_child(popover, crate::Box::new())
            .expect("content");
        let inner = runtime
            .append_child(
                content,
                Button::new().with_padding(drawen_core::Insets::all(12.0)),
            )
            .expect("inner action");
        let constraints = Constraints::tight(size(400.0, 300.0)).expect("valid constraints");
        runtime.layout(constraints).expect("closed layout succeeds");
        runtime.focus_widget(opener).expect("opener is focusable");
        handle.open();
        runtime.invalidate_layout();
        runtime.layout(constraints).expect("open layout succeeds");

        // Click the popover anchor inside the modal to open the nested menu.
        let anchor_bounds = runtime.bounds(anchor).expect("anchor has bounds");
        let center = drawen_core::Point::new(
            anchor_bounds.origin.x + anchor_bounds.size.width() / 2.0,
            anchor_bounds.origin.y + anchor_bounds.size.height() / 2.0,
        );
        let _ = runtime.dispatch_event(&drawen_core::InputEvent::PointerDown(
            drawen_core::PointerEvent {
                pointer_id: drawen_core::PointerId(1),
                position: center,
                button: Some(drawen_core::PointerButton::Primary),
                buttons: drawen_core::PointerButtons::PRIMARY,
                modifiers: drawen_core::Modifiers::NONE,
            },
        ));
        runtime
            .layout(constraints)
            .expect("popover layout succeeds");
        assert_eq!(runtime.focused_widget(), Some(anchor));

        let _ = runtime.dispatch_event(&tab());
        assert_eq!(runtime.focused_widget(), Some(inner));

        // First Escape closes the popover and returns focus to its anchor.
        let _ = runtime.dispatch_event(&escape());
        assert_eq!(popover_closed.get(), 1);
        assert!(handle.is_open());
        assert_eq!(runtime.focused_widget(), Some(anchor));

        // Second Escape closes the modal and returns focus to the opener.
        let _ = runtime.dispatch_event(&escape());
        assert!(!handle.is_open());
        assert_eq!(runtime.focused_widget(), Some(opener));
    }
}
