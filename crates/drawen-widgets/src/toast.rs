use std::{cell::RefCell, rc::Rc, time::Duration};

use drawen_core::{Color, Constraints, Insets, Point, Rect, Size};
use drawen_render::{PaintContext, TextLayout, TextStyle};
use drawen_ui::{
    Accessibility, AccessibilityRole, DssStyle, LayoutContext, LayoutResult, Signal, StyleTarget,
    UiEnvironment, Widget,
};

const DEFAULT_DURATION: Duration = Duration::from_secs(4);
const MAX_VISIBLE_TOASTS: usize = 5;
const MIN_WIDTH: f32 = 240.0;
const MAX_WIDTH: f32 = 360.0;
const STRIPE_WIDTH: f32 = 3.0;
const DEFAULT_MARGIN: f32 = 16.0;
const GAP: f32 = 8.0;
const CARD_PADDING: Insets = Insets::new(10.0, 14.0, 10.0, 16.0);

/// Semantic color family used by a [`Toast`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ToastTone {
    /// Informational message using the theme accent.
    #[default]
    Info,
    /// Positive confirmation.
    Success,
    /// Caution or pending state.
    Warning,
    /// Error or destructive state.
    Danger,
}

impl ToastTone {
    fn color(self, theme: &dyn drawen_ui::Theme) -> Color {
        match self {
            Self::Info => theme.accent(),
            Self::Success => theme.success(),
            Self::Warning => theme.warning(),
            Self::Danger => theme.danger(),
        }
    }
}

/// The window corner where a [`ToastHost`] stacks its notifications.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ToastCorner {
    /// Top-left corner; the newest toast sits at the top.
    TopLeft,
    /// Top-right corner; the newest toast sits at the top.
    TopRight,
    /// Bottom-left corner; the newest toast sits at the bottom.
    BottomLeft,
    /// Bottom-right corner; the newest toast sits at the bottom.
    #[default]
    BottomRight,
}

/// One transient notification shown by a [`ToastHost`].
///
/// A toast is plain data: the host owns the queue, paints the cards above the
/// tree, and removes them when their duration elapses.
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    message: String,
    tone: ToastTone,
    duration: Duration,
}

impl Toast {
    /// Creates an informational toast that stays for four seconds.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            tone: ToastTone::Info,
            duration: DEFAULT_DURATION,
        }
    }

    /// Returns this toast with an explicit tone.
    #[must_use]
    pub const fn with_tone(mut self, tone: ToastTone) -> Self {
        self.tone = tone;
        self
    }

    /// Returns this toast with an explicit visible duration.
    #[must_use]
    pub const fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Returns the message text.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the tone.
    #[must_use]
    pub const fn tone(&self) -> ToastTone {
        self.tone
    }

    /// Returns the visible duration.
    #[must_use]
    pub const fn duration(&self) -> Duration {
        self.duration
    }
}

struct ToastEntry {
    id: u64,
    toast: Toast,
    elapsed: Duration,
    remaining: Duration,
}

#[derive(Default)]
struct ToastQueue {
    entries: Vec<ToastEntry>,
    next_id: u64,
}

/// Shared queue handle for a [`ToastHost`].
///
/// The handle lets application code show and dismiss notifications after the
/// host has been appended to the runtime. Showing a toast does not request a
/// paint pass; set the application's dirty flag and invalidate paint once per
/// event, exactly like any other signal handler.
#[derive(Clone)]
pub struct ToastHandle {
    queue: Rc<RefCell<ToastQueue>>,
    shown: Signal<u64>,
}

impl ToastHandle {
    /// Queues `toast` and returns its identifier.
    ///
    /// The queue keeps at most five toasts; showing a sixth drops the oldest.
    /// Emits [`Self::shown`] with the new identifier.
    #[must_use]
    pub fn show(&self, toast: Toast) -> u64 {
        let id = {
            let mut queue = self.queue.borrow_mut();
            let id = queue.next_id;
            queue.next_id += 1;
            let remaining = toast.duration();
            queue.entries.push(ToastEntry {
                id,
                toast,
                elapsed: Duration::ZERO,
                remaining,
            });
            if queue.entries.len() > MAX_VISIBLE_TOASTS {
                queue.entries.remove(0);
            }
            id
        };
        self.shown.emit(&id);
        id
    }

    /// Removes the toast with `id` before its duration elapses.
    ///
    /// Returns whether a toast was removed.
    #[must_use]
    pub fn dismiss(&self, id: u64) -> bool {
        let mut queue = self.queue.borrow_mut();
        let before = queue.entries.len();
        queue.entries.retain(|entry| entry.id != id);
        queue.entries.len() != before
    }

    /// Removes every queued toast.
    pub fn clear(&self) {
        self.queue.borrow_mut().entries.clear();
    }

    /// Returns the number of queued toasts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.queue.borrow().entries.len()
    }

    /// Returns whether the queue is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queue.borrow().entries.is_empty()
    }

    /// Returns a signal emitted with the identifier of each shown toast.
    #[must_use]
    pub fn shown(&self) -> Signal<u64> {
        self.shown.clone()
    }
}

/// A zero-size host that stacks transient notifications in a window corner.
///
/// The host paints its cards through [`Widget::paint_floating`], so they appear
/// above the tree and any open overlays without participating in layout, hit
/// testing, or focus. Like tooltip bubbles, toasts are non-interactive: they
/// fade in, stay for their duration, fade out, and remove themselves.
///
/// ```rust,ignore
/// let host = ToastHost::new();
/// let handle = host.handle();
/// let host = runtime.append_child(root, host)?;
/// // From a signal handler or window_event:
/// handle.show(Toast::new("Project saved").with_tone(ToastTone::Success));
/// // ... then invalidate paint and request one redraw.
/// ```
///
/// Timed dismissal runs through [`Widget::advance_animation`]; while any toast
/// is queued the host reports [`Widget::is_animating`], so
/// [`drawen_ui::UiRuntime::next_frame_in`] keeps scheduling frames. The queue
/// keeps at most five toasts and drops the oldest when a sixth arrives.
pub struct ToastHost {
    queue: Rc<RefCell<ToastQueue>>,
    corner: ToastCorner,
    margin: f32,
    viewport: Option<Rect>,
    padding: Insets,
    radius: f32,
    fade_in: Duration,
    fade_out: Duration,
    reduced_motion: bool,
    shown: Signal<u64>,
}

impl ToastHost {
    /// Creates an empty host stacking toasts in the bottom-right corner.
    #[must_use]
    pub fn new() -> Self {
        Self {
            queue: Rc::new(RefCell::new(ToastQueue::default())),
            corner: ToastCorner::BottomRight,
            margin: DEFAULT_MARGIN,
            viewport: None,
            padding: CARD_PADDING,
            radius: 0.0,
            fade_in: Duration::from_millis(140),
            fade_out: Duration::from_millis(280),
            reduced_motion: false,
            shown: Signal::new(),
        }
    }

    /// Returns this host stacking toasts in `corner`.
    #[must_use]
    pub const fn with_corner(mut self, corner: ToastCorner) -> Self {
        self.corner = corner;
        self
    }

    /// Returns this host with a minimum distance from the window edges.
    #[must_use]
    pub const fn with_margin(mut self, margin: f32) -> Self {
        self.margin = margin.max(0.0);
        self
    }

    /// Returns a cloneable handle that shows and dismisses toasts.
    #[must_use]
    pub fn handle(&self) -> ToastHandle {
        ToastHandle {
            queue: Rc::clone(&self.queue),
            shown: self.shown.clone(),
        }
    }

    fn alpha(&self, entry: &ToastEntry) -> f32 {
        if self.reduced_motion || self.fade_in.is_zero() {
            return 1.0;
        }
        let fade_in = entry.elapsed.as_secs_f32() / self.fade_in.as_secs_f32();
        let fade_out =
            entry.remaining.as_secs_f32() / self.fade_out.as_secs_f32().max(f32::EPSILON);
        fade_in.min(fade_out).clamp(0.0, 1.0)
    }
}

impl Default for ToastHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ToastHost {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        _constraints: Constraints,
    ) -> LayoutResult {
        let style = context.style();
        let environment = context.environment();
        let theme = environment.theme();
        self.viewport = context.viewport();
        self.padding = style.padding.unwrap_or(CARD_PADDING);
        self.radius = style.corner_radius.unwrap_or_else(|| theme.corner_radius());
        self.fade_in = theme.animation_duration();
        self.fade_out = self.fade_in.saturating_mul(2);
        self.reduced_motion = !environment.animations_enabled() || self.fade_in.is_zero();
        LayoutResult::new(Size::ZERO)
    }

    fn paint_floating(
        &self,
        context: &mut PaintContext<'_>,
        environment: &UiEnvironment,
        _bounds: Rect,
    ) {
        let Some(viewport) = self.viewport else {
            return;
        };
        let queue = self.queue.borrow();
        if queue.entries.is_empty() {
            return;
        }
        let theme = environment.theme();
        let font = environment.font_for(&DssStyle::default());
        let max_text_width = (MAX_WIDTH - self.padding.horizontal()).max(1.0);

        let mut cards: Vec<(Point, Size, Color, f32, TextLayout)> = Vec::new();
        let mut total_height = 0.0;
        for entry in &queue.entries {
            let tone = entry.toast.tone().color(theme);
            let layout = environment.text_shaper().layout(
                entry.toast.message(),
                &TextStyle::new_with_font(font.clone(), theme.foreground()),
                Some(max_text_width),
            );
            let width =
                (layout.size().width() + self.padding.horizontal()).clamp(MIN_WIDTH, MAX_WIDTH);
            let height = layout.size().height() + self.padding.vertical();
            let x = match self.corner {
                ToastCorner::TopLeft | ToastCorner::BottomLeft => viewport.origin.x + self.margin,
                ToastCorner::TopRight | ToastCorner::BottomRight => {
                    viewport.right() - self.margin - width
                }
            };
            cards.push((
                Point::new(x, 0.0),
                size(width, height),
                tone,
                self.alpha(entry),
                layout,
            ));
            total_height += height;
        }
        #[allow(clippy::cast_precision_loss)]
        let gap_total = GAP * (cards.len() as f32 - 1.0);
        total_height += gap_total;

        let top = match self.corner {
            ToastCorner::TopLeft | ToastCorner::TopRight => viewport.origin.y + self.margin,
            ToastCorner::BottomLeft | ToastCorner::BottomRight => {
                viewport.bottom() - self.margin - total_height
            }
        };
        // Bottom corners keep the newest toast at the bottom edge; top corners
        // keep the newest at the top edge.
        let ordered: Vec<usize> = match self.corner {
            ToastCorner::TopLeft | ToastCorner::TopRight => (0..cards.len()).rev().collect(),
            ToastCorner::BottomLeft | ToastCorner::BottomRight => (0..cards.len()).collect(),
        };
        let mut y = top;
        for index in ordered {
            let (origin, card_size, tone, alpha, layout) = cards[index].clone();
            let card = Rect::new(Point::new(origin.x, y), card_size);
            let shadow = Rect::new(Point::new(card.origin.x, card.origin.y + 2.0), card.size);
            context.fill_rounded_rect(shadow, self.radius, theme.shadow().with_alpha(0.35 * alpha));
            context.fill_rounded_rect(
                card,
                self.radius,
                blend(theme.raised_surface(), tone, 0.10).with_alpha(alpha),
            );
            context.stroke_rounded_rect(card, self.radius, 1.0, theme.border().with_alpha(alpha));
            let stripe_height = (card.size.height() - 12.0).max(0.0);
            if stripe_height > 0.0 {
                context.fill_rounded_rect(
                    Rect::new(
                        Point::new(card.origin.x + 5.0, card.origin.y + 6.0),
                        size(STRIPE_WIDTH, stripe_height),
                    ),
                    STRIPE_WIDTH / 2.0,
                    tone.with_alpha(alpha),
                );
            }
            let text_height = layout.size().height();
            context.draw_text(
                layout
                    .clone()
                    .with_color(theme.foreground().with_alpha(alpha)),
                Point::new(
                    card.origin.x + self.padding.left,
                    card.origin.y + (card.size.height() - text_height) / 2.0,
                ),
            );
            y += card.size.height() + GAP;
        }
    }

    fn advance_animation(&mut self, delta: Duration, settle: bool) -> bool {
        let fade_in = self.fade_in;
        let mut queue = self.queue.borrow_mut();
        let mut changed = false;
        queue.entries.retain_mut(|entry| {
            entry.elapsed = entry.elapsed.saturating_add(delta);
            if settle {
                entry.elapsed = entry.elapsed.max(fade_in);
            }
            entry.remaining = entry.remaining.saturating_sub(delta);
            if entry.remaining.is_zero() {
                changed = true;
                false
            } else {
                true
            }
        });
        let fading = !self.reduced_motion
            && queue
                .entries
                .iter()
                .any(|entry| entry.elapsed < fade_in || entry.remaining < self.fade_out);
        changed || fading
    }

    fn is_animating(&self) -> bool {
        !self.queue.borrow().entries.is_empty()
    }

    fn participates_in_hit_test(&self) -> bool {
        false
    }

    fn accessibility(&self) -> Accessibility {
        let queue = self.queue.borrow();
        let label = queue
            .entries
            .iter()
            .map(|entry| entry.toast.message())
            .collect::<Vec<_>>()
            .join("; ");
        Accessibility {
            hidden: queue.entries.is_empty(),
            ..Accessibility::new(AccessibilityRole::Label).with_label(label)
        }
    }

    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("toast-host")
    }
}

fn size(width: f32, height: f32) -> Size {
    Size::new(width, height).expect("toast sizes are non-negative and not NaN")
}

fn blend(base: Color, overlay: Color, amount: f32) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    Color::rgba(
        base.red + (overlay.red - base.red) * amount,
        base.green + (overlay.green - base.green) * amount,
        base.blue + (overlay.blue - base.blue) * amount,
        base.alpha + (overlay.alpha - base.alpha) * amount,
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use drawen_core::{Constraints, Point, Size};
    use drawen_render::{DisplayCommand, DisplayList, PaintContext};
    use drawen_ui::UiRuntime;

    use super::{Toast, ToastCorner, ToastHandle, ToastHost, ToastTone};

    fn size(width: f32, height: f32) -> Size {
        Size::new(width, height).expect("valid test size")
    }

    fn laid_out(host: ToastHost) -> (UiRuntime, ToastHandle) {
        let handle = host.handle();
        let mut runtime = UiRuntime::new(host);
        runtime
            .layout(Constraints::tight(size(400.0, 300.0)).expect("valid constraints"))
            .expect("layout succeeds");
        (runtime, handle)
    }

    fn painted_texts(runtime: &mut UiRuntime) -> Vec<(String, Point)> {
        runtime.invalidate_paint();
        let mut display_list = DisplayList::new();
        runtime
            .paint(&mut PaintContext::new(&mut display_list))
            .expect("paint succeeds");
        display_list
            .commands()
            .iter()
            .filter_map(|command| match command {
                DisplayCommand::DrawText { layout, origin } => {
                    Some((layout.text().to_owned(), *origin))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn showing_a_toast_paints_a_card_above_the_tree() {
        let (mut runtime, handle) = laid_out(ToastHost::new());
        let shown = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let _subscription = handle.shown().subscribe({
            let shown = std::rc::Rc::clone(&shown);
            move |id| shown.borrow_mut().push(*id)
        });
        let _ = handle.show(Toast::new("Project saved").with_tone(ToastTone::Success));
        assert_eq!(*shown.borrow(), vec![0]);
        assert_eq!(handle.len(), 1);
        runtime.advance_animations(Duration::from_millis(200));

        let texts = painted_texts(&mut runtime);
        assert!(texts.iter().any(|(text, _)| text == "Project saved"));
    }

    #[test]
    fn a_toast_removes_itself_after_its_duration() {
        let (mut runtime, handle) = laid_out(ToastHost::new());
        let _ = handle.show(Toast::new("Temporary").with_duration(Duration::from_millis(100)));
        runtime.advance_animations(Duration::from_millis(200));
        assert!(handle.is_empty());
        let texts = painted_texts(&mut runtime);
        assert_eq!(texts, [] as [(std::string::String, drawen_core::Point); 0]);
    }

    #[test]
    fn bottom_right_stacks_the_newest_toast_at_the_bottom() {
        let (mut runtime, handle) = laid_out(ToastHost::new());
        let _ = handle.show(Toast::new("First"));
        let _ = handle.show(Toast::new("Second"));
        runtime.advance_animations(Duration::from_millis(200));

        let texts = painted_texts(&mut runtime);
        let first = texts
            .iter()
            .find(|(text, _)| text == "First")
            .expect("first toast painted");
        let second = texts
            .iter()
            .find(|(text, _)| text == "Second")
            .expect("second toast painted");
        assert!(first.1.x > 120.0 && second.1.x > 120.0);
        assert!(first.1.y > 150.0 && second.1.y > 150.0);
        assert!(second.1.y > first.1.y);
    }

    #[test]
    fn top_left_stacks_the_newest_toast_at_the_top() {
        let (mut runtime, handle) = laid_out(ToastHost::new().with_corner(ToastCorner::TopLeft));
        let _ = handle.show(Toast::new("First"));
        let _ = handle.show(Toast::new("Second"));
        runtime.advance_animations(Duration::from_millis(200));

        let texts = painted_texts(&mut runtime);
        let first = texts
            .iter()
            .find(|(text, _)| text == "First")
            .expect("first toast painted");
        let second = texts
            .iter()
            .find(|(text, _)| text == "Second")
            .expect("second toast painted");
        assert!(first.1.x < 100.0 && second.1.x < 100.0);
        assert!(second.1.y < first.1.y);
    }

    #[test]
    fn the_queue_caps_at_five_and_dismiss_removes_early() {
        let (mut runtime, handle) = laid_out(ToastHost::new());
        let mut ids = Vec::new();
        for index in 0..6 {
            ids.push(handle.show(Toast::new(format!("Toast {index}"))));
        }
        assert_eq!(handle.len(), 5);
        assert!(!handle.dismiss(ids[0]));
        assert!(handle.dismiss(ids[5]));
        assert_eq!(handle.len(), 4);
        handle.clear();
        assert!(handle.is_empty());
        runtime.advance_animations(Duration::from_millis(16));
        assert_eq!(
            painted_texts(&mut runtime),
            [] as [(std::string::String, drawen_core::Point); 0]
        );
    }

    #[test]
    fn frames_are_scheduled_only_while_toasts_are_queued() {
        let (mut runtime, handle) = laid_out(ToastHost::new());
        assert!(runtime.next_frame_in().is_none());
        let _ = handle.show(Toast::new("Busy").with_duration(Duration::from_millis(100)));
        assert!(runtime.next_frame_in().is_some());
        runtime.advance_animations(Duration::from_millis(200));
        assert!(runtime.next_frame_in().is_none());
    }
}
