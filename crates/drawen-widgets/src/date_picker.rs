use std::fmt;

use drawen_core::{
    Color, Constraints, InputEvent, Key, NamedKey, Point, PointerButton, PointerId, Rect, Size,
};
use drawen_render::{PaintContext, TextLayout, TextStyle};
use drawen_ui::{
    Accessibility, AccessibilityAction, AccessibilityActions, AccessibilityRole,
    AccessibilityState, DssStyle, EventContext, EventPhase, EventStatus, LayoutContext,
    LayoutResult, Property, Signal, StyleTarget, UiEnvironment, Widget,
};

const DEFAULT_WIDTH: f32 = 280.0;
const DEFAULT_HEIGHT: f32 = 248.0;
const HEADER_HEIGHT: f32 = 28.0;
const WEEKDAY_HEIGHT: f32 = 20.0;
const GRID_ROWS: usize = 6;
const GRID_COLUMNS: usize = 7;
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const WEEKDAYS: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

/// A civil calendar date in the proleptic Gregorian calendar.
///
/// The type carries no clock and no time zone: widgets never read platform
/// time, so applications supply the initial date. Day arithmetic uses the
/// standard civil-from-days conversion, so month and year boundaries and leap
/// years behave exactly like a calendar.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Date {
    year: i32,
    month: u8,
    day: u8,
}

impl Date {
    /// Creates a date, or `None` when the month or day is out of range.
    #[must_use]
    pub const fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        if month < 1 || month > 12 || day < 1 || day > Self::days_in_month(year, month) {
            return None;
        }
        Some(Self { year, month, day })
    }

    /// Returns the year.
    #[must_use]
    pub const fn year(self) -> i32 {
        self.year
    }

    /// Returns the month, 1 through 12.
    #[must_use]
    pub const fn month(self) -> u8 {
        self.month
    }

    /// Returns the day of the month, 1 through 31.
    #[must_use]
    pub const fn day(self) -> u8 {
        self.day
    }

    /// Returns the number of days in a month, accounting for leap years.
    #[must_use]
    pub const fn days_in_month(year: i32, month: u8) -> u8 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                    29
                } else {
                    28
                }
            }
            _ => 0,
        }
    }

    /// Returns the weekday, 0 for Monday through 6 for Sunday.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub const fn weekday(self) -> u8 {
        // Sakamoto's algorithm, shifted so Monday is zero.
        let table = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let year = self.year - (self.month < 3) as i32;
        let sunday_zero = (year + year.div_euclid(4) - year.div_euclid(100)
            + year.div_euclid(400)
            + table[(self.month - 1) as usize]
            + self.day as i32)
            .rem_euclid(7);
        ((sunday_zero + 6) % 7) as u8
    }

    /// Returns the date `days` away, or `None` when it leaves the supported
    /// year range.
    #[must_use]
    pub fn add_days(self, days: i32) -> Option<Self> {
        Self::from_days(self.to_days() + days)
    }

    /// Returns the day count since 1970-01-01 (Howard Hinnant's conversion).
    #[must_use]
    pub const fn to_days(self) -> i32 {
        let year = self.year - (self.month <= 2) as i32;
        let era = if year >= 0 { year } else { year - 399 } / 400;
        let year_of_era = year - era * 400;
        let month_shift = if self.month > 2 { -3 } else { 9 };
        let day_of_year = (153 * (self.month as i32 + month_shift) + 2) / 5 + self.day as i32 - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    /// Builds a date from a day count since 1970-01-01.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub const fn from_days(days: i32) -> Option<Self> {
        let shifted = days + 719_468;
        let era = if shifted >= 0 {
            shifted
        } else {
            shifted - 146_096
        } / 146_097;
        let day_of_era = shifted - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let year = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let mp = (5 * day_of_year + 2) / 153;
        let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u8;
        let month = (mp + if mp < 10 { 3 } else { -9 }) as u8;
        let year = year + (month <= 2) as i32;
        Self::new(year, month, day)
    }
}

impl fmt::Display for Date {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:04}-{:02}-{:02}",
            self.year, self.month, self.day
        )
    }
}

/// The pressable regions of the calendar header and grid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Cell {
    PreviousMonth,
    NextMonth,
    Day(u8),
}

/// A month calendar for picking a single date.
///
/// The grid shows one month at a time with Monday-first weeks; the header
/// arrows and `PageUp`/`PageDown` move between months. The selection is a
/// [`Property<Option<Date>>`]: bind application state with
/// `runtime.bind(&picker.selected(), &state.deadline)`. [`Self::changed`] fires
/// only when the user picks a day; the displayed month is a separate
/// [`Property<(i32, u8)>`] available through [`Self::month`].
///
/// Clicking a day selects it; arrow keys move the cursor by a day or a week
/// (crossing month boundaries moves the displayed month), `Home`/`End` jump to
/// the month ends, and Enter or Space selects the cursor day. The widget never
/// reads the platform clock: supply the initial date with [`Self::with_value`].
pub struct DatePicker {
    month: Property<(i32, u8)>,
    selected: Property<Option<Date>>,
    changed: Signal<Option<Date>>,
    cursor: Date,
    focused: bool,
    pressed: Option<(PointerId, Option<Cell>)>,
    size: Size,
    header: Option<TextLayout>,
    weekdays: Vec<TextLayout>,
    days: Vec<TextLayout>,
}

impl DatePicker {
    /// Creates a picker showing January 2000 with no selection.
    #[must_use]
    pub fn new() -> Self {
        Self::with_month(2000, 1)
    }

    /// Creates a picker showing `year` and `month` with no selection.
    ///
    /// # Panics
    ///
    /// Panics when `month` is outside 1 through 12.
    #[must_use]
    pub fn with_month(year: i32, month: u8) -> Self {
        assert!((1..=12).contains(&month), "month must be 1 through 12");
        Self {
            month: Property::normalized((year, month), |(year, month)| (year, month.clamp(1, 12))),
            selected: Property::new(None),
            changed: Signal::new(),
            cursor: Date::new(year, month, 1).expect("first day of a valid month exists"),
            focused: false,
            pressed: None,
            size: Size::ZERO,
            header: None,
            weekdays: Vec::new(),
            days: Vec::new(),
        }
    }

    /// Returns this picker with `value` selected and its month displayed.
    #[must_use]
    pub fn with_value(mut self, value: Date) -> Self {
        self.selected.set(Some(value));
        self.month.set((value.year(), value.month()));
        self.cursor = value;
        self
    }

    /// Returns the selection property shared with this picker.
    ///
    /// The property's `changed` signal fires on every write; [`Self::changed`]
    /// fires only on user interaction. Read the date with
    /// `picker.selected().get()`.
    pub fn selected(&self) -> Property<Option<Date>> {
        self.selected.clone()
    }

    /// Returns a signal emitted when the user picks a day.
    #[must_use]
    pub fn changed(&self) -> Signal<Option<Date>> {
        self.changed.clone()
    }

    /// Returns the displayed month property, `(year, month)`.
    ///
    /// Header arrows, `PageUp`/`PageDown`, and cursor moves that cross a month
    /// boundary write this property; binding it lets application state drive
    /// the visible month.
    pub fn month(&self) -> Property<(i32, u8)> {
        self.month.clone()
    }

    /// Selects a date without emitting the user-only [`Self::changed`], and
    /// displays its month. `None` clears the selection.
    pub fn set_selected(&mut self, value: Option<Date>) {
        self.selected.set(value);
        if let Some(value) = value {
            self.month.set((value.year(), value.month()));
            self.cursor = value;
        }
    }

    /// Displays a month without changing the selection.
    ///
    /// # Panics
    ///
    /// Panics when `month` is outside 1 through 12.
    pub fn show_month(&mut self, year: i32, month: u8) {
        assert!((1..=12).contains(&month), "month must be 1 through 12");
        self.month.set((year, month));
        self.sync_cursor();
    }

    /// Returns the first day of the displayed month.
    ///
    /// # Panics
    ///
    /// Panics when the displayed month is invalid; the month property's
    /// normalizer clamps every write to 1 through 12, so public writes cannot
    /// reach that state.
    #[must_use]
    pub fn month_start(&self) -> Date {
        let (year, month) = self.month.get();
        Date::new(year, month, 1).expect("displayed month is valid")
    }

    fn sync_cursor(&mut self) {
        let (year, month) = self.month.get();
        let day = self.cursor.day().min(Date::days_in_month(year, month));
        self.cursor = Date::new(year, month, day).expect("clamped day exists");
    }

    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn shift_month(&mut self, direction: i32) {
        let (year, month) = self.month.get();
        let index = i32::from(month) - 1 + direction;
        let year = year + index.div_euclid(12);
        let month = (index.rem_euclid(12) + 1) as u8;
        self.month.set((year, month));
        self.sync_cursor();
    }

    fn move_cursor(&mut self, days: i32) {
        if let Some(next) = self.cursor.add_days(days) {
            self.cursor = next;
            self.month.set((next.year(), next.month()));
        }
    }

    fn select_cursor(&mut self, context: &mut EventContext<'_>) {
        self.select(self.cursor, context);
    }

    fn select(&mut self, date: Date, context: &mut EventContext<'_>) {
        self.cursor = date;
        if self.selected.get() != Some(date) {
            self.selected.set(Some(date));
            self.changed.emit(&Some(date));
        }
        context.request_redraw();
    }

    #[allow(clippy::cast_precision_loss)]
    fn cell_width(&self) -> f32 {
        (self.size.width() / GRID_COLUMNS as f32).max(1.0)
    }

    #[allow(clippy::cast_precision_loss)]
    fn cell_height(&self) -> f32 {
        ((self.size.height() - HEADER_HEIGHT - WEEKDAY_HEIGHT) / GRID_ROWS as f32).max(1.0)
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        clippy::cast_possible_wrap
    )]
    fn cell_at(&self, position: Point) -> Option<Cell> {
        if position.y < HEADER_HEIGHT {
            let next_left = self.size.width() - 28.0;
            if position.x >= next_left {
                return Some(Cell::NextMonth);
            }
            if position.x >= next_left - 20.0 {
                return Some(Cell::PreviousMonth);
            }
            return None;
        }
        if position.y < HEADER_HEIGHT + WEEKDAY_HEIGHT {
            return None;
        }
        let column = (position.x / self.cell_width()).floor();
        let row = ((position.y - HEADER_HEIGHT - WEEKDAY_HEIGHT) / self.cell_height()).floor();
        if column < 0.0 || column >= GRID_COLUMNS as f32 || row < 0.0 || row >= GRID_ROWS as f32 {
            return None;
        }
        let index = row as usize * GRID_COLUMNS + column as usize;
        let offset = usize::from(self.month_start().weekday());
        let day = index as i32 - offset as i32 + 1;
        let (year, month) = self.month.get();
        let days = i32::from(Date::days_in_month(year, month));
        if day < 1 || day > days {
            return None;
        }
        Some(Cell::Day(day as u8))
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        clippy::cast_possible_wrap
    )]
    fn relayout(&mut self, environment: &UiEnvironment, style: &DssStyle) {
        let font = environment.font_for(style);
        let foreground = style.foreground_or(environment.theme().foreground());
        let text_style = TextStyle::new_with_font(font.clone(), foreground);
        let muted_style = TextStyle::new_with_font(font, foreground.with_alpha(0.55));
        let (year, month) = self.month.get();
        let title = format!("{} {year}", MONTHS[usize::from(month) - 1]);
        self.header = Some(environment.text_shaper().layout(&title, &text_style, None));
        self.weekdays = WEEKDAYS
            .iter()
            .map(|label| environment.text_shaper().layout(label, &muted_style, None))
            .collect();
        let offset = usize::from(self.month_start().weekday());
        let days = usize::from(Date::days_in_month(year, month));
        let previous = usize::from(Date::days_in_month(
            if month == 1 { year - 1 } else { year },
            if month == 1 { 12 } else { month - 1 },
        ));
        self.days = (0..GRID_ROWS * GRID_COLUMNS)
            .map(|index| {
                let number = index as i32 - offset as i32 + 1;
                let (label, style) = if number < 1 {
                    ((previous as i32 + number).to_string(), &muted_style)
                } else if number > days as i32 {
                    ((number - days as i32).to_string(), &muted_style)
                } else {
                    (number.to_string(), &text_style)
                };
                environment.text_shaper().layout(&label, style, None)
            })
            .collect();
    }
}

impl Default for DatePicker {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for DatePicker {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let style = context.style();
        self.relayout(context.environment(), &style);
        let max = constraints.max();
        let width = if max.width().is_finite() {
            max.width()
        } else {
            DEFAULT_WIDTH
        };
        let height = if max.height().is_finite() {
            max.height()
        } else {
            DEFAULT_HEIGHT
        };
        self.size = constraints.constrain(size(width, height));
        LayoutResult::new(self.size)
    }

    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("date-picker")
    }

    fn paint(&self, context: &mut PaintContext<'_>, environment: &UiEnvironment, bounds: Rect) {
        self.paint_styled(context, environment, bounds, &DssStyle::default());
    }

    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        clippy::cast_possible_wrap
    )]
    fn paint_styled(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        bounds: Rect,
        style: &DssStyle,
    ) {
        let theme = environment.theme();
        let accent = style.accent_or(theme.accent());
        let foreground = style.foreground_or(theme.foreground());
        context.fill_rounded_rect(
            bounds,
            style.corner_radius_or(theme.corner_radius()),
            style.background_or(theme.surface()),
        );
        context.stroke_rounded_rect(
            bounds,
            style.corner_radius_or(theme.corner_radius()),
            1.0,
            style.border.map_or(theme.border(), |border| border.color),
        );
        if let Some(header) = &self.header {
            context.draw_text(
                header.clone(),
                Point::new(
                    bounds.origin.x + 12.0,
                    bounds.origin.y + (HEADER_HEIGHT - header.size().height()) / 2.0,
                ),
            );
        }
        // Header arrows: left half pages back, right half pages forward.
        let mid_y = bounds.origin.y + HEADER_HEIGHT / 2.0;
        paint_horizontal_arrow(
            context,
            Point::new(bounds.origin.x + self.size.width() - 44.0, mid_y - 4.0),
            false,
            foreground,
        );
        paint_horizontal_arrow(
            context,
            Point::new(bounds.origin.x + self.size.width() - 24.0, mid_y - 4.0),
            true,
            foreground,
        );
        let cell_width = self.cell_width();
        let cell_height = self.cell_height();
        for (column, layout) in self.weekdays.iter().enumerate() {
            context.draw_text(
                layout.clone(),
                Point::new(
                    bounds.origin.x
                        + column as f32 * cell_width
                        + (cell_width - layout.size().width()) / 2.0,
                    bounds.origin.y
                        + HEADER_HEIGHT
                        + (WEEKDAY_HEIGHT - layout.size().height()) / 2.0,
                ),
            );
        }
        let grid_top = bounds.origin.y + HEADER_HEIGHT + WEEKDAY_HEIGHT;
        let offset = usize::from(self.month_start().weekday());
        let days = usize::from(Date::days_in_month(self.month.get().0, self.month.get().1));
        let selected = self.selected.get();
        for (index, layout) in self.days.iter().enumerate() {
            let row = index / GRID_COLUMNS;
            let column = index % GRID_COLUMNS;
            let cell = Rect::new(
                Point::new(
                    bounds.origin.x + column as f32 * cell_width,
                    grid_top + row as f32 * cell_height,
                ),
                size(cell_width, cell_height),
            );
            let number = index as i32 - offset as i32 + 1;
            let in_month = number >= 1 && number <= days as i32;
            if in_month {
                let date = Date::new(self.month.get().0, self.month.get().1, number as u8)
                    .expect("in-month day exists");
                if selected == Some(date) {
                    context.fill_rounded_rect(cell, 4.0, theme.selection());
                } else if self.focused && self.cursor == date {
                    context.stroke_rounded_rect(cell, 4.0, 1.0, accent.with_alpha(0.6));
                }
            }
            context.draw_text(
                layout.clone(),
                Point::new(
                    cell.origin.x + (cell_width - layout.size().width()) / 2.0,
                    cell.origin.y + (cell_height - layout.size().height()) / 2.0,
                ),
            );
        }
    }

    fn handle_event(&mut self, context: &mut EventContext<'_>, event: &InputEvent) -> EventStatus {
        if context.phase() != EventPhase::Target {
            return EventStatus::Ignored;
        }
        match event {
            InputEvent::PointerDown(pointer) if pointer.button == Some(PointerButton::Primary) => {
                context.request_focus();
                context.capture_pointer(pointer.pointer_id);
                self.pressed = Some((pointer.pointer_id, self.cell_at(pointer.position)));
                context.request_redraw();
                EventStatus::Handled
            }
            InputEvent::PointerUp(pointer) => {
                let Some((id, cell)) = self.pressed.take() else {
                    return EventStatus::Ignored;
                };
                context.release_pointer(pointer.pointer_id);
                let Some(cell) = cell else {
                    context.request_redraw();
                    return EventStatus::Handled;
                };
                if id != pointer.pointer_id || self.cell_at(pointer.position) != Some(cell) {
                    context.request_redraw();
                    return EventStatus::Handled;
                }
                match cell {
                    Cell::PreviousMonth => self.shift_month(-1),
                    Cell::NextMonth => self.shift_month(1),
                    Cell::Day(day) => {
                        let (year, month) = self.month.get();
                        if let Some(date) = Date::new(year, month, day) {
                            self.select(date, context);
                        }
                    }
                }
                context.request_layout();
                EventStatus::Handled
            }
            InputEvent::PointerCancel(pointer) => {
                self.pressed = None;
                context.release_pointer(pointer.pointer_id);
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
        Accessibility::new(AccessibilityRole::Table)
            .with_value(
                self.selected
                    .get()
                    .map(|date| date.to_string())
                    .unwrap_or_default(),
            )
            .with_states(AccessibilityState::FOCUSABLE)
            .with_actions(AccessibilityActions::FOCUS | AccessibilityActions::SET_VALUE)
    }

    fn accessibility_action(&mut self, action: &AccessibilityAction) -> bool {
        match action {
            AccessibilityAction::SetValue(value) => {
                let mut parts = value.split('-').map(str::parse::<i32>);
                let (Some(Ok(year)), Some(Ok(month)), Some(Ok(day))) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    return false;
                };
                let Ok(month) = u8::try_from(month) else {
                    return false;
                };
                let Ok(day) = u8::try_from(day) else {
                    return false;
                };
                if let Some(date) = Date::new(year, month, day) {
                    self.set_selected(Some(date));
                    return true;
                }
                false
            }
            _ => false,
        }
    }
}

impl DatePicker {
    fn handle_key(
        &mut self,
        context: &mut EventContext<'_>,
        key: &drawen_core::KeyEvent,
    ) -> EventStatus {
        match key.key {
            Key::Named(NamedKey::ArrowLeft) => self.move_cursor(-1),
            Key::Named(NamedKey::ArrowRight) => self.move_cursor(1),
            Key::Named(NamedKey::ArrowUp) => self.move_cursor(-7),
            Key::Named(NamedKey::ArrowDown) => self.move_cursor(7),
            Key::Named(NamedKey::PageUp) => self.shift_month(-1),
            Key::Named(NamedKey::PageDown) => self.shift_month(1),
            Key::Named(NamedKey::Home) => {
                self.cursor = self.month_start();
            }
            Key::Named(NamedKey::End) => {
                let (year, month) = self.month.get();
                self.cursor = Date::new(year, month, Date::days_in_month(year, month))
                    .expect("last day of a valid month exists");
            }
            Key::Named(NamedKey::Enter | NamedKey::Space) => {
                self.select_cursor(context);
                return EventStatus::Handled;
            }
            _ => return EventStatus::Ignored,
        }
        context.request_layout();
        EventStatus::Handled
    }
}

/// Paints an 8x8 triangle pointing left or right for the month pager.
fn paint_horizontal_arrow(
    context: &mut PaintContext<'_>,
    origin: Point,
    right: bool,
    color: Color,
) {
    for step in 0u8..4 {
        let height = 8.0 - 2.0 * f32::from(step);
        let x = if right {
            f32::from(step)
        } else {
            3.0 - f32::from(step)
        };
        context.fill_rect(
            Rect::new(
                Point::new(origin.x + x, origin.y + (8.0 - height) / 2.0),
                size(1.0, height),
            ),
            color,
        );
    }
}

fn size(width: f32, height: f32) -> Size {
    Size::new(width, height).expect("date picker sizes are non-negative and not NaN")
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // tests assert exact expected geometry by design
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use drawen_core::{KeyCode, Modifiers, PointerButtons, PointerEvent};
    use drawen_ui::UiRuntime;

    use super::*;

    fn viewport() -> Constraints {
        Constraints::tight(Size::new(280.0, 248.0).expect("valid size")).expect("valid constraints")
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
    fn civil_date_arithmetic_handles_leap_years_and_weekdays() {
        assert!(Date::new(2024, 2, 29).is_some());
        assert!(Date::new(2023, 2, 29).is_none());
        assert!(Date::new(2026, 13, 1).is_none());
        assert_eq!(Date::days_in_month(2000, 2), 29);
        assert_eq!(Date::days_in_month(1900, 2), 28);
        // 2026-11-27 is a Friday; Monday is zero.
        assert_eq!(Date::new(2026, 11, 27).expect("valid date").weekday(), 4);
        let epoch = Date::new(1970, 1, 1).expect("valid date");
        assert_eq!(epoch.to_days(), 0);
        assert_eq!(Date::from_days(0), Some(epoch));
        assert_eq!(
            Date::new(2026, 1, 31).expect("valid date").add_days(1),
            Date::new(2026, 2, 1)
        );
        assert_eq!(
            Date::new(2024, 2, 28).expect("valid date").add_days(1),
            Date::new(2024, 2, 29)
        );
    }

    #[test]
    fn clicking_a_day_selects_it_and_emits_changed() {
        let picker = DatePicker::with_month(2026, 11);
        let selected = picker.selected();
        let changes = Rc::new(RefCell::new(Vec::new()));
        let _subscription = picker.changed().subscribe({
            let changes = Rc::clone(&changes);
            move |date| changes.borrow_mut().push(*date)
        });
        let mut runtime = UiRuntime::new(picker);
        runtime.layout(viewport()).expect("layout succeeds");

        // 2026-11-01 is a Sunday (column 6); day 27 sits in row 4, column 4.
        let cell_width = 280.0 / 7.0;
        let cell_height = (248.0 - 28.0 - 20.0) / 6.0;
        let x = 4.0 * cell_width + cell_width / 2.0;
        let y = 28.0 + 20.0 + 4.0 * cell_height + cell_height / 2.0;
        let _ = runtime.dispatch_event(&pointer(Point::new(x, y), false));
        let _ = runtime.dispatch_event(&pointer(Point::new(x, y), true));
        assert_eq!(selected.get(), Date::new(2026, 11, 27));
        assert_eq!(*changes.borrow(), vec![Date::new(2026, 11, 27)]);
    }

    #[test]
    fn header_arrows_page_months_and_clamp_the_cursor() {
        let picker =
            DatePicker::with_month(2026, 1).with_value(Date::new(2026, 1, 31).expect("valid date"));
        let month = picker.month();
        let mut runtime = UiRuntime::new(picker);
        runtime.layout(viewport()).expect("layout succeeds");

        let next = Point::new(280.0 - 14.0, 14.0);
        let _ = runtime.dispatch_event(&pointer(next, false));
        let _ = runtime.dispatch_event(&pointer(next, true));
        assert_eq!(month.get(), (2026, 2));
        runtime.layout(viewport()).expect("relayout succeeds");
        let previous = Point::new(280.0 - 34.0, 14.0);
        let _ = runtime.dispatch_event(&pointer(previous, false));
        let _ = runtime.dispatch_event(&pointer(previous, true));
        assert_eq!(month.get(), (2026, 1));
    }

    #[test]
    fn arrow_keys_cross_month_boundaries_and_enter_selects() {
        let picker =
            DatePicker::with_month(2026, 1).with_value(Date::new(2026, 1, 31).expect("valid date"));
        let selected = picker.selected();
        let month = picker.month();
        let mut runtime = UiRuntime::new(picker);
        runtime.layout(viewport()).expect("layout succeeds");
        runtime
            .focus_widget(runtime.root())
            .expect("picker focuses");

        let _ = runtime.dispatch_event(&key(NamedKey::ArrowRight));
        assert_eq!(month.get(), (2026, 2));
        let _ = runtime.dispatch_event(&key(NamedKey::Enter));
        assert_eq!(selected.get(), Date::new(2026, 2, 1));
        let _ = runtime.dispatch_event(&key(NamedKey::ArrowLeft));
        let _ = runtime.dispatch_event(&key(NamedKey::Enter));
        assert_eq!(selected.get(), Date::new(2026, 1, 31));
    }

    #[test]
    fn set_selected_and_binding_write_without_user_changed() {
        let picker = DatePicker::new();
        let selected = picker.selected();
        let user_changes = Rc::new(RefCell::new(0));
        let _subscription = picker.changed().subscribe({
            let user_changes = Rc::clone(&user_changes);
            move |_| *user_changes.borrow_mut() += 1
        });
        let state = Property::new(Date::new(2026, 11, 27));
        let mut runtime = UiRuntime::new(picker);
        let _binding = runtime.bind(&selected, &state);
        runtime.layout(viewport()).expect("layout succeeds");
        assert_eq!(selected.get(), Date::new(2026, 11, 27));

        state.set(Date::new(2027, 1, 1));
        assert_eq!(selected.get(), Date::new(2027, 1, 1));
        assert_eq!(*user_changes.borrow(), 0);
    }
}
