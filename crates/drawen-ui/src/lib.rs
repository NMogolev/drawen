//! Retained widget tree, layout lifecycle, routing, and invalidation for Drawen.
//!
//! [`UiRuntime`] owns the widget nodes, resolves the style of each node, drives
//! measure/layout, routes input through capture, target, and bubble phases, and
//! retains a [`DisplayList`](drawen_render::DisplayList) until something
//! invalidates it. [`Widget`] implementations own only their state and
//! behavior; parent links, bounds, focus, hover, pointer capture, and dirty
//! flags live in the runtime.
//!
//! [`UiEnvironment`] carries the theme, logical DPI, locale, default font, and
//! text shaper. [`Signal`] and [`Subscription`] provide typed, UI-thread-local
//! notifications without a global event bus; [`Property`] adds a stored value
//! that [`UiRuntime::bind`] copies into widget properties.

mod accessibility;
mod column;
mod command;
mod dss;
mod environment;
mod event;
mod layout;
mod overlay;
mod row;
mod runtime;
mod style;
mod styled;
mod theme;
mod widget;

pub use drawen_render::{
    DEFAULT_DPI, Font, FontDatabase, FontDescriptor, FontFamily, FontMetrics, FontSize, FontStyle,
    FontWeight,
};

pub(crate) use accessibility::SemanticsBuilder;
pub use accessibility::{
    Accessibility, AccessibilityAction, AccessibilityActions, AccessibilityRole,
    AccessibilityState, AccessibilityTextSelection, SemanticNode, SemanticTree,
    accessibility_action_from_accesskit, widget_id_from_accesskit,
};
pub use column::Column;
pub use command::{KeyChord, KeyboardCommand};
pub use drawen_signals::{Binding, Property, PropertyDamage, Signal, Subscription};
pub use dss::{
    ChildPosition, DssError, DssStyle, DssTheme, PseudoClass, Selector, StyleRule, StyleSheet,
    StyleTarget,
};
pub use environment::UiEnvironment;
pub use event::{EventContext, EventPhase, EventStatus, ResizeEdge, WindowCommand};
pub use layout::{ChildLayout, LayoutResult};
pub use overlay::{
    DismissPolicy, OverlayAlign, OverlayOptions, OverlayPlacement, OverlaySide, ResolvedOverlay,
    place_overlay,
};
pub use row::Row;
pub use runtime::{
    DirtyFlags, FrameDiagnostics, LayoutContext, RuntimeStats, UiRuntime, UiRuntimeError,
};
pub use style::{Border, SurfaceStyle};
pub use styled::Styled;
pub use theme::{DarkTheme, LightTheme, SystemAppearance, SystemTheme, Theme};
pub use widget::Widget;
