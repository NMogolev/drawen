use drawen_core::{Constraints, Rect, Size};
use drawen_render::PaintContext;
use drawen_ui::{
    Accessibility, AccessibilityRole, DssStyle, LayoutContext, LayoutResult, Property, StyleTarget,
    UiEnvironment, Widget,
};

const DEFAULT_WIDTH: f32 = 120.0;
const HEIGHT: f32 = 12.0;

/// A passive horizontal indicator for progress in the inclusive `0.0..=1.0` range.
///
/// The progress is a clamped [`Property`]: bind application state to it with
/// `runtime.bind(&bar.progress(), &state.fraction)` and every copied write stays
/// inside `0.0..=1.0`. The indicator is passive, so it has no user-interaction
/// signal; the property's `changed` signal fires on every write.
pub struct ProgressBar {
    progress: Property<f32>,
}

impl ProgressBar {
    /// Creates an empty progress indicator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            progress: Property::normalized(0.0, |value: f32| {
                if value.is_finite() {
                    value.clamp(0.0, 1.0)
                } else {
                    0.0
                }
            }),
        }
    }

    /// Creates a progress indicator with `progress` clamped to `0.0..=1.0`.
    ///
    /// # Panics
    ///
    /// Panics when `progress` is non-finite.
    #[must_use]
    pub fn with_progress(progress: f32) -> Self {
        let mut result = Self::new();
        result.set_progress(progress);
        result
    }

    /// Returns the progress property shared with this indicator.
    ///
    /// Every write is clamped to `0.0..=1.0` before it is stored, including
    /// `UiRuntime::bind` copies, so bound application state stays in range; a
    /// non-finite write falls back to `0.0`. Bind state with
    /// `runtime.bind(&bar.progress(), &state.fraction)` and read the scalar with
    /// `bar.progress().get()`. The property's `changed` signal fires on every
    /// write.
    pub fn progress(&self) -> Property<f32> {
        self.progress.clone()
    }

    /// Sets normalized progress, clamping it to `0.0..=1.0`.
    ///
    /// Emits `progress().changed()` when the clamped value differs. When the
    /// property is watched or bound through the runtime, the change invalidates
    /// paint.
    ///
    /// # Panics
    ///
    /// Panics when `progress` is non-finite.
    pub fn set_progress(&mut self, progress: f32) {
        assert!(progress.is_finite(), "progress must be finite");
        self.progress.set(progress);
    }
}

impl Default for ProgressBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for ProgressBar {
    fn layout(&mut self, _: &mut LayoutContext<'_>, constraints: Constraints) -> LayoutResult {
        let width = if constraints.max().width().is_finite() {
            constraints.max().width()
        } else {
            DEFAULT_WIDTH
        };
        LayoutResult::new(constraints.constrain(size(width, HEIGHT)))
    }

    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("progress-bar")
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
        let theme = environment.theme();
        let radius = style
            .corner_radius
            .unwrap_or(bounds.size.height() / 2.0)
            .min(bounds.size.height() / 2.0);
        context.fill_rounded_rect(
            bounds,
            radius,
            style
                .background
                .unwrap_or_else(|| theme.control_background()),
        );
        if let Some(border) = style.border {
            context.stroke_rounded_rect(bounds, radius, border.width, border.color);
        }
        let filled_width = bounds.size.width() * self.progress.get();
        if filled_width > 0.0 {
            let filled = Rect::new(bounds.origin, size(filled_width, bounds.size.height()));
            context.fill_rounded_rect(
                filled,
                radius.min(filled_width / 2.0),
                style.accent.unwrap_or_else(|| theme.accent()),
            );
        }
    }

    fn accessibility(&self) -> Accessibility {
        Accessibility::new(AccessibilityRole::ProgressIndicator).with_numeric_value(
            f64::from(self.progress.get()),
            0.0,
            1.0,
        )
    }
}

fn size(width: f32, height: f32) -> Size {
    Size::new(width, height).expect("progress bar sizes are non-negative and not NaN")
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use drawen_render::{DisplayCommand, DisplayList};
    use drawen_ui::UiRuntime;

    use super::*;

    #[test]
    fn paints_the_normalized_filled_portion() {
        let mut runtime = UiRuntime::new(ProgressBar::with_progress(0.25));
        let mut display_list = DisplayList::new();
        runtime
            .layout(Constraints::tight(size(80.0, HEIGHT)).expect("valid constraints"))
            .expect("progress bar layout succeeds");
        runtime
            .paint(&mut PaintContext::new(&mut display_list))
            .expect("progress bar paint succeeds");

        assert!(matches!(
            display_list.commands(),
            [
                DisplayCommand::FillRoundedRect { .. },
                DisplayCommand::FillRoundedRect { rect, .. },
            ] if (rect.size.width() - 20.0).abs() <= f32::EPSILON
        ));
    }

    #[test]
    fn clamps_programmatic_progress() {
        let mut progress = ProgressBar::with_progress(2.0);
        assert!((progress.progress().get() - 1.0).abs() <= f32::EPSILON);
        progress.set_progress(-1.0);
        assert!(progress.progress().get().abs() <= f32::EPSILON);
    }

    #[test]
    fn bound_progress_is_clamped_to_the_unit_range() {
        let fraction = Property::new(0.25_f32);
        let bar = ProgressBar::new();
        let progress = bar.progress();
        let mut runtime = UiRuntime::new(bar);
        let _binding = runtime.bind(&progress, &fraction);
        runtime
            .layout(Constraints::tight(size(80.0, HEIGHT)).expect("valid constraints"))
            .expect("progress bar layout succeeds");
        assert!((progress.get() - 0.25).abs() <= f32::EPSILON);

        // A binding copies the source verbatim; the property clamps it.
        fraction.set(2.0);
        assert!((progress.get() - 1.0).abs() <= f32::EPSILON);

        fraction.set(-1.0);
        assert!(progress.get().abs() <= f32::EPSILON);
    }

    #[test]
    fn set_progress_emits_property_changed() {
        let bar = ProgressBar::new();
        let progress = bar.progress();
        let changes = Rc::new(RefCell::new(Vec::new()));
        let _subscription = progress.subscribe({
            let changes = Rc::clone(&changes);
            move |value| changes.borrow_mut().push(*value)
        });
        let mut runtime = UiRuntime::new(bar);
        runtime
            .layout(Constraints::tight(size(80.0, HEIGHT)).expect("valid constraints"))
            .expect("progress bar layout succeeds");

        progress.set(0.5);
        assert_eq!(*changes.borrow(), vec![0.5]);

        // An equal write is silent.
        progress.set(0.5);
        assert_eq!(changes.borrow().len(), 1);
    }
}
