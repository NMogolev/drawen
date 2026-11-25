//! Plain-data snapshots of the semantic tree, shaped for language models.
//!
//! A [`Snapshot`] mirrors [`SemanticTree`] as owned, `Send` data: small integer
//! ids, `snake_case` role names, readable state and action words, and rounded
//! bounds. It renders either as compact JSON (feature `json`) or as an indented
//! text outline that costs few tokens in a model prompt.

use std::collections::HashMap;
use std::fmt::Write as _;

use drawen_core::WidgetId;
use drawen_ui::{
    AccessibilityActions, AccessibilityRole, AccessibilityState, SemanticNode, SemanticTree,
};

/// One semantic node in a [`Snapshot`].
///
/// `id` is the widget's arena index, printed as `#id` in the text outline.
/// Indices are stable for the lifetime of a widget; commands resolve them
/// against the current tree, so a stale index fails instead of hitting a
/// recycled slot.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
pub struct SnapshotNode {
    /// Arena index of the widget.
    pub id: u32,
    /// Parent arena index, absent for the root.
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "Option::is_none"))]
    pub parent: Option<u32>,
    /// Child arena indices in tree order.
    #[cfg_attr(
        feature = "json",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub children: Vec<u32>,
    /// `snake_case` semantic role, such as `button` or `text_field`.
    pub role: String,
    /// Accessible name, derived from child text when not set explicitly.
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "Option::is_none"))]
    pub label: Option<String>,
    /// Longer description or help text.
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "Option::is_none"))]
    pub description: Option<String>,
    /// Textual value of a text field, combo box, or similar control.
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "Option::is_none"))]
    pub value: Option<String>,
    /// Numeric value of a slider or progress indicator.
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "Option::is_none"))]
    pub numeric_value: Option<f64>,
    /// Lower bound for [`Self::numeric_value`].
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "Option::is_none"))]
    pub min_numeric_value: Option<f64>,
    /// Upper bound for [`Self::numeric_value`].
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "Option::is_none"))]
    pub max_numeric_value: Option<f64>,
    /// Readable state words, such as `focused`, `checked`, or `disabled`.
    #[cfg_attr(
        feature = "json",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub states: Vec<String>,
    /// Supported action words, such as `click`, `toggle`, or `set_value`.
    #[cfg_attr(
        feature = "json",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub actions: Vec<String>,
    /// Absolute bounds in logical pixels: `[x, y, width, height]`.
    pub bounds: [f32; 4],
}

/// An owned, thread-safe snapshot of the laid-out semantic tree.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
pub struct Snapshot {
    /// Arena index of the focused widget, if any.
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "Option::is_none"))]
    pub focus: Option<u32>,
    /// Every semantic node in depth-first tree order.
    pub nodes: Vec<SnapshotNode>,
}

impl Snapshot {
    /// Captures a semantic tree as plain data.
    #[must_use]
    pub fn from_tree(tree: &SemanticTree) -> Self {
        Self {
            focus: tree.focused().map(WidgetId::index),
            nodes: tree.nodes().iter().map(SnapshotNode::from_node).collect(),
        }
    }

    /// Looks up a node by arena index.
    #[must_use]
    pub fn node(&self, id: u32) -> Option<&SnapshotNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// Renders the token-efficient text outline.
    ///
    /// ```text
    /// focus=#3
    /// #0 window "Settings" @(0,0,640,480)
    ///   #1 group "Profile" @(8,8,624,200)
    ///     #2 label "Name" @(16,16,40,20)
    ///     #3 text_field "Name" value="Ada" [focus,set_value] @(16,40,200,28)
    ///     #4 button "Save" [focus,click] @(16,80,80,28)
    /// ```
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        match self.focus {
            Some(id) => {
                let _ = writeln!(out, "focus=#{id}");
            }
            None => out.push_str("focus=none\n"),
        }
        let mut depths: HashMap<u32, usize> = HashMap::new();
        for node in &self.nodes {
            let depth = node
                .parent
                .and_then(|parent| depths.get(&parent).copied())
                .map_or(0, |parent_depth| parent_depth + 1);
            depths.insert(node.id, depth);
            for _ in 0..depth {
                out.push_str("  ");
            }
            let _ = write!(out, "#{} {}", node.id, node.role);
            if let Some(label) = &node.label {
                let _ = write!(out, " {label:?}");
            }
            if let Some(value) = &node.value {
                let _ = write!(out, " value={value:?}");
            }
            if let Some(value) = node.numeric_value {
                let _ = write!(out, " value={value}");
                if let Some(min) = node.min_numeric_value {
                    let _ = write!(out, " min={min}");
                }
                if let Some(max) = node.max_numeric_value {
                    let _ = write!(out, " max={max}");
                }
            }
            for state in &node.states {
                let _ = write!(out, " {state}");
            }
            if !node.actions.is_empty() {
                let _ = write!(out, " [{}]", node.actions.join(","));
            }
            let [x, y, width, height] = node.bounds;
            let _ = writeln!(
                out,
                " @({},{},{},{})",
                x.round(),
                y.round(),
                width.round(),
                height.round()
            );
        }
        out
    }
}

impl SnapshotNode {
    fn from_node(node: &SemanticNode) -> Self {
        Self {
            id: node.id().index(),
            parent: node.parent().map(WidgetId::index),
            children: node.children().iter().map(|id| id.index()).collect(),
            role: role_name(node.role()).to_owned(),
            label: node.label().map(str::to_owned),
            description: node.description().map(str::to_owned),
            value: node.value().map(str::to_owned),
            numeric_value: node.numeric_value(),
            min_numeric_value: node.min_numeric_value(),
            max_numeric_value: node.max_numeric_value(),
            states: state_names(node.states()),
            actions: action_names(node.actions()),
            bounds: [
                node.bounds().origin.x,
                node.bounds().origin.y,
                node.bounds().size.width(),
                node.bounds().size.height(),
            ],
        }
    }
}

/// Returns the `snake_case` name of a semantic role.
#[must_use]
pub fn role_name(role: AccessibilityRole) -> &'static str {
    match role {
        AccessibilityRole::GenericContainer => "generic_container",
        AccessibilityRole::Application => "application",
        AccessibilityRole::Window => "window",
        AccessibilityRole::Dialog => "dialog",
        AccessibilityRole::Pane => "pane",
        AccessibilityRole::Group => "group",
        AccessibilityRole::Toolbar => "toolbar",
        AccessibilityRole::MenuBar => "menu_bar",
        AccessibilityRole::Menu => "menu",
        AccessibilityRole::MenuItem => "menu_item",
        AccessibilityRole::Button => "button",
        AccessibilityRole::CheckBox => "checkbox",
        AccessibilityRole::RadioButton => "radio_button",
        AccessibilityRole::Switch => "switch",
        AccessibilityRole::TextField => "text_field",
        AccessibilityRole::MultilineTextField => "multiline_text_field",
        AccessibilityRole::Label => "label",
        AccessibilityRole::Image => "image",
        AccessibilityRole::List => "list",
        AccessibilityRole::ListBox => "list_box",
        AccessibilityRole::ComboBox => "combo_box",
        AccessibilityRole::ListItem => "list_item",
        AccessibilityRole::Table => "table",
        AccessibilityRole::Row => "row",
        AccessibilityRole::Cell => "cell",
        AccessibilityRole::TabList => "tab_list",
        AccessibilityRole::Tab => "tab",
        AccessibilityRole::TabPanel => "tab_panel",
        AccessibilityRole::Slider => "slider",
        AccessibilityRole::ProgressIndicator => "progress_indicator",
        AccessibilityRole::ScrollBar => "scroll_bar",
        AccessibilityRole::ScrollView => "scroll_view",
        AccessibilityRole::Splitter => "splitter",
        AccessibilityRole::Heading => "heading",
        AccessibilityRole::Link => "link",
        AccessibilityRole::Tooltip => "tooltip",
        AccessibilityRole::Document => "document",
        _ => "unknown",
    }
}

const STATE_NAMES: &[(AccessibilityState, &str)] = &[
    (AccessibilityState::FOCUSED, "focused"),
    (AccessibilityState::DISABLED, "disabled"),
    (AccessibilityState::SELECTED, "selected"),
    (AccessibilityState::CHECKED, "checked"),
    (AccessibilityState::MIXED, "mixed"),
    (AccessibilityState::EXPANDED, "expanded"),
    (AccessibilityState::READ_ONLY, "read_only"),
    (AccessibilityState::REQUIRED, "required"),
    (AccessibilityState::INVALID, "invalid"),
    (AccessibilityState::BUSY, "busy"),
    (AccessibilityState::MULTILINE, "multiline"),
    (AccessibilityState::PASSWORD, "password"),
    (AccessibilityState::MODAL, "modal"),
    (AccessibilityState::MULTISELECTABLE, "multiselectable"),
];

const ACTION_NAMES: &[(AccessibilityActions, &str)] = &[
    (AccessibilityActions::FOCUS, "focus"),
    (AccessibilityActions::CLICK, "click"),
    (AccessibilityActions::TOGGLE, "toggle"),
    (AccessibilityActions::SET_VALUE, "set_value"),
    (AccessibilityActions::INCREMENT, "increment"),
    (AccessibilityActions::DECREMENT, "decrement"),
    (AccessibilityActions::EXPAND, "expand"),
    (AccessibilityActions::COLLAPSE, "collapse"),
    (AccessibilityActions::SCROLL, "scroll"),
    (AccessibilityActions::SELECT, "select"),
];

fn state_names(states: AccessibilityState) -> Vec<String> {
    STATE_NAMES
        .iter()
        .filter(|(flag, _)| states.contains(*flag))
        .map(|(_, name)| (*name).to_owned())
        .collect()
}

fn action_names(actions: AccessibilityActions) -> Vec<String> {
    ACTION_NAMES
        .iter()
        .filter(|(flag, _)| actions.contains(*flag))
        .map(|(_, name)| (*name).to_owned())
        .collect()
}

/// Normalizes a role or label fragment for forgiving comparisons: lowercase
/// alphanumerics only, so `TextField`, `text_field`, and `textfield` all match.
pub(crate) fn normalize(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use drawen::{Button, Color, Constraints, Size, Text, UiRuntime, render::TextStyle};

    use super::Snapshot;

    #[test]
    fn text_outline_lists_roles_labels_and_actions() {
        let mut runtime = UiRuntime::new(drawen::Box::new());
        let button = runtime
            .append_child(runtime.root(), Button::new())
            .expect("root exists");
        runtime
            .append_child(
                button,
                Text::styled("Save", TextStyle::new(14.0, Color::BLACK)),
            )
            .expect("button exists");
        runtime
            .layout(
                Constraints::tight(Size::new(320.0, 200.0).expect("valid size")).expect("valid"),
            )
            .expect("layout succeeds");

        let snapshot = Snapshot::from_tree(&runtime.semantic_tree());
        let text = snapshot.to_text();
        assert!(text.starts_with("focus=none\n"), "{text}");
        assert!(text.contains("button \"Save\""), "{text}");
        assert!(text.contains("click"), "{text}");
        assert!(text.contains("@("), "{text}");
    }
}
