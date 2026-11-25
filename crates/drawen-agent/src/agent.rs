//! The UI-thread session: observe the tree and apply agent commands.
//!
//! [`Agent`] borrows a [`UiRuntime`] per call and never leaves the UI thread.
//! Semantic commands route through [`UiRuntime::perform_accessibility_action`],
//! the same path native assistive technology uses; key, text, and pointer
//! commands route through [`UiRuntime::dispatch_event`], the same path real
//! input uses. Application signal handlers therefore run exactly as they would
//! for a human user.

use drawen_core::{
    InputEvent, KeyCode, KeyEvent, Modifiers, Point, PointerButton, PointerButtons, PointerEvent,
    PointerId, WidgetId,
};
use drawen_ui::{AccessibilityAction, EventStatus, SemanticNode, SemanticTree, UiRuntime};

use crate::command::{modifier_flags, parse_key};
use crate::snapshot::{normalize, role_name};
use crate::{Command, Snapshot, Target};

/// The agent's synthesized pointer contact, kept apart from real devices.
const AGENT_POINTER: PointerId = PointerId(u64::MAX);

/// Observes and drives a [`UiRuntime`] on the UI thread.
#[derive(Clone, Copy, Debug, Default)]
pub struct Agent;

impl Agent {
    /// Creates an agent session.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Captures the current semantic tree as a plain-data snapshot.
    ///
    /// Bounds reflect the last layout pass; call after the event loop has laid
    /// out the tree at least once.
    #[must_use]
    pub fn observe(&self, runtime: &UiRuntime) -> Snapshot {
        Snapshot::from_tree(&runtime.semantic_tree())
    }

    /// Applies one command and returns the outcome with a fresh snapshot.
    ///
    /// The reply always carries the post-command observation, so one round
    /// trip is enough for an agent loop: act, then see the result. Layout is
    /// refreshed first when the command invalidated it, so bounds are current.
    pub fn execute(&self, runtime: &mut UiRuntime, command: &Command) -> Reply {
        let result = apply(runtime, command);
        let _ = runtime.ensure_layout();
        let snapshot = Snapshot::from_tree(&runtime.semantic_tree());
        match result {
            Ok(handled) => Reply {
                ok: true,
                handled,
                error: None,
                snapshot: Some(snapshot),
            },
            Err(error) => Reply {
                ok: false,
                handled: false,
                error: Some(error),
                snapshot: Some(snapshot),
            },
        }
    }
}

/// The outcome of one [`Agent::execute`] call.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
pub struct Reply {
    /// Whether the command was well-formed and its target resolved.
    pub ok: bool,
    /// Whether the runtime or a widget actually handled the interaction.
    ///
    /// `ok` with `handled == false` means the target exists but declined the
    /// action, for example a disabled button.
    pub handled: bool,
    /// Human-readable failure reason when `ok` is false.
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "Option::is_none"))]
    pub error: Option<String>,
    /// The post-command observation, absent only for transport-level errors.
    #[cfg_attr(feature = "json", serde(skip_serializing_if = "Option::is_none"))]
    pub snapshot: Option<Snapshot>,
}

impl Reply {
    /// Builds a transport-level failure without a snapshot.
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            handled: false,
            error: Some(message.into()),
            snapshot: None,
        }
    }
}

fn apply(runtime: &mut UiRuntime, command: &Command) -> Result<bool, String> {
    match command {
        Command::Observe => Ok(false),
        Command::Click { target } => semantic(runtime, target, &AccessibilityAction::Click),
        Command::Toggle { target } => semantic(runtime, target, &AccessibilityAction::Toggle),
        Command::Focus { target } => semantic(runtime, target, &AccessibilityAction::Focus),
        Command::Select { target } => semantic(runtime, target, &AccessibilityAction::Select),
        Command::SetValue { target, value } => semantic(
            runtime,
            target,
            &AccessibilityAction::SetValue(value.clone()),
        ),
        Command::Increment { target } => semantic(runtime, target, &AccessibilityAction::Increment),
        Command::Decrement { target } => semantic(runtime, target, &AccessibilityAction::Decrement),
        Command::Expand { target } => semantic(runtime, target, &AccessibilityAction::Expand),
        Command::Collapse { target } => semantic(runtime, target, &AccessibilityAction::Collapse),
        Command::Scroll { target, dx, dy } => {
            if !dx.is_finite() || !dy.is_finite() {
                return Err(format!("scroll delta must be finite, got ({dx}, {dy})"));
            }
            semantic(
                runtime,
                target,
                &AccessibilityAction::ScrollBy(Point::new(*dx, *dy)),
            )
        }
        Command::PressKey { key, modifiers } => {
            let key = parse_key(key).ok_or_else(|| format!("unknown key {key:?}"))?;
            let event = KeyEvent {
                key,
                code: KeyCode::Unidentified,
                repeat: false,
                modifiers: modifier_flags(modifiers),
            };
            let down = runtime.dispatch_event(&InputEvent::KeyDown(event.clone()));
            let up = runtime.dispatch_event(&InputEvent::KeyUp(event));
            Ok(is_handled(down) || is_handled(up))
        }
        Command::TypeText { text } => Ok(is_handled(
            runtime.dispatch_event(&InputEvent::TextInput(text.clone())),
        )),
        Command::ClickAt { x, y } => click_at(runtime, *x, *y),
    }
}

fn is_handled(status: EventStatus) -> bool {
    matches!(status, EventStatus::Handled)
}

fn semantic(
    runtime: &mut UiRuntime,
    target: &Target,
    action: &AccessibilityAction,
) -> Result<bool, String> {
    let tree = runtime.semantic_tree();
    let id = resolve(&tree, target)?;
    Ok(runtime.perform_accessibility_action(id, action))
}

fn resolve(tree: &SemanticTree, target: &Target) -> Result<WidgetId, String> {
    match target {
        Target::Id(index) => tree
            .nodes()
            .iter()
            .find(|node| node.id().index() == *index)
            .map(SemanticNode::id)
            .ok_or_else(|| format!("no widget #{index} in the current tree")),
        Target::Name(name) => query(tree, None, Some(name)),
        Target::Query { role, label } => {
            if role.is_none() && label.is_none() {
                return Err("empty target query: give an id, a role, or a label".to_owned());
            }
            query(tree, role.as_deref(), label.as_deref())
        }
    }
}

fn query(tree: &SemanticTree, role: Option<&str>, label: Option<&str>) -> Result<WidgetId, String> {
    let wanted_role = role.map(normalize);
    let wanted_label = label.map(str::to_lowercase);
    let matches: Vec<_> = tree
        .nodes()
        .iter()
        .filter(|node| {
            wanted_role
                .as_ref()
                .is_none_or(|wanted| normalize(role_name(node.role())) == *wanted)
                && wanted_label.as_ref().is_none_or(|wanted| {
                    node.label()
                        .is_some_and(|label| label.to_lowercase().contains(wanted))
                })
        })
        .collect();
    if let Some(wanted) = label {
        let exact: Vec<_> = matches
            .iter()
            .filter(|node| {
                node.label()
                    .is_some_and(|label| label.eq_ignore_ascii_case(wanted))
            })
            .collect();
        if exact.len() == 1 {
            return Ok(exact[0].id());
        }
    }
    match matches.len() {
        0 => Err(format!("no widget matches {}", describe(role, label))),
        1 => Ok(matches[0].id()),
        _ => {
            let candidates: Vec<String> = matches
                .iter()
                .take(8)
                .map(|node| {
                    format!(
                        "#{} {} {:?}",
                        node.id().index(),
                        role_name(node.role()),
                        node.label().unwrap_or("")
                    )
                })
                .collect();
            Err(format!(
                "ambiguous target {}: matches {}",
                describe(role, label),
                candidates.join(", ")
            ))
        }
    }
}

fn describe(role: Option<&str>, label: Option<&str>) -> String {
    match (role, label) {
        (Some(role), Some(label)) => format!("role {role:?} with label {label:?}"),
        (Some(role), None) => format!("role {role:?}"),
        (None, Some(label)) => format!("label {label:?}"),
        (None, None) => "the empty query".to_owned(),
    }
}

fn click_at(runtime: &mut UiRuntime, x: f32, y: f32) -> Result<bool, String> {
    if !x.is_finite() || !y.is_finite() {
        return Err(format!("click position must be finite, got ({x}, {y})"));
    }
    let base = PointerEvent {
        pointer_id: AGENT_POINTER,
        position: Point::new(x, y),
        button: None,
        buttons: PointerButtons::NONE,
        modifiers: Modifiers::NONE,
    };
    let mut handled = is_handled(runtime.dispatch_event(&InputEvent::PointerMove(base.clone())));
    let down = PointerEvent {
        button: Some(PointerButton::Primary),
        buttons: PointerButtons::PRIMARY,
        ..base.clone()
    };
    handled |= is_handled(runtime.dispatch_event(&InputEvent::PointerDown(down)));
    let up = PointerEvent {
        button: Some(PointerButton::Primary),
        ..base
    };
    handled |= is_handled(runtime.dispatch_event(&InputEvent::PointerUp(up)));
    Ok(handled)
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use drawen::{
        Box as DrawenBox, Button, Checkbox, Color, Column, Constraints, Size, Subscription, Text,
        TextField, UiRuntime, render::TextStyle,
    };

    use super::{Agent, Command, Target};

    struct Demo {
        runtime: UiRuntime,
        clicks: Rc<Cell<u32>>,
        toggles: Rc<Cell<bool>>,
        _subscriptions: Vec<Subscription>,
    }

    fn demo_app() -> Demo {
        let mut runtime = UiRuntime::new(DrawenBox::new());
        let clicks = Rc::new(Cell::new(0));
        let toggles = Rc::new(Cell::new(false));
        let column = runtime
            .append_child(runtime.root(), Column::new())
            .expect("root exists");

        runtime
            .append_child(column, TextField::new())
            .expect("column exists");
        let checkbox = Checkbox::new();
        let toggle_subscription = checkbox.toggled().subscribe({
            let toggles = Rc::clone(&toggles);
            move |checked| toggles.set(*checked)
        });
        let checkbox = runtime
            .append_child(column, checkbox)
            .expect("column exists");
        runtime
            .append_child(
                checkbox,
                Text::styled("Agree", TextStyle::new(14.0, Color::BLACK)),
            )
            .expect("checkbox exists");
        let button = Button::new();
        let click_subscription = button.activated().subscribe({
            let clicks = Rc::clone(&clicks);
            move |()| clicks.set(clicks.get() + 1)
        });
        let button = runtime.append_child(column, button).expect("column exists");
        runtime
            .append_child(
                button,
                Text::styled("Save", TextStyle::new(14.0, Color::BLACK)),
            )
            .expect("button exists");
        runtime
            .layout(
                Constraints::tight(Size::new(320.0, 240.0).expect("valid size")).expect("valid"),
            )
            .expect("layout succeeds");
        Demo {
            runtime,
            clicks,
            toggles,
            _subscriptions: vec![toggle_subscription, click_subscription],
        }
    }

    #[test]
    fn click_by_name_activates_the_button() {
        let mut demo = demo_app();
        let agent = Agent::new();
        let reply = agent.execute(
            &mut demo.runtime,
            &Command::Click {
                target: Target::Name("Save".to_owned()),
            },
        );
        assert!(reply.ok, "{:?}", reply.error);
        assert!(reply.handled);
        assert_eq!(demo.clicks.get(), 1);
        assert!(reply.snapshot.is_some());
    }

    #[test]
    fn set_value_writes_the_field_and_shows_in_snapshot() {
        let mut demo = demo_app();
        let agent = Agent::new();
        let reply = agent.execute(
            &mut demo.runtime,
            &Command::SetValue {
                target: Target::Query {
                    role: Some("textfield".to_owned()),
                    label: None,
                },
                value: "Grace".to_owned(),
            },
        );
        assert!(reply.ok && reply.handled, "{:?}", reply.error);
        let snapshot = reply.snapshot.expect("reply carries a snapshot");
        let field = snapshot
            .nodes
            .iter()
            .find(|node| node.role == "text_field")
            .expect("field is in the snapshot");
        assert_eq!(field.value.as_deref(), Some("Grace"));
    }

    #[test]
    fn toggle_checks_the_checkbox() {
        let mut demo = demo_app();
        let agent = Agent::new();
        let reply = agent.execute(
            &mut demo.runtime,
            &Command::Toggle {
                target: Target::Name("Agree".to_owned()),
            },
        );
        assert!(reply.ok && reply.handled, "{:?}", reply.error);
        assert!(demo.toggles.get());
    }

    #[test]
    fn unknown_target_fails_with_a_fresh_snapshot() {
        let mut demo = demo_app();
        let agent = Agent::new();
        let reply = agent.execute(
            &mut demo.runtime,
            &Command::Click {
                target: Target::Name("Nonexistent".to_owned()),
            },
        );
        assert!(!reply.ok);
        assert!(
            reply
                .error
                .expect("error is set")
                .contains("no widget matches")
        );
        assert_eq!(demo.clicks.get(), 0);
        assert!(reply.snapshot.is_some());
    }

    #[test]
    fn stale_id_fails_instead_of_hitting_a_recycled_slot() {
        let mut demo = demo_app();
        let agent = Agent::new();
        let reply = agent.execute(
            &mut demo.runtime,
            &Command::Click {
                target: Target::Id(9999),
            },
        );
        assert!(!reply.ok);
    }

    #[test]
    fn press_key_enter_activates_the_focused_button() {
        let mut demo = demo_app();
        let agent = Agent::new();
        let focus = agent.execute(
            &mut demo.runtime,
            &Command::Focus {
                target: Target::Name("Save".to_owned()),
            },
        );
        assert!(focus.ok && focus.handled);
        let reply = agent.execute(
            &mut demo.runtime,
            &Command::PressKey {
                key: "enter".to_owned(),
                modifiers: Vec::new(),
            },
        );
        assert!(reply.ok, "{:?}", reply.error);
        assert_eq!(demo.clicks.get(), 1);
    }

    #[test]
    fn type_text_reaches_the_focused_field() {
        let mut demo = demo_app();
        let agent = Agent::new();
        agent.execute(
            &mut demo.runtime,
            &Command::Focus {
                target: Target::Query {
                    role: Some("text_field".to_owned()),
                    label: None,
                },
            },
        );
        let reply = agent.execute(
            &mut demo.runtime,
            &Command::TypeText {
                text: "hi".to_owned(),
            },
        );
        assert!(reply.ok && reply.handled, "{:?}", reply.error);
        let snapshot = reply.snapshot.expect("snapshot");
        let field = snapshot
            .nodes
            .iter()
            .find(|node| node.role == "text_field")
            .expect("field");
        assert_eq!(field.value.as_deref(), Some("hi"));
    }
}
