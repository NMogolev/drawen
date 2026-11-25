use drawen_core::{
    Color, Constraints, FocusChanged, InputEvent, Insets, Key, Modifiers, NamedKey, Point,
    PointerButton, Rect, Size, TextPreedit, WheelDelta,
};
use drawen_render::{PaintContext, TextLayout, TextStyle};
use drawen_ui::{
    Accessibility, AccessibilityAction, AccessibilityActions, AccessibilityRole,
    AccessibilityState, EventContext, EventPhase, EventStatus, LayoutContext, LayoutResult,
    Signal, StyleTarget, UiEnvironment, Widget,
};

use crate::text_editor::TextEditor;

const MIN_WIDTH: f32 = 320.0;
const ROW_HEIGHT: f32 = 32.0;
const MAX_VISIBLE_ROWS: usize = 8;
const ITEM_INSET: f32 = 10.0;
const HINT_GAP: f32 = 24.0;
const QUERY_VERTICAL_PADDING: f32 = 8.0;
const DIVIDER_GAP: f32 = 6.0;
const DEFAULT_PADDING: Insets = Insets::new(8.0, 8.0, 8.0, 8.0);
const DEFAULT_PLACEHOLDER: &str = "Type a command…";
const EMPTY_MESSAGE: &str = "No matching commands";

/// One command offered by a [`CommandPalette`].
///
/// Commands carry plain data instead of child widgets because the palette
/// filters, scrolls, and highlights its rows without rebuilding a subtree.
#[derive(Clone, Debug, PartialEq)]
pub struct PaletteCommand {
    label: String,
    hint: Option<String>,
}

impl PaletteCommand {
    /// Creates a command with `label`.
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            hint: None,
        }
    }

    /// Returns this command with a right-aligned hint, such as a shortcut.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Returns the visible label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the hint, if one is configured.
    #[must_use]
    pub fn hint(&self) -> Option<&str> {
        self.hint.as_deref()
    }
}

/// A modal search palette that filters commands as the user types.
///
/// `CommandPalette` is data-driven like [`crate::Menu`]: it owns its query
/// field and rows, paints its own surface, and handles pointer and keyboard
/// input without child widgets. Place it as the single child of a
/// [`crate::Modal`] and focus it when the modal opens:
///
/// ```text
/// modal
/// └── command-palette   (query field + filtered rows, modal overlay root)
/// ```
///
/// ```rust,ignore
/// let modal = Modal::new();
/// let handle = modal.handle();
/// let modal = runtime.append_child(root, modal)?;
/// let palette = CommandPalette::new([
///     PaletteCommand::new("Open file").with_hint("Ctrl+O"),
///     PaletteCommand::new("Save all"),
/// ]);
/// let executed = palette.executed();
/// let palette = runtime.append_child(modal, palette)?;
/// // On open: handle.open(); invalidate layout; focus_widget(palette).
/// ```
///
/// Typing filters commands by case-insensitive substring. Arrow keys move the
/// active row, Enter executes it, and a primary click on a row executes it
/// directly. Executing emits [`Self::executed`] with the command's index in
/// the original list and asks the runtime to dismiss the surrounding overlay,
/// which closes a light-dismiss [`crate::Modal`]. Escape is handled by the
/// runtime's overlay dismissal, not by the palette itself.
pub struct CommandPalette {
    commands: Vec<PaletteCommand>,
    placeholder: String,
    editor: TextEditor,
    preedit: Option<TextPreedit>,
    filtered: Vec<usize>,
    active: Option<usize>,
    scroll: usize,
    pressed: Option<usize>,
    dragging: bool,
    focused: bool,
    executed: Signal<usize>,
    size: Size,
    padding: Insets,
    query_height: f32,
    query_scroll_x: f32,
    query_layout: TextLayout,
    placeholder_layout: Option<TextLayout>,
    empty_layout: Option<TextLayout>,
    label_layouts: Vec<Option<TextLayout>>,
    hint_layouts: Vec<Option<TextLayout>>,
    text_color: Color,
    muted_color: Color,
    hover_color: Color,
    surface_color: Color,
    border_color: Color,
    accent_color: Color,
    selection_color: Color,
    shadow_color: Color,
    radius: f32,
}

impl CommandPalette {
    /// Creates a palette offering `commands`.
    #[must_use]
    pub fn new(commands: impl IntoIterator<Item = PaletteCommand>) -> Self {
        let commands: Vec<PaletteCommand> = commands.into_iter().collect();
        let filtered: Vec<usize> = (0..commands.len()).collect();
        Self {
            commands,
            placeholder: DEFAULT_PLACEHOLDER.to_owned(),
            editor: TextEditor::with_text(false, ""),
            preedit: None,
            active: (!filtered.is_empty()).then_some(0),
            filtered,
            scroll: 0,
            pressed: None,
            dragging: false,
            focused: false,
            executed: Signal::new(),
            size: Size::ZERO,
            padding: DEFAULT_PADDING,
            query_height: 0.0,
            query_scroll_x: 0.0,
            query_layout: empty_layout(),
            placeholder_layout: None,
            empty_layout: None,
            label_layouts: Vec::new(),
            hint_layouts: Vec::new(),
            text_color: Color::BLACK,
            muted_color: Color::BLACK,
            hover_color: Color::TRANSPARENT,
            surface_color: Color::WHITE,
            border_color: Color::TRANSPARENT,
            accent_color: Color::BLACK,
            selection_color: Color::TRANSPARENT,
            shadow_color: Color::TRANSPARENT,
            radius: 0.0,
        }
    }

    /// Returns this palette with a custom query placeholder.
    #[must_use]
    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Returns the configured commands in their original order.
    #[must_use]
    pub fn commands(&self) -> &[PaletteCommand] {
        &self.commands
    }

    /// Returns the current query text.
    #[must_use]
    pub fn query(&self) -> &str {
        self.editor.text()
    }

    /// Returns the original indices of the commands matching the query.
    #[must_use]
    pub fn matched(&self) -> &[usize] {
        &self.filtered
    }

    /// Returns the original index of the active command, if any.
    #[must_use]
    pub fn active_command(&self) -> Option<usize> {
        self.active.and_then(|index| self.filtered.get(index).copied())
    }

    /// Returns a signal emitted with the executed command's original index.
    ///
    /// The signal fires before the surrounding overlay is asked to dismiss.
    #[must_use]
    pub fn executed(&self) -> Signal<usize> {
        self.executed.clone()
    }

    /// Recomputes the matching rows, keeping the active command when it still
    /// matches and falling back to the first match otherwise.
    fn refilter(&mut self) {
        let previous = self.active_command();
        let query = self.editor.text().to_lowercase();
        self.filtered = self
            .commands
            .iter()
            .enumerate()
            .filter(|(_, command)| query.is_empty() || command.label().to_lowercase().contains(&query))
            .map(|(index, _)| index)
            .collect();
        self.active = previous
            .and_then(|command| self.filtered.iter().position(|index| *index == command))
            .or((!self.filtered.is_empty()).then_some(0));
        if self.active.is_none() {
            self.scroll = 0;
        }
        self.ensure_active_visible();
    }

    fn visible_rows(&self) -> usize {
        if self.filtered.is_empty() {
            1
        } else {
            self.filtered.len().min(MAX_VISIBLE_ROWS)
        }
    }

    fn rows_top(&self) -> f32 {
        self.padding.top + self.query_height + DIVIDER_GAP * 2.0 + 1.0
    }

    fn row_at(&self, point: Point) -> Option<usize> {
        if point.x < 0.0 || point.x > self.size.width() {
            return None;
        }
        let offset = point.y - self.rows_top();
        if offset < 0.0 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let row = (offset / ROW_HEIGHT) as usize;
        let index = self.scroll + row;
        (row < self.visible_rows() && index < self.filtered.len()).then_some(index)
    }

    fn in_query(&self, point: Point) -> bool {
        point.x >= 0.0
            && point.x <= self.size.width()
            && point.y >= self.padding.top
            && point.y < self.padding.top + self.query_height
    }

    fn ensure_active_visible(&mut self) {
        let Some(active) = self.active else {
            return;
        };
        if active < self.scroll {
            self.scroll = active;
        } else if active >= self.scroll + MAX_VISIBLE_ROWS {
            self.scroll = active + 1 - MAX_VISIBLE_ROWS;
        }
        let max_scroll = self.filtered.len().saturating_sub(MAX_VISIBLE_ROWS);
        self.scroll = self.scroll.min(max_scroll);
    }

    fn set_active(&mut self, active: Option<usize>, context: &mut EventContext<'_>) {
        if self.active == active {
            return;
        }
        self.active = active;
        self.ensure_active_visible();
        context.request_redraw();
    }

    fn step(&mut self, forward: bool, context: &mut EventContext<'_>) {
        let len = self.filtered.len();
        if len == 0 {
            return;
        }
        let next = match self.active {
            Some(current) if forward => (current + 1).min(len - 1),
            Some(current) => current.saturating_sub(1),
            None => 0,
        };
        self.set_active(Some(next), context);
    }

    fn activate(&mut self, index: usize, context: &mut EventContext<'_>) -> bool {
        let Some(&command) = self.filtered.get(index) else {
            return false;
        };
        self.active = Some(index);
        self.executed.emit(&command);
        context.request_overlay_dismiss();
        context.request_redraw();
        true
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

    fn query_text_origin(&self) -> Point {
        Point::new(
            self.padding.left + ITEM_INSET - self.query_scroll_x,
            self.padding.top + QUERY_VERTICAL_PADDING,
        )
    }

    fn query_viewport_width(&self) -> f32 {
        (self.size.width() - self.padding.horizontal() - ITEM_INSET * 2.0).max(1.0)
    }

    fn ensure_query_caret_visible(&mut self) {
        let caret = self.query_layout.caret_rect(self.display().1, 1.0).origin.x;
        let width = self.query_viewport_width();
        if caret < self.query_scroll_x {
            self.query_scroll_x = caret;
        } else if caret + 1.0 > self.query_scroll_x + width {
            self.query_scroll_x = caret + 1.0 - width;
        }
        self.query_scroll_x = self.query_scroll_x.max(0.0);
    }

    fn hit_test_caret(&self, position: Point) -> usize {
        let origin = self.query_text_origin();
        self.query_layout.byte_at_point(Point::new(
            position.x - origin.x,
            position.y - origin.y,
        ))
    }

    /// Applies `edit`, refiltering and requesting layout when the query changed.
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
        self.ensure_query_caret_visible();
        context.request_layout();
        true
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
                let Some(index) = self.active else {
                    return EventStatus::Ignored;
                };
                if self.activate(index, context) {
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
                self.ensure_query_caret_visible();
                context.request_redraw();
                EventStatus::Handled
            }
            Key::Named(NamedKey::ArrowRight) => {
                self.editor.move_horizontal(true, command, extend);
                self.ensure_query_caret_visible();
                context.request_redraw();
                EventStatus::Handled
            }
            Key::Named(NamedKey::Home) => {
                self.editor.move_to(0, extend);
                self.ensure_query_caret_visible();
                context.request_redraw();
                EventStatus::Handled
            }
            Key::Named(NamedKey::End) => {
                self.editor.move_to(self.editor.text().len(), extend);
                self.ensure_query_caret_visible();
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
                    self.ensure_query_caret_visible();
                    context.request_redraw();
                    EventStatus::Handled
                } else {
                    EventStatus::Ignored
                }
            }
            _ => EventStatus::Ignored,
        }
    }
    fn paint_query(&self, context: &mut PaintContext<'_>, surface: Rect) {
        let query_rect = Rect::new(
            Point::new(
                surface.origin.x + self.padding.left + ITEM_INSET,
                surface.origin.y + self.padding.top,
            ),
            size(
                (self.size.width() - self.padding.horizontal() - ITEM_INSET * 2.0).max(0.0),
                self.query_height,
            ),
        );
        let origin = Point::new(
            query_rect.origin.x - self.query_scroll_x,
            surface.origin.y + self.padding.top + QUERY_VERTICAL_PADDING,
        );
        context.with_clip(query_rect, |context| {
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
                let (_, caret) = self.display();
                let caret_rect = self.query_layout.caret_rect(caret, 1.0);
                context.fill_rect(
                    Rect::new(
                        Point::new(caret_rect.origin.x + origin.x, query_rect.origin.y),
                        size(1.0, query_rect.size.height()),
                    ),
                    self.accent_color,
                );
            }
        });
    }

    fn paint_rows(&self, context: &mut PaintContext<'_>, surface: Rect) {
        let divider_y = surface.origin.y + self.padding.top + self.query_height + DIVIDER_GAP;
        context.fill_rect(
            Rect::new(
                Point::new(surface.origin.x + self.padding.left, divider_y),
                size(self.size.width() - self.padding.horizontal(), 1.0),
            ),
            self.border_color,
        );

        let rows_top = surface.origin.y + self.rows_top();
        if self.filtered.is_empty() {
            if let Some(empty) = &self.empty_layout {
                let text_height = empty.size().height();
                context.draw_text(
                    empty.clone(),
                    Point::new(
                        surface.origin.x + self.padding.left + ITEM_INSET,
                        rows_top + (ROW_HEIGHT - text_height) / 2.0,
                    ),
                );
            }
            return;
        }
        for row in 0..self.visible_rows() {
            let index = self.scroll + row;
            let Some(&command) = self.filtered.get(index) else {
                break;
            };
            #[allow(clippy::cast_precision_loss)]
            let row_y = rows_top + row as f32 * ROW_HEIGHT;
            let row_rect = Rect::new(
                Point::new(surface.origin.x + self.padding.left, row_y),
                size(self.size.width() - self.padding.horizontal(), ROW_HEIGHT),
            );
            if self.active == Some(index) {
                context.fill_rounded_rect(row_rect, self.radius * 0.6, self.hover_color);
            }
            if let Some(text) = &self.label_layouts[command] {
                let text_height = text.size().height();
                context.draw_text(
                    text.clone(),
                    Point::new(
                        row_rect.origin.x + ITEM_INSET,
                        row_rect.origin.y + (ROW_HEIGHT - text_height) / 2.0,
                    ),
                );
            }
            if let Some(hint) = &self.hint_layouts[command] {
                let hint_width = hint.size().width();
                let hint_height = hint.size().height();
                context.draw_text(
                    hint.clone(),
                    Point::new(
                        row_rect.right() - ITEM_INSET - hint_width,
                        row_rect.origin.y + (ROW_HEIGHT - hint_height) / 2.0,
                    ),
                );
            }
        }
    }
}

impl Widget for CommandPalette {
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
        self.accent_color = style.accent_or(theme.accent());
        self.selection_color = theme.selection();
        self.shadow_color = theme.shadow();
        self.radius = style.corner_radius.unwrap_or_else(|| theme.corner_radius());

        let text_style = TextStyle::new_with_font(font.clone(), self.text_color);
        let (query, _) = self.display();
        self.query_layout = environment.text_shaper().layout(&query, &text_style, None);
        let font_size = font.pixel_size(environment.logical_dpi());
        self.query_height = (self.query_layout.size().height() + QUERY_VERTICAL_PADDING * 2.0)
            .max(font_size + QUERY_VERTICAL_PADDING * 2.0);
        self.placeholder_layout = (!self.placeholder.is_empty()).then(|| {
            environment.text_shaper().layout(
                &self.placeholder,
                &TextStyle::new_with_font(font.clone(), self.muted_color),
                None,
            )
        });

        self.refilter();
        self.label_layouts.clear();
        self.hint_layouts.clear();
        let mut row_width: f32 = 0.0;
        for command in &self.commands {
            let layout =
                environment
                    .text_shaper()
                    .layout(command.label(), &text_style, None);
            let mut width = layout.size().width();
            self.label_layouts.push(Some(layout));
            let hint = command.hint().map(|hint| {
                let layout = environment.text_shaper().layout(
                    hint,
                    &TextStyle::new_with_font(font.clone(), self.muted_color),
                    None,
                );
                width += HINT_GAP + layout.size().width();
                layout
            });
            self.hint_layouts.push(hint);
            row_width = row_width.max(width);
        }
        self.empty_layout = Some(environment.text_shaper().layout(
            EMPTY_MESSAGE,
            &TextStyle::new_with_font(font.clone(), self.muted_color),
            None,
        ));

        let inner_width = (row_width + ITEM_INSET * 2.0)
            .max(self.query_layout.size().width() + ITEM_INSET * 2.0)
            .max(self.placeholder_layout.as_ref().map_or(0.0, |layout| {
                layout.size().width() + ITEM_INSET * 2.0
            }));
        let width = (inner_width + self.padding.horizontal()).max(MIN_WIDTH);
        #[allow(clippy::cast_precision_loss)]
        let rows_height = self.visible_rows() as f32 * ROW_HEIGHT;
        let height =
            self.padding.vertical() + self.query_height + DIVIDER_GAP * 2.0 + 1.0 + rows_height;
        self.size = constraints.constrain(size(width, height));
        self.ensure_active_visible();
        self.ensure_query_caret_visible();
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
        self.paint_query(context, surface);
        self.paint_rows(context, surface);
    }

    #[allow(clippy::too_many_lines)]
    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        if context.phase() != EventPhase::Target {
            return EventStatus::Ignored;
        }
        match event {
            InputEvent::PointerDown(pointer) if pointer.button == Some(PointerButton::Primary) => {
                context.request_focus();
                context.capture_pointer(pointer.pointer_id);
                if self.in_query(pointer.position) {
                    let byte = self.hit_test_caret(pointer.position);
                    if pointer.modifiers.contains(Modifiers::SHIFT) {
                        self.editor.move_to(byte, true);
                    } else {
                        self.editor.set_caret(byte);
                    }
                    self.dragging = true;
                    context.request_redraw();
                } else {
                    self.pressed = self.row_at(pointer.position);
                    self.set_active(self.pressed, context);
                }
                EventStatus::Handled
            }
            InputEvent::PointerMove(pointer) => {
                if self.dragging {
                    if pointer.buttons.contains_button(PointerButton::Primary) {
                        self.editor
                            .move_to(self.hit_test_caret(pointer.position), true);
                        context.request_redraw();
                        return EventStatus::Handled;
                    }
                    return EventStatus::Ignored;
                }
                if let Some(index) = self.row_at(pointer.position) {
                    self.set_active(Some(index), context);
                }
                EventStatus::Ignored
            }
            InputEvent::PointerUp(pointer) if pointer.button == Some(PointerButton::Primary) => {
                context.release_pointer(pointer.pointer_id);
                self.dragging = false;
                let index = self.row_at(pointer.position);
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
            InputEvent::PointerCancel(pointer) => {
                self.dragging = false;
                self.pressed = None;
                context.release_pointer(pointer.pointer_id);
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
                if self.filtered.len() <= MAX_VISIBLE_ROWS {
                    return EventStatus::Ignored;
                }
                let delta = match wheel.delta {
                    WheelDelta::Pixels(delta) => delta.y,
                    WheelDelta::Lines(delta) => delta.y * ROW_HEIGHT * 3.0,
                };
                let max_scroll = self.filtered.len() - MAX_VISIBLE_ROWS;
                #[allow(
                    clippy::cast_precision_loss,
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss
                )]
                let next = (self.scroll as f32 - delta / ROW_HEIGHT)
                    .round()
                    .clamp(0.0, max_scroll as f32) as usize;
                if next == self.scroll {
                    return EventStatus::Ignored;
                }
                self.scroll = next;
                context.request_redraw();
                EventStatus::Handled
            }
            InputEvent::KeyDown(key) => self.handle_key(context, key),
            InputEvent::FocusChanged(FocusChanged { focused }) => {
                self.focused = *focused == Some(context.current_target());
                if !self.focused {
                    self.dragging = false;
                    self.pressed = None;
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

    fn accepts_focus(&self) -> bool {
        true
    }

    fn tab_index(&self) -> Option<i32> {
        Some(-1)
    }

    fn accessibility(&self) -> Accessibility {
        let mut accessibility = Accessibility::new(AccessibilityRole::ComboBox)
            .with_label("Command palette")
            .with_value(self.editor.text())
            .with_states(AccessibilityState::FOCUSABLE | AccessibilityState::EXPANDED)
            .with_actions(
                AccessibilityActions::FOCUS
                    | AccessibilityActions::SET_VALUE
                    | AccessibilityActions::SELECT,
            );
        if self.editor.has_selection() {
            accessibility =
                accessibility.with_text_selection(self.editor.anchor(), self.editor.focus());
        }
        accessibility
    }

    fn accessibility_action(&mut self, action: &AccessibilityAction) -> bool {
        match action {
            AccessibilityAction::SetValue(value) => {
                self.editor.set_text(value.clone());
                self.preedit = None;
                self.query_scroll_x = 0.0;
                self.refilter();
                true
            }
            AccessibilityAction::Select => {
                self.editor.select_all();
                true
            }
            _ => false,
        }
    }

    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("command-palette")
    }
}

fn size(width: f32, height: f32) -> Size {
    Size::new(width, height).expect("palette sizes are non-negative and not NaN")
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

    use drawen_core::{
        Constraints, InputEvent, Key, KeyCode, Modifiers, NamedKey, Point, PointerButtons,
        PointerEvent, PointerId, Rect, Size,
    };
    use drawen_ui::UiRuntime;

    use super::{CommandPalette, PaletteCommand, DEFAULT_PADDING, DIVIDER_GAP, ROW_HEIGHT};
    use crate::Modal;

    fn size(width: f32, height: f32) -> Size {
        Size::new(width, height).expect("valid test size")
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

    fn sample_palette() -> CommandPalette {
        CommandPalette::new([
            PaletteCommand::new("Open file").with_hint("Ctrl+O"),
            PaletteCommand::new("Copy path").with_hint("Ctrl+Shift+C"),
            PaletteCommand::new("Save all").with_hint("Ctrl+S"),
        ])
    }

    fn row_point(bounds: Rect, row: usize) -> Point {
        // Recover the query height from the measured panel so the test does
        // not depend on the default font metrics.
        let query_height = bounds.size.height()
            - DEFAULT_PADDING.vertical()
            - DIVIDER_GAP * 2.0
            - 1.0
            - 3.0 * ROW_HEIGHT;
        let rows_top = DEFAULT_PADDING.top + query_height + DIVIDER_GAP * 2.0 + 1.0;
        #[allow(clippy::cast_precision_loss)]
        let y = rows_top + row as f32 * ROW_HEIGHT + ROW_HEIGHT / 2.0;
        Point::new(bounds.origin.x + 20.0, bounds.origin.y + y)
    }

    #[test]
    fn typing_filters_and_enter_executes_the_first_match() {
        let palette = sample_palette();
        let executed = Rc::new(RefCell::new(Vec::new()));
        let _subscription = palette.executed().subscribe({
            let executed = Rc::clone(&executed);
            move |index| executed.borrow_mut().push(*index)
        });
        let mut runtime = UiRuntime::new(palette);
        runtime
            .layout(Constraints::loose(size(600.0, 600.0)))
            .expect("layout succeeds");
        runtime
            .focus_widget(runtime.root())
            .expect("palette is focusable");

        let _ = runtime.dispatch_event(&InputEvent::TextInput("copy".to_string()));
        let _ = runtime.dispatch_event(&key(NamedKey::Enter));
        assert_eq!(*executed.borrow(), vec![1]);
    }

    #[test]
    fn arrow_keys_move_the_active_row() {
        let palette = sample_palette();
        let executed = Rc::new(RefCell::new(Vec::new()));
        let _subscription = palette.executed().subscribe({
            let executed = Rc::clone(&executed);
            move |index| executed.borrow_mut().push(*index)
        });
        let mut runtime = UiRuntime::new(palette);
        runtime
            .layout(Constraints::loose(size(600.0, 600.0)))
            .expect("layout succeeds");
        runtime
            .focus_widget(runtime.root())
            .expect("palette is focusable");

        let _ = runtime.dispatch_event(&key(NamedKey::ArrowDown));
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowDown));
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowDown));
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowUp));
        let _ = runtime.dispatch_event(&key(NamedKey::Enter));
        assert_eq!(*executed.borrow(), vec![1]);
    }

    #[test]
    fn backspace_refilters_and_enter_executes_the_match() {
        let palette = sample_palette();
        let executed = Rc::new(RefCell::new(Vec::new()));
        let _subscription = palette.executed().subscribe({
            let executed = Rc::clone(&executed);
            move |index| executed.borrow_mut().push(*index)
        });
        let mut runtime = UiRuntime::new(palette);
        runtime
            .layout(Constraints::loose(size(600.0, 600.0)))
            .expect("layout succeeds");
        runtime
            .focus_widget(runtime.root())
            .expect("palette is focusable");

        let _ = runtime.dispatch_event(&InputEvent::TextInput("copyx".to_string()));
        let _ = runtime.dispatch_event(&key(NamedKey::Enter));
        assert!(executed.borrow().is_empty());
        let _ = runtime.dispatch_event(&key(NamedKey::Backspace));
        let _ = runtime.dispatch_event(&key(NamedKey::Enter));
        assert_eq!(*executed.borrow(), vec![1]);
        let semantics = runtime.semantic_tree();
        let node = semantics.node(runtime.root()).expect("root node");
        assert_eq!(node.value(), Some("copy"));
    }

    #[test]
    fn pointer_release_executes_the_row_under_the_pointer() {
        let palette = sample_palette();
        let executed = Rc::new(RefCell::new(Vec::new()));
        let _subscription = palette.executed().subscribe({
            let executed = Rc::clone(&executed);
            move |index| executed.borrow_mut().push(*index)
        });
        let mut runtime = UiRuntime::new(palette);
        runtime
            .layout(Constraints::loose(size(600.0, 600.0)))
            .expect("layout succeeds");

        let point = row_point(
            runtime.bounds(runtime.root()).expect("palette has bounds"),
            2,
        );
        let _ = runtime.dispatch_event(&pointer(point, false));
        let _ = runtime.dispatch_event(&pointer(point, true));
        assert_eq!(*executed.borrow(), vec![2]);
    }

    #[test]
    fn executing_dismisses_the_surrounding_modal() {
        let modal = Modal::new();
        let handle = modal.handle();
        let palette = sample_palette();
        let executed = Rc::new(RefCell::new(Vec::new()));
        let _subscription = palette.executed().subscribe({
            let executed = Rc::clone(&executed);
            move |index| executed.borrow_mut().push(*index)
        });
        let mut runtime = UiRuntime::new(modal);
        let root = runtime.root();
        let palette = runtime
            .append_child(root, palette)
            .expect("modal accepts one child");
        handle.open();
        runtime
            .layout(Constraints::tight(size(600.0, 600.0)).expect("valid constraints"))
            .expect("layout succeeds");
        runtime
            .focus_widget(palette)
            .expect("palette is focusable");

        let _ = runtime.dispatch_event(&key(NamedKey::Enter));
        assert_eq!(*executed.borrow(), vec![0]);
        assert!(!handle.is_open());
    }

    #[test]
    fn a_query_without_matches_ignores_enter() {
        let palette = sample_palette();
        let executed = Rc::new(RefCell::new(Vec::new()));
        let _subscription = palette.executed().subscribe({
            let executed = Rc::clone(&executed);
            move |index| executed.borrow_mut().push(*index)
        });
        let mut runtime = UiRuntime::new(palette);
        runtime
            .layout(Constraints::loose(size(600.0, 600.0)))
            .expect("layout succeeds");
        runtime
            .focus_widget(runtime.root())
            .expect("palette is focusable");

        let _ = runtime.dispatch_event(&InputEvent::TextInput("zzz".to_string()));
        let _ = runtime.dispatch_event(&key(NamedKey::Enter));
        assert!(executed.borrow().is_empty());
    }
}
