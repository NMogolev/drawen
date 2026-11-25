//! Plain-data snapshots of the semantic tree, shaped for language models.
//!
//! A [`Snapshot`] mirrors [`SemanticTree`] as owned, `Send` data: small integer
//! ids, `snake_case` role names, readable state and action words, and rounded
//! bounds. It renders either as compact JSON (feature `json`) or as an indented
//! text outline that costs few tokens in a model prompt.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use drawen_core::WidgetId;
use drawen_ui::{
    AccessibilityActions, AccessibilityRole, AccessibilityState, SemanticNode, SemanticTree,
};

/// Default node budget for one observation.
///
/// A virtualized list or a deeply nested form can expose thousands of semantic
/// nodes; an unbounded snapshot would spend the agent's whole context window on
/// the UI. Observations keep the first [`DEFAULT_MAX_NODES`] nodes in tree order
/// and report `total` and `truncated` so the agent can narrow the observation
/// with `{"op":"observe","target":<id>}` instead of reading the root again.
pub const DEFAULT_MAX_NODES: usize = 200;

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
///
/// Observations are bounded: `nodes` holds at most the requested budget, while
/// `total` counts every node in the observed scope and `truncated` says whether
/// the budget cut some off. Ids stay global arena indices even for subtree
/// observations, so commands can target any listed node directly.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
pub struct Snapshot {
    /// Arena index of the focused widget, if any.
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "Option::is_none"))]
    pub focus: Option<u32>,
    /// Arena index of the observation root; the tree root for whole-tree views.
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "Option::is_none"))]
    pub root: Option<u32>,
    /// Node count in the observed scope before the budget was applied.
    pub total: usize,
    /// Whether the node budget omitted part of the observed scope.
    #[cfg_attr(feature = "json", serde(default, skip_serializing_if = "is_false"))]
    pub truncated: bool,
    /// Semantic nodes in depth-first tree order, at most the requested budget.
    pub nodes: Vec<SnapshotNode>,
}

#[cfg(feature = "json")]
fn is_false(value: &bool) -> bool {
    !*value
}

impl Snapshot {
    /// Captures a whole semantic tree as plain data, without a node budget.
    #[must_use]
    pub fn from_tree(tree: &SemanticTree) -> Self {
        Self::capture(tree, None, usize::MAX)
    }

    /// Captures an observation rooted at `root` (or the whole tree when `None`),
    /// keeping at most `max_nodes` nodes in depth-first tree order.
    ///
    /// `total` counts the full scope and `truncated` reports the cut, so an
    /// agent can switch to subtree observations instead of re-reading a large
    /// root. Child links are pruned to the nodes actually included.
    #[must_use]
    pub fn capture(tree: &SemanticTree, root: Option<WidgetId>, max_nodes: usize) -> Self {
        let scope = scope_ids(tree, root);
        let total = scope.len();
        let included: HashSet<u32> = scope.iter().take(max_nodes).map(|id| id.index()).collect();
        let nodes = scope
            .iter()
            .take(max_nodes)
            .filter_map(|id| tree.node(*id))
            .map(|node| {
                let mut snapshot = SnapshotNode::from_node(node);
                snapshot.children.retain(|child| included.contains(child));
                if root == Some(node.id()) {
                    snapshot.parent = None;
                }
                snapshot
            })
            .collect();
        Self {
            focus: tree.focused().map(WidgetId::index),
            root: root
                .or_else(|| tree.nodes().first().map(SemanticNode::id))
                .map(WidgetId::index),
            total,
            truncated: total > included.len(),
            nodes,
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
    /// focus=#3 total=6
    /// #0 window "Settings" @(0,0,640,480)
    ///   #1 group "Profile" @(8,8,624,200)
    ///     #2 label "Name" @(16,16,40,20)
    ///     #3 text_field "Name" value="Ada" [focus,set_value] @(16,40,200,28)
    ///     #4 button "Save" [focus,click] @(16,80,80,28)
    /// ```
    ///
    /// A truncated observation adds the word `truncated` to the header.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        match self.focus {
            Some(id) => {
                let _ = write!(out, "focus=#{id}");
            }
            None => out.push_str("focus=none"),
        }
        let _ = write!(out, " total={}", self.total);
        if self.truncated {
            out.push_str(" truncated");
        }
        out.push('\n');
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

/// Collects the observed scope in depth-first tree order.
fn scope_ids(tree: &SemanticTree, root: Option<WidgetId>) -> Vec<WidgetId> {
    let Some(root) = root else {
        return tree.nodes().iter().map(SemanticNode::id).collect();
    };
    let mut ids = Vec::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        let Some(node) = tree.node(id) else {
            continue;
        };
        ids.push(id);
        stack.extend(node.children().iter().rev().copied());
    }
    ids
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
    use drawen::{Button, Color, Column, Constraints, Size, Text, UiRuntime, render::TextStyle};

    use super::{DEFAULT_MAX_NODES, Snapshot};

    fn button_app() -> UiRuntime {
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
        runtime
    }

    #[test]
    fn text_outline_lists_roles_labels_and_actions() {
        let runtime = button_app();
        let snapshot = Snapshot::from_tree(&runtime.semantic_tree());
        let text = snapshot.to_text();
        assert!(text.starts_with("focus=none total="), "{text}");
        assert!(text.contains("button \"Save\""), "{text}");
        assert!(text.contains("click"), "{text}");
        assert!(text.contains("@("), "{text}");
        assert!(!snapshot.truncated);
    }

    #[test]
    fn a_budget_truncates_and_reports_the_full_total() {
        let mut runtime = UiRuntime::new(drawen::Box::new());
        let column = runtime
            .append_child(runtime.root(), Column::new())
            .expect("root exists");
        for index in 0..30 {
            runtime
                .append_child(
                    column,
                    Text::styled(format!("Row {index}"), TextStyle::new(14.0, Color::BLACK)),
                )
                .expect("column exists");
        }
        runtime
            .layout(
                Constraints::tight(Size::new(320.0, 900.0).expect("valid size")).expect("valid"),
            )
            .expect("layout succeeds");

        let tree = runtime.semantic_tree();
        let full = Snapshot::from_tree(&tree);
        assert_eq!(full.total, full.nodes.len());
        assert!(!full.truncated);

        let cut = Snapshot::capture(&tree, None, 5);
        assert!(cut.truncated);
        assert_eq!(cut.total, full.total);
        assert_eq!(cut.nodes.len(), 5);
        assert!(cut.to_text().starts_with("focus=none total="));
        assert!(cut.to_text().contains(" truncated\n"));
        // Child links never point at omitted nodes.
        for node in &cut.nodes {
            for child in &node.children {
                assert!(cut.node(*child).is_some());
            }
        }
        assert!(DEFAULT_MAX_NODES > 0);
    }

    #[test]
    fn a_subtree_observation_keeps_global_ids() {
        let runtime = button_app();
        let tree = runtime.semantic_tree();
        let button = tree
            .nodes()
            .iter()
            .find(|node| super::role_name(node.role()) == "button")
            .expect("button is in the tree");
        let subtree = Snapshot::capture(&tree, Some(button.id()), usize::MAX);
        assert_eq!(subtree.root, Some(button.id().index()));
        assert_eq!(subtree.total, 2); // the button and its label
        assert_eq!(subtree.nodes.len(), 2);
        assert_eq!(subtree.nodes[0].id, button.id().index());
        assert_eq!(subtree.nodes[0].parent, None);
        assert!(!subtree.truncated);
    }
}
