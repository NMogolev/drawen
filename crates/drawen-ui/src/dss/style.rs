use drawen_core::{Color, Insets};
use drawen_render::{DEFAULT_DPI, FontFamily, FontSize, FontStyle, FontWeight};

use crate::Border;

/// Typed visual values produced by the DSS cascade.
///
/// Every field is optional so a rule can override exactly one property without
/// resetting values supplied by a less specific rule or a semantic theme.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DssStyle {
    /// Surface fill from `background` or `background-color`.
    pub background: Option<Color>,
    /// Text and icon color from `color`.
    pub foreground: Option<Color>,
    /// Focus and selection color from `accent-color`.
    pub accent: Option<Color>,
    /// Surface border from `border`.
    pub border: Option<Border>,
    /// Inner surface inset from `padding`.
    pub padding: Option<Insets>,
    /// Standard sibling spacing from `gap`.
    pub spacing: Option<f32>,
    /// Ordered family preferences from `font-family`.
    pub font_families: Option<Vec<FontFamily>>,
    /// Text size from `font-size`.
    pub font_size: Option<FontSize>,
    /// Text weight from `font-weight`.
    pub font_weight: Option<FontWeight>,
    /// Text slant from `font-style`.
    pub font_style: Option<FontStyle>,
    /// Surface corner radius from `border-radius`.
    pub corner_radius: Option<f32>,
    /// Preferred width from `width`.
    pub width: Option<f32>,
    /// Preferred height from `height`.
    pub height: Option<f32>,
    /// Lower width bound from `min-width`.
    pub min_width: Option<f32>,
    /// Lower height bound from `min-height`.
    pub min_height: Option<f32>,
    /// Upper width bound from `max-width`.
    pub max_width: Option<f32>,
    /// Upper height bound from `max-height`.
    pub max_height: Option<f32>,
}

impl DssStyle {
    pub(super) fn apply(&mut self, style: &Self) {
        if style.background.is_some() {
            self.background = style.background;
        }
        if style.foreground.is_some() {
            self.foreground = style.foreground;
        }
        if style.accent.is_some() {
            self.accent = style.accent;
        }
        if style.border.is_some() {
            self.border = style.border;
        }
        if style.padding.is_some() {
            self.padding = style.padding;
        }
        if style.spacing.is_some() {
            self.spacing = style.spacing;
        }
        if style.font_families.is_some() {
            self.font_families.clone_from(&style.font_families);
        }
        if style.font_size.is_some() {
            self.font_size = style.font_size;
        }
        if style.font_weight.is_some() {
            self.font_weight = style.font_weight;
        }
        if style.font_style.is_some() {
            self.font_style = style.font_style;
        }
        if style.corner_radius.is_some() {
            self.corner_radius = style.corner_radius;
        }
        if style.width.is_some() {
            self.width = style.width;
        }
        if style.height.is_some() {
            self.height = style.height;
        }
        if style.min_width.is_some() {
            self.min_width = style.min_width;
        }
        if style.min_height.is_some() {
            self.min_height = style.min_height;
        }
        if style.max_width.is_some() {
            self.max_width = style.max_width;
        }
        if style.max_height.is_some() {
            self.max_height = style.max_height;
        }
    }

    /// Fills inheritable properties that this style does not define.
    ///
    /// `color`, `font-family`, `font-size`, `font-weight`, `font-style`, and
    /// `accent-color` inherit from a parent node. Surface properties such as
    /// `background`, `border`, `padding`, `gap`, and `border-radius` do not.
    pub fn inherit_from(&mut self, parent: &Self) {
        self.foreground = self.foreground.or(parent.foreground);
        if self.font_families.is_none() {
            self.font_families.clone_from(&parent.font_families);
        }
        self.font_size = self.font_size.or(parent.font_size);
        self.font_weight = self.font_weight.or(parent.font_weight);
        self.font_style = self.font_style.or(parent.font_style);
        self.accent = self.accent.or(parent.accent);
    }

    /// Returns whether no declaration produced a value.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.background.is_none()
            && self.foreground.is_none()
            && self.accent.is_none()
            && self.border.is_none()
            && self.padding.is_none()
            && self.spacing.is_none()
            && self.font_families.is_none()
            && self.font_size.is_none()
            && self.font_weight.is_none()
            && self.font_style.is_none()
            && self.corner_radius.is_none()
            && self.width.is_none()
            && self.height.is_none()
            && self.min_width.is_none()
            && self.min_height.is_none()
            && self.max_width.is_none()
            && self.max_height.is_none()
    }

    /// Returns `background`, or `fallback` when DSS did not supply it.
    #[must_use]
    pub fn background_or(&self, fallback: Color) -> Color {
        self.background.unwrap_or(fallback)
    }

    /// Returns `color`, or `fallback` when DSS did not supply it.
    #[must_use]
    pub fn foreground_or(&self, fallback: Color) -> Color {
        self.foreground.unwrap_or(fallback)
    }

    /// Returns `accent-color`, or `fallback` when DSS did not supply it.
    #[must_use]
    pub fn accent_or(&self, fallback: Color) -> Color {
        self.accent.unwrap_or(fallback)
    }

    /// Returns `border`, or `fallback` when DSS did not supply it.
    #[must_use]
    pub fn border_or(&self, fallback: Option<Border>) -> Option<Border> {
        self.border.or(fallback)
    }

    /// Returns `padding`, or `fallback` when DSS did not supply it.
    #[must_use]
    pub fn padding_or(&self, fallback: Insets) -> Insets {
        self.padding.unwrap_or(fallback)
    }

    /// Returns `gap`, or `fallback` when DSS did not supply it.
    #[must_use]
    pub fn spacing_or(&self, fallback: f32) -> f32 {
        self.spacing.unwrap_or(fallback)
    }

    /// Resolves `font-size` to logical pixels at `dpi`.
    #[must_use]
    pub fn font_size_pixels(&self, dpi: f32) -> Option<f32> {
        self.font_size.map(|size| size.pixels_at(dpi))
    }

    /// Returns `font-size` in logical pixels at [`DEFAULT_DPI`], or `fallback`.
    #[must_use]
    pub fn font_size_or(&self, fallback: f32) -> f32 {
        self.font_size_pixels(DEFAULT_DPI).unwrap_or(fallback)
    }

    /// Returns `font-weight`, or `fallback` when DSS did not supply it.
    #[must_use]
    pub fn font_weight_or(&self, fallback: FontWeight) -> FontWeight {
        self.font_weight.unwrap_or(fallback)
    }

    /// Returns `font-style`, or `fallback` when DSS did not supply it.
    #[must_use]
    pub fn font_style_or(&self, fallback: FontStyle) -> FontStyle {
        self.font_style.unwrap_or(fallback)
    }

    /// Returns `border-radius`, or `fallback` when DSS did not supply it.
    #[must_use]
    pub fn corner_radius_or(&self, fallback: f32) -> f32 {
        self.corner_radius.unwrap_or(fallback)
    }
}
