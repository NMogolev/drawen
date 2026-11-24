use drawen_core::{
    Color, Constraints, InputEvent, Insets, Key, NamedKey, Point, PointerButton, Rect, Size,
};
use drawen_render::{PaintContext, TextLayout, TextStyle};
use drawen_ui::{
    Accessibility, AccessibilityActions, AccessibilityRole, AccessibilityState, EventContext,
    EventPhase, EventStatus, LayoutContext, LayoutResult, Signal, StyleTarget, UiEnvironment,
    Widget,
};

const MIN_WIDTH: f32 = 160.0;
const ITEM_HEIGHT: f32 = 30.0;
const SEPARATOR_HEIGHT: f32 = 9.0;
const ITEM_INSET: f32 = 8.0;
const SHORTCUT_GAP: f32 = 24.0;
const DEFAULT_PADDING: Insets = Insets::new(4.0, 4.0, 4.0, 4.0);
const DISABLED_ALPHA: f32 = 0.45;

/// One row in a [`Menu`].
///
/// Items carry plain data instead of child widgets because a menu paints a
/// variable number of rows, highlights the active row, and must survive being
/// opened and closed without rebuilding its subtree.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuEntry {
    label: String,
    shortcut: Option<String>,
    enabled: bool,
    separator: bool,
}

impl MenuEntry {
    /// Creates a selectable item with `label`.
    #[must_use]
    pub fn item(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            shortcut: None,
            enabled: true,
            separator: false,
        }
    }

    /// Creates a horizontal separator.
    #[must_use]
    pub const fn separator() -> Self {
        Self {
            label: String::new(),
            shortcut: None,
            enabled: false,
            separator: true,
        }
    }

    /// Returns this item with a right-aligned shortcut hint.
    #[must_use]
    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// Returns this item disabled, keeping its label visible but muted.
    #[must_use]
    pub const fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Returns this item with an explicit enabled state.
    #[must_use]
    pub const fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Returns the visible label, or an empty string for a separator.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the shortcut hint, if one is configured.
    #[must_use]
    pub fn shortcut(&self) -> Option<&str> {
        self.shortcut.as_deref()
    }

    /// Returns whether this row is a separator.
    #[must_use]
    pub const fn is_separator(&self) -> bool {
        self.separator
    }

    /// Returns whether this row can be activated.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled && !self.separator
    }
}

/// A vertical list of actions suitable for a popover, context menu, or dialog.
///
/// `Menu` is data-driven: it owns its labels, paints its own surface, and
/// handles pointer and keyboard navigation without child widgets. Place it in a
/// [`crate::Popover`] content slot to get an anchored dropdown or context menu;
/// selecting an item asks the runtime to dismiss that overlay.
///
/// ```text
/// popover
/// ├── button          (anchor)
/// └── menu            (floating content)
/// ```
pub struct Menu {
    entries: Vec<MenuEntry>,
    active: Option<usize>,
    pressed: Option<usize>,
    selected: Signal<usize>,
    size: Size,
    padding: Insets,
    text_layouts: Vec<Option<TextLayout>>,
    shortcut_layouts: Vec<Option<TextLayout>>,
    text_color: Color,
    muted_color: Color,
    hover_color: Color,
    surface_color: Color,
    border_color: Color,
    radius: f32,
    shadow_color: Color,
}

impl Menu {
    /// Creates a menu from `entries`.
    #[must_use]
    pub fn new(entries: impl IntoIterator<Item = MenuEntry>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
            active: None,
            pressed: None,
            selected: Signal::new(),
            size: Size::ZERO,
            padding: DEFAULT_PADDING,
            text_layouts: Vec::new(),
            shortcut_layouts: Vec::new(),
            text_color: Color::BLACK,
            muted_color: Color::BLACK,
            hover_color: Color::TRANSPARENT,
            surface_color: Color::WHITE,
            border_color: Color::TRANSPARENT,
            radius: 0.0,
            shadow_color: Color::TRANSPARENT,
        }
    }

    /// Returns the configured entries in display order.
    #[must_use]
    pub fn entries(&self) -> &[MenuEntry] {
        &self.entries
    }

    /// Returns the index of the highlighted row, if any.
    #[must_use]
    pub const fn active(&self) -> Option<usize> {
        self.active
    }

    /// Returns a signal emitted with the selected entry index.
    ///
    /// Separators and disabled entries never emit.
    #[must_use]
    pub fn selected(&self) -> Signal<usize> {
        self.selected.clone()
    }

    fn entry_height(entry: &MenuEntry) -> f32 {
        if entry.is_separator() {
            SEPARATOR_HEIGHT
        } else {
            ITEM_HEIGHT
        }
    }

    fn index_at(&self, point: Point) -> Option<usize> {
        if point.x < 0.0 || point.x > self.size.width() {
            return None;
        }
        let mut y = self.padding.top;
        for (index, entry) in self.entries.iter().enumerate() {
            let height = Self::entry_height(entry);
            if point.y >= y && point.y < y + height {
                return (!entry.is_separator()).then_some(index);
            }
            y += height;
        }
        None
    }

    fn set_active(&mut self, active: Option<usize>, context: &mut EventContext<'_>) {
        if self.active == active {
            return;
        }
        self.active = active;
        context.request_redraw();
    }

    fn step(&mut self, forward: bool, context: &mut EventContext<'_>) {
        let len = self.entries.len();
        if len == 0 {
            return;
        }
        let mut index = self.active;
        for _ in 0..len {
            index = match index {
                Some(current) if forward => Some((current + 1) % len),
                Some(current) => Some((current + len - 1) % len),
                None if forward => Some(0),
                None => Some(len - 1),
            };
            let Some(candidate) = index else {
                return;
            };
            if self.entries[candidate].is_enabled() {
                self.set_active(Some(candidate), context);
                return;
            }
        }
    }

    fn edge(&mut self, forward: bool, context: &mut EventContext<'_>) {
        let candidates: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new(0..self.entries.len())
        } else {
            Box::new((0..self.entries.len()).rev())
        };
        if let Some(index) = candidates
            .into_iter()
            .find(|index| self.entries[*index].is_enabled())
        {
            self.set_active(Some(index), context);
        }
    }

    fn activate(&mut self, index: usize, context: &mut EventContext<'_>) -> bool {
        let Some(entry) = self.entries.get(index) else {
            return false;
        };
        if !entry.is_enabled() {
            return false;
        }
        self.active = Some(index);
        self.selected.emit(&index);
        context.request_overlay_dismiss();
        context.request_redraw();
        true
    }
}

impl Widget for Menu {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let style = context.style();
        let environment = context.environment();
        let theme = environment.theme();
        let font = environment.font_for(&style);
        self.padding = style.padding.unwrap_or(DEFAULT_PADDING);
        self.text_color = style.foreground_or(theme.foreground());
        self.muted_color = theme.muted_foreground();
        self.hover_color = theme.control_hover_background();
        self.surface_color = style.background.unwrap_or_else(|| theme.raised_surface());
        self.border_color = style
            .border
            .map_or_else(|| theme.border(), |border| border.color);
        self.radius = style.corner_radius.unwrap_or_else(|| theme.corner_radius());
        self.shadow_color = theme.shadow();

        self.text_layouts.clear();
        self.shortcut_layouts.clear();
        let mut label_width: f32 = 0.0;
        let mut shortcut_width: f32 = 0.0;
        let mut height = self.padding.vertical();
        for entry in &self.entries {
            height += Self::entry_height(entry);
            if entry.is_separator() {
                self.text_layouts.push(None);
                self.shortcut_layouts.push(None);
                continue;
            }
            let layout = environment.text_shaper().layout(
                entry.label(),
                &TextStyle::new_with_font(font.clone(), self.text_color),
                None,
            );
            label_width = label_width.max(layout.size().width());
            self.text_layouts.push(Some(layout));
            let shortcut = entry.shortcut().map(|shortcut| {
                let layout = environment.text_shaper().layout(
                    shortcut,
                    &TextStyle::new_with_font(font.clone(), self.muted_color),
                    None,
                );
                shortcut_width = shortcut_width.max(layout.size().width());
                layout
            });
            self.shortcut_layouts.push(shortcut);
        }

        let shortcut_space = if shortcut_width > 0.0 {
            SHORTCUT_GAP + shortcut_width
        } else {
            0.0
        };
        let width = (label_width + shortcut_space + self.padding.horizontal() + ITEM_INSET * 2.0)
            .max(MIN_WIDTH);
        self.size = constraints.constrain(size(width, height));
        LayoutResult::new(self.size)
    }

    fn paint(&self, context: &mut PaintContext<'_>, _: &UiEnvironment, bounds: Rect) {
        if self.size.width() <= 0.0 || self.size.height() <= 0.0 {
            return;
        }
        let surface = Rect::new(bounds.origin, self.size);
        let shadow = Rect::new(
            Point::new(surface.origin.x, surface.origin.y + 2.0),
            surface.size,
        );
        context.fill_rounded_rect(shadow, self.radius, self.shadow_color.with_alpha(0.35));
        context.fill_rounded_rect(surface, self.radius, self.surface_color);
        context.stroke_rounded_rect(surface, self.radius, 1.0, self.border_color);

        let mut y = surface.origin.y + self.padding.top;
        for (index, entry) in self.entries.iter().enumerate() {
            let height = Self::entry_height(entry);
            if entry.is_separator() {
                let line = Rect::new(
                    Point::new(surface.origin.x + self.padding.left, y + height / 2.0),
                    size(surface.size.width() - self.padding.horizontal(), 1.0),
                );
                context.fill_rect(line, self.border_color);
                y += height;
                continue;
            }

            let row = Rect::new(
                Point::new(surface.origin.x + self.padding.left, y),
                size(
                    surface.size.width() - self.padding.horizontal(),
                    ITEM_HEIGHT,
                ),
            );
            if self.active == Some(index) && entry.is_enabled() {
                context.fill_rounded_rect(row, self.radius * 0.6, self.hover_color);
            }
            let text_color = if entry.is_enabled() {
                self.text_color
            } else {
                self.text_color.with_alpha(DISABLED_ALPHA)
            };
            if let Some(text) = &self.text_layouts[index] {
                let text = text.clone().with_color(text_color);
                let text_height = text.size().height();
                context.draw_text(
                    text,
                    Point::new(
                        row.origin.x + ITEM_INSET,
                        row.origin.y + (ITEM_HEIGHT - text_height) / 2.0,
                    ),
                );
            }
            if let Some(shortcut) = &self.shortcut_layouts[index] {
                let shortcut = shortcut.clone().with_color(self.muted_color.with_alpha(
                    if entry.is_enabled() {
                        1.0
                    } else {
                        DISABLED_ALPHA
                    },
                ));
                let shortcut_width = shortcut.size().width();
                let shortcut_height = shortcut.size().height();
                context.draw_text(
                    shortcut,
                    Point::new(
                        row.right() - ITEM_INSET - shortcut_width,
                        row.origin.y + (ITEM_HEIGHT - shortcut_height) / 2.0,
                    ),
                );
            }
            y += height;
        }
    }

    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        if context.phase() != EventPhase::Target {
            return EventStatus::Ignored;
        }
        match event {
            InputEvent::PointerMove(pointer) => {
                let index = self.index_at(pointer.position);
                if index != self.active {
                    self.set_active(index, context);
                }
                EventStatus::Ignored
            }
            InputEvent::PointerDown(pointer) if pointer.button == Some(PointerButton::Primary) => {
                context.request_focus();
                context.capture_pointer(pointer.pointer_id);
                self.pressed = self.index_at(pointer.position);
                self.set_active(self.pressed, context);
                EventStatus::Handled
            }
            InputEvent::PointerUp(pointer) if pointer.button == Some(PointerButton::Primary) => {
                context.release_pointer(pointer.pointer_id);
                let index = self.index_at(pointer.position);
                let pressed = self.pressed.take();
                let handled = pressed.is_some() && pressed == index && {
                    let index = index.expect("pressed index matches the release index");
                    self.activate(index, context)
                };
                context.request_redraw();
                if handled {
                    EventStatus::Handled
                } else {
                    EventStatus::Ignored
                }
            }
            InputEvent::PointerCancel(_) => {
                self.pressed = None;
                EventStatus::Ignored
            }
            InputEvent::PointerLeave(_) => {
                if self.pressed.is_none() {
                    self.set_active(None, context);
                }
                EventStatus::Ignored
            }
            InputEvent::KeyDown(key) if !key.repeat => match key.key {
                Key::Named(NamedKey::ArrowDown) => {
                    self.step(true, context);
                    EventStatus::Handled
                }
                Key::Named(NamedKey::ArrowUp) => {
                    self.step(false, context);
                    EventStatus::Handled
                }
                Key::Named(NamedKey::Home) => {
                    self.edge(true, context);
                    EventStatus::Handled
                }
                Key::Named(NamedKey::End) => {
                    self.edge(false, context);
                    EventStatus::Handled
                }
                Key::Named(NamedKey::Enter | NamedKey::Space) => {
                    let Some(index) = self.active else {
                        return EventStatus::Ignored;
                    };
                    if self.activate(index, context) {
                        EventStatus::Handled
                    } else {
                        EventStatus::Ignored
                    }
                }
                _ => EventStatus::Ignored,
            },
            _ => EventStatus::Ignored,
        }
    }

    fn accepts_focus(&self) -> bool {
        true
    }

    fn tab_index(&self) -> Option<i32> {
        Some(-1)
    }

    fn accessibility(&self) -> Accessibility {
        Accessibility::new(AccessibilityRole::Menu)
            .with_states(AccessibilityState::FOCUSABLE)
            .with_actions(AccessibilityActions::FOCUS)
    }

    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("menu")
    }
}

fn size(width: f32, height: f32) -> Size {
    Size::new(width, height).expect("menu sizes are non-negative and not NaN")
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use drawen_core::{
        Constraints, InputEvent, Key, KeyCode, Modifiers, NamedKey, PointerButtons, PointerEvent,
        PointerId, Size,
    };
    use drawen_ui::UiRuntime;

    use super::{Menu, MenuEntry};

    fn size(width: f32, height: f32) -> Size {
        Size::new(width, height).expect("valid test size")
    }

    fn pointer(position: drawen_core::Point, up: bool) -> InputEvent {
        let event = PointerEvent {
            pointer_id: PointerId(1),
            position,
            button: Some(drawen_core::PointerButton::Primary),
            buttons: PointerButtons::PRIMARY,
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

    fn sample_menu() -> Menu {
        Menu::new([
            MenuEntry::item("Cut").with_shortcut("Ctrl+X"),
            MenuEntry::item("Copy").with_shortcut("Ctrl+C"),
            MenuEntry::separator(),
            MenuEntry::item("Paste").with_shortcut("Ctrl+V"),
        ])
    }

    #[test]
    fn measures_rows_separators_and_shortcuts() {
        let mut runtime = UiRuntime::new(sample_menu());
        runtime
            .layout(Constraints::loose(size(400.0, 400.0)))
            .expect("layout succeeds");
        let bounds = runtime.bounds(runtime.root()).expect("root has bounds");
        assert!((bounds.size.width() - 160.0).abs() < f32::EPSILON);
        assert!((bounds.size.height() - (8.0 + 3.0 * 30.0 + 9.0)).abs() < f32::EPSILON);
    }

    #[test]
    fn pointer_release_selects_the_row_under_the_pointer() {
        let menu = sample_menu();
        let selected = Rc::new(RefCell::new(Vec::new()));
        let _subscription = menu.selected().subscribe({
            let selected = Rc::clone(&selected);
            move |index| selected.borrow_mut().push(*index)
        });
        let mut runtime = UiRuntime::new(menu);
        runtime
            .layout(Constraints::loose(size(400.0, 400.0)))
            .expect("layout succeeds");

        let _ = runtime.dispatch_event(&pointer(drawen_core::Point::new(20.0, 49.0), false));
        let _ = runtime.dispatch_event(&pointer(drawen_core::Point::new(20.0, 49.0), true));
        assert_eq!(*selected.borrow(), vec![1]);
    }

    #[test]
    fn keyboard_skips_separators_and_disabled_rows() {
        let menu = Menu::new([
            MenuEntry::item("Cut"),
            MenuEntry::separator(),
            MenuEntry::item("Copy").disabled(),
            MenuEntry::item("Paste"),
        ]);
        let selected = Rc::new(RefCell::new(Vec::new()));
        let _subscription = menu.selected().subscribe({
            let selected = Rc::clone(&selected);
            move |index| selected.borrow_mut().push(*index)
        });
        let mut runtime = UiRuntime::new(menu);
        runtime
            .layout(Constraints::loose(size(400.0, 400.0)))
            .expect("layout succeeds");
        runtime
            .focus_widget(runtime.root())
            .expect("menu is focusable");

        let _ = runtime.dispatch_event(&key(NamedKey::ArrowDown));
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowDown));
        let _ = runtime.dispatch_event(&key(NamedKey::Enter));
        assert_eq!(*selected.borrow(), vec![3]);
    }

    #[test]
    fn disabled_rows_ignore_pointer_activation() {
        let menu = Menu::new([MenuEntry::item("Unavailable").disabled()]);
        let selected = Rc::new(RefCell::new(Vec::new()));
        let _subscription = menu.selected().subscribe({
            let selected = Rc::clone(&selected);
            move |index| selected.borrow_mut().push(*index)
        });
        let mut runtime = UiRuntime::new(menu);
        runtime
            .layout(Constraints::loose(size(400.0, 400.0)))
            .expect("layout succeeds");

        let point = drawen_core::Point::new(20.0, 19.0);
        let _ = runtime.dispatch_event(&pointer(point, false));
        let _ = runtime.dispatch_event(&pointer(point, true));
        assert!(selected.borrow().is_empty());
    }

    #[test]
    fn selection_dismisses_the_surrounding_popover() {
        use std::cell::Cell;

        use drawen_core::Insets;

        use crate::{Box, Button, Popover, PopoverTrigger};

        let popover = Popover::new().with_trigger(PopoverTrigger::Primary);
        let closed = Rc::new(Cell::new(0));
        let _closed = popover.closed().subscribe({
            let closed = Rc::clone(&closed);
            move |()| closed.set(closed.get() + 1)
        });
        let menu = sample_menu();
        let selected = Rc::new(RefCell::new(Vec::new()));
        let _subscription = menu.selected().subscribe({
            let selected = Rc::clone(&selected);
            move |index| selected.borrow_mut().push(*index)
        });
        let mut runtime = UiRuntime::new(Box::new().with_padding(Insets::all(10.0)));
        let root = runtime.root();
        let popover = runtime.append_child(root, popover).expect("popover");
        runtime
            .append_child(popover, Button::new().with_padding(Insets::all(10.0)))
            .expect("anchor");
        let menu = runtime.append_child(popover, menu).expect("content");
        runtime
            .layout(Constraints::tight(size(400.0, 400.0)).expect("valid constraints"))
            .expect("layout succeeds");

        let _ = runtime.dispatch_event(&pointer(drawen_core::Point::new(15.0, 15.0), false));
        let _ = runtime.dispatch_event(&pointer(drawen_core::Point::new(15.0, 15.0), true));
        runtime
            .layout(Constraints::tight(size(400.0, 400.0)).expect("valid constraints"))
            .expect("open layout succeeds");

        let bounds = runtime.bounds(menu).expect("menu has bounds");
        let point = drawen_core::Point::new(bounds.origin.x + 20.0, bounds.origin.y + 19.0);
        let _ = runtime.dispatch_event(&pointer(point, false));
        let _ = runtime.dispatch_event(&pointer(point, true));
        assert_eq!(*selected.borrow(), vec![0]);
        assert_eq!(closed.get(), 1);
        assert_eq!(runtime.focused_widget(), None);
    }
}
