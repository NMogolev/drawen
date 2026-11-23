use drawen_core::{Point, Rect, Size};

/// The side of its anchor on which a floating overlay prefers to appear.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlaySide {
    /// Above the anchor.
    Top,
    /// Below the anchor.
    Bottom,
    /// To the left of the anchor.
    Left,
    /// To the right of the anchor.
    Right,
}

impl OverlaySide {
    const fn opposite(self) -> Self {
        match self {
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Top,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// Alignment of a floating overlay along the anchor's cross axis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayAlign {
    /// Align the overlay's leading edge with the anchor's leading edge.
    Start,
    /// Center the overlay on the anchor.
    Center,
    /// Align the overlay's trailing edge with the anchor's trailing edge.
    End,
}

/// Placement request for a floating overlay.
///
/// The runtime flips the overlay to the opposite side when the requested side
/// would leave the viewport, then clamps it inside the viewport margin. Widgets
/// therefore describe intent rather than exact coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayPlacement {
    /// Preferred side of the anchor.
    pub side: OverlaySide,
    /// Cross-axis alignment relative to the anchor.
    pub align: OverlayAlign,
    /// Gap between the anchor and the overlay, in logical pixels.
    pub offset: f32,
}

impl OverlayPlacement {
    /// Creates a placement with the standard four-pixel gap.
    #[must_use]
    pub const fn new(side: OverlaySide, align: OverlayAlign) -> Self {
        Self {
            side,
            align,
            offset: 4.0,
        }
    }

    /// Returns this placement with an explicit gap.
    #[must_use]
    pub const fn with_offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }

    /// A centered placement below the anchor.
    #[must_use]
    pub const fn bottom() -> Self {
        Self::new(OverlaySide::Bottom, OverlayAlign::Center)
    }

    /// A centered placement above the anchor.
    #[must_use]
    pub const fn top() -> Self {
        Self::new(OverlaySide::Top, OverlayAlign::Center)
    }

    /// A start-aligned placement to the right of the anchor.
    #[must_use]
    pub const fn right() -> Self {
        Self::new(OverlaySide::Right, OverlayAlign::Start)
    }

    /// A start-aligned placement to the left of the anchor.
    #[must_use]
    pub const fn left() -> Self {
        Self::new(OverlaySide::Left, OverlayAlign::Start)
    }
}

impl Default for OverlayPlacement {
    fn default() -> Self {
        Self::bottom()
    }
}

/// How a floating overlay reacts to interaction outside its subtree.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DismissPolicy {
    /// The owner decides when the overlay closes.
    #[default]
    None,
    /// A primary click outside the overlay and its anchor asks the owner to close it.
    Light,
}

/// Layout and interaction options for a floating overlay child.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayOptions {
    /// Preferred placement relative to the anchor.
    pub placement: OverlayPlacement,
    /// Outside-click dismissal behavior.
    pub dismiss: DismissPolicy,
    /// Minimum distance to keep from the viewport edges.
    pub margin: f32,
}

impl OverlayOptions {
    /// Creates light-dismiss options using `placement`.
    #[must_use]
    pub const fn new(placement: OverlayPlacement) -> Self {
        Self {
            placement,
            dismiss: DismissPolicy::Light,
            margin: 8.0,
        }
    }

    /// Returns these options with an explicit dismiss policy.
    #[must_use]
    pub const fn with_dismiss(mut self, dismiss: DismissPolicy) -> Self {
        self.dismiss = dismiss;
        self
    }

    /// Returns these options with an explicit viewport margin.
    #[must_use]
    pub const fn with_margin(mut self, margin: f32) -> Self {
        self.margin = margin;
        self
    }
}

impl Default for OverlayOptions {
    fn default() -> Self {
        Self::new(OverlayPlacement::default())
    }
}

/// Positions `size` against `anchor`, flipping sides and clamping to `viewport`.
///
/// `viewport` is the runtime's window rectangle. Passing `None` keeps the
/// requested side and performs no collision handling, which is useful for
/// headless layouts with unbounded constraints.
#[must_use]
pub fn place_overlay(
    anchor: Rect,
    size: Size,
    viewport: Option<Rect>,
    options: OverlayOptions,
) -> Point {
    let placement = options.placement;
    let offset = placement.offset.max(0.0);
    let origin = |side: OverlaySide| -> Point {
        match side {
            OverlaySide::Bottom => Point::new(
                align_x(anchor, size, placement.align),
                anchor.bottom() + offset,
            ),
            OverlaySide::Top => Point::new(
                align_x(anchor, size, placement.align),
                anchor.origin.y - size.height() - offset,
            ),
            OverlaySide::Right => Point::new(
                anchor.right() + offset,
                align_y(anchor, size, placement.align),
            ),
            OverlaySide::Left => Point::new(
                anchor.origin.x - size.width() - offset,
                align_y(anchor, size, placement.align),
            ),
        }
    };

    let Some(viewport) = viewport else {
        return origin(placement.side);
    };
    if viewport.size.width() <= 0.0 || viewport.size.height() <= 0.0 {
        return origin(placement.side);
    }
    let margin = options.margin.max(0.0);
    let mut candidate = origin(placement.side);
    if !fits(candidate, size, viewport, margin) {
        let flipped = origin(placement.side.opposite());
        if fits(flipped, size, viewport, margin) {
            candidate = flipped;
        }
    }
    clamp_to_viewport(candidate, size, viewport, margin)
}

fn align_x(anchor: Rect, size: Size, align: OverlayAlign) -> f32 {
    match align {
        OverlayAlign::Start => anchor.origin.x,
        OverlayAlign::Center => anchor.origin.x + (anchor.size.width() - size.width()) / 2.0,
        OverlayAlign::End => anchor.right() - size.width(),
    }
}

fn align_y(anchor: Rect, size: Size, align: OverlayAlign) -> f32 {
    match align {
        OverlayAlign::Start => anchor.origin.y,
        OverlayAlign::Center => anchor.origin.y + (anchor.size.height() - size.height()) / 2.0,
        OverlayAlign::End => anchor.bottom() - size.height(),
    }
}

fn fits(origin: Point, size: Size, viewport: Rect, margin: f32) -> bool {
    let left = viewport.origin.x + margin;
    let top = viewport.origin.y + margin;
    let right = viewport.right() - margin;
    let bottom = viewport.bottom() - margin;
    origin.x >= left
        && origin.y >= top
        && origin.x + size.width() <= right
        && origin.y + size.height() <= bottom
}

fn clamp_to_viewport(origin: Point, size: Size, viewport: Rect, margin: f32) -> Point {
    let left = viewport.origin.x + margin;
    let top = viewport.origin.y + margin;
    let max_x = (viewport.right() - margin - size.width()).max(left);
    let max_y = (viewport.bottom() - margin - size.height()).max(top);
    Point::new(origin.x.clamp(left, max_x), origin.y.clamp(top, max_y))
}

#[cfg(test)]
mod tests {
    use drawen_core::{Point, Rect, Size};

    use super::{
        DismissPolicy, OverlayAlign, OverlayOptions, OverlayPlacement, OverlaySide, place_overlay,
    };

    fn size(width: f32, height: f32) -> Size {
        Size::new(width, height).expect("valid test size")
    }

    fn viewport(width: f32, height: f32) -> Rect {
        Rect::new(Point::ZERO, size(width, height))
    }

    #[test]
    fn flips_below_placement_above_when_the_viewport_is_full() {
        let anchor = Rect::new(Point::new(100.0, 170.0), size(80.0, 24.0));
        let options = OverlayOptions::new(OverlayPlacement::new(
            OverlaySide::Bottom,
            OverlayAlign::Center,
        ));
        let origin = place_overlay(
            anchor,
            size(120.0, 80.0),
            Some(viewport(320.0, 200.0)),
            options,
        );

        assert!((origin.x - 80.0).abs() < f32::EPSILON);
        assert!((origin.y - 86.0).abs() < f32::EPSILON);
    }

    #[test]
    fn clamps_to_the_viewport_margin() {
        let anchor = Rect::new(Point::new(300.0, 10.0), size(20.0, 20.0));
        let options = OverlayOptions::new(OverlayPlacement::new(
            OverlaySide::Bottom,
            OverlayAlign::Center,
        ))
        .with_margin(8.0);
        let origin = place_overlay(
            anchor,
            size(120.0, 80.0),
            Some(viewport(320.0, 200.0)),
            options,
        );

        assert!((origin.x - 192.0).abs() < f32::EPSILON);
        assert!((origin.y - 34.0).abs() < f32::EPSILON);
    }

    #[test]
    fn unbounded_layout_keeps_the_requested_side() {
        let anchor = Rect::new(Point::new(10.0, 10.0), size(20.0, 20.0));
        let options = OverlayOptions::new(OverlayPlacement::top());
        let origin = place_overlay(anchor, size(40.0, 30.0), None, options);

        assert!((origin.x - 0.0).abs() < f32::EPSILON);
        assert!((origin.y + 24.0).abs() < f32::EPSILON);
    }

    #[test]
    fn dismiss_policy_is_configurable() {
        let options = OverlayOptions::default().with_dismiss(DismissPolicy::None);
        assert_eq!(options.dismiss, DismissPolicy::None);
    }
}
