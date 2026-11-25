//! Editable combobox with a filtered floating suggestion list.

use std::{cell::RefCell, rc::Rc};

use drawen_core::{
    Color, Constraints, FocusChanged, InputEvent, Insets, Key, Modifiers, NamedKey, Point,
    PointerButton, Rect, Size, TextPreedit, WheelDelta,
};
use drawen_render::{PaintContext, TextLayout, TextStyle};
use drawen_ui::{
    Accessibility, AccessibilityAction, AccessibilityActions, AccessibilityRole,
    AccessibilityState, ChildLayout, DismissPolicy, DssStyle, EventContext, EventPhase,
    EventStatus, LayoutContext, LayoutResult, OverlayAlign, OverlayOptions, OverlayPlacement,
    OverlaySide, Signal, StyleTarget, UiEnvironment, Widget,
};

use crate::text_editor::TextEditor;

const DEFAULT_WIDTH: f32 = 200.0;
const HORIZONTAL_PADDING: f32 = 8.0;
const VERTICAL_PADDING: f32 = 6.0;
const POPUP_ROW_HEIGHT: f32 = 28.0;
const MAX_POPUP_ROWS: usize = 8;

type OptionFilter = dyn Fn(&str, &str) -> bool;

/// Render state shared between an [`Autocomplete`] and its popup child.
///
/// Both widgets live on the UI thread; the parent publishes the filtered rows
/// and active highlight during layout, and the popup reports hover, wheel, and
/// clicks back through the same cell.
#[derive(Default)]
struct PopupState {
    rows: Vec<(usize, String)>,
    active: Option<usize>,
    hovered: Option<usize>,
    click: Option<usize>,
    scroll: usize,
    width: f32,
}

/// An editable combobox: typing filters options into a floating list.
///
/// The field owns its query text (with input-method preedit, selection, and
/// clipboard support) and the option data. Append the popup returned by
/// [`Self::popup`] as the widget's single child; it floats through the runtime
/// overlay layer below the field, or at the caret with
/// [`Self::with_caret_anchor`] for mention-style completion:
///
/// ```text
/// autocomplete
/// └── autocomplete-popup   (floating suggestions, measured only while open)
/// ```
///
/// ```rust,ignore
/// let field = Autocomplete::new(["alice", "bob", "carol"]);
/// let selected = field.selected();
/// let popup = field.popup();
/// let field = runtime.append_child(parent, field)?;
/// runtime.append_child(field, popup)?;
/// ```
///
/// `ArrowUp`/`ArrowDown` move the active suggestion, `Enter` and pointer clicks
/// commit it (setting the query to the option and emitting [`Self::selected`]),
/// and `Escape` or an outside click closes the list. Filtering defaults to a
/// case-insensitive substring match; override it with [`Self::with_filter`].
pub struct Autocomplete {
    editor: TextEditor,
    options: Vec<String>,
    filter: Box<OptionFilter>,
    filtered: Vec<usize>,
    active: Option<usize>,
    open: bool,
    focused: bool,
    caret_anchor: bool,
    preedit: Option<TextPreedit>,
    placeholder: Option<String>,
    placeholder_layout: Option<TextLayout>,
    query_layout: TextLayout,
    size: Size,
    scroll_x: f32,
    padding: Insets,
    background: Color,
    border: Color,
    text_color: Color,
    selection_color: Color,
    radius: f32,
    shared: Rc<RefCell<PopupState>>,
    query_changed: Signal<String>,
    selected: Signal<usize>,
}

impl Autocomplete {
    /// Creates an empty combobox over `options`.
    #[must_use]
    pub fn new(options: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let options: Vec<String> = options.into_iter().map(Into::into).collect();
        let filtered = (0..options.len()).collect();
        Self {
            editor: TextEditor::with_text(false, ""),
            options,
            filter: Box::new(|query, option| {
                option.to_lowercase().contains(&query.to_lowercase())
            }),
            filtered,
            active: None,
            open: false,
            focused: false,
            caret_anchor: false,
            preedit: None,
            placeholder: None,
            placeholder_layout: None,
            query_layout: empty_layout(),
            size: Size::ZERO,
            scroll_x: 0.0,
            padding: default_padding(),
            background: Color::WHITE,
            border: Color::TRANSPARENT,
            text_color: Color::BLACK,
            selection_color: Color::BLACK,
            radius: 0.0,
            shared: Rc::new(RefCell::new(PopupState::default())),
            query_changed: Signal::new(),
            selected: Signal::new(),
        }
    }

    /// Returns the popup child to append as this widget's single child.
    ///
    /// Call this before moving the field into the runtime; the popup shares
    /// state with the field and has no standalone behavior.
    #[must_use]
    pub fn popup(&self) -> AutocompletePopup {
        AutocompletePopup {
            shared: Rc::clone(&self.shared),
            size: Size::ZERO,
        }
    }

    /// Replaces the match predicate `filter(query, option)`.
    #[must_use]
    pub fn with_filter(mut self, filter: impl Fn(&str, &str) -> bool + 'static) -> Self {
        self.filter = Box::new(filter);
        self.refilter();
        self
    }

    /// Sets a hint shown while the query is empty.
    #[must_use]
    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Anchors the popup at the caret instead of the field's leading edge.
    ///
    /// Use this for mention-style completion inside a larger composition flow.
    #[must_use]
    pub fn with_caret_anchor(mut self, caret_anchor: bool) -> Self {
        self.caret_anchor = caret_anchor;
        self
    }

    /// Returns the current query text.
    #[must_use]
    pub fn query(&self) -> &str {
        self.editor.text()
    }

    /// Replaces the query without emitting signals and refilters the options.
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.editor.set_text(query);
        self.preedit = None;
        self.scroll_x = 0.0;
        self.refilter();
    }

    /// Returns whether the suggestion list is open.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// Returns a signal emitted after user input changes the query.
    #[must_use]
    pub fn query_changed(&self) -> Signal<String> {
        self.query_changed.clone()
    }

    /// Returns a signal emitted with the original option index on commit.
    #[must_use]
    pub fn selected(&self) -> Signal<usize> {
        self.selected.clone()
    }

    fn refilter(&mut self) {
        let query = self.editor.text();
        self.filtered = (0..self.options.len())
            .filter(|index| (self.filter)(query, &self.options[*index]))
            .collect();
        self.active = (!self.filtered.is_empty()).then_some(0);
    }

    fn display(&self) -> (String, usize) {
        let selection = self.editor.selection();
        let preedit = self
            .preedit
            .as_ref()
            .map_or("", |preedit| preedit.text.as_str());
        if preedit.is_empty() {
            return (self.editor.text().to_owned(), self.editor.focus());
        }
        let mut text = String::with_capacity(self.editor.text().len() + preedit.len());
        text.push_str(&self.editor.text()[..selection.start]);
        text.push_str(preedit);
        text.push_str(&self.editor.text()[selection.end..]);
        let preedit_caret = self.preedit.as_ref().map_or(preedit.len(), |preedit| {
            preedit
                .cursor
                .map_or(preedit.text.len(), |(_, end)| end)
                .min(preedit.text.len())
        });
        (text, selection.start + preedit_caret)
    }

    fn text_origin(&self) -> Point {
        Point::new(self.padding.left - self.scroll_x, self.padding.top)
    }

    fn viewport_width(&self) -> f32 {
        (self.size.width() - self.padding.horizontal()).max(1.0)
    }

    fn ensure_caret_visible(&mut self) {
        let caret = self.query_layout.caret_rect(self.display().1, 1.0).origin.x;
        let width = self.viewport_width();
        if caret < self.scroll_x {
            self.scroll_x = caret;
        } else if caret + 1.0 > self.scroll_x + width {
            self.scroll_x = caret + 1.0 - width;
        }
        self.scroll_x = self.scroll_x.max(0.0);
    }

    fn caret_rect(&self) -> Rect {
        let caret = self.query_layout.caret_rect(self.display().1, 1.0);
        Rect::new(
            Point::new(
                self.padding.left + caret.origin.x - self.scroll_x,
                self.padding.top + caret.origin.y,
            ),
            size(1.0, caret.size.height().max(1.0)),
        )
    }

    fn apply_edit(
        &mut self,
        context: &mut EventContext<'_>,
        edit: impl FnOnce(&mut TextEditor) -> bool,
    ) -> bool {
        let before = self.editor.text().to_owned();
        if !edit(&mut self.editor) {
            return false;
        }
        if before == self.editor.text() {
            context.request_redraw();
            return true;
        }
        self.refilter();
        self.open = self.focused && !self.filtered.is_empty();
        self.ensure_caret_visible();
        self.query_changed.emit(&self.editor.text().to_owned());
        context.request_layout();
        true
    }

    fn commit(&mut self, position: usize, context: &mut EventContext<'_>) -> bool {
        if self.commit_inner(position) {
            context.request_layout();
            true
        } else {
            false
        }
    }

    fn commit_inner(&mut self, position: usize) -> bool {
        let Some(&index) = self.filtered.get(position) else {
            return false;
        };
        self.editor.set_text(self.options[index].clone());
        self.preedit = None;
        self.scroll_x = 0.0;
        self.open = false;
        self.refilter();
        self.selected.emit(&index);
        self.query_changed.emit(&self.editor.text().to_owned());
        true
    }

    fn step(&mut self, forward: bool, context: &mut EventContext<'_>) {
        if self.filtered.is_empty() {
            return;
        }
        if self.open {
            let count = self.filtered.len();
            self.active = Some(match self.active {
                Some(active) if forward => (active + 1).min(count - 1),
                Some(active) => active.saturating_sub(1),
                None => 0,
            });
        } else {
            self.open = true;
            self.active = Some(0);
        }
        context.request_layout();
    }

    fn copy(&self, context: &EventContext<'_>) -> bool {
        let Some(selection) = self.editor.selected_text() else {
            return false;
        };
        context.write_clipboard(selection);
        true
    }

    fn cut(&mut self, context: &mut EventContext<'_>) -> bool {
        if !self.copy(context) {
            return false;
        }
        self.apply_edit(context, TextEditor::delete_selection);
        true
    }

    fn paste(&mut self, context: &mut EventContext<'_>) -> bool {
        let Some(text) = context.read_clipboard() else {
            return false;
        };
        self.apply_edit(context, |editor| editor.insert(&text));
        true
    }

    fn handle_key(
        &mut self,
        context: &mut EventContext<'_>,
        key: &drawen_core::KeyEvent,
    ) -> EventStatus {
        let extend = key.modifiers.contains(Modifiers::SHIFT);
        let command =
            key.modifiers.contains(Modifiers::CONTROL) || key.modifiers.contains(Modifiers::META);
        match &key.key {
            Key::Named(NamedKey::ArrowDown) => {
                self.step(true, context);
                EventStatus::Handled
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.step(false, context);
                EventStatus::Handled
            }
            Key::Named(NamedKey::Enter) if !key.repeat => {
                if self.open && self.active.is_some_and(|active| self.commit(active, context)) {
                    EventStatus::Handled
                } else {
                    EventStatus::Ignored
                }
            }
            Key::Named(NamedKey::Escape) => {
                if self.open {
                    self.open = false;
                    context.request_layout();
                    EventStatus::Handled
                } else {
                    EventStatus::Ignored
                }
            }
            Key::Named(NamedKey::Backspace) => {
                self.apply_edit(context, |editor| editor.backspace(command));
                EventStatus::Handled
            }
            Key::Named(NamedKey::Delete) => {
                self.apply_edit(context, |editor| editor.delete(command));
                EventStatus::Handled
            }
            Key::Named(NamedKey::ArrowLeft) => {
                self.editor.move_horizontal(false, command, extend);
                self.ensure_caret_visible();
                context.request_redraw();
                EventStatus::Handled
            }
            Key::Named(NamedKey::ArrowRight) => {
                self.editor.move_horizontal(true, command, extend);
                self.ensure_caret_visible();
                context.request_redraw();
                EventStatus::Handled
            }
            Key::Named(NamedKey::Home) => {
                self.editor.move_to(0, extend);
                self.ensure_caret_visible();
                context.request_redraw();
                EventStatus::Handled
            }
            Key::Named(NamedKey::End) => {
                self.editor.move_to(self.editor.text().len(), extend);
                self.ensure_caret_visible();
                context.request_redraw();
                EventStatus::Handled
            }
            _ if command => {
                let handled = match &key.key {
                    Key::Character(character) if character.eq_ignore_ascii_case("a") => {
                        self.editor.select_all();
                        true
                    }
                    Key::Character(character) if character.eq_ignore_ascii_case("c") => {
                        self.copy(context)
                    }
                    Key::Character(character) if character.eq_ignore_ascii_case("x") => {
                        self.cut(context)
                    }
                    Key::Character(character) if character.eq_ignore_ascii_case("v") => {
                        self.paste(context)
                    }
                    Key::Character(character) if character.eq_ignore_ascii_case("z") => {
                        if extend {
                            self.apply_edit(context, TextEditor::redo)
                        } else {
                            self.apply_edit(context, TextEditor::undo)
                        }
                    }
                    Key::Character(character) if character.eq_ignore_ascii_case("y") => {
                        self.apply_edit(context, TextEditor::redo)
                    }
                    _ => false,
                };
                if handled {
                    self.ensure_caret_visible();
                    context.request_redraw();
                    EventStatus::Handled
                } else {
                    EventStatus::Ignored
                }
            }
            _ => EventStatus::Ignored,
        }
    }
}

impl Widget for Autocomplete {
    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("autocomplete")
    }

    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let style = context.style();
        let environment = context.environment();
        let theme = environment.theme();
        self.padding = style.padding.unwrap_or_else(|| theme.control_padding());
        self.background = style
            .background
            .unwrap_or_else(|| theme.control_background());
        self.border = style
            .border
            .map_or_else(|| theme.border(), |border| border.color);
        self.text_color = style.foreground_or(theme.foreground());
        self.selection_color = theme.accent().with_alpha(0.35);
        self.radius = style.corner_radius.unwrap_or_else(|| theme.corner_radius());

        // Consume a popup click recorded during the previous event pass.
        let click = self.shared.borrow_mut().click.take();
        if let Some(position) = click {
            self.commit_inner(position);
        }

        let font = environment.font_for(&style);
        let text_style = TextStyle::new_with_font(font.clone(), self.text_color);
        let (text, _) = self.display();
        self.query_layout = environment
            .text_shaper()
            .layout(&text, &text_style, None);
        self.placeholder_layout = self.placeholder.as_ref().map(|placeholder| {
            environment.text_shaper().layout(
                placeholder,
                &TextStyle::new_with_font(font, self.text_color.with_alpha(0.5)),
                None,
            )
        });
        let height = (self.query_layout.size().height() + self.padding.vertical()).max(1.0);
        let max = constraints.max();
        let width = if max.width().is_finite() {
            max.width()
        } else {
            DEFAULT_WIDTH
        };
        self.size = constraints.constrain(size(width, height));
        self.ensure_caret_visible();

        {
            let mut shared = self.shared.borrow_mut();
            shared.rows = self
                .filtered
                .iter()
                .map(|&index| (index, self.options[index].clone()))
                .collect();
            shared.active = self.active;
            shared.width = self.size.width();
            if !self.open {
                shared.hovered = None;
                shared.scroll = 0;
            }
        }

        let mut children = Vec::new();
        if self.open && self.focused && !self.filtered.is_empty() && context.child_count() > 0 {
            let max = context
                .viewport()
                .map_or_else(|| constraints.max(), |viewport| viewport.size);
            let anchor = if self.caret_anchor {
                self.caret_rect()
            } else {
                Rect::new(Point::ZERO, self.size)
            };
            let (panel, _, origin) = context
                .layout_child_overlay(
                    0,
                    Constraints::loose(max),
                    anchor,
                    OverlayOptions::new(OverlayPlacement::new(
                        OverlaySide::Bottom,
                        OverlayAlign::Start,
                    ))
                    .with_dismiss(DismissPolicy::Light),
                )
                .expect("runtime child index is valid");
            children.push(ChildLayout::new(panel, origin));
        }
        LayoutResult::with_children(self.size, children)
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        self.paint_styled(context, environment, bounds, &DssStyle::default());
    }

    fn paint_styled(
        &self,
        context: &mut PaintContext<'_>,
        _: &UiEnvironment,
        bounds: Rect,
        _: &DssStyle,
    ) {
        let surface = Rect::new(bounds.origin, self.size);
        context.fill_rounded_rect(surface, self.radius, self.background);
        context.stroke_rounded_rect(surface, self.radius, 1.0, self.border);
        let origin = Point::new(
            surface.origin.x + self.text_origin().x,
            surface.origin.y + self.text_origin().y,
        );
        let clip = Rect::new(
            Point::new(surface.origin.x + self.padding.left, surface.origin.y),
            size(self.viewport_width(), self.size.height()),
        );
        context.with_clip(clip, |context| {
            if self.preedit.is_none() && self.editor.has_selection() {
                for rect in self.query_layout.selection_rects(self.editor.selection()) {
                    context.fill_rect(
                        Rect::new(
                            Point::new(rect.origin.x + origin.x, rect.origin.y + origin.y),
                            rect.size,
                        ),
                        self.selection_color,
                    );
                }
            }
            if self.editor.text().is_empty() && self.preedit.is_none() {
                if let Some(placeholder) = &self.placeholder_layout {
                    context.draw_text(placeholder.clone(), origin);
                }
            } else {
                context.draw_text(self.query_layout.clone(), origin);
            }
            if self.focused {
                let caret = self.caret_rect();
                context.fill_rect(
                    Rect::new(
                        Point::new(caret.origin.x + surface.origin.x, caret.origin.y + surface.origin.y),
                        caret.size,
                    ),
                    self.text_color,
                );
            }
        });
    }

    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        if context.phase() != EventPhase::Target {
            return EventStatus::Ignored;
        }
        match event {
            InputEvent::PointerDown(pointer) => {
                if pointer.button != Some(PointerButton::Primary) {
                    return EventStatus::Ignored;
                }
                context.request_focus();
                let origin = self.text_origin();
                let byte = self.query_layout.byte_at_point(Point::new(
                    pointer.position.x - origin.x,
                    pointer.position.y - origin.y,
                ));
                self.editor.set_caret(byte);
                self.open = !self.filtered.is_empty();
                context.request_layout();
                EventStatus::Handled
            }
            InputEvent::TextInput(text) => {
                let had_preedit = self.preedit.take().is_some();
                self.apply_edit(context, |editor| editor.insert(text));
                if had_preedit {
                    context.request_layout();
                }
                EventStatus::Handled
            }
            InputEvent::TextPreedit(preedit) => {
                self.preedit = (!preedit.text.is_empty()).then(|| preedit.clone());
                context.request_layout();
                EventStatus::Handled
            }
            InputEvent::Wheel(wheel) => {
                let delta = match wheel.delta {
                    WheelDelta::Pixels(delta) => delta,
                    WheelDelta::Lines(delta) => Point::new(delta.x * 24.0, delta.y * 24.0),
                };
                let total_width = self.query_layout.size().width() + self.padding.horizontal();
                if total_width <= self.viewport_width() {
                    return EventStatus::Ignored;
                }
                self.scroll_x = (self.scroll_x - delta.y - delta.x).max(0.0);
                context.request_redraw();
                EventStatus::Handled
            }
            InputEvent::KeyDown(key) => self.handle_key(context, key),
            InputEvent::FocusChanged(FocusChanged { focused }) => {
                self.focused = *focused == Some(context.current_target());
                if !self.focused {
                    self.open = false;
                    if self.preedit.take().is_some() {
                        context.request_layout();
                    }
                }
                context.request_redraw();
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn dismiss_overlay(&mut self) -> bool {
        if self.open {
            self.open = false;
            true
        } else {
            false
        }
    }

    fn ime_cursor_area(&self) -> Option<Rect> {
        self.focused.then(|| self.caret_rect())
    }

    fn accepts_focus(&self) -> bool {
        true
    }

    fn accessibility(&self) -> Accessibility {
        let mut accessibility = Accessibility::new(AccessibilityRole::ComboBox)
            .with_value(self.editor.text())
            .with_states(AccessibilityState::FOCUSABLE)
            .with_actions(
                AccessibilityActions::FOCUS
                    | AccessibilityActions::SET_VALUE
                    | AccessibilityActions::SELECT,
            );
        if self.open {
            accessibility = accessibility.with_states(AccessibilityState::EXPANDED);
        }
        accessibility
    }

    fn accessibility_action(&mut self, action: &AccessibilityAction) -> bool {
        match action {
            AccessibilityAction::SetValue(value) => {
                self.set_query(value.clone());
                true
            }
            _ => false,
        }
    }
}

/// The floating suggestion list owned by an [`Autocomplete`].
///
/// Append it as the field's single child through [`Autocomplete::popup`]; it
/// has no behavior of its own and renders the rows the field publishes.
pub struct AutocompletePopup {
    shared: Rc<RefCell<PopupState>>,
    size: Size,
}

impl AutocompletePopup {
    fn row_at(&self, position: Point) -> Option<usize> {
        if !self.size.contains(position) {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let row = (position.y / POPUP_ROW_HEIGHT) as usize + self.shared.borrow().scroll;
        (row < self.shared.borrow().rows.len()).then_some(row)
    }
}

impl Widget for AutocompletePopup {
    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("autocomplete-popup")
    }

    fn layout(
        &mut self,
        _context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let mut shared = self.shared.borrow_mut();
        let visible = shared.rows.len().min(MAX_POPUP_ROWS);
        if let Some(active) = shared.active {
            if active < shared.scroll {
                shared.scroll = active;
            } else if active >= shared.scroll + visible {
                shared.scroll = active + 1 - visible;
            }
        }
        shared.scroll = shared
            .scroll
            .min(shared.rows.len().saturating_sub(visible));
        #[allow(clippy::cast_precision_loss)]
        let height = visible as f32 * POPUP_ROW_HEIGHT;
        self.size = constraints.constrain(size(shared.width.max(1.0), height.max(1.0)));
        LayoutResult::new(self.size)
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        let theme = environment.theme();
        let shared = self.shared.borrow();
        context.fill_rounded_rect(
            Rect::new(bounds.origin, self.size),
            theme.corner_radius(),
            theme.raised_surface(),
        );
        context.stroke_rounded_rect(
            Rect::new(bounds.origin, self.size),
            theme.corner_radius(),
            1.0,
            theme.border(),
        );
        let font = environment.font_for(&DssStyle::default());
        let style = TextStyle::new_with_font(font, theme.foreground());
        let visible = self.size.height() / POPUP_ROW_HEIGHT;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let visible = visible as usize;
        for (slot, (_, label)) in shared
            .rows
            .iter()
            .enumerate()
            .skip(shared.scroll)
            .take(visible)
        {
            #[allow(clippy::cast_precision_loss)]
            let row_y = (slot - shared.scroll) as f32 * POPUP_ROW_HEIGHT;
            let row_bounds = Rect::new(
                Point::new(bounds.origin.x, bounds.origin.y + row_y),
                size(self.size.width(), POPUP_ROW_HEIGHT),
            );
            if shared.active == Some(slot) || shared.hovered == Some(slot) {
                context.fill_rect(row_bounds, theme.accent().with_alpha(0.22));
            }
            let layout = environment.text_shaper().layout(label, &style, None);
            let text_height = layout.size().height();
            context.with_clip(row_bounds, |context| {
                context.draw_text(
                    layout,
                    Point::new(
                        row_bounds.origin.x + HORIZONTAL_PADDING,
                        row_bounds.origin.y + (POPUP_ROW_HEIGHT - text_height) / 2.0,
                    ),
                );
            });
        }
    }

    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        if context.phase() != EventPhase::Target {
            return EventStatus::Ignored;
        }
        match event {
            InputEvent::PointerMove(pointer) => {
                let hovered = self.row_at(pointer.position);
                let mut shared = self.shared.borrow_mut();
                if shared.hovered != hovered {
                    shared.hovered = hovered;
                    context.request_redraw();
                }
                EventStatus::Handled
            }
            InputEvent::PointerDown(pointer) => {
                if pointer.button != Some(PointerButton::Primary) {
                    return EventStatus::Ignored;
                }
                if let Some(row) = self.row_at(pointer.position) {
                    self.shared.borrow_mut().click = Some(row);
                    context.request_layout();
                }
                EventStatus::Handled
            }
            InputEvent::Wheel(wheel) => {
                let delta = match wheel.delta {
                    WheelDelta::Pixels(delta) => delta.y,
                    WheelDelta::Lines(delta) => delta.y * POPUP_ROW_HEIGHT * 3.0,
                };
                let mut shared = self.shared.borrow_mut();
                let visible = shared.rows.len().min(MAX_POPUP_ROWS);
                let max_scroll = shared.rows.len().saturating_sub(visible);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let steps = (-delta / POPUP_ROW_HEIGHT).round() as i32;
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_possible_wrap,
                    clippy::cast_sign_loss
                )]
                let next = (shared.scroll as i32 + steps).clamp(0, max_scroll as i32) as usize;
                if next != shared.scroll {
                    shared.scroll = next;
                    context.request_layout();
                }
                EventStatus::Handled
            }
            _ => EventStatus::Ignored,
        }
    }

    fn accessibility(&self) -> Accessibility {
        Accessibility::new(AccessibilityRole::List)
    }
}

fn default_padding() -> Insets {
    Insets::new(
        VERTICAL_PADDING,
        HORIZONTAL_PADDING,
        VERTICAL_PADDING,
        HORIZONTAL_PADDING,
    )
}

fn size(width: f32, height: f32) -> Size {
    Size::new(width, height).expect("autocomplete sizes are non-negative and not NaN")
}

fn empty_layout() -> TextLayout {
    drawen_render::FontdueTextShaper::ubuntu_light().layout(
        "",
        &TextStyle::new(1.0, Color::BLACK),
        None,
    )
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use drawen_core::{KeyCode, KeyEvent, PointerButtons, PointerEvent, PointerId};
    use drawen_ui::UiRuntime;

    use super::*;

    fn viewport() -> Constraints {
        Constraints::loose(Size::new(300.0, 200.0).expect("valid viewport"))
    }

    fn pointer_down(x: f32, y: f32) -> InputEvent {
        InputEvent::PointerDown(PointerEvent {
            pointer_id: PointerId(1),
            position: Point::new(x, y),
            button: Some(PointerButton::Primary),
            buttons: PointerButtons::PRIMARY,
            modifiers: Modifiers::NONE,
        })
    }

    fn key(named: NamedKey) -> InputEvent {
        InputEvent::KeyDown(KeyEvent {
            key: Key::Named(named),
            code: KeyCode::Unidentified,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
    }

    fn expanded(runtime: &UiRuntime) -> bool {
        runtime
            .semantic_tree()
            .nodes()
            .iter()
            .find(|node| node.role() == AccessibilityRole::ComboBox)
            .is_some_and(|node| node.states().contains(AccessibilityState::EXPANDED))
    }

    fn setup() -> (
        UiRuntime,
        Signal<usize>,
        Rc<RefCell<Vec<usize>>>,
        drawen_ui::Subscription,
    ) {
        let field = Autocomplete::new(["alice", "bob", "carol"]);
        let selected = field.selected();
        let observed = Rc::new(RefCell::new(Vec::new()));
        let subscription = selected.subscribe({
            let observed = Rc::clone(&observed);
            move |index| observed.borrow_mut().push(*index)
        });
        let popup = field.popup();
        let mut runtime = UiRuntime::new(field);
        let root = runtime.root();
        runtime.append_child(root, popup).expect("popup appends");
        runtime.layout(viewport()).expect("layout succeeds");
        (runtime, selected, observed, subscription)
    }

    #[test]
    fn typing_filters_and_enter_commits_the_active_option() {
        let (mut runtime, selected, observed, _subscription) = setup();
        assert!(runtime.dispatch_event(&pointer_down(10.0, 5.0)).is_handled());
        runtime.layout(viewport()).expect("layout succeeds");
        assert!(expanded(&runtime), "focus opens the full list");

        assert!(runtime.dispatch_event(&InputEvent::TextInput("bo".into())).is_handled());
        runtime.layout(viewport()).expect("layout succeeds");
        assert!(runtime.dispatch_event(&key(NamedKey::Enter)).is_handled());
        runtime.layout(viewport()).expect("layout succeeds");

        assert_eq!(*observed.borrow(), vec![1]);
        assert!(!expanded(&runtime), "commit closes the list");
        drop(selected);
    }

    #[test]
    fn clicking_a_popup_row_commits_it() {
        let (mut runtime, selected, observed, _subscription) = setup();
        assert!(runtime.dispatch_event(&pointer_down(10.0, 5.0)).is_handled());
        let field_height = runtime.layout(viewport()).expect("layout succeeds").size().height();

        // The popup floats below the field with the default overlay margin;
        // with an empty query the second row is bob (option index 1).
        let y = field_height + 8.0 + POPUP_ROW_HEIGHT + 5.0;
        assert!(runtime.dispatch_event(&pointer_down(10.0, y)).is_handled());
        runtime.layout(viewport()).expect("layout succeeds");

        assert_eq!(*observed.borrow(), vec![1]);
        assert!(!expanded(&runtime));
        drop(selected);
    }

    #[test]
    fn escape_closes_the_list_without_committing() {
        let (mut runtime, selected, observed, _subscription) = setup();
        assert!(runtime.dispatch_event(&pointer_down(10.0, 5.0)).is_handled());
        runtime.layout(viewport()).expect("layout succeeds");
        assert!(expanded(&runtime));

        assert!(runtime.dispatch_event(&key(NamedKey::Escape)).is_handled());
        runtime.layout(viewport()).expect("layout succeeds");
        assert!(!expanded(&runtime));
        assert!(observed.borrow().is_empty());
        drop(selected);
    }

    #[test]
    fn arrow_keys_walk_the_filtered_rows() {
        let (mut runtime, selected, observed, _subscription) = setup();
        assert!(runtime.dispatch_event(&pointer_down(10.0, 5.0)).is_handled());
        runtime.layout(viewport()).expect("layout succeeds");
        assert!(runtime.dispatch_event(&InputEvent::TextInput("a".into())).is_handled());
        runtime.layout(viewport()).expect("layout succeeds");
        // "a" matches alice and carol; step to the second match and commit.
        assert!(runtime.dispatch_event(&key(NamedKey::ArrowDown)).is_handled());
        runtime.layout(viewport()).expect("layout succeeds");
        assert!(runtime.dispatch_event(&key(NamedKey::Enter)).is_handled());
        runtime.layout(viewport()).expect("layout succeeds");
        assert_eq!(*observed.borrow(), vec![2]);
        drop(selected);
    }
}