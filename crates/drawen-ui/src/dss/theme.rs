use std::time::Duration;

use drawen_core::{Color, Insets};
use drawen_render::{DEFAULT_DPI, FontFamily, FontStyle, FontWeight};

use crate::{Theme, dss::StyleSheet, dss::StyleTarget, dss::style::DssStyle};

/// A [`Theme`] whose values are resolved from a [`StyleSheet`].
///
/// The stylesheet remains the source of truth at construction time. Values not
/// supplied by DSS continue to come from `T`, preserving custom themes and the
/// appearance of existing applications.
#[derive(Clone, Debug)]
pub struct DssTheme<T> {
    fallback: T,
    root: DssStyle,
}

impl<T> DssTheme<T> {
    /// Returns the semantic theme used for properties absent from DSS.
    #[must_use]
    pub const fn fallback(&self) -> &T {
        &self.fallback
    }
}

impl<T: Theme> DssTheme<T> {
    /// Resolves `stylesheet` against the standard theme targets.
    #[must_use]
    pub fn new(fallback: T, stylesheet: &StyleSheet) -> Self {
        Self {
            fallback,
            root: stylesheet.resolve(&StyleTarget::root()),
        }
    }
}

impl<T: Theme> Theme for DssTheme<T> {
    fn background(&self) -> Color {
        self.root
            .background
            .unwrap_or_else(|| self.fallback.background())
    }

    fn foreground(&self) -> Color {
        self.root
            .foreground
            .unwrap_or_else(|| self.fallback.foreground())
    }

    fn accent(&self) -> Color {
        self.root.accent.unwrap_or_else(|| self.fallback.accent())
    }

    fn surface(&self) -> Color {
        self.root
            .background
            .unwrap_or_else(|| self.fallback.surface())
    }

    fn raised_surface(&self) -> Color {
        self.fallback.raised_surface()
    }

    fn border(&self) -> Color {
        self.root
            .border
            .map_or_else(|| self.fallback.border(), |border| border.color)
    }

    fn focus_ring(&self) -> Color {
        self.root
            .accent
            .unwrap_or_else(|| self.fallback.focus_ring())
    }

    fn shadow(&self) -> Color {
        self.fallback.shadow()
    }

    fn accent_foreground(&self) -> Color {
        self.fallback.accent_foreground()
    }

    fn selection(&self) -> Color {
        self.root.accent.map_or_else(
            || self.fallback.selection(),
            |accent| accent.with_alpha(0.24),
        )
    }

    fn disabled_foreground(&self) -> Color {
        self.fallback.disabled_foreground()
    }

    fn control_background(&self) -> Color {
        self.fallback.control_background()
    }

    fn control_pressed_background(&self) -> Color {
        self.fallback.control_pressed_background()
    }

    fn control_hover_background(&self) -> Color {
        self.fallback.control_hover_background()
    }

    fn control_padding(&self) -> Insets {
        self.fallback.control_padding()
    }

    fn spacing(&self) -> f32 {
        self.root.spacing.unwrap_or_else(|| self.fallback.spacing())
    }

    fn font_size(&self) -> f32 {
        self.root
            .font_size_pixels(DEFAULT_DPI)
            .unwrap_or_else(|| self.fallback.font_size())
    }

    fn font_family(&self) -> Option<&str> {
        self.root
            .font_families
            .as_ref()
            .and_then(|families| families.first())
            .map(FontFamily::as_str)
            .or_else(|| self.fallback.font_family())
    }

    fn font_weight(&self) -> FontWeight {
        self.root
            .font_weight
            .unwrap_or_else(|| self.fallback.font_weight())
    }

    fn font_style(&self) -> FontStyle {
        self.root
            .font_style
            .unwrap_or_else(|| self.fallback.font_style())
    }

    fn corner_radius(&self) -> f32 {
        self.root
            .corner_radius
            .unwrap_or_else(|| self.fallback.corner_radius())
    }

    fn animation_duration(&self) -> Duration {
        self.fallback.animation_duration()
    }
}
