use core::fmt;

use drawen_core::{Affine, Color, Diagnostic, DiagnosticReporter, Point, Rect, Size};
use drawen_render::{DisplayCommand, DisplayList, RasterImage, TextLayout};

use crate::{Pixel, PixelBuffer};

/// Deterministic reference renderer for Drawen display lists.
#[derive(Debug, Default)]
pub struct SoftwareRenderer;

impl SoftwareRenderer {
    /// Renders `display_list` into `target` at one physical pixel per logical pixel.
    ///
    /// For a high-DPI target, use [`Self::render_with_scale_factor`].
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] for invalid geometry or unbalanced state commands.
    pub fn render(
        &self,
        display_list: &DisplayList,
        target: &mut PixelBuffer,
    ) -> Result<(), RenderError> {
        self.render_with_scale_factor(display_list, target, 1.0)
    }

    /// Renders logical-pixel display commands into a physical-pixel `target`.
    ///
    /// All display-list geometry, including text sizes and transforms, remains
    /// in device-independent logical pixels. `scale_factor` is applied exactly
    /// once while sampling the physical target.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::InvalidScaleFactor`] for a non-finite or
    /// non-positive scale factor, and other errors for invalid commands.
    #[allow(clippy::too_many_lines)]
    pub fn render_with_scale_factor(
        &self,
        display_list: &DisplayList,
        target: &mut PixelBuffer,
        scale_factor: f32,
    ) -> Result<(), RenderError> {
        if !scale_factor.is_finite() || scale_factor <= 0.0 {
            return Err(RenderError::InvalidScaleFactor);
        }
        let mut state = RenderState::default();
        let mut saved_states = Vec::new();

        for command in display_list.commands() {
            match command {
                DisplayCommand::Save => saved_states.push(state.clone()),
                DisplayCommand::Restore => {
                    state = saved_states.pop().ok_or(RenderError::UnmatchedRestore)?;
                }
                DisplayCommand::PopClip => {
                    state.clips.pop().ok_or(RenderError::UnmatchedClipPop)?;
                }
                DisplayCommand::Transform { transform } => {
                    if !transform.is_finite() {
                        return Err(RenderError::NonFiniteGeometry);
                    }
                    state.transform = state.transform.then(*transform);
                }
                DisplayCommand::PushClip { rect } => {
                    validate_rect(*rect)?;
                    state.clips.push(Clip {
                        rect: *rect,
                        inverse: state
                            .transform
                            .inverse()
                            .ok_or(RenderError::SingularTransform)?,
                    });
                }
                DisplayCommand::FillRect { rect, color } => {
                    Self::fill_shape(
                        target,
                        &state,
                        scale_factor,
                        *color,
                        Shape::fill(*rect, 0.0),
                    )?;
                }
                DisplayCommand::FillRoundedRect {
                    rect,
                    radius,
                    color,
                } => {
                    Self::fill_shape(
                        target,
                        &state,
                        scale_factor,
                        *color,
                        Shape::fill(*rect, *radius),
                    )?;
                }
                DisplayCommand::Shadow {
                    rect,
                    radius,
                    blur,
                    color,
                } => {
                    Self::fill_shape(
                        target,
                        &state,
                        scale_factor,
                        *color,
                        Shape::shadow(*rect, *radius, *blur),
                    )?;
                }
                DisplayCommand::StrokeRect { rect, width, color } => {
                    Self::fill_shape(
                        target,
                        &state,
                        scale_factor,
                        *color,
                        Shape::stroke(*rect, 0.0, *width),
                    )?;
                }
                DisplayCommand::StrokeRoundedRect {
                    rect,
                    radius,
                    width,
                    color,
                } => {
                    Self::fill_shape(
                        target,
                        &state,
                        scale_factor,
                        *color,
                        Shape::stroke(*rect, *radius, *width),
                    )?;
                }
                DisplayCommand::DrawText { layout, origin } => {
                    Self::draw_text(target, layout, *origin, &state, scale_factor)?;
                }
                DisplayCommand::DrawImage { image, destination } => {
                    Self::draw_image(target, image, *destination, &state, scale_factor)?;
                }
                DisplayCommand::DrawTexture { .. } => {
                    return Err(RenderError::UnsupportedExternalTexture);
                }
            }
        }

        if saved_states.is_empty() {
            Ok(())
        } else {
            Err(RenderError::UnmatchedSave)
        }
    }

    /// Renders a display list and reports a diagnostic if rendering fails.
    ///
    /// # Errors
    ///
    /// Returns the same [`RenderError`] as [`Self::render`] after reporting an
    /// error diagnostic.
    pub fn render_with_diagnostics(
        &self,
        display_list: &DisplayList,
        target: &mut PixelBuffer,
        reporter: &mut dyn DiagnosticReporter,
    ) -> Result<(), RenderError> {
        let result = self.render(display_list, target);
        if let Err(error) = result {
            reporter.report(&Diagnostic::error("drawen-software", error.to_string()));
        }
        result
    }

    fn fill_shape(
        target: &mut PixelBuffer,
        state: &RenderState,
        scale_factor: f32,
        color: Color,
        shape: Shape,
    ) -> Result<(), RenderError> {
        validate_rect(shape.rect)?;
        validate_radius(shape.radius)?;
        validate_stroke_width(shape.stroke_width)?;
        validate_blur(shape.blur)?;
        let inverse = state
            .transform
            .inverse()
            .ok_or(RenderError::SingularTransform)?;
        let bounds = if shape.blur > 0.0 {
            expand(shape.rect, shape.blur)
        } else {
            shape
                .stroke_width
                .map_or(shape.rect, |width| expand(shape.rect, width * 0.5))
        };
        let (start_x, start_y, end_x, end_y) =
            physical_bounds(bounds, state.transform, target, scale_factor);
        if start_x >= end_x || start_y >= end_y {
            return Ok(());
        }

        // Hard-edged fills are convex, so their intersection with a scanline is
        // one contiguous span. Find that span with a binary search instead of
        // running the full shape test on every pixel.
        if shape.blur == 0.0 && shape.stroke_width.is_none() {
            fill_convex_shape(
                target,
                state,
                scale_factor,
                color,
                shape,
                inverse,
                (start_x, start_y, end_x, end_y),
            );
            return Ok(());
        }

        // General paths step through logical and local coordinates incrementally;
        // the affine transform is linear, so each pixel only needs two adds.
        let inv_scale = scale_factor.recip();
        let step_local_x = inverse.xx * inv_scale;
        let step_local_y = inverse.xy * inv_scale;
        let has_clips = !state.clips.is_empty();
        let solid_source = (shape.blur == 0.0).then(|| SourceOver::new(color));

        let mut point_y = logical_pixel_center(0, start_y, scale_factor).y;
        for y in start_y..end_y {
            let mut point_x = logical_pixel_center(start_x, 0, scale_factor).x;
            let mut local = inverse.transform_point(Point::new(point_x, point_y));
            for x in start_x..end_x {
                if !has_clips || is_visible(Point::new(point_x, point_y), state) {
                    if shape.blur > 0.0 {
                        let coverage = shadow_coverage(shape.rect, shape.radius, shape.blur, local);
                        if coverage > 0.0 {
                            blend_pixel(target, x, y, color.with_alpha(color.alpha * coverage));
                        }
                    } else {
                        let inside = match shape.stroke_width {
                            None => contains_rounded_rect(shape.rect, shape.radius, local),
                            Some(width) => contains_stroke(shape.rect, shape.radius, width, local),
                        };
                        if inside {
                            if let Some(source) = solid_source {
                                blend_pixel_source(target, x, y, source);
                            } else {
                                blend_pixel(target, x, y, color);
                            }
                        }
                    }
                }
                point_x += inv_scale;
                local.x += step_local_x;
                local.y += step_local_y;
            }
            point_y += inv_scale;
        }
        Ok(())
    }

    fn draw_text(
        target: &mut PixelBuffer,
        layout: &TextLayout,
        origin: Point,
        state: &RenderState,
        scale_factor: f32,
    ) -> Result<(), RenderError> {
        if !origin.x.is_finite() || !origin.y.is_finite() {
            return Err(RenderError::NonFiniteGeometry);
        }
        let inverse = state
            .transform
            .inverse()
            .ok_or(RenderError::SingularTransform)?;
        let inv_scale = scale_factor.recip();
        let step_local_x = inverse.xx * inv_scale;
        let step_local_y = inverse.xy * inv_scale;
        let has_clips = !state.clips.is_empty();
        let text_color = layout.color();
        let source_red = component(text_color.red);
        let source_green = component(text_color.green);
        let source_blue = component(text_color.blue);
        let source_alpha = component(text_color.alpha);
        let glyph_source = GlyphSource::new(source_red, source_green, source_blue, source_alpha);

        for run in layout.glyph_runs() {
            if !run.font_size().is_finite() || run.font_size() <= 0.0 {
                continue;
            }
            let raster_size = run.font_size() * scale_factor;
            if !raster_size.is_finite() || raster_size <= 0.0 {
                return Err(RenderError::NonFiniteGeometry);
            }
            for glyph in run.glyphs() {
                let glyph_origin =
                    Point::new(origin.x + glyph.position().x, origin.y + glyph.position().y);
                if !glyph_origin.x.is_finite() || !glyph_origin.y.is_finite() {
                    return Err(RenderError::NonFiniteGeometry);
                }
                let bitmap = run.font().rasterize_cached(glyph.glyph_id(), raster_size);
                let glyph_rect = Rect::new(
                    glyph_origin,
                    Size::new(
                        as_logical_coordinate_usize(bitmap.width()) / scale_factor,
                        as_logical_coordinate_usize(bitmap.height()) / scale_factor,
                    )
                    .map_err(|_| RenderError::NonFiniteGeometry)?,
                );
                let (start_x, start_y, end_x, end_y) =
                    physical_bounds(glyph_rect, state.transform, target, scale_factor);
                let mut point_y = logical_pixel_center(0, start_y, scale_factor).y;
                for y in start_y..end_y {
                    let mut point_x = logical_pixel_center(start_x, 0, scale_factor).x;
                    let mut local = inverse.transform_point(Point::new(point_x, point_y));
                    for x in start_x..end_x {
                        if !has_clips || is_visible(Point::new(point_x, point_y), state) {
                            let bitmap_x =
                                coordinate_to_usize((local.x - glyph_origin.x) * scale_factor);
                            let bitmap_y =
                                coordinate_to_usize((local.y - glyph_origin.y) * scale_factor);
                            if let Some(&coverage) = bitmap.coverage().get(
                                bitmap_y
                                    .saturating_mul(bitmap.width())
                                    .saturating_add(bitmap_x),
                            ) && coverage != 0
                            {
                                glyph_source.apply(target, x, y, f32::from(coverage) / 255.0);
                            }
                        }
                        point_x += inv_scale;
                        local.x += step_local_x;
                        local.y += step_local_y;
                    }
                    point_y += inv_scale;
                }
            }
        }
        Ok(())
    }

    fn draw_image(
        target: &mut PixelBuffer,
        image: &RasterImage,
        destination: Rect,
        state: &RenderState,
        scale_factor: f32,
    ) -> Result<(), RenderError> {
        validate_rect(destination)?;
        if image.width() == 0
            || image.height() == 0
            || destination.size.width() == 0.0
            || destination.size.height() == 0.0
        {
            return Ok(());
        }
        let inverse = state
            .transform
            .inverse()
            .ok_or(RenderError::SingularTransform)?;
        let inv_scale = scale_factor.recip();
        let step_local_x = inverse.xx * inv_scale;
        let step_local_y = inverse.xy * inv_scale;
        let has_clips = !state.clips.is_empty();
        let (start_x, start_y, end_x, end_y) =
            physical_bounds(destination, state.transform, target, scale_factor);
        let mut point_y = logical_pixel_center(0, start_y, scale_factor).y;
        for y in start_y..end_y {
            let mut point_x = logical_pixel_center(start_x, 0, scale_factor).x;
            let mut local = inverse.transform_point(Point::new(point_x, point_y));
            for x in start_x..end_x {
                if (!has_clips || is_visible(Point::new(point_x, point_y), state))
                    && destination.contains(local)
                {
                    let source_x = image_coordinate(
                        (local.x - destination.origin.x) / destination.size.width(),
                        image.width(),
                    );
                    let source_y = image_coordinate(
                        (local.y - destination.origin.y) / destination.size.height(),
                        image.height(),
                    );
                    let offset = source_y
                        .saturating_mul(usize::try_from(image.width()).unwrap_or(usize::MAX))
                        .saturating_add(source_x)
                        .saturating_mul(4);
                    if let Some(rgba) = image.pixels().get(offset..offset.saturating_add(4)) {
                        blend_pixel_rgba8(target, x, y, [rgba[0], rgba[1], rgba[2], rgba[3]]);
                    }
                }
                point_x += inv_scale;
                local.x += step_local_x;
                local.y += step_local_y;
            }
            point_y += inv_scale;
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
struct RenderState {
    transform: Affine,
    clips: Vec<Clip>,
}

#[derive(Clone)]
struct Clip {
    rect: Rect,
    inverse: Affine,
}

#[derive(Clone, Copy)]
struct Shape {
    rect: Rect,
    radius: f32,
    stroke_width: Option<f32>,
    blur: f32,
}

impl Shape {
    const fn fill(rect: Rect, radius: f32) -> Self {
        Self {
            rect,
            radius,
            stroke_width: None,
            blur: 0.0,
        }
    }

    const fn stroke(rect: Rect, radius: f32, width: f32) -> Self {
        Self {
            rect,
            radius,
            stroke_width: Some(width),
            blur: 0.0,
        }
    }

    const fn shadow(rect: Rect, radius: f32, blur: f32) -> Self {
        Self {
            rect,
            radius,
            stroke_width: None,
            blur,
        }
    }
}

/// Why a [`SoftwareRenderer`] could not execute a display list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderError {
    /// A coordinate, radius, width, or transform component was not finite.
    NonFiniteGeometry,
    /// A scale factor was not finite and positive.
    InvalidScaleFactor,
    /// A transformed operation required an inverse of a singular transform.
    SingularTransform,
    /// A restore was issued without a preceding save.
    UnmatchedRestore,
    /// A clip pop was issued without a preceding clip.
    UnmatchedClipPop,
    /// Rendering ended with a state that was never restored.
    UnmatchedSave,
    /// The display list referenced a GPU texture that the software renderer
    /// cannot sample.
    UnsupportedExternalTexture,
}

impl fmt::Display for RenderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NonFiniteGeometry => "renderer received non-finite or negative geometry",
            Self::InvalidScaleFactor => "render scale factor must be finite and positive",
            Self::SingularTransform => "renderer received a singular transform",
            Self::UnmatchedRestore => "display list restored an empty state stack",
            Self::UnmatchedClipPop => "display list popped an empty clip stack",
            Self::UnmatchedSave => "display list ended with an unbalanced save",
            Self::UnsupportedExternalTexture => {
                "software renderer cannot sample GPU external textures"
            }
        })
    }
}

impl std::error::Error for RenderError {}

fn validate_rect(rect: Rect) -> Result<(), RenderError> {
    if !rect.origin.x.is_finite()
        || !rect.origin.y.is_finite()
        || !rect.size.width().is_finite()
        || !rect.size.height().is_finite()
    {
        Err(RenderError::NonFiniteGeometry)
    } else {
        Ok(())
    }
}

fn validate_radius(radius: f32) -> Result<(), RenderError> {
    if radius.is_finite() && radius >= 0.0 {
        Ok(())
    } else {
        Err(RenderError::NonFiniteGeometry)
    }
}

fn validate_stroke_width(width: Option<f32>) -> Result<(), RenderError> {
    if width.is_none_or(|value| value.is_finite() && value >= 0.0) {
        Ok(())
    } else {
        Err(RenderError::NonFiniteGeometry)
    }
}

fn validate_blur(blur: f32) -> Result<(), RenderError> {
    if blur.is_finite() && blur >= 0.0 {
        Ok(())
    } else {
        Err(RenderError::NonFiniteGeometry)
    }
}

fn shadow_coverage(rect: Rect, radius: f32, blur: f32, point: Point) -> f32 {
    let distance = rounded_rect_distance(rect, radius, point);
    if distance <= 0.0 {
        return 1.0;
    }
    1.0 - smoothstep(0.0, blur, distance)
}

fn rounded_rect_distance(rect: Rect, radius: f32, point: Point) -> f32 {
    let half_width = rect.size.width() * 0.5;
    let half_height = rect.size.height() * 0.5;
    let radius = radius.max(0.0).min(half_width.min(half_height));
    let center = Point::new(rect.origin.x + half_width, rect.origin.y + half_height);
    let offset_x = (point.x - center.x).abs() - (half_width - radius);
    let offset_y = (point.y - center.y).abs() - (half_height - radius);
    let outside = offset_x.max(0.0).hypot(offset_y.max(0.0));
    outside + offset_x.max(offset_y).min(0.0) - radius
}

fn smoothstep(edge_start: f32, edge_end: f32, value: f32) -> f32 {
    if edge_end <= edge_start {
        return if value < edge_start { 0.0 } else { 1.0 };
    }
    let t = ((value - edge_start) / (edge_end - edge_start)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn expand(rect: Rect, amount: f32) -> Rect {
    Rect::new(
        Point::new(rect.origin.x - amount, rect.origin.y - amount),
        Size::new(
            rect.size.width() + amount * 2.0,
            rect.size.height() + amount * 2.0,
        )
        .expect("expanding a finite non-negative rectangle stays valid"),
    )
}

fn contains_stroke(rect: Rect, radius: f32, width: f32, point: Point) -> bool {
    if width == 0.0 {
        return false;
    }
    let outer = expand(rect, width * 0.5);
    let inner = Rect::new(
        Point::new(rect.origin.x + width * 0.5, rect.origin.y + width * 0.5),
        Size::new(
            (rect.size.width() - width).max(0.0),
            (rect.size.height() - width).max(0.0),
        )
        .expect("clamped stroke interior is valid"),
    );
    let outer_radius =
        (radius + width * 0.5).min(outer.size.width().min(outer.size.height()) * 0.5);
    let inner_radius = (radius - width * 0.5)
        .max(0.0)
        .min(inner.size.width().min(inner.size.height()) * 0.5);
    contains_rounded_rect(outer, outer_radius, point)
        && !contains_rounded_rect(inner, inner_radius, point)
}

fn contains_rounded_rect(rect: Rect, radius: f32, point: Point) -> bool {
    if rect.size.width() <= 0.0 || rect.size.height() <= 0.0 {
        return false;
    }
    if !rect.contains(point) {
        return false;
    }
    let radius = radius.min(rect.size.width().min(rect.size.height()) * 0.5);
    if radius == 0.0 {
        return true;
    }
    let left = rect.origin.x + radius;
    let right = rect.right() - radius;
    let top = rect.origin.y + radius;
    let bottom = rect.bottom() - radius;
    let closest_x = point.x.clamp(left, right.max(left));
    let closest_y = point.y.clamp(top, bottom.max(top));
    let dx = point.x - closest_x;
    let dy = point.y - closest_y;
    dx * dx + dy * dy <= radius * radius
}

fn is_visible(point: Point, state: &RenderState) -> bool {
    state
        .clips
        .iter()
        .all(|clip| clip.rect.contains(clip.inverse.transform_point(point)))
}

#[allow(clippy::too_many_arguments)]
fn fill_convex_shape(
    target: &mut PixelBuffer,
    state: &RenderState,
    scale_factor: f32,
    color: Color,
    shape: Shape,
    inverse: Affine,
    (start_x, start_y, end_x, end_y): (u32, u32, u32, u32),
) {
    let clipped = !state.clips.is_empty();
    for y in start_y..end_y {
        // Clips are convex, so their intersection with this row is one
        // interval. Resolve it before looking for the shape edges: a clipped
        // row then searches the visible span instead of the whole shape
        // bounds, and a row the shape covers end to end needs no search at all.
        let (row_start, row_end) = if clipped {
            let Some(span) = clip_row_span(state, y, start_x, end_x, scale_factor) else {
                continue;
            };
            span
        } else {
            (start_x, end_x)
        };
        let inside = |x: u32| {
            let point = logical_pixel_center(x, y, scale_factor);
            contains_rounded_rect(shape.rect, shape.radius, inverse.transform_point(point))
        };
        let (first, last) = if row_start < row_end && inside(row_start) && inside(row_end - 1) {
            // A convex shape contains every pixel between two inside pixels, so
            // the row is covered from the clip edge to the clip edge.
            (row_start, row_end)
        } else {
            let Some(seed) = rounded_rect_seed(
                shape.rect,
                shape.radius,
                inverse,
                y,
                row_start,
                row_end,
                scale_factor,
            ) else {
                continue;
            };
            let first = first_true(row_start, seed + 1, inside);
            // Convex containment makes the true region a single interval, so the
            // first false pixel after a known inside pixel ends the span.
            let last = first_true(seed, row_end, |x| !inside(x));
            (first, last)
        };
        if !clipped {
            fill_row(target, y, first, last, color);
            continue;
        }
        // The clip span is a conservative superset of the visible pixels, so
        // trim the boundary with the exact clip test.
        let (mut first, mut last) = (first, last);
        while first < last && !is_visible(logical_pixel_center(first, y, scale_factor), state) {
            first += 1;
        }
        while last > first && !is_visible(logical_pixel_center(last - 1, y, scale_factor), state) {
            last -= 1;
        }
        fill_row(target, y, first, last, color);
    }
}

/// Returns the integer x range on row `y` where every active clip is visible.
///
/// The result is a conservative superset: callers must still test the two edge
/// pixels with [`is_visible`]. The clip rectangles are convex and their inverse
/// transforms are affine, so each row is an interval.
fn clip_row_span(
    state: &RenderState,
    y: u32,
    start_x: u32,
    end_x: u32,
    scale_factor: f32,
) -> Option<(u32, u32)> {
    let row_y = (as_logical_coordinate(y) + 0.5) / scale_factor;
    let x_scale = scale_factor.recip();
    let mut low = as_logical_coordinate(start_x) - 1.0;
    let mut high = as_logical_coordinate(end_x) + 1.0;
    for clip in &state.clips {
        let inverse = clip.inverse;
        // local_x(x) = inverse.xx * (x + 0.5) / scale + inverse.yx * row_y + inverse.dx
        let x_slope = inverse.xx * x_scale;
        let x_offset = inverse.xx * 0.5 * x_scale + inverse.yx * row_y + inverse.dx;
        let y_slope = inverse.xy * x_scale;
        let y_offset = inverse.xy * 0.5 * x_scale + inverse.yy * row_y + inverse.dy;
        for (slope, offset, from, to) in [
            (x_slope, x_offset, clip.rect.origin.x, clip.rect.right()),
            (y_slope, y_offset, clip.rect.origin.y, clip.rect.bottom()),
        ] {
            let (clip_low, clip_high) = linear_band(slope, offset, from, to)?;
            low = low.max(clip_low);
            high = high.min(clip_high);
            if low >= high {
                return None;
            }
        }
    }
    let first = floor_to_pixel(low - 0.5, end_x).max(start_x);
    let last = ceil_to_pixel(high - 0.5 + 1.0, end_x).max(first);
    (first < last).then_some((first, last))
}

/// Solves `low <= slope * x + offset < high` for a continuous x interval.
fn linear_band(slope: f32, offset: f32, low: f32, high: f32) -> Option<(f32, f32)> {
    if slope == 0.0 {
        return (offset >= low && offset < high).then_some((f32::NEG_INFINITY, f32::INFINITY));
    }
    let first = (low - offset) / slope;
    let second = (high - offset) / slope;
    Some(if slope > 0.0 {
        (first, second)
    } else {
        (second, first)
    })
}

fn first_true(mut low: u32, mut high: u32, predicate: impl Fn(u32) -> bool) -> u32 {
    while low < high {
        let middle = low + (high - low) / 2;
        if predicate(middle) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    low
}

#[allow(clippy::too_many_arguments)]
fn rounded_rect_seed(
    rect: Rect,
    radius: f32,
    inverse: Affine,
    y: u32,
    start_x: u32,
    end_x: u32,
    scale_factor: f32,
) -> Option<u32> {
    if start_x >= end_x {
        return None;
    }
    let radius = radius.min(rect.size.width().min(rect.size.height()) * 0.5);
    let left = rect.origin.x + radius;
    let right = rect.right() - radius;
    let top = rect.origin.y + radius;
    let bottom = rect.bottom() - radius;
    let row_y = (as_logical_coordinate(y) + 0.5) / scale_factor;
    let x_scale = scale_factor.recip();
    let local_x = (inverse.xx * x_scale, inverse.yx * row_y + inverse.dx);
    let local_y = (inverse.xy * x_scale, inverse.yy * row_y + inverse.dy);
    let lower = as_logical_coordinate(start_x) + 0.5;
    let upper = as_logical_coordinate(end_x.saturating_sub(1)) + 0.5;
    let mut boundaries = [lower, upper, lower, upper, lower, upper];
    let mut boundary_count = 2;
    for (slope, offset, first, second) in [
        (local_x.0, local_x.1, left, right),
        (local_y.0, local_y.1, top, bottom),
    ] {
        if slope != 0.0 {
            for boundary in [first, second] {
                let position = (boundary - offset) / slope;
                if position > lower && position < upper {
                    boundaries[boundary_count] = position;
                    boundary_count += 1;
                }
            }
        }
    }
    boundaries[..boundary_count].sort_by(f32::total_cmp);
    let mut candidates = [lower; 11];
    let mut candidate_count = 0;
    for boundary in &boundaries[..boundary_count] {
        candidates[candidate_count] = *boundary;
        candidate_count += 1;
    }
    for window in boundaries[..boundary_count].windows(2) {
        let start = window[0];
        let end = window[1];
        let middle = (start + end) * 0.5;
        let (x_active, x_boundary) =
            active_distance_term(local_x.0 * middle + local_x.1, left, right);
        let (y_active, y_boundary) =
            active_distance_term(local_y.0 * middle + local_y.1, top, bottom);
        let coefficient = if x_active { local_x.0 * local_x.0 } else { 0.0 }
            + if y_active { local_y.0 * local_y.0 } else { 0.0 };
        let linear = if x_active {
            local_x.0 * (local_x.1 - x_boundary)
        } else {
            0.0
        } + if y_active {
            local_y.0 * (local_y.1 - y_boundary)
        } else {
            0.0
        };
        if coefficient > 0.0 {
            candidates[candidate_count] = (-linear / coefficient).clamp(start, end);
            candidate_count += 1;
        }
    }
    for candidate in &candidates[..candidate_count] {
        let nearest = (candidate - 0.5).round();
        for offset in [-1.0, 0.0, 1.0] {
            let pixel = nearest + offset;
            if pixel >= as_logical_coordinate(start_x) && pixel < as_logical_coordinate(end_x) {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let pixel = pixel as u32;
                let point = logical_pixel_center(pixel, y, scale_factor);
                if contains_rounded_rect(rect, radius, inverse.transform_point(point)) {
                    return Some(pixel);
                }
            }
        }
    }
    None
}

fn active_distance_term(value: f32, low: f32, high: f32) -> (bool, f32) {
    if value < low {
        (true, low)
    } else if value > high {
        (true, high)
    } else {
        (false, value)
    }
}

fn fill_row(target: &mut PixelBuffer, y: u32, start_x: u32, end_x: u32, color: Color) {
    let width = target.width();
    let start = usize::try_from(start_x.min(width)).unwrap_or(usize::MAX);
    let end = usize::try_from(end_x.min(width)).unwrap_or(usize::MAX);
    if start >= end {
        return;
    }
    let Some(row) = target.row_mut(y) else {
        return;
    };
    let Some(span) = row.get_mut(start..end) else {
        return;
    };
    if color.alpha.is_finite() && color.alpha >= 1.0 {
        // Source-over with an opaque source ignores the destination entirely.
        let pixel = Pixel::rgba(to_u8(color.red), to_u8(color.green), to_u8(color.blue), 255);
        span.fill(pixel);
    } else {
        let source = SourceOver::new(color);
        for destination in span {
            *destination = source.apply(*destination);
        }
    }
}

fn physical_bounds(
    rect: Rect,
    transform: Affine,
    target: &PixelBuffer,
    scale_factor: f32,
) -> (u32, u32, u32, u32) {
    let corners = [
        rect.origin,
        Point::new(rect.right(), rect.origin.y),
        Point::new(rect.origin.x, rect.bottom()),
        Point::new(rect.right(), rect.bottom()),
    ];
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for corner in corners {
        let point = transform.transform_point(corner);
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    (
        floor_to_pixel(min_x * scale_factor, target.width()),
        floor_to_pixel(min_y * scale_factor, target.height()),
        ceil_to_pixel(max_x * scale_factor, target.width()),
        ceil_to_pixel(max_y * scale_factor, target.height()),
    )
}

fn logical_pixel_center(x: u32, y: u32, scale_factor: f32) -> Point {
    Point::new(
        (as_logical_coordinate(x) + 0.5) / scale_factor,
        (as_logical_coordinate(y) + 0.5) / scale_factor,
    )
}

fn blend_pixel(target: &mut PixelBuffer, x: u32, y: u32, color: Color) {
    if let Some(pixel) = target.get_mut(x, y) {
        *pixel = blend(*pixel, color);
    }
}

fn blend_pixel_source(target: &mut PixelBuffer, x: u32, y: u32, source: SourceOver) {
    if let Some(pixel) = target.get_mut(x, y) {
        *pixel = source.apply(*pixel);
    }
}

fn blend_pixel_rgba8(target: &mut PixelBuffer, x: u32, y: u32, rgba: [u8; 4]) {
    if let Some(pixel) = target.get_mut(x, y) {
        if pixel.alpha == 255 {
            *pixel = Pixel::rgba(
                blend_opaque_channel(rgba[0], rgba[3], pixel.red),
                blend_opaque_channel(rgba[1], rgba[3], pixel.green),
                blend_opaque_channel(rgba[2], rgba[3], pixel.blue),
                255,
            );
        } else {
            *pixel = blend(*pixel, Color::rgba8(rgba[0], rgba[1], rgba[2], rgba[3]));
        }
    }
}

fn blend(destination: Pixel, source: Color) -> Pixel {
    SourceOver::new(source).apply(destination)
}

/// Source-over terms that stay constant for one draw color.
#[derive(Clone, Copy)]
struct SourceOver {
    alpha: f32,
    inverse_alpha: f32,
    red: f32,
    green: f32,
    blue: f32,
    alpha_byte: u8,
    red_byte: u8,
    green_byte: u8,
    blue_byte: u8,
}

impl SourceOver {
    fn new(source: Color) -> Self {
        Self::from_components(
            component(source.red),
            component(source.green),
            component(source.blue),
            component(source.alpha),
        )
    }

    /// Builds terms from components that are already finite and clamped.
    fn from_components(red: f32, green: f32, blue: f32, alpha: f32) -> Self {
        Self {
            alpha,
            inverse_alpha: 1.0 - alpha,
            red: red * alpha,
            green: green * alpha,
            blue: blue * alpha,
            alpha_byte: to_u8(alpha),
            red_byte: to_u8(red),
            green_byte: to_u8(green),
            blue_byte: to_u8(blue),
        }
    }

    fn apply(self, destination: Pixel) -> Pixel {
        // The UI framebuffer is opaque almost everywhere. With a fully opaque
        // destination the output alpha is 1 and source-over needs no
        // unpremultiply and no division at all.
        if destination.alpha == 255 {
            return Pixel::rgba(
                blend_opaque_channel(self.red_byte, self.alpha_byte, destination.red),
                blend_opaque_channel(self.green_byte, self.alpha_byte, destination.green),
                blend_opaque_channel(self.blue_byte, self.alpha_byte, destination.blue),
                255,
            );
        }

        // Keep the reference float path bit-identical: divide rather than
        // multiply by a reciprocal, and apply the destination scale in the
        // same order as the original blender.
        let destination_alpha = f32::from(destination.alpha) / 255.0;
        let output_alpha = self.alpha + destination_alpha * self.inverse_alpha;

        let destination_red = f32::from(destination.red) / 255.0 * destination_alpha;
        let destination_green = f32::from(destination.green) / 255.0 * destination_alpha;
        let destination_blue = f32::from(destination.blue) / 255.0 * destination_alpha;

        let (red, green, blue) = if output_alpha == 0.0 {
            (0.0, 0.0, 0.0)
        } else {
            (
                (self.red + destination_red * self.inverse_alpha) / output_alpha,
                (self.green + destination_green * self.inverse_alpha) / output_alpha,
                (self.blue + destination_blue * self.inverse_alpha) / output_alpha,
            )
        };
        Pixel::rgba(to_u8(red), to_u8(green), to_u8(blue), to_u8(output_alpha))
    }
}

/// Source terms shared by every covered pixel in one text layout.
#[derive(Clone, Copy)]
struct GlyphSource {
    red: f32,
    green: f32,
    blue: f32,
    alpha: f32,
    red_byte: u8,
    green_byte: u8,
    blue_byte: u8,
}

impl GlyphSource {
    /// Builds terms from finite, clamped color components.
    fn new(red: f32, green: f32, blue: f32, alpha: f32) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
            red_byte: to_u8_clamped(red),
            green_byte: to_u8_clamped(green),
            blue_byte: to_u8_clamped(blue),
        }
    }

    fn apply(self, target: &mut PixelBuffer, x: u32, y: u32, coverage: f32) {
        let Some(destination) = target.get_mut(x, y) else {
            return;
        };
        let alpha = self.alpha * coverage;
        let alpha_byte = to_u8_clamped(alpha);
        if destination.alpha == 255 {
            *destination = Pixel::rgba(
                blend_opaque_channel(self.red_byte, alpha_byte, destination.red),
                blend_opaque_channel(self.green_byte, alpha_byte, destination.green),
                blend_opaque_channel(self.blue_byte, alpha_byte, destination.blue),
                255,
            );
            return;
        }

        let inverse_alpha = 1.0 - alpha;
        let destination_alpha = f32::from(destination.alpha) / 255.0;
        let output_alpha = alpha + destination_alpha * inverse_alpha;
        let destination_red = f32::from(destination.red) / 255.0 * destination_alpha;
        let destination_green = f32::from(destination.green) / 255.0 * destination_alpha;
        let destination_blue = f32::from(destination.blue) / 255.0 * destination_alpha;
        let (red, green, blue) = if output_alpha == 0.0 {
            (0.0, 0.0, 0.0)
        } else {
            (
                (self.red * alpha + destination_red * inverse_alpha) / output_alpha,
                (self.green * alpha + destination_green * inverse_alpha) / output_alpha,
                (self.blue * alpha + destination_blue * inverse_alpha) / output_alpha,
            )
        };
        *destination = Pixel::rgba(to_u8(red), to_u8(green), to_u8(blue), to_u8(output_alpha));
    }
}

fn blend_opaque_channel(source: u8, source_alpha: u8, destination: u8) -> u8 {
    let source_alpha = u32::from(source_alpha);
    let inverse_alpha = 255 - source_alpha;
    let scaled = u32::from(source) * source_alpha + u32::from(destination) * inverse_alpha + 127;
    #[allow(clippy::cast_possible_truncation)]
    {
        (scaled / 255) as u8
    }
}

fn component(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn to_u8(value: f32) -> u8 {
    // Every input is non-negative after `component`, so truncating
    // `scaled + 0.5` matches `round()` without a libm call on baseline x86-64.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        (component(value) * 255.0 + 0.5) as u8
    }
}

fn to_u8_clamped(value: f32) -> u8 {
    // Callers have already validated that `value` is finite and within 0..=1.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        (value * 255.0 + 0.5) as u8
    }
}

fn as_logical_coordinate(value: u32) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    {
        value as f32
    }
}

fn as_logical_coordinate_usize(value: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    {
        value as f32
    }
}

fn floor_to_pixel(coordinate: f32, limit: u32) -> u32 {
    coordinate_to_pixel(coordinate.floor(), limit)
}

fn ceil_to_pixel(coordinate: f32, limit: u32) -> u32 {
    coordinate_to_pixel(coordinate.ceil(), limit)
}

fn coordinate_to_pixel(coordinate: f32, limit: u32) -> u32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        coordinate.clamp(0.0, as_logical_coordinate(limit)) as u32
    }
}

fn coordinate_to_usize(coordinate: f32) -> usize {
    if coordinate.is_sign_negative() || !coordinate.is_finite() {
        return usize::MAX;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        coordinate.floor() as usize
    }
}

fn image_coordinate(position: f32, length: u32) -> usize {
    let coordinate = (position * as_logical_coordinate(length)).floor();
    let coordinate = coordinate.clamp(0.0, as_logical_coordinate(length.saturating_sub(1)));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        coordinate as usize
    }
}

#[cfg(test)]
mod tests {
    use drawen_core::{Affine, Color, DiagnosticSeverity, Point, Rect, Size};
    use drawen_render::{
        DisplayCommand, DisplayList, FontdueTextShaper, PaintContext, RasterImage, TextStyle,
        TextureId,
    };

    use super::{Pixel, PixelBuffer, RenderError, SoftwareRenderer};

    /// Reference renderer for the randomized fill test. It evaluates every
    /// pixel center independently instead of using the scanline binary search,
    /// so it has no monotonicity assumption to violate.
    fn render_reference(
        display_list: &DisplayList,
        target: &mut PixelBuffer,
        scale_factor: f32,
    ) -> Result<(), RenderError> {
        let mut state = super::RenderState::default();
        let mut saved_states = Vec::new();
        for command in display_list.commands() {
            match command {
                DisplayCommand::Save => saved_states.push(state.clone()),
                DisplayCommand::Restore => {
                    state = saved_states.pop().ok_or(RenderError::UnmatchedRestore)?;
                }
                DisplayCommand::PopClip => {
                    state.clips.pop().ok_or(RenderError::UnmatchedClipPop)?;
                }
                DisplayCommand::Transform { transform } => {
                    state.transform = state.transform.then(*transform);
                }
                DisplayCommand::PushClip { rect } => state.clips.push(super::Clip {
                    rect: *rect,
                    inverse: state
                        .transform
                        .inverse()
                        .ok_or(RenderError::SingularTransform)?,
                }),
                DisplayCommand::FillRect { rect, color } => {
                    reference_fill(target, &state, scale_factor, *rect, 0.0, *color);
                }
                DisplayCommand::FillRoundedRect {
                    rect,
                    radius,
                    color,
                } => {
                    reference_fill(target, &state, scale_factor, *rect, *radius, *color);
                }
                _ => panic!("reference renderer only supports the fill commands used by this test"),
            }
        }
        if saved_states.is_empty() {
            Ok(())
        } else {
            Err(RenderError::UnmatchedSave)
        }
    }

    fn reference_fill(
        target: &mut PixelBuffer,
        state: &super::RenderState,
        scale_factor: f32,
        rect: Rect,
        radius: f32,
        color: Color,
    ) {
        let inverse = state
            .transform
            .inverse()
            .expect("test transforms are invertible");
        let (start_x, start_y, end_x, end_y) =
            super::physical_bounds(rect, state.transform, target, scale_factor);
        for y in start_y..end_y {
            for x in start_x..end_x {
                let point = super::logical_pixel_center(x, y, scale_factor);
                if !state.clips.is_empty() && !super::is_visible(point, state) {
                    continue;
                }
                if super::contains_rounded_rect(rect, radius, inverse.transform_point(point)) {
                    super::blend_pixel(target, x, y, color);
                }
            }
        }
    }

    /// Deterministic xorshift so the test is reproducible without a proptest
    /// dependency. A single fixed seed covers all cases.
    struct XorShift(u64);

    impl XorShift {
        fn new(seed: u64) -> Self {
            Self(seed | 1)
        }

        fn next_u32(&mut self) -> u32 {
            let mut value = self.0;
            value ^= value << 13;
            value ^= value >> 7;
            value ^= value << 17;
            self.0 = value;
            #[allow(clippy::cast_possible_truncation)]
            {
                (value >> 32) as u32
            }
        }

        fn unit(&mut self) -> f32 {
            #[allow(clippy::cast_precision_loss)]
            {
                (self.next_u32() >> 16) as f32 / 65_535.0
            }
        }

        fn range(&mut self, low: f32, high: f32) -> f32 {
            low + (high - low) * self.unit()
        }

        fn below(&mut self, limit: u32) -> u32 {
            self.next_u32() % limit
        }

        fn byte(&mut self) -> u8 {
            #[allow(clippy::cast_possible_truncation)]
            {
                (self.next_u32() & 0xff) as u8
            }
        }

        fn signed_scale(&mut self) -> f32 {
            let magnitude = self.range(0.5, 2.0);
            if self.below(2) == 0 {
                -magnitude
            } else {
                magnitude
            }
        }
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect::new(
            Point::new(x, y),
            Size::new(width, height).expect("valid test size"),
        )
    }

    #[test]
    fn convex_fill_fast_path_matches_reference_for_randomized_geometry() {
        let mut random = XorShift::new(0x5EED_C0DE_D15A_1A7E);
        for iteration in 0..500 {
            let mut list = DisplayList::new();
            let transform = Affine::scale(random.signed_scale(), random.signed_scale()).then(
                Affine::translate(random.range(-12.0, 12.0), random.range(-12.0, 12.0)),
            );
            let outer_clip = rect(
                random.range(-8.0, 8.0),
                random.range(-8.0, 8.0),
                random.range(2.0, 18.0),
                random.range(2.0, 18.0),
            );
            let inner_clip = rect(
                random.range(-8.0, 8.0),
                random.range(-8.0, 8.0),
                random.range(2.0, 18.0),
                random.range(2.0, 18.0),
            );
            PaintContext::new(&mut list).with_transform(transform, |paint| {
                paint.with_clip(outer_clip, |paint| {
                    paint.with_clip(inner_clip, |paint| {
                        for _ in 0..=random.below(6) {
                            let width = random.range(0.1, 16.0);
                            let height = random.range(0.1, 16.0);
                            let shape = rect(
                                random.range(-12.0, 12.0),
                                random.range(-12.0, 12.0),
                                width,
                                height,
                            );
                            let color = Color::rgba8(
                                random.byte(),
                                random.byte(),
                                random.byte(),
                                random.byte(),
                            );
                            if random.below(2) == 0 {
                                paint.fill_rect(shape, color);
                            } else {
                                paint.fill_rounded_rect(
                                    shape,
                                    random.range(0.0, width.min(height) * 0.5),
                                    color,
                                );
                            }
                        }
                    });
                });
            });
            let scale_factor = random.range(0.5, 2.0);
            let mut actual = PixelBuffer::new(32, 24).expect("small test buffer");
            let mut expected = PixelBuffer::new(32, 24).expect("small test buffer");

            SoftwareRenderer
                .render_with_scale_factor(&list, &mut actual, scale_factor)
                .expect("randomized display list is valid");
            render_reference(&list, &mut expected, scale_factor)
                .expect("randomized display list is valid");

            assert_eq!(
                actual.pixels(),
                expected.pixels(),
                "randomized iteration {iteration}"
            );
        }
    }

    #[test]
    fn blends_translucent_fill_over_opaque_destination_with_integer_lerp() {
        let mut list = DisplayList::new();
        let mut paint = PaintContext::new(&mut list);
        paint.fill_rect(rect(0.0, 0.0, 1.0, 1.0), Color::WHITE);
        paint.fill_rect(rect(0.0, 0.0, 1.0, 1.0), Color::rgba8(32, 96, 160, 128));
        let mut pixels = PixelBuffer::new(1, 1).expect("small test buffer");

        SoftwareRenderer
            .render(&list, &mut pixels)
            .expect("valid display list");

        assert_eq!(pixels.get(0, 0), Some(Pixel::rgba(143, 175, 207, 255)));
    }

    #[test]
    fn renders_filled_rectangles_with_scoped_clips() {
        let mut list = DisplayList::new();
        let mut paint = PaintContext::new(&mut list);
        paint.fill_rect(rect(0.0, 0.0, 4.0, 4.0), Color::rgba8(255, 0, 0, 255));
        paint.with_clip(rect(1.0, 1.0, 2.0, 2.0), |context| {
            context.fill_rect(rect(0.0, 0.0, 4.0, 4.0), Color::rgba8(0, 0, 255, 255));
        });
        let mut pixels = PixelBuffer::new(4, 4).expect("small test buffer");

        SoftwareRenderer
            .render(&list, &mut pixels)
            .expect("valid display list");

        assert_eq!(pixels.get(0, 0), Some(Pixel::rgba(255, 0, 0, 255)));
        assert_eq!(pixels.get(1, 1), Some(Pixel::rgba(0, 0, 255, 255)));
        assert_eq!(pixels.get(3, 3), Some(Pixel::rgba(255, 0, 0, 255)));
    }

    #[test]
    fn clip_that_covers_the_row_still_respects_rounded_corners() {
        // The clip covers the whole shape, so every row is filled end to end by
        // the coverage shortcut except at the rounded corners, which must stay
        // transparent.
        let mut list = DisplayList::new();
        PaintContext::new(&mut list).with_clip(rect(0.0, 0.0, 8.0, 8.0), |context| {
            context.fill_rounded_rect(rect(0.0, 0.0, 8.0, 8.0), 2.0, Color::BLACK);
        });
        let mut pixels = PixelBuffer::new(8, 8).expect("small test buffer");

        SoftwareRenderer
            .render(&list, &mut pixels)
            .expect("valid display list");

        assert_eq!(pixels.get(0, 0), Some(Pixel::TRANSPARENT));
        assert_eq!(pixels.get(7, 7), Some(Pixel::TRANSPARENT));
        assert_eq!(pixels.get(7, 0), Some(Pixel::TRANSPARENT));
        assert_eq!(pixels.get(3, 3), Some(Pixel::rgba(0, 0, 0, 255)));
        assert_eq!(pixels.get(0, 3), Some(Pixel::rgba(0, 0, 0, 255)));
    }

    #[test]
    fn clip_that_cuts_the_shape_limits_both_edges() {
        // The clip is inside the shape, so the fill starts and ends at the clip
        // boundary. A clip larger than the shape must still stop at the shape
        // and leave the corners transparent.
        let mut list = DisplayList::new();
        PaintContext::new(&mut list).with_clip(rect(2.6, 3.0, 2.8, 2.0), |context| {
            context.fill_rounded_rect(rect(0.0, 0.0, 8.0, 8.0), 2.0, Color::BLACK);
        });
        let mut clipped = PixelBuffer::new(8, 8).expect("small test buffer");
        SoftwareRenderer
            .render(&list, &mut clipped)
            .expect("valid display list");

        assert_eq!(clipped.get(2, 4), Some(Pixel::TRANSPARENT));
        assert_eq!(clipped.get(3, 4), Some(Pixel::rgba(0, 0, 0, 255)));
        assert_eq!(clipped.get(4, 4), Some(Pixel::rgba(0, 0, 0, 255)));
        assert_eq!(clipped.get(5, 4), Some(Pixel::TRANSPARENT));
        assert_eq!(clipped.get(6, 4), Some(Pixel::TRANSPARENT));

        let mut list = DisplayList::new();
        PaintContext::new(&mut list).with_clip(rect(-4.0, -4.0, 16.0, 16.0), |context| {
            context.fill_rounded_rect(rect(0.0, 0.0, 8.0, 8.0), 2.0, Color::BLACK);
        });
        let mut widened = PixelBuffer::new(8, 8).expect("small test buffer");
        SoftwareRenderer
            .render(&list, &mut widened)
            .expect("valid display list");

        assert_eq!(widened.get(0, 0), Some(Pixel::TRANSPARENT));
        assert_eq!(widened.get(7, 7), Some(Pixel::TRANSPARENT));
        assert_eq!(widened.get(4, 4), Some(Pixel::rgba(0, 0, 0, 255)));
    }

    #[test]
    fn rejects_external_gpu_textures() {
        let mut list = DisplayList::new();
        PaintContext::new(&mut list).draw_texture(TextureId::new(1), rect(0.0, 0.0, 1.0, 1.0));
        let mut pixels = PixelBuffer::new(1, 1).expect("small test buffer");

        assert_eq!(
            SoftwareRenderer.render(&list, &mut pixels),
            Err(RenderError::UnsupportedExternalTexture)
        );
    }

    #[test]
    fn transforms_are_isolated_by_save_restore() {
        let mut list = DisplayList::new();
        let mut paint = PaintContext::new(&mut list);
        paint.with_transform(Affine::translate(2.0, 0.0), |context| {
            context.fill_rect(rect(0.0, 0.0, 1.0, 1.0), Color::BLACK);
        });
        paint.fill_rect(rect(0.0, 0.0, 1.0, 1.0), Color::WHITE);
        let mut pixels = PixelBuffer::new(3, 1).expect("small test buffer");

        SoftwareRenderer
            .render(&list, &mut pixels)
            .expect("valid display list");

        assert_eq!(pixels.get(0, 0), Some(Pixel::rgba(255, 255, 255, 255)));
        assert_eq!(pixels.get(1, 0), Some(Pixel::TRANSPARENT));
        assert_eq!(pixels.get(2, 0), Some(Pixel::rgba(0, 0, 0, 255)));
    }

    #[test]
    fn rotated_fills_match_per_pixel_containment() {
        // The scanline fast path finds one span per row; a rotated fill is still
        // convex, so the span must agree with testing every pixel center.
        let rotation = Affine::rotate(0.5);
        let shape = rect(-2.0, -1.0, 4.0, 2.0);
        let mut list = DisplayList::new();
        PaintContext::new(&mut list).with_transform(rotation, |context| {
            context.fill_rect(shape, Color::WHITE);
        });
        let mut pixels = PixelBuffer::new(20, 20).expect("small test buffer");

        SoftwareRenderer
            .render(&list, &mut pixels)
            .expect("valid display list");

        let inverse = rotation.inverse().expect("rotation is invertible");
        for y in 0..20 {
            for x in 0..20 {
                let point = Point::new(as_test_coordinate(x) + 0.5, as_test_coordinate(y) + 0.5);
                let expected = if shape.contains(inverse.transform_point(point)) {
                    255
                } else {
                    0
                };
                assert_eq!(
                    pixels.get(x, y).expect("pixel is in bounds").alpha,
                    expected,
                    "pixel ({x}, {y})"
                );
            }
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn scanline_fast_path_matches_per_pixel_reference() {
        let mut rng = XorShift::new(0x9E37_79B9_7F4A_7C15);
        let scale_factors = [0.5_f32, 1.0, 1.25, 2.0];
        let alphas = [0_u8, 64, 128, 200, 255];

        for case in 0..256 {
            let width = 1 + rng.below(40);
            let height = 1 + rng.below(40);
            let scale_factor = scale_factors[usize::try_from(rng.below(4)).expect("small index")];
            let logical_width = as_test_coordinate(width) / scale_factor;
            let logical_height = as_test_coordinate(height) / scale_factor;

            let transform = loop {
                let xx = rng.signed_scale();
                let yy = rng.signed_scale();
                let yx = rng.range(-0.9, 0.9);
                let xy = rng.range(-0.9, 0.9);
                if (xx * yy - yx * xy).abs() > 0.3 {
                    break Affine::new(
                        xx,
                        yx,
                        xy,
                        yy,
                        logical_width * 0.5 + rng.range(-4.0, 4.0),
                        logical_height * 0.5 + rng.range(-4.0, 4.0),
                    );
                }
            };

            let shape = rect(
                rng.range(-12.0, 0.0),
                rng.range(-12.0, 0.0),
                rng.range(1.0, 20.0),
                rng.range(1.0, 20.0),
            );
            let rounded = rng.below(2) == 0;
            let radius = if rounded {
                rng.range(0.0, shape.size.width().min(shape.size.height()) * 0.5)
            } else {
                0.0
            };
            let color = Color::rgba(
                rng.unit(),
                rng.unit(),
                rng.unit(),
                f32::from(alphas[usize::try_from(rng.below(5)).expect("small index")]) / 255.0,
            );

            let transform_first = rng.below(2) == 1;
            let clip_count = rng.below(3);
            let mut clips = Vec::new();
            for _ in 0..clip_count {
                let clip = if transform_first {
                    rect(
                        rng.range(
                            shape.origin.x - 6.0,
                            shape.origin.x + shape.size.width() * 0.5,
                        ),
                        rng.range(
                            shape.origin.y - 6.0,
                            shape.origin.y + shape.size.height() * 0.5,
                        ),
                        rng.range(1.0, shape.size.width() + 12.0),
                        rng.range(1.0, shape.size.height() + 12.0),
                    )
                } else {
                    rect(
                        rng.range(-6.0, logical_width * 0.5),
                        rng.range(-6.0, logical_height * 0.5),
                        rng.range(1.0, logical_width + 12.0),
                        rng.range(1.0, logical_height + 12.0),
                    )
                };
                clips.push(clip);
            }

            let mut list = DisplayList::new();
            let mut paint = PaintContext::new(&mut list);
            paint.save();
            if transform_first {
                paint.with_transform(transform, |context| {
                    for clip in &clips {
                        context.push_clip(*clip);
                    }
                    record_fill(context, shape, radius, rounded, color);
                    for _ in &clips {
                        context.pop_clip();
                    }
                });
            } else {
                for clip in &clips {
                    paint.push_clip(*clip);
                }
                paint.with_transform(transform, |context| {
                    record_fill(context, shape, radius, rounded, color);
                });
                for _ in &clips {
                    paint.pop_clip();
                }
            }
            paint.restore();

            let mut fast = PixelBuffer::new(width, height).expect("small test buffer");
            let mut reference = PixelBuffer::new(width, height).expect("small test buffer");
            for y in 0..height {
                for x in 0..width {
                    let alpha = if rng.below(4) == 0 { rng.byte() } else { 255 };
                    let pixel = Pixel::rgba(rng.byte(), rng.byte(), rng.byte(), alpha);
                    *fast.get_mut(x, y).expect("seed pixel is in bounds") = pixel;
                    *reference.get_mut(x, y).expect("seed pixel is in bounds") = pixel;
                }
            }

            SoftwareRenderer
                .render_with_scale_factor(&list, &mut fast, scale_factor)
                .expect("valid fast-path display list");
            render_reference(&list, &mut reference, scale_factor)
                .expect("valid reference display list");

            assert_eq!(
                fast.pixels(),
                reference.pixels(),
                "scanline fast path diverged from the per-pixel reference: \
                 case {case}, {width}x{height}, scale {scale_factor}, \
                 rounded {rounded}, radius {radius}, clips {clip_count}, \
                 transform {transform:?}"
            );
        }
    }

    fn record_fill(
        paint: &mut PaintContext<'_>,
        shape: Rect,
        radius: f32,
        rounded: bool,
        color: Color,
    ) {
        if rounded {
            paint.fill_rounded_rect(shape, radius, color);
        } else {
            paint.fill_rect(shape, color);
        }
    }

    fn as_test_coordinate(value: u32) -> f32 {
        #[allow(clippy::cast_precision_loss)]
        {
            value as f32
        }
    }

    #[test]
    fn rasterizes_rounded_rectangles_and_borders() {
        let mut list = DisplayList::new();
        let mut paint = PaintContext::new(&mut list);
        paint.fill_rounded_rect(rect(0.0, 0.0, 6.0, 6.0), 2.0, Color::WHITE);
        paint.stroke_rect(rect(1.5, 1.5, 3.0, 3.0), 1.0, Color::BLACK);
        let mut pixels = PixelBuffer::new(6, 6).expect("small test buffer");

        SoftwareRenderer
            .render(&list, &mut pixels)
            .expect("valid display list");

        assert_eq!(pixels.get(0, 0), Some(Pixel::TRANSPARENT));
        assert_eq!(pixels.get(1, 1), Some(Pixel::rgba(0, 0, 0, 255)));
        assert_eq!(pixels.get(3, 3), Some(Pixel::rgba(255, 255, 255, 255)));
    }

    #[test]
    fn rounded_rect_containment_accepts_unrepresentable_half_widths() {
        // `origin + radius` can land just past `right - radius` after f32
        // rounding; the clamp must not panic for shapes that are still valid.
        let shape = rect(0.1, 0.1, 0.01, 0.01);
        let radius = 0.005;
        let point = Point::new(0.105, 0.105);

        assert!(super::contains_rounded_rect(shape, radius, point));
    }

    #[test]
    fn renders_soft_shadows_with_a_falloff() {
        let mut list = DisplayList::new();
        PaintContext::new(&mut list).draw_shadow(rect(3.0, 3.0, 4.0, 4.0), 1.0, 3.0, Color::BLACK);
        let mut pixels = PixelBuffer::new(12, 12).expect("small test buffer");

        SoftwareRenderer
            .render(&list, &mut pixels)
            .expect("valid shadow display list");

        let inside = pixels.get(5, 5).expect("pixel inside the shadow");
        assert_eq!(inside.alpha, 255);
        let edge = pixels.get(1, 5).expect("pixel in the falloff");
        assert!(edge.alpha > 0 && edge.alpha < 255);
        assert_eq!(
            pixels.get(11, 11).expect("pixel outside the shadow"),
            Pixel::TRANSPARENT
        );
    }

    #[test]
    fn maps_logical_geometry_to_a_high_dpi_target_once() {
        let mut list = DisplayList::new();
        PaintContext::new(&mut list).fill_rect(rect(0.0, 0.0, 1.0, 1.0), Color::BLACK);
        let mut pixels = PixelBuffer::new(2, 2).expect("small test buffer");

        SoftwareRenderer
            .render_with_scale_factor(&list, &mut pixels, 2.0)
            .expect("valid display list");

        assert!(
            pixels
                .pixels()
                .iter()
                .all(|pixel| *pixel == Pixel::rgba(0, 0, 0, 255))
        );
    }

    #[test]
    fn scales_and_rasterizes_images() {
        let image = RasterImage::new(
            2,
            1,
            vec![
                255, 0, 0, 255, // red
                0, 0, 255, 255, // blue
            ],
        )
        .expect("valid RGBA image");
        let mut list = DisplayList::new();
        PaintContext::new(&mut list).draw_image(image, rect(0.0, 0.0, 4.0, 1.0));
        let mut pixels = PixelBuffer::new(4, 1).expect("small test buffer");

        SoftwareRenderer
            .render(&list, &mut pixels)
            .expect("valid image display list");

        assert_eq!(pixels.get(0, 0), Some(Pixel::rgba(255, 0, 0, 255)));
        assert_eq!(pixels.get(1, 0), Some(Pixel::rgba(255, 0, 0, 255)));
        assert_eq!(pixels.get(2, 0), Some(Pixel::rgba(0, 0, 255, 255)));
        assert_eq!(pixels.get(3, 0), Some(Pixel::rgba(0, 0, 255, 255)));
    }

    #[test]
    fn rasterizes_text_under_the_active_transform_and_clip() {
        let layout = FontdueTextShaper::ubuntu_light().layout(
            "T",
            &TextStyle::new(20.0, Color::BLACK),
            None,
        );
        let mut list = DisplayList::new();
        let mut paint = PaintContext::new(&mut list);
        paint.with_clip(rect(5.0, 0.0, 5.0, 24.0), |context| {
            context.translate(Point::new(4.0, 0.0));
            context.draw_text(layout, Point::new(1.0, 1.0));
        });
        let mut pixels = PixelBuffer::new(24, 24).expect("small test buffer");

        SoftwareRenderer
            .render(&list, &mut pixels)
            .expect("valid text display list");

        assert!(pixels.pixels().iter().any(|pixel| pixel.alpha > 0));
        assert!(pixels.pixels().chunks_exact(24).all(|row| {
            row[..5].iter().all(|pixel| pixel.alpha == 0)
                && row[10..].iter().all(|pixel| pixel.alpha == 0)
        }));
    }

    #[test]
    fn reports_unmatched_restore_as_a_diagnostic() {
        let mut list = DisplayList::new();
        PaintContext::new(&mut list).restore();
        let mut pixels = PixelBuffer::new(1, 1).expect("small test buffer");
        let mut diagnostics = Vec::new();

        assert_eq!(
            SoftwareRenderer.render_with_diagnostics(&list, &mut pixels, &mut diagnostics),
            Err(RenderError::UnmatchedRestore)
        );
        assert_eq!(diagnostics[0].severity(), DiagnosticSeverity::Error);
        assert_eq!(diagnostics[0].component(), "drawen-software");
    }
}
