//! Drawen Style Sheets: a typed, CSS-inspired cascade for retained widgets.
//!
//! The implementation is split into a parser, a selector engine, typed
//! declaration values, the resolved style payload, and the theme adapter so
//! each concern can grow without turning one file into a monolith.

mod error;
mod parser;
mod selector;
mod style;
mod theme;
mod value;

#[cfg(test)]
mod tests;

pub use error::DssError;
pub use selector::{PseudoClass, Selector, StyleTarget};
pub use style::DssStyle;
pub use theme::DssTheme;

use std::str::FromStr;

/// A parsed DSS stylesheet.
///
/// DSS uses a deliberately small CSS-compatible subset for UI styling. The
/// first version supports type, id, class, and state selectors; the `:root`
/// rule; and typed visual properties. It does not yet support descendant
/// selectors, variables, or layout properties whose ownership belongs to a
/// parent widget.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleSheet {
    rules: Vec<StyleRule>,
    /// Rule indices sorted by `(specificity, source order)` once at parse time.
    ///
    /// Keeping this index avoids sorting and allocating on every node style
    /// resolution. The public rule list remains in source order.
    cascade: Vec<usize>,
}

impl StyleSheet {
    /// Parses a stylesheet from UTF-8 DSS source.
    ///
    /// Unknown properties, unsupported selector syntax, and invalid values are
    /// rejected rather than silently ignored. This keeps stylesheet mistakes
    /// visible while DSS is still intentionally small.
    ///
    /// # Errors
    ///
    /// Returns [`DssError`] when `source` is not valid DSS.
    pub fn parse(source: &str) -> Result<Self, DssError> {
        let rules = parser::Parser::new(source)
            .parse_rules()?
            .into_iter()
            .map(|(selector, style)| StyleRule { selector, style })
            .collect();
        let mut stylesheet = Self {
            rules,
            cascade: Vec::new(),
        };
        stylesheet.rebuild_cascade();
        Ok(stylesheet)
    }

    /// Returns the stylesheet rules in source order.
    #[must_use]
    pub fn rules(&self) -> &[StyleRule] {
        &self.rules
    }

    /// Resolves all matching rules for `target` using CSS specificity and
    /// source order.
    ///
    /// The returned style is sparse: a missing value means that no matching
    /// rule supplied it. Widgets can therefore use a local value, the resolved
    /// DSS value, or their semantic theme default in that order.
    #[must_use]
    pub fn resolve(&self, target: &StyleTarget) -> DssStyle {
        let mut resolved = DssStyle::default();
        for &index in &self.cascade {
            let rule = &self.rules[index];
            if rule.selector.matches(target) {
                resolved.apply(&rule.style);
            }
        }
        resolved
    }

    fn rebuild_cascade(&mut self) {
        self.cascade = (0..self.rules.len()).collect();
        self.cascade.sort_by_key(|index| {
            let rule = &self.rules[*index];
            (rule.selector.specificity(), *index)
        });
    }

    /// Resolves the sheet into a [`Theme`](crate::Theme) while preserving
    /// missing values from `fallback`.
    ///
    /// `:root` supplies global semantic values. Control-specific selectors are
    /// resolved per node by the runtime and are intentionally not mapped onto
    /// the semantic theme.
    #[must_use]
    pub fn theme<T: crate::Theme>(&self, fallback: T) -> DssTheme<T> {
        DssTheme::new(fallback, self)
    }
}

impl FromStr for StyleSheet {
    type Err = DssError;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        Self::parse(source)
    }
}

/// A single selector and declaration block from a [`StyleSheet`].
#[derive(Clone, Debug, PartialEq)]
pub struct StyleRule {
    selector: Selector,
    style: DssStyle,
}

impl StyleRule {
    /// Returns the selector that identifies matching targets.
    #[must_use]
    pub const fn selector(&self) -> &Selector {
        &self.selector
    }

    /// Returns the declarations defined by this rule.
    #[must_use]
    pub const fn style(&self) -> &DssStyle {
        &self.style
    }
}
