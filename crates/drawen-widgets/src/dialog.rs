use drawen_core::{Color, Constraints, Insets, Point, Rect, Size};
use drawen_render::PaintContext;
use drawen_ui::{
    Accessibility, AccessibilityRole, AccessibilityState, ChildLayout, DssStyle, LayoutContext,
    LayoutResult, StyleTarget, SurfaceStyle, UiEnvironment, Widget,
};

const DEFAULT_PADDING: Insets = Insets::all(20.0);
const DEFAULT_ELEVATION: f32 = 16.0;

/// The visual panel of a modal dialog.
///
/// `Dialog` is a single-child surface with rounded corners, a border, padding,
/// and a soft shadow. It reports [`AccessibilityRole::Dialog`] with the modal
/// state, so assistive technologies announce the centered panel rather than the
/// invisible [`crate::Modal`] host around it.
///
/// Place it as the only child of a [`crate::Modal`] and put the title, body, and
/// action row inside a [`drawen_ui::Column`]:
///
/// ```text
/// modal
/// └── dialog
///     └── column
///         ├── text      (title)
///         ├── text      (body)
///         └── row       (cancel, confirm)
/// ```
pub struct Dialog {
    style: SurfaceStyle,
    elevation: f32,
    shadow_color: Option<Color>,
}

impl Dialog {
    /// Creates a dialog panel with theme surface, padding, and elevation.
    #[must_use]
    pub fn new() -> Self {
        Self {
            style: SurfaceStyle {
                background: None,
                border: None,
                corner_radius: None,
                padding: Some(DEFAULT_PADDING),
            },
            elevation: DEFAULT_ELEVATION,
            shadow_color: None,
        }
    }

    /// Returns this dialog with an explicit background color.
    #[must_use]
    pub const fn with_background(mut self, color: Color) -> Self {
        self.style.background = Some(color);
        self
    }

    /// Returns this dialog with a border.
    #[must_use]
    pub const fn with_border(mut self, border: drawen_ui::Border) -> Self {
        self.style.border = Some(border);
        self
    }

    /// Returns this dialog with rounded corners of `radius` logical pixels.
    #[must_use]
    pub const fn with_corner_radius(mut self, radius: f32) -> Self {
        self.style.corner_radius = Some(radius);
        self
    }

    /// Returns this dialog with an inset between its edge and direct child.
    #[must_use]
    pub const fn with_padding(mut self, padding: Insets) -> Self {
        self.style.padding = Some(padding);
        self
    }

    /// Returns this dialog with an explicit shadow elevation.
    #[must_use]
    pub const fn with_elevation(mut self, elevation: f32) -> Self {
        self.elevation = elevation.max(0.0);
        self
    }

    /// Returns this dialog with an explicit shadow color.
    #[must_use]
    pub const fn with_shadow_color(mut self, color: Color) -> Self {
        self.shadow_color = Some(color);
        self
    }
}

impl Default for Dialog {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Dialog {
    fn layout(
        &mut self,
        context: &mut LayoutContext<'_>,
        constraints: Constraints,
    ) -> LayoutResult {
        let style = context.style();
        let padding = self
            .style
            .padding
            .or(style.padding)
            .unwrap_or(DEFAULT_PADDING);
        let max = constraints.max();
        let child_max = size(
            (max.width() - padding.horizontal()).max(0.0),
            (max.height() - padding.vertical()).max(0.0),
        );
        let mut children = Vec::new();
        let child_size = if context.child_count() == 1 {
            let (id, layout) = context
                .layout_child(0, Constraints::loose(child_max))
                .expect("runtime child index is valid");
            children.push(ChildLayout::new(id, Point::new(padding.left, padding.top)));
            layout.size()
        } else {
            Size::ZERO
        };
        LayoutResult::with_children(
            constraints.constrain(size(
                child_size.width() + padding.horizontal(),
                child_size.height() + padding.vertical(),
            )),
            children,
        )
    }

    fn style_target(&self) -> StyleTarget {
        StyleTarget::new("dialog")
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
        let radius = self
            .style
            .corner_radius
            .or(style.corner_radius)
            .unwrap_or_else(|| theme.corner_radius());
        let shadow = self.shadow_color.unwrap_or_else(|| theme.shadow());
        context.draw_shadow(bounds, radius, self.elevation, shadow);
        let background = self
            .style
            .background
            .or(style.background)
            .unwrap_or_else(|| theme.raised_surface());
        context.fill_rounded_rect(bounds, radius, background);
        if let Some(border) = self.style.border.or(style.border) {
            context.stroke_rounded_rect(bounds, radius, border.width, border.color);
        }
    }

    fn accessibility(&self) -> Accessibility {
        Accessibility::new(AccessibilityRole::Dialog).with_states(AccessibilityState::MODAL)
    }
}

fn size(width: f32, height: f32) -> Size {
    Size::new(width, height).expect("dialog sizes are non-negative and not NaN")
}

#[cfg(test)]
mod tests {
    use drawen_core::{Color, Constraints, Insets, Size};
    use drawen_render::{DisplayCommand, DisplayList, PaintContext};
    use drawen_ui::UiRuntime;

    use super::Dialog;
    use crate::Text;

    fn size(width: f32, height: f32) -> Size {
        Size::new(width, height).expect("valid test size")
    }

    #[test]
    fn measures_padding_and_content() {
        let mut runtime = UiRuntime::new(Dialog::new().with_padding(Insets::all(10.0)));
        let root = runtime.root();
        runtime
            .append_child(
                root,
                Text::styled("Body", drawen_render::TextStyle::new(12.0, Color::BLACK)),
            )
            .expect("content");
        runtime
            .layout(Constraints::loose(size(400.0, 300.0)))
            .expect("layout succeeds");
        let bounds = runtime.bounds(root).expect("root has bounds");
        assert!(bounds.size.width() > 20.0);
        assert!(bounds.size.height() > 20.0);
    }

    #[test]
    fn reports_the_dialog_role_and_modal_state() {
        use drawen_ui::{AccessibilityRole, AccessibilityState, Widget};

        let accessibility = Dialog::new().accessibility();
        assert_eq!(accessibility.role, AccessibilityRole::Dialog);
        assert!(accessibility.states.contains(AccessibilityState::MODAL));
    }

    #[test]
    fn paints_a_shadow_then_the_surface() {
        let mut runtime = UiRuntime::new(Dialog::new().with_elevation(4.0));
        let mut display_list = DisplayList::new();
        runtime
            .layout(Constraints::tight(size(120.0, 80.0)).expect("valid constraints"))
            .expect("layout succeeds");
        runtime
            .paint(&mut PaintContext::new(&mut display_list))
            .expect("paint succeeds");

        let commands = display_list.commands();
        assert!(matches!(commands[0], DisplayCommand::Shadow { .. }));
        assert!(matches!(
            commands.last(),
            Some(DisplayCommand::FillRoundedRect { .. })
        ));
    }
}
