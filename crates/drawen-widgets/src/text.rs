use std::{
    ops::Range,
    time::{Duration, Instant},
};

use drawen_core::{
    Constraints, InputEvent, Key, Modifiers, NamedKey, Point, PointerButton, PointerId, Rect,
};
use drawen_render::{Font, FontdueTextShaper, PaintContext, TextLayout, TextShaper, TextStyle};
use drawen_ui::{
    Accessibility, AccessibilityRole, AccessibilityState, DssStyle, EventContext, EventPhase,
    EventStatus, LayoutContext, LayoutResult, StyleTarget, UiEnvironment, Widget,
};

const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(400);

/// A widget that paints text.
///
/// [`Self::new`] accepts a pre-shaped layout for fixed-size text. Use
/// [`Self::wrapping`] for text that must reflow to its available width with a
/// specific shaper, or [`Self::styled`] to use the runtime's font database.
///
/// By default the text is decorative: it ignores pointer input and focus. Call
/// [`Self::selectable`] to let the reader drag a selection, double-click a word,
/// and copy it with Ctrl+C (Ctrl+A selects everything). A selectable label still
/// never edits its text.
pub struct Text {
    layout: TextLayout,
    source: Option<TextSource>,
    selectable: bool,
    selection: Range<usize>,
    anchor: usize,
    dragging: Option<PointerId>,
    last_click: Option<(Instant, Point)>,
}

struct TextSource {
    text: String,
    shaper: Option<FontdueTextShaper>,
    style: TextStyle,
    last_layout: Option<TextLayoutRequest>,
}

#[derive(PartialEq)]
struct TextLayoutRequest {
    font: Font,
    width: Option<f32>,
    text_shaper_generation: Option<u64>,
}

impl Text {
    /// Creates a text widget from a pre-shaped layout.
    #[must_use]
    pub const fn new(layout: TextLayout) -> Self {
        Self {
            layout,
            source: None,
            selectable: false,
            selection: 0..0,
            anchor: 0,
            dragging: None,
            last_click: None,
        }
    }

    /// Creates text that reshapes and wraps to its available width during layout.
    ///
    /// The supplied shaper is used for this widget. Use [`Self::styled`] when
    /// the runtime environment's font database should be used instead.
    #[must_use]
    pub fn wrapping(text: impl Into<String>, shaper: FontdueTextShaper, style: TextStyle) -> Self {
        let text = text.into();
        let layout = shaper.layout(&text, &style, None);
        Self {
            layout,
            source: Some(TextSource {
                text,
                shaper: Some(shaper),
                style,
                last_layout: None,
            }),
            selectable: false,
            selection: 0..0,
            anchor: 0,
            dragging: None,
            last_click: None,
        }
    }

    /// Creates text that reshapes and wraps with the runtime's text shaper.
    ///
    /// The style supplies the base font request and color. Per-node DSS font
    /// properties and the application default font are applied on top during
    /// layout, matching the behavior of standard controls.
    #[must_use]
    pub fn styled(text: impl Into<String>, style: TextStyle) -> Self {
        let text = text.into();
        let layout = TextLayout::empty(&text, style.color());
        Self {
            layout,
            source: Some(TextSource {
                text,
                shaper: None,
                style,
                last_layout: None,
            }),
            selectable: false,
            selection: 0..0,
            anchor: 0,
            dragging: None,
            last_click: None,
        }
    }

    /// Returns the pre-shaped text layout.
    #[must_use]
    pub const fn text_layout(&self) -> &TextLayout {
        &self.layout
    }

    /// Makes the text selectable so the reader can copy it.
    ///
    /// A selectable label accepts focus and pointer input: drag to select a
    /// range, double-click to select a word, Shift-click to extend, Ctrl+C to
    /// copy, Ctrl+A to select everything, and Escape to clear. The text itself
    /// is never edited.
    #[must_use]
    pub const fn selectable(mut self) -> Self {
        self.selectable = true;
        self
    }

    /// Returns the currently selected byte range, empty when nothing is selected.
    #[must_use]
    pub fn selection(&self) -> Range<usize> {
        self.selection.clone()
    }

    fn byte_at(&self, position: Point) -> usize {
        self.layout.byte_at_point(position)
    }

    fn set_selection(&mut self, anchor: usize, focus: usize) {
        self.anchor = anchor;
        self.selection = if anchor <= focus {
            anchor..focus
        } else {
            focus..anchor
        };
    }

    fn selected_text(&self) -> Option<&str> {
        (!self.selection.is_empty()).then(|| &self.layout.text()[self.selection.clone()])
    }

    fn select_word_at(&mut self, byte: usize) {
        let text = self.layout.text();
        let mut start = byte;
        let mut end = byte;
        while start > 0 {
            let previous = previous_boundary(text, start);
            if !text[previous..start].chars().all(is_word_character) {
                break;
            }
            start = previous;
        }
        while end < text.len() {
            let next = next_boundary(text, end);
            if !text[end..next].chars().all(is_word_character) {
                break;
            }
            end = next;
        }
        self.set_selection(start, end);
    }
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn previous_boundary(text: &str, byte: usize) -> usize {
    let mut boundary = byte.saturating_sub(1);
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

fn next_boundary(text: &str, byte: usize) -> usize {
    let mut boundary = (byte + 1).min(text.len());
    while boundary < text.len() && !text.is_char_boundary(boundary) {
        boundary += 1;
    }
    boundary
}

impl Widget for Text {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let style = context.style();
        if let Some(source) = &mut self.source {
            let max_width = constraints.max().width();
            let width = max_width.is_finite().then_some(max_width.max(0.0));
            let font = context.environment().font_from(source.style.font(), &style);
            let color = style.foreground.unwrap_or(source.style.color());
            let request = TextLayoutRequest {
                font,
                width,
                text_shaper_generation: source
                    .shaper
                    .is_none()
                    .then(|| context.environment().text_shaper_generation()),
            };
            if source.last_layout.as_ref() != Some(&request) {
                let shaper: &dyn TextShaper = match &source.shaper {
                    Some(shaper) => shaper,
                    None => context.environment().text_shaper(),
                };
                let text_style = TextStyle::new_with_font(request.font.clone(), color);
                self.layout = shaper.layout(&source.text, &text_style, request.width);
                source.last_layout = Some(request);
            } else if self.layout.color() != color {
                self.layout = self.layout.clone().with_color(color);
            }
        } else if let Some(color) = style.foreground
            && self.layout.color() != color
        {
            self.layout = self.layout.clone().with_color(color);
        }
        LayoutResult::new(constraints.constrain(self.layout.size()))
    }

    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("text")
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        self.paint_styled(context, environment, bounds, &DssStyle::default());
    }

    fn paint_styled(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        bounds: Rect,
        style: &DssStyle,
    ) {
        let layout = match style.foreground {
            Some(color) if color != self.layout.color() => self.layout.clone().with_color(color),
            _ => self.layout.clone(),
        };
        if self.selectable && !self.selection.is_empty() {
            let selection_color = environment.theme().selection();
            for rect in layout.selection_rects(self.selection.clone()) {
                context.fill_rect(
                    Rect::new(
                        Point::new(
                            rect.origin.x + bounds.origin.x,
                            rect.origin.y + bounds.origin.y,
                        ),
                        rect.size,
                    ),
                    selection_color,
                );
            }
        }
        context.draw_text(layout, bounds.origin);
    }

    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        if !self.selectable || context.phase() != EventPhase::Target {
            return EventStatus::Ignored;
        }
        match event {
            InputEvent::PointerDown(pointer) if pointer.button == Some(PointerButton::Primary) => {
                context.request_focus();
                context.capture_pointer(pointer.pointer_id);
                let now = Instant::now();
                let double_click = self.last_click.is_some_and(|(time, last)| {
                    now.duration_since(time) <= DOUBLE_CLICK_INTERVAL
                        && (last.x - pointer.position.x).abs() <= 4.0
                        && (last.y - pointer.position.y).abs() <= 4.0
                });
                self.last_click = Some((now, pointer.position));
                let byte = self.byte_at(pointer.position);
                if double_click {
                    self.select_word_at(byte);
                } else if pointer.modifiers.contains(Modifiers::SHIFT) {
                    self.set_selection(self.anchor, byte);
                } else {
                    self.set_selection(byte, byte);
                }
                self.dragging = Some(pointer.pointer_id);
                context.request_redraw();
                EventStatus::Handled
            }
            InputEvent::PointerMove(pointer) if self.dragging == Some(pointer.pointer_id) => {
                let byte = self.byte_at(pointer.position);
                self.set_selection(self.anchor, byte);
                context.request_redraw();
                EventStatus::Handled
            }
            InputEvent::PointerUp(pointer) if self.dragging == Some(pointer.pointer_id) => {
                self.dragging = None;
                context.release_pointer(pointer.pointer_id);
                EventStatus::Handled
            }
            InputEvent::PointerCancel(pointer) if self.dragging == Some(pointer.pointer_id) => {
                self.dragging = None;
                context.release_pointer(pointer.pointer_id);
                context.request_redraw();
                EventStatus::Handled
            }
            InputEvent::KeyDown(key) if !key.repeat => {
                let command = key.modifiers.contains(Modifiers::CONTROL)
                    || key.modifiers.contains(Modifiers::META);
                match &key.key {
                    Key::Character(character) if command && character.eq_ignore_ascii_case("c") => {
                        if let Some(selection) = self.selected_text() {
                            context.write_clipboard(selection);
                        }
                        EventStatus::Handled
                    }
                    Key::Character(character) if command && character.eq_ignore_ascii_case("a") => {
                        self.set_selection(0, self.layout.text().len());
                        context.request_redraw();
                        EventStatus::Handled
                    }
                    Key::Named(NamedKey::Escape) => {
                        self.set_selection(0, 0);
                        context.request_redraw();
                        EventStatus::Handled
                    }
                    _ => EventStatus::Ignored,
                }
            }
            _ => EventStatus::Ignored,
        }
    }

    fn participates_in_hit_test(&self) -> bool {
        self.selectable
    }

    fn accepts_focus(&self) -> bool {
        self.selectable
    }

    fn accessibility(&self) -> Accessibility {
        let accessibility =
            Accessibility::new(AccessibilityRole::Label).with_value(self.layout.text());
        if self.selectable {
            accessibility.with_states(AccessibilityState::FOCUSABLE | AccessibilityState::READ_ONLY)
        } else {
            accessibility
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc, sync::Arc};

    use drawen_core::{Color, Constraints, Point, Size};
    use drawen_render::{
        DisplayCommand, DisplayList, FontdueTextShaper, PaintContext, TextLayout, TextShaper,
        TextStyle,
    };
    use drawen_ui::{UiEnvironment, UiRuntime};

    use super::Text;

    struct CountingTextShaper {
        calls: Rc<Cell<usize>>,
        inner: FontdueTextShaper,
    }

    impl TextShaper for CountingTextShaper {
        fn layout(&self, text: &str, style: &TextStyle, width: Option<f32>) -> TextLayout {
            self.calls.set(self.calls.get() + 1);
            self.inner.layout(text, style, width)
        }
    }

    #[test]
    fn uses_its_text_layout_for_layout_and_paint() {
        let layout = FontdueTextShaper::ubuntu_light().layout(
            "Text",
            &TextStyle::new(12.0, Color::BLACK),
            None,
        );
        let mut runtime = UiRuntime::new(Text::new(layout.clone()));
        let mut list = DisplayList::new();

        assert_eq!(
            runtime
                .layout(Constraints::UNBOUNDED)
                .expect("text layout succeeds")
                .size(),
            layout.size()
        );
        runtime
            .paint(&mut PaintContext::new(&mut list))
            .expect("text paint succeeds");

        assert_eq!(
            list.commands(),
            &[DisplayCommand::DrawText {
                layout: Arc::new(layout),
                origin: Point::ZERO,
            }]
        );
    }

    #[test]
    fn wraps_to_the_available_width() {
        let mut runtime = UiRuntime::new(Text::wrapping(
            "one two three",
            FontdueTextShaper::ubuntu_light(),
            TextStyle::new(16.0, Color::BLACK),
        ));

        let result = runtime
            .layout(Constraints::loose(
                drawen_core::Size::new(30.0, 200.0).expect("valid text constraints"),
            ))
            .expect("wrapping text layout succeeds");

        assert!(result.size().height() > 16.0);
        assert!(result.size().width() <= 30.0);
    }

    #[test]
    fn avoids_reshaping_when_the_text_request_is_unchanged() {
        let calls = Rc::new(Cell::new(0));
        let mut environment = UiEnvironment::default();
        environment.set_text_shaper(CountingTextShaper {
            calls: Rc::clone(&calls),
            inner: FontdueTextShaper::ubuntu_light(),
        });
        let mut runtime = UiRuntime::with_environment(
            Text::styled("cached", TextStyle::new(16.0, Color::BLACK)),
            environment,
        );

        runtime
            .layout(Constraints::UNBOUNDED)
            .expect("first layout succeeds");
        runtime
            .layout(Constraints::UNBOUNDED)
            .expect("unchanged layout succeeds");
        assert_eq!(calls.get(), 1);

        runtime
            .layout(Constraints::loose(
                Size::new(40.0, 40.0).expect("valid constraints"),
            ))
            .expect("resized layout succeeds");
        assert_eq!(calls.get(), 2);
    }
}

#[cfg(test)]
mod selection_tests {
    use std::{cell::RefCell, rc::Rc};

    use drawen_core::{
        Clipboard, Color, Constraints, InputEvent, Key, KeyCode, Modifiers, NamedKey, Point,
        PointerButton, PointerButtons, PointerEvent, PointerId, Size,
    };
    use drawen_render::{FontdueTextShaper, TextLayout, TextStyle};
    use drawen_ui::UiRuntime;

    use super::Text;

    #[derive(Clone, Default)]
    struct SharedClipboard(Rc<RefCell<String>>);

    impl Clipboard for SharedClipboard {
        fn read_text(&mut self) -> Option<String> {
            let text = self.0.borrow();
            (!text.is_empty()).then(|| text.clone())
        }

        fn write_text(&mut self, text: &str) {
            text.clone_into(&mut self.0.borrow_mut());
        }
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

    fn drag(position: Point) -> InputEvent {
        InputEvent::PointerMove(PointerEvent {
            pointer_id: PointerId(1),
            position,
            button: None,
            buttons: PointerButtons::PRIMARY,
            modifiers: Modifiers::NONE,
        })
    }

    fn command(character: &str) -> InputEvent {
        InputEvent::KeyDown(drawen_core::KeyEvent {
            key: Key::Character(character.to_string()),
            code: KeyCode::Unidentified,
            repeat: false,
            modifiers: Modifiers::CONTROL,
        })
    }

    fn named(key: NamedKey) -> InputEvent {
        InputEvent::KeyDown(drawen_core::KeyEvent {
            key: Key::Named(key),
            code: KeyCode::Unidentified,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
    }

    fn sample() -> (TextLayout, UiRuntime, SharedClipboard) {
        let layout = FontdueTextShaper::ubuntu_light().layout(
            "hello world",
            &TextStyle::new(16.0, Color::BLACK),
            None,
        );
        let mut runtime = UiRuntime::new(Text::new(layout.clone()).selectable());
        let clipboard = SharedClipboard::default();
        runtime.set_clipboard(clipboard.clone());
        runtime
            .layout(Constraints::loose(
                Size::new(400.0, 100.0).expect("valid constraints"),
            ))
            .expect("layout succeeds");
        (layout, runtime, clipboard)
    }

    fn middle_y(layout: &TextLayout) -> f32 {
        layout.size().height() / 2.0
    }

    #[test]
    fn dragging_across_the_text_selects_and_copies_everything() {
        let (layout, mut runtime, clipboard) = sample();
        let y = middle_y(&layout);
        let _ = runtime.dispatch_event(&pointer(Point::new(0.0, y), false));
        let _ = runtime.dispatch_event(&drag(Point::new(layout.size().width() + 50.0, y)));
        let _ = runtime.dispatch_event(&pointer(Point::new(layout.size().width() + 50.0, y), true));
        let _ = runtime.dispatch_event(&command("c"));
        assert_eq!(*clipboard.0.borrow(), "hello world");
    }

    #[test]
    fn double_click_selects_a_word_and_copies_it() {
        let (layout, mut runtime, clipboard) = sample();
        let point = Point::new(layout.caret_x(8) + 1.0, middle_y(&layout));
        let _ = runtime.dispatch_event(&pointer(point, false));
        let _ = runtime.dispatch_event(&pointer(point, true));
        let _ = runtime.dispatch_event(&pointer(point, false));
        let _ = runtime.dispatch_event(&pointer(point, true));
        let _ = runtime.dispatch_event(&command("c"));
        assert_eq!(*clipboard.0.borrow(), "world");
    }

    #[test]
    fn ctrl_a_selects_all_and_escape_clears() {
        let (_layout, mut runtime, clipboard) = sample();
        runtime
            .focus_widget(runtime.root())
            .expect("selectable text accepts focus");
        let _ = runtime.dispatch_event(&command("a"));
        let _ = runtime.dispatch_event(&command("c"));
        assert_eq!(*clipboard.0.borrow(), "hello world");

        clipboard.0.borrow_mut().clear();
        let _ = runtime.dispatch_event(&named(NamedKey::Escape));
        let _ = runtime.dispatch_event(&command("c"));
        assert!(clipboard.0.borrow().is_empty());
    }

    #[test]
    fn plain_text_is_not_selectable() {
        let layout = FontdueTextShaper::ubuntu_light().layout(
            "hello world",
            &TextStyle::new(16.0, Color::BLACK),
            None,
        );
        let mut runtime = UiRuntime::new(Text::new(layout));
        let clipboard = SharedClipboard::default();
        runtime.set_clipboard(clipboard.clone());
        runtime
            .layout(Constraints::loose(
                Size::new(400.0, 100.0).expect("valid constraints"),
            ))
            .expect("layout succeeds");
        assert!(runtime.focus_widget(runtime.root()).is_ok());
        assert!(runtime.focused_widget().is_none());
        let _ = runtime.dispatch_event(&command("a"));
        let _ = runtime.dispatch_event(&command("c"));
        assert!(clipboard.0.borrow().is_empty());
    }
}
