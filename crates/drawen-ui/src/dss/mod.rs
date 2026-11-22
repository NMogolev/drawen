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
mod variables;

#[cfg(test)]
mod tests;

pub use error::DssError;
pub use selector::{ChildPosition, PseudoClass, Selector, StyleTarget};
pub use style::DssStyle;
pub use theme::DssTheme;

use std::{collections::HashMap, str::FromStr};

use parser::ParsedRule;
use variables::CustomProperties;

/// A parsed DSS stylesheet.
///
/// DSS uses a deliberately small CSS-compatible subset for UI styling. It
/// supports type, id, class, and state selectors; descendant and child
/// combinators; structural pseudo-classes; `:not()`/`:is()`; custom properties
/// with `var()`; `calc()`; `!important`; the `:root` rule; typed visual
/// properties; and size properties that the runtime folds into the constraints
/// it offers a child.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleSheet {
    rules: Vec<StyleRule>,
    /// Rule indices sorted by `(specificity, source order)` once at parse time.
    ///
    /// Keeping this index avoids sorting and allocating on every node style
    /// resolution. The public rule list remains in source order.
    cascade: Vec<usize>,
    /// Important declarations, applied after every normal declaration.
    cascade_important: Vec<usize>,
}

impl StyleSheet {
    /// Parses a stylesheet from UTF-8 DSS source.
    ///
    /// Unknown properties, unsupported selector syntax, and invalid values are
    /// rejected rather than silently ignored. This keeps stylesheet mistakes
    /// visible while DSS remains a typed subset of CSS.
    ///
    /// # Errors
    ///
    /// Returns [`DssError`] when `source` is not valid DSS.
    pub fn parse(source: &str) -> Result<Self, DssError> {
        let parsed = parser::Parser::new(source).parse_rules()?;
        let properties = collect_custom_properties(&parsed);
        let mut rules = Vec::with_capacity(parsed.len());
        for rule in parsed {
            let mut style = DssStyle::default();
            let mut important = DssStyle::default();
            for declaration in rule.declarations {
                if declaration.property.starts_with("--") {
                    if declaration.property.len() == 2 {
                        return Err(DssError::new(
                            declaration.position,
                            "custom properties require a name after `--`",
                        ));
                    }
                    continue;
                }
                let value =
                    variables::substitute(&declaration.value, &properties, declaration.position)?;
                let target = if declaration.important {
                    &mut important
                } else {
                    &mut style
                };
                value::set_property(
                    target,
                    &declaration.property,
                    value.trim(),
                    declaration.position,
                )?;
            }
            rules.push(StyleRule {
                selector: rule.selector,
                style,
                important,
            });
        }
        let mut stylesheet = Self {
            rules,
            cascade: Vec::new(),
            cascade_important: Vec::new(),
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
        // `!important` declarations form a second cascade layer: no normal
        // declaration can override them, regardless of specificity.
        for &index in &self.cascade_important {
            let rule = &self.rules[index];
            if rule.selector.matches(target) {
                resolved.apply(&rule.important);
            }
        }
        resolved
    }

    fn rebuild_cascade(&mut self) {
        let mut normal = Vec::new();
        let mut important = Vec::new();
        for (index, rule) in self.rules.iter().enumerate() {
            if !rule.style.is_empty() {
                normal.push(index);
            }
            if !rule.important.is_empty() {
                important.push(index);
            }
        }
        let sort = |cascade: &mut Vec<usize>, rules: &[StyleRule]| {
            cascade.sort_by_key(|index| {
                let rule = &rules[*index];
                (rule.selector.specificity(), *index)
            });
        };
        sort(&mut normal, &self.rules);
        sort(&mut important, &self.rules);
        self.cascade = normal;
        self.cascade_important = important;
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
    important: DssStyle,
}

impl StyleRule {
    /// Returns the selector that identifies matching targets.
    #[must_use]
    pub const fn selector(&self) -> &Selector {
        &self.selector
    }

    /// Returns the normal declarations defined by this rule.
    #[must_use]
    pub const fn style(&self) -> &DssStyle {
        &self.style
    }

    /// Returns the declarations this rule marked `!important`.
    ///
    /// The runtime applies this layer after every normal declaration, so it is
    /// exposed mainly for tooling that inspects a parsed stylesheet.
    #[must_use]
    pub const fn important_style(&self) -> &DssStyle {
        &self.important
    }
}

/// Collects custom properties into a global token map in source order.
///
/// DSS custom properties are stylesheet tokens, not per-selector values: every
/// definition contributes to the same map and later definitions win. This is a
/// deliberate simplification over browser CSS that keeps the typed resolution
/// path free of per-node variable environments.
fn collect_custom_properties(rules: &[ParsedRule]) -> CustomProperties {
    let mut properties = HashMap::new();
    for rule in rules {
        for declaration in &rule.declarations {
            if declaration.property.starts_with("--") && declaration.property.len() > 2 {
                properties.insert(declaration.property.clone(), declaration.value.clone());
            }
        }
    }
    properties
}
