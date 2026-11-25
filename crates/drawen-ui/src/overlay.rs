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

/// The outcome of resolving an overlay placement against the viewport.
///
/// `available` is the largest size that fits on `side` without covering the
/// anchor or leaving the viewport margin. Callers whose content is bigger than
/// `available` should shrink to it and let the content scroll instead of
/// overflowing over the anchor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedOverlay {
    /// Top-left origin in the anchor's coordinate space.
    pub origin: Point,
    /// The side actually used after flipping and space comparison.
    pub side: OverlaySide,
    /// Maximum size that fits on `side`; infinite per axis without a viewport.
    pub available: Size,
}

/// Positions `size` against `anchor`, flipping sides and clamping to `viewport`.
///
/// `viewport` is the runtime's window rectangle. Passing `None` keeps the
/// requested side and performs no collision handling, which is useful for
/// headless layouts with unbounded constraints.
///
/// When neither side fits the requested size, the side with more available
/// space wins and [`ResolvedOverlay::available`] reports how far the content
/// must shrink to stay inside the viewport.
///
/// # Panics
///
/// Panics when `anchor` or `viewport` carries NaN coordinates, which makes the
/// computed available space invalid.
#[must_use]
pub fn place_overlay(
    anchor: Rect,
    size: Size,
    viewport: Option<Rect>,
    options: OverlayOptions,
) -> ResolvedOverlay {
    let placement = options.placement;
    let unbounded = ResolvedOverlay {
        origin: overlay_origin(anchor, size, placement, placement.side),
        side: placement.side,
        available: Size::new(f32::INFINITY, f32::INFINITY).expect("infinite size is valid"),
    };
    let Some(viewport) = viewport else {
        return unbounded;
    };
    if viewport.size.width() <= 0.0 || viewport.size.height() <= 0.0 {
        return unbounded;
    }
    let margin = options.margin.max(0.0);
    // The flip decision only looks at the axis the side extends along.
    // Cross-axis overflow is resolved by clamping, so a wide overlay near the
    // right edge still flips vertically instead of covering its anchor.
    let fits = |side: OverlaySide| {
        fits_side(
            overlay_origin(anchor, size, placement, side),
            size,
            viewport,
            margin,
            side,
        )
    };
    let opposite = placement.side.opposite();
    let chosen = if fits(placement.side) {
        placement.side
    } else if fits(opposite) {
        opposite
    } else {
        // Neither side fits: keep the side with more room so shrinking the
        // content to `available` loses as little as possible.
        let primary = |side: OverlaySide| match side {
            OverlaySide::Top | OverlaySide::Bottom => {
                available_space(anchor, viewport, margin, placement.offset, side).height()
            }
            OverlaySide::Left | OverlaySide::Right => {
                available_space(anchor, viewport, margin, placement.offset, side).width()
            }
        };
        if primary(opposite) > primary(placement.side) {
            opposite
        } else {
            placement.side
        }
    };
    let origin = clamp_to_viewport(
        overlay_origin(anchor, size, placement, chosen),
        size,
        viewport,
        margin,
    );
    ResolvedOverlay {
        origin,
        side: chosen,
        available: available_space(anchor, viewport, margin, placement.offset, chosen),
    }
}

/// The largest overlay size that fits on `side` of `anchor` inside `viewport`.
///
/// The primary axis measures from the anchor edge (plus the placement gap) to
/// the far viewport edge minus the margin; the cross axis spans the whole
/// viewport minus both margins because clamping can slide the overlay along it.
fn available_space(
    anchor: Rect,
    viewport: Rect,
    margin: f32,
    offset: f32,
    side: OverlaySide,
) -> Size {
    let offset = offset.max(0.0);
    let cross_width = (viewport.size.width() - margin * 2.0).max(0.0);
    let cross_height = (viewport.size.height() - margin * 2.0).max(0.0);
    let (width, height) = match side {
        OverlaySide::Bottom => (
            cross_width,
            (viewport.bottom() - margin - anchor.bottom() - offset).max(0.0),
        ),
        OverlaySide::Top => (
            cross_width,
            (anchor.origin.y - offset - viewport.origin.y - margin).max(0.0),
        ),
        OverlaySide::Right => (
            (viewport.right() - margin - anchor.right() - offset).max(0.0),
            cross_height,
        ),
        OverlaySide::Left => (
            (anchor.origin.x - offset - viewport.origin.x - margin).max(0.0),
            cross_height,
        ),
    };
    Size::new(width, height).expect("available space is non-negative and not NaN")
}

fn overlay_origin(anchor: Rect, size: Size, placement: OverlayPlacement, edge: OverlaySide) -> Point {
    let offset = placement.offset.max(0.0);
    match edge {
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

/// Whether the overlay fits along the axis the requested side extends along.
///
/// The cross axis is deliberately ignored: it is clamped into the viewport
/// after the side is chosen, and must not veto a flip on the primary axis.
fn fits_side(origin: Point, size: Size, viewport: Rect, margin: f32, along: OverlaySide) -> bool {
    match along {
        OverlaySide::Top | OverlaySide::Bottom => {
            origin.y >= viewport.origin.y + margin
                && origin.y + size.height() <= viewport.bottom() - margin
        }
        OverlaySide::Left | OverlaySide::Right => {
            origin.x >= viewport.origin.x + margin
                && origin.x + size.width() <= viewport.right() - margin
        }
    }
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
        let resolved = place_overlay(
            anchor,
            size(120.0, 80.0),
            Some(viewport(320.0, 200.0)),
            options,
        );

        assert_eq!(resolved.side, OverlaySide::Top);
        assert!((resolved.origin.x - 80.0).abs() < f32::EPSILON);
        assert!((resolved.origin.y - 86.0).abs() < f32::EPSILON);
    }

    #[test]
    fn clamps_to_the_viewport_margin() {
        let anchor = Rect::new(Point::new(300.0, 10.0), size(20.0, 20.0));
        let options = OverlayOptions::new(OverlayPlacement::new(
            OverlaySide::Bottom,
            OverlayAlign::Center,
        ))
        .with_margin(8.0);
        let resolved = place_overlay(
            anchor,
            size(120.0, 80.0),
            Some(viewport(320.0, 200.0)),
            options,
        );

        assert!((resolved.origin.x - 192.0).abs() < f32::EPSILON);
        assert!((resolved.origin.y - 34.0).abs() < f32::EPSILON);
    }

    #[test]
    fn unbounded_layout_keeps_the_requested_side() {
        let anchor = Rect::new(Point::new(10.0, 10.0), size(20.0, 20.0));
        let options = OverlayOptions::new(OverlayPlacement::top());
        let resolved = place_overlay(anchor, size(40.0, 30.0), None, options);

        assert_eq!(resolved.side, OverlaySide::Top);
        assert!(resolved.available.width().is_infinite());
        assert!(resolved.available.height().is_infinite());
        assert!((resolved.origin.x - 0.0).abs() < f32::EPSILON);
        assert!((resolved.origin.y + 24.0).abs() < f32::EPSILON);
    }

    #[test]
    fn cross_axis_overflow_does_not_block_the_vertical_flip() {
        // The overlay overflows the right edge on X and the bottom edge on Y.
        // Only the Y overflow may veto the requested side; the flip upward
        // must still happen so the overlay does not cover its anchor.
        let anchor = Rect::new(Point::new(280.0, 170.0), size(20.0, 20.0));
        let options = OverlayOptions::new(OverlayPlacement::new(
            OverlaySide::Bottom,
            OverlayAlign::Center,
        ));
        let resolved = place_overlay(
            anchor,
            size(120.0, 80.0),
            Some(viewport(320.0, 200.0)),
            options,
        );

        assert!((resolved.origin.x - 192.0).abs() < f32::EPSILON);
        assert!((resolved.origin.y - 86.0).abs() < f32::EPSILON);
        assert!(
            resolved.origin.y + 80.0 <= anchor.origin.y,
            "overlay covers its anchor"
        );
    }

    #[test]
    fn cross_axis_overflow_does_not_block_the_horizontal_flip() {
        let anchor = Rect::new(Point::new(300.0, 170.0), size(20.0, 20.0));
        let options = OverlayOptions::new(OverlayPlacement::new(
            OverlaySide::Right,
            OverlayAlign::Start,
        ));
        let resolved = place_overlay(
            anchor,
            size(120.0, 80.0),
            Some(viewport(320.0, 200.0)),
            options,
        );

        assert!((resolved.origin.x - 176.0).abs() < f32::EPSILON);
        assert!((resolved.origin.y - 112.0).abs() < f32::EPSILON);
        assert!(
            resolved.origin.x + 120.0 <= anchor.origin.x,
            "overlay covers its anchor"
        );
    }

    #[test]
    fn reports_the_available_size_on_the_resolved_side() {
        // Anchor near the bottom: below there is only 20px minus margin and
        // gap, above there is plenty. The flip reports the room above.
        let anchor = Rect::new(Point::new(100.0, 170.0), size(80.0, 20.0));
        let options = OverlayOptions::new(OverlayPlacement::bottom()).with_margin(8.0);
        let resolved = place_overlay(
            anchor,
            size(120.0, 80.0),
            Some(viewport(320.0, 200.0)),
            options,
        );
        assert_eq!(resolved.side, OverlaySide::Top);
        // Above: 170 - 4 (gap) - 8 (margin) = 158; cross axis: 320 - 16.
        assert!((resolved.available.height() - 158.0).abs() < f32::EPSILON);
        assert!((resolved.available.width() - 304.0).abs() < f32::EPSILON);
    }

    #[test]
    fn oversized_content_resolves_to_the_side_with_more_room() {
        // A 300px-tall overlay cannot fit above or below a centered anchor in
        // a 200px window. The anchor sits lower, so above has more room and
        // must win; `available` tells the caller how far to shrink.
        let anchor = Rect::new(Point::new(100.0, 140.0), size(80.0, 20.0));
        let options = OverlayOptions::new(OverlayPlacement::bottom()).with_margin(8.0);
        let resolved = place_overlay(
            anchor,
            size(120.0, 300.0),
            Some(viewport(320.0, 200.0)),
            options,
        );
        assert_eq!(resolved.side, OverlaySide::Top);
        assert!((resolved.available.height() - 128.0).abs() < f32::EPSILON);
        // Shrinking to `available` keeps the overlay inside the viewport.
        let shrunk = place_overlay(
            anchor,
            resolved.available,
            Some(viewport(320.0, 200.0)),
            options,
        );
        assert_eq!(shrunk.side, OverlaySide::Top);
        assert!(shrunk.origin.y >= 8.0);
        assert!(shrunk.origin.y + shrunk.available.height() <= 192.0);
    }

    #[test]
    fn dismiss_policy_is_configurable() {
        let options = OverlayOptions::default().with_dismiss(DismissPolicy::None);
        assert_eq!(options.dismiss, DismissPolicy::None);
    }
}
