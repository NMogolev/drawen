use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

use drawen_core::{
    Color, Constraints, InputEvent, Key, NamedKey, Point, PointerButton, PointerId, Rect, Size,
    WheelDelta,
};
use drawen_render::{PaintContext, TextLayout, TextStyle};
use drawen_ui::{
    Accessibility, AccessibilityAction, AccessibilityActions, AccessibilityRole,
    AccessibilityState, DssStyle, EventContext, EventPhase, EventStatus, LayoutContext,
    LayoutResult, Property, Signal, StyleTarget, UiEnvironment, Widget,
};

const DEFAULT_WIDTH: f32 = 240.0;
const ROW_PADDING: f32 = 4.0;
const INDENT: f32 = 16.0;
const TWISTY_WIDTH: f32 = 16.0;
const LEFT_PADDING: f32 = 8.0;
const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(400);

/// One node of a [`TreeView`]: a stable id, a display label, and child nodes.
///
/// Ids address selection and expansion, so they must be unique across the
/// whole tree; labels are what the user reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeNode {
    id: String,
    label: String,
    children: Vec<TreeNode>,
}

impl TreeNode {
    /// Creates a leaf node with a stable id and a display label.
    #[must_use]
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            children: Vec::new(),
        }
    }

    /// Returns this node with one appended child.
    #[must_use]
    pub fn child(mut self, child: Self) -> Self {
        self.children.push(child);
        self
    }

    /// Returns this node with `children` appended after any existing ones.
    #[must_use]
    pub fn with_children(mut self, children: Vec<Self>) -> Self {
        self.children.extend(children);
        self
    }

    /// Returns the stable id used by selection and expansion.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the display label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the child nodes in display order.
    #[must_use]
    pub fn children(&self) -> &[Self] {
        &self.children
    }
}

/// A row of the flattened visible tree, rebuilt whenever structure changes.
#[derive(Clone, Debug)]
struct Row {
    id: String,
    label: String,
    depth: usize,
    has_children: bool,
    expanded: bool,
    parent: Option<usize>,
}

/// A hierarchical list with expandable branches and single selection.
///
/// The tree flattens to the rows whose ancestors are all expanded; selection
/// addresses nodes by id, so it survives expanding and collapsing. The
/// selection is a [`Property<Option<String>>`]: bind application state with
/// `runtime.bind(&tree.selected(), &state.current_node)`. [`Self::changed`]
/// fires only on user selection, [`Self::activated`] on double-click or Enter,
/// and [`Self::toggled`] on user expand/collapse.
///
/// Clicking a row selects it; clicking its twisty expands or collapses the
/// branch. `ArrowUp`/`ArrowDown` move the selection, `ArrowRight` expands or
/// enters a branch, `ArrowLeft` collapses or leaves it, `Home`/`End` jump to the
/// ends, and `Enter` activates the selected row. Content taller than the
/// control scrolls with the wheel.
pub struct TreeView {
    roots: Vec<TreeNode>,
    expanded_ids: BTreeSet<String>,
    rows: Vec<Row>,
    layouts: Vec<TextLayout>,
    selected: Property<Option<String>>,
    changed: Signal<Option<String>>,
    activated: Signal<String>,
    toggled: Signal<(String, bool)>,
    cursor: Option<usize>,
    row_height: f32,
    scroll_y: f32,
    size: Size,
    focused: bool,
    pressed_pointer: Option<PointerId>,
    last_click: Option<(Instant, Point)>,
}

impl TreeView {
    /// Creates an empty tree.
    #[must_use]
    pub fn new() -> Self {
        Self {
            roots: Vec::new(),
            expanded_ids: BTreeSet::new(),
            rows: Vec::new(),
            layouts: Vec::new(),
            selected: Property::new(None),
            changed: Signal::new(),
            activated: Signal::new(),
            toggled: Signal::new(),
            cursor: None,
            row_height: 22.0,
            scroll_y: 0.0,
            size: Size::ZERO,
            focused: false,
            pressed_pointer: None,
            last_click: None,
        }
    }

    /// Returns this tree displaying `roots`.
    #[must_use]
    pub fn with_roots(mut self, roots: Vec<TreeNode>) -> Self {
        self.set_roots(roots);
        self
    }

    /// Replaces the displayed nodes, pruning expansion and selection to ids
    /// that still exist.
    pub fn set_roots(&mut self, roots: Vec<TreeNode>) {
        self.roots = roots;
        let ids = self.all_ids();
        self.expanded_ids.retain(|id| ids.contains(id));
        let selected = self.selected.get();
        if selected.is_some_and(|id| !ids.contains(&id)) {
            self.selected.set(None);
        }
        self.rebuild_rows();
    }

    /// Returns the selection property shared with this tree.
    ///
    /// The property's `changed` signal fires on every write; [`Self::changed`]
    /// fires only on user interaction. Read the id with `tree.selected().get()`.
    pub fn selected(&self) -> Property<Option<String>> {
        self.selected.clone()
    }

    /// Returns a signal emitted when the user selects a different node.
    #[must_use]
    pub fn changed(&self) -> Signal<Option<String>> {
        self.changed.clone()
    }

    /// Returns a signal emitted when the user activates a row with a
    /// double-click or Enter.
    #[must_use]
    pub fn activated(&self) -> Signal<String> {
        self.activated.clone()
    }

    /// Returns a signal emitted when the user expands or collapses a branch,
    /// carrying the node id and its new expanded state.
    #[must_use]
    pub fn toggled(&self) -> Signal<(String, bool)> {
        self.toggled.clone()
    }

    /// Selects a node by id without emitting the user-only [`Self::changed`].
    ///
    /// Unknown ids clear the selection. The cursor follows the selection so
    /// keyboard navigation continues from the programmatic choice.
    pub fn set_selected(&mut self, id: Option<&str>) {
        let id = id.filter(|id| self.rows.iter().any(|row| row.id == *id));
        self.selected.set(id.map(str::to_owned));
        self.cursor = id.and_then(|id| self.rows.iter().position(|row| row.id == id));
        self.ensure_cursor_visible();
    }

    /// Returns whether the branch `id` is expanded.
    #[must_use]
    pub fn is_expanded(&self, id: &str) -> bool {
        self.expanded_ids.contains(id)
    }

    /// Expands a branch without emitting the user-only [`Self::toggled`].
    pub fn expand(&mut self, id: &str) {
        if self.expanded_ids.insert(id.to_owned()) {
            self.rebuild_rows();
        }
    }

    /// Collapses a branch without emitting the user-only [`Self::toggled`].
    pub fn collapse(&mut self, id: &str) {
        if self.expanded_ids.remove(id) {
            self.rebuild_rows();
        }
    }

    /// Returns the number of currently visible rows.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn all_ids(&self) -> BTreeSet<String> {
        let mut ids = BTreeSet::new();
        let mut stack: Vec<&TreeNode> = self.roots.iter().collect();
        while let Some(node) = stack.pop() {
            ids.insert(node.id.clone());
            stack.extend(node.children.iter());
        }
        ids
    }

    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        let mut stack: Vec<(&TreeNode, usize, Option<usize>)> = self
            .roots
            .iter()
            .rev()
            .map(|node| (node, 0, None))
            .collect();
        while let Some((node, depth, parent)) = stack.pop() {
            let index = rows.len();
            let expanded = self.expanded_ids.contains(&node.id);
            rows.push(Row {
                id: node.id.clone(),
                label: node.label.clone(),
                depth,
                has_children: !node.children.is_empty(),
                expanded,
                parent,
            });
            if expanded {
                stack.extend(
                    node.children
                        .iter()
                        .rev()
                        .map(|child| (child, depth + 1, Some(index))),
                );
            }
        }
        self.rows = rows;
        self.layouts.clear();
        if self.cursor.is_some_and(|cursor| cursor >= self.rows.len()) {
            self.cursor = self.rows.len().checked_sub(1);
        }
        let selected = self.selected.get();
        if selected.is_some_and(|id| !self.rows.iter().any(|row| row.id == id)) {
            // A selected node hidden inside a collapsed branch keeps its id;
            // the cursor falls back to the nearest visible ancestor row.
            self.cursor = self.cursor.or_else(|| (!self.rows.is_empty()).then_some(0));
        }
        self.scroll_y = self.scroll_y.min(self.max_scroll());
    }

    fn max_scroll(&self) -> f32 {
        (self.content_height() - self.size.height()).max(0.0)
    }

    #[allow(clippy::cast_precision_loss)]
    fn content_height(&self) -> f32 {
        self.rows.len() as f32 * self.row_height
    }

    #[allow(clippy::cast_precision_loss)]
    fn ensure_cursor_visible(&mut self) {
        let Some(cursor) = self.cursor else {
            return;
        };
        let top = cursor as f32 * self.row_height;
        if top < self.scroll_y {
            self.scroll_y = top;
        } else if top + self.row_height > self.scroll_y + self.size.height() {
            self.scroll_y = top + self.row_height - self.size.height();
        }
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    fn row_at(&self, position: Point) -> Option<usize> {
        let index = ((position.y + self.scroll_y) / self.row_height).floor();
        if index < 0.0 || index >= self.rows.len() as f32 {
            return None;
        }
        Some(index as usize)
    }

    #[allow(clippy::cast_precision_loss)]
    fn twisty_at(&self, row: usize, position: Point) -> bool {
        let left = LEFT_PADDING + self.rows[row].depth as f32 * INDENT;
        position.x >= left && position.x < left + TWISTY_WIDTH
    }

    fn select_row(&mut self, row: usize, context: &mut EventContext<'_>, user: bool) {
        self.cursor = Some(row);
        let id = self.rows[row].id.clone();
        let current = self.selected.get();
        if current.as_deref() != Some(id.as_str()) {
            self.selected.set(Some(id));
            if user {
                self.changed.emit(&self.selected.get());
            }
        }
        self.ensure_cursor_visible();
        context.request_redraw();
    }

    fn toggle_row(&mut self, row: usize, context: &mut EventContext<'_>, user: bool) {
        let node = &self.rows[row];
        if !node.has_children {
            return;
        }
        let id = node.id.clone();
        let expanded = if node.expanded {
            self.expanded_ids.remove(&id);
            false
        } else {
            self.expanded_ids.insert(id.clone());
            true
        };
        self.rebuild_rows();
        self.cursor = self.rows.iter().position(|row| row.id == id);
        if user {
            self.toggled.emit(&(id, expanded));
        }
        context.request_layout();
    }

    fn move_cursor(&mut self, row: usize, context: &mut EventContext<'_>) {
        self.select_row(row, context, true);
    }
}

impl Default for TreeView {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for TreeView {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let style = context.style();
        let environment = context.environment();
        let font = environment.font_for(&style);
        let font_size = font.pixel_size(environment.logical_dpi());
        self.row_height = font_size + ROW_PADDING * 2.0;
        let text_style =
            TextStyle::new_with_font(font, style.foreground_or(environment.theme().foreground()));
        self.layouts = self
            .rows
            .iter()
            .map(|row| {
                environment
                    .text_shaper()
                    .layout(&row.label, &text_style, None)
            })
            .collect();
        let max = constraints.max();
        let width = if max.width().is_finite() {
            max.width()
        } else {
            DEFAULT_WIDTH
        };
        let natural_height = self.content_height();
        let height = if max.height().is_finite() {
            max.height().min(natural_height)
        } else {
            natural_height
        };
        self.size = constraints.constrain(size(width, height));
        self.scroll_y = self.scroll_y.min(self.max_scroll());
        LayoutResult::new(self.size)
    }

    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("tree-view")
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        self.paint_styled(context, environment, bounds, &DssStyle::default());
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    fn paint_styled(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        bounds: Rect,
        style: &DssStyle,
    ) {
        let theme = environment.theme();
        let foreground = style.foreground_or(theme.foreground());
        let accent = style.accent_or(theme.accent());
        context.fill_rounded_rect(
            bounds,
            style.corner_radius_or(theme.corner_radius()),
            style.background_or(theme.surface()),
        );
        let selected = self.selected.get();
        context.with_clip(bounds, |context| {
            let first = (self.scroll_y / self.row_height).floor() as usize;
            let visible = (self.size.height() / self.row_height).ceil() as usize + 1;
            for row in first..(first + visible).min(self.rows.len()) {
                let node = &self.rows[row];
                let top = bounds.origin.y + row as f32 * self.row_height - self.scroll_y;
                let row_rect = Rect::new(
                    Point::new(bounds.origin.x, top),
                    size(self.size.width(), self.row_height),
                );
                if selected.as_deref() == Some(node.id.as_str()) {
                    context.fill_rect(row_rect, theme.selection());
                } else if self.focused && self.cursor == Some(row) {
                    context.stroke_rect(row_rect, 1.0, accent.with_alpha(0.5));
                }
                let twisty_x = bounds.origin.x + LEFT_PADDING + node.depth as f32 * INDENT;
                if node.has_children {
                    paint_twisty(
                        context,
                        Point::new(
                            twisty_x + (TWISTY_WIDTH - 8.0) / 2.0,
                            top + (self.row_height - 8.0) / 2.0,
                        ),
                        node.expanded,
                        foreground,
                    );
                }
                if let Some(layout) = self.layouts.get(row) {
                    let layout = if layout.color() == foreground {
                        layout.clone()
                    } else {
                        layout.clone().with_color(foreground)
                    };
                    let text_height = layout.size().height();
                    context.draw_text(
                        layout,
                        Point::new(
                            twisty_x + TWISTY_WIDTH,
                            top + (self.row_height - text_height) / 2.0,
                        ),
                    );
                }
            }
        });
    }

    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        if context.phase() != EventPhase::Target {
            return EventStatus::Ignored;
        }
        match event {
            InputEvent::PointerDown(pointer) if pointer.button == Some(PointerButton::Primary) => {
                context.request_focus();
                context.capture_pointer(pointer.pointer_id);
                self.pressed_pointer = Some(pointer.pointer_id);
                let Some(row) = self.row_at(pointer.position) else {
                    return EventStatus::Handled;
                };
                if self.twisty_at(row, pointer.position) && self.rows[row].has_children {
                    self.toggle_row(row, context, true);
                    return EventStatus::Handled;
                }
                let now = Instant::now();
                let double_click = self.last_click.is_some_and(|(time, last)| {
                    now.duration_since(time) <= DOUBLE_CLICK_INTERVAL
                        && (last.x - pointer.position.x).abs() <= 4.0
                        && (last.y - pointer.position.y).abs() <= 4.0
                });
                self.last_click = Some((now, pointer.position));
                self.select_row(row, context, true);
                if double_click {
                    let id = self.rows[row].id.clone();
                    self.activated.emit(&id);
                }
                EventStatus::Handled
            }
            InputEvent::PointerUp(pointer) if self.pressed_pointer == Some(pointer.pointer_id) => {
                self.pressed_pointer = None;
                context.release_pointer(pointer.pointer_id);
                EventStatus::Handled
            }
            InputEvent::PointerCancel(pointer) => {
                self.pressed_pointer = None;
                context.release_pointer(pointer.pointer_id);
                EventStatus::Handled
            }
            InputEvent::Wheel(wheel) => {
                let delta = match wheel.delta {
                    WheelDelta::Pixels(delta) => delta,
                    WheelDelta::Lines(delta) => Point::new(delta.x * 24.0, delta.y * 24.0),
                };
                if self.max_scroll() <= 0.0 {
                    return EventStatus::Ignored;
                }
                self.scroll_y = (self.scroll_y - delta.y).clamp(0.0, self.max_scroll());
                context.request_redraw();
                EventStatus::Handled
            }
            InputEvent::KeyDown(key) if !key.repeat => self.handle_key(context, key),
            InputEvent::FocusChanged(focus) => {
                self.focused = focus.focused == Some(context.current_target());
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
        let label = self
            .selected
            .get()
            .and_then(|id| self.rows.iter().find(|row| row.id == id))
            .map(|row| row.label.clone())
            .unwrap_or_default();
        Accessibility::new(AccessibilityRole::List)
            .with_value(label)
            .with_states(AccessibilityState::FOCUSABLE)
            .with_actions(
                AccessibilityActions::FOCUS
                    | AccessibilityActions::SELECT
                    | AccessibilityActions::EXPAND
                    | AccessibilityActions::COLLAPSE,
            )
    }

    fn accessibility_action(&mut self, action: &AccessibilityAction) -> bool {
        let Some(cursor) = self.cursor else {
            return false;
        };
        match action {
            AccessibilityAction::Select => {
                let id = self.rows[cursor].id.clone();
                self.selected.set(Some(id));
                true
            }
            AccessibilityAction::Expand => {
                let id = self.rows[cursor].id.clone();
                self.expand(&id);
                true
            }
            AccessibilityAction::Collapse => {
                let id = self.rows[cursor].id.clone();
                self.collapse(&id);
                true
            }
            _ => false,
        }
    }
}

impl TreeView {
    fn handle_key(
        &mut self,
        context: &mut EventContext<'_>,
        key: &drawen_core::KeyEvent,
    ) -> EventStatus {
        if self.rows.is_empty() {
            return EventStatus::Ignored;
        }
        let cursor = self.cursor.unwrap_or(0);
        match key.key {
            Key::Named(NamedKey::ArrowDown) => {
                self.move_cursor((cursor + 1).min(self.rows.len() - 1), context);
                EventStatus::Handled
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.move_cursor(cursor.saturating_sub(1), context);
                EventStatus::Handled
            }
            Key::Named(NamedKey::ArrowRight) => {
                let row = &self.rows[cursor];
                if row.has_children && !row.expanded {
                    self.toggle_row(cursor, context, true);
                } else if row.has_children {
                    self.move_cursor(cursor + 1, context);
                }
                EventStatus::Handled
            }
            Key::Named(NamedKey::ArrowLeft) => {
                let row = &self.rows[cursor];
                if row.expanded {
                    self.toggle_row(cursor, context, true);
                } else if let Some(parent) = row.parent {
                    self.move_cursor(parent, context);
                }
                EventStatus::Handled
            }
            Key::Named(NamedKey::Home) => {
                self.move_cursor(0, context);
                EventStatus::Handled
            }
            Key::Named(NamedKey::End) => {
                self.move_cursor(self.rows.len() - 1, context);
                EventStatus::Handled
            }
            Key::Named(NamedKey::Enter) => {
                let id = self.rows[cursor].id.clone();
                self.activated.emit(&id);
                EventStatus::Handled
            }
            Key::Named(NamedKey::Space) => {
                self.toggle_row(cursor, context, true);
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }
}

/// Paints an 8x8 disclosure triangle: down when expanded, right when closed.
fn paint_twisty(context: &mut PaintContext<'_>, origin: Point, expanded: bool, color: Color) {
    for step in 0u8..4 {
        let (width, height) = if expanded {
            (8.0 - 2.0 * f32::from(step), 1.0)
        } else {
            (1.0, 8.0 - 2.0 * f32::from(step))
        };
        let offset = if expanded {
            Point::new((8.0 - width) / 2.0, f32::from(step))
        } else {
            Point::new(f32::from(step), (8.0 - height) / 2.0)
        };
        context.fill_rect(
            Rect::new(
                Point::new(origin.x + offset.x, origin.y + offset.y),
                size(width, height),
            ),
            color,
        );
    }
}

fn size(width: f32, height: f32) -> Size {
    Size::new(width, height).expect("tree view sizes are non-negative and not NaN")
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // tests assert exact expected geometry by design
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use drawen_core::{KeyCode, Modifiers, PointerButtons, PointerEvent, WheelDelta, WheelEvent};
    use drawen_ui::UiRuntime;

    use super::*;

    fn sample_tree() -> TreeView {
        TreeView::new().with_roots(vec![
            TreeNode::new("src", "Source")
                .child(TreeNode::new("main", "main.rs"))
                .child(TreeNode::new("lib", "lib.rs")),
            TreeNode::new("docs", "Docs"),
        ])
    }

    fn viewport(height: f32) -> Constraints {
        Constraints::loose(Size::new(200.0, height).expect("valid size"))
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

    #[test]
    fn twisty_click_expands_and_collapses_and_emits_toggled() {
        let tree = sample_tree();
        let toggled = Rc::new(RefCell::new(Vec::new()));
        let _subscription = tree.toggled().subscribe({
            let toggled = Rc::clone(&toggled);
            move |event| toggled.borrow_mut().push(event.clone())
        });
        let mut runtime = UiRuntime::new(tree);
        runtime.layout(viewport(400.0)).expect("layout succeeds");
        let collapsed = runtime
            .bounds(runtime.root())
            .expect("bounds")
            .size
            .height();

        // Row 0 twisty sits at x = 8..24, vertically centered in the first row.
        let _ = runtime.dispatch_event(&pointer(Point::new(12.0, 10.0), false));
        let _ = runtime.dispatch_event(&pointer(Point::new(12.0, 10.0), true));
        runtime.layout(viewport(400.0)).expect("relayout succeeds");
        let expanded = runtime
            .bounds(runtime.root())
            .expect("bounds")
            .size
            .height();
        assert!(expanded > collapsed);
        assert_eq!(*toggled.borrow(), vec![("src".to_owned(), true)]);

        let _ = runtime.dispatch_event(&pointer(Point::new(12.0, 10.0), false));
        let _ = runtime.dispatch_event(&pointer(Point::new(12.0, 10.0), true));
        runtime.layout(viewport(400.0)).expect("relayout succeeds");
        assert_eq!(
            runtime
                .bounds(runtime.root())
                .expect("bounds")
                .size
                .height(),
            collapsed
        );
        assert_eq!(
            *toggled.borrow(),
            vec![("src".to_owned(), true), ("src".to_owned(), false)]
        );
    }

    #[test]
    fn selection_follows_clicks_and_arrow_keys_in_visible_order() {
        let tree = sample_tree();
        let selected = tree.selected();
        let changes = Rc::new(RefCell::new(Vec::new()));
        let _subscription = tree.changed().subscribe({
            let changes = Rc::clone(&changes);
            move |id| changes.borrow_mut().push(id.clone())
        });
        let mut runtime = UiRuntime::new(tree);
        runtime.layout(viewport(400.0)).expect("layout succeeds");
        runtime.focus_widget(runtime.root()).expect("tree focuses");

        // Expand Source so the visible order is src, main, lib, docs.
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowRight));
        runtime.layout(viewport(400.0)).expect("relayout succeeds");
        let _ = runtime.dispatch_event(&pointer(Point::new(60.0, 10.0), false));
        let _ = runtime.dispatch_event(&pointer(Point::new(60.0, 10.0), true));
        assert_eq!(selected.get().as_deref(), Some("src"));
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowDown));
        assert_eq!(selected.get().as_deref(), Some("main"));
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowDown));
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowDown));
        assert_eq!(selected.get().as_deref(), Some("docs"));
        assert_eq!(
            *changes.borrow(),
            vec![
                Some("src".to_owned()),
                Some("main".to_owned()),
                Some("lib".to_owned()),
                Some("docs".to_owned())
            ]
        );
    }

    #[test]
    fn arrow_left_collapses_then_climbs_to_the_parent() {
        let tree = sample_tree();
        let selected = tree.selected();
        let mut runtime = UiRuntime::new(tree);
        runtime.layout(viewport(400.0)).expect("layout succeeds");
        runtime.focus_widget(runtime.root()).expect("tree focuses");

        let _ = runtime.dispatch_event(&key(NamedKey::ArrowRight)); // expand src
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowRight)); // enter main
        assert_eq!(selected.get().as_deref(), Some("main"));
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowLeft)); // climb to src
        assert_eq!(selected.get().as_deref(), Some("src"));
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowLeft)); // collapse src
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowDown));
        assert_eq!(selected.get().as_deref(), Some("docs"));
    }

    #[test]
    fn double_click_and_enter_activate_the_selected_row() {
        let tree = sample_tree();
        let activated = Rc::new(RefCell::new(Vec::new()));
        let _subscription = tree.activated().subscribe({
            let activated = Rc::clone(&activated);
            move |id| activated.borrow_mut().push(id.clone())
        });
        let mut runtime = UiRuntime::new(tree);
        runtime.layout(viewport(400.0)).expect("layout succeeds");

        let _ = runtime.dispatch_event(&pointer(Point::new(60.0, 10.0), false));
        let _ = runtime.dispatch_event(&pointer(Point::new(60.0, 10.0), true));
        let _ = runtime.dispatch_event(&pointer(Point::new(60.0, 10.0), false));
        let _ = runtime.dispatch_event(&pointer(Point::new(60.0, 10.0), true));
        let _ = runtime.dispatch_event(&key(NamedKey::Enter));
        assert_eq!(
            *activated.borrow(),
            vec!["src".to_owned(), "src".to_owned()]
        );
    }

    #[test]
    fn set_selected_and_binding_write_without_user_changed() {
        let tree = sample_tree();
        let selected = tree.selected();
        let user_changes = Rc::new(RefCell::new(0));
        let _subscription = tree.changed().subscribe({
            let user_changes = Rc::clone(&user_changes);
            move |_| *user_changes.borrow_mut() += 1
        });
        let state = Property::new(Some("docs".to_owned()));
        let mut runtime = UiRuntime::new(tree);
        let _binding = runtime.bind(&selected, &state);
        runtime.layout(viewport(400.0)).expect("layout succeeds");
        assert_eq!(selected.get().as_deref(), Some("docs"));

        state.set(Some("src".to_owned()));
        assert_eq!(selected.get().as_deref(), Some("src"));
        assert_eq!(*user_changes.borrow(), 0);
    }

    #[test]
    fn wheel_scrolls_only_when_content_exceeds_the_viewport() {
        let tree = sample_tree();
        let mut runtime = UiRuntime::new(tree);
        runtime.layout(viewport(30.0)).expect("layout succeeds");
        let wheel = InputEvent::Wheel(WheelEvent {
            position: Point::new(50.0, 10.0),
            delta: WheelDelta::Pixels(Point::new(0.0, -40.0)),
            modifiers: Modifiers::NONE,
        });
        let _ = runtime.dispatch_event(&wheel);
        // Two rows of 22px fit 30px viewport: content 44px, max scroll 14px.
        // Selection keyboard navigation keeps the cursor inside the viewport.
        runtime.focus_widget(runtime.root()).expect("tree focuses");
        let _ = runtime.dispatch_event(&key(NamedKey::End));
        let bounds = runtime.bounds(runtime.root()).expect("bounds");
        assert_eq!(bounds.size.height(), 30.0);
    }
}
