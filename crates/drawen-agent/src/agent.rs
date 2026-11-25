//! The UI-thread session: observe the tree and apply agent commands.
//!
//! [`Agent`] borrows a [`UiRuntime`] per call and never leaves the UI thread.
//! Semantic commands route through [`UiRuntime::perform_accessibility_action`],
//! the same path native assistive technology uses; key, text, and pointer
//! commands route through [`UiRuntime::dispatch_event`], the same path real
//! input uses. Application signal handlers therefore run exactly as they would
//! for a human user.

use std::time::Duration;

use drawen_core::{
    InputEvent, KeyCode, KeyEvent, Modifiers, Point, PointerButton, PointerButtons, PointerEvent,
    PointerId, WidgetId,
};
use drawen_ui::{AccessibilityAction, EventStatus, SemanticNode, SemanticTree, UiRuntime};

use crate::command::{modifier_flags, parse_key};
use crate::snapshot::{DEFAULT_MAX_NODES, normalize, role_name};
use crate::{Command, Snapshot, Target};

/// The agent's synthesized pointer contact, kept apart from real devices.
const AGENT_POINTER: PointerId = PointerId(u64::MAX);
/// Upper bound for one `advance` or `wait_until_idle` budget, in milliseconds.
const MAX_CLOCK_MS: f64 = 60_000.0;
/// Deterministic clock step: one 60 Hz frame.
const FRAME_MS: f64 = 1000.0 / 60.0;

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
    /// The observation keeps at most [`DEFAULT_MAX_NODES`] nodes and reports
    /// `total` and `truncated`; observe a subtree by id to see more of a large
    /// application. Bounds reflect the last layout pass; call after the event
    /// loop has laid out the tree at least once.
    #[must_use]
    pub fn observe(&self, runtime: &UiRuntime) -> Snapshot {
        Snapshot::capture(&runtime.semantic_tree(), None, DEFAULT_MAX_NODES)
    }

    /// Applies one command and returns the outcome with a fresh snapshot.
    ///
    /// The reply always carries the post-command observation, so one round
    /// trip is enough for an agent loop: act, then see the result. Layout is
    /// refreshed first when the command invalidated it, so bounds are current.
    /// An `observe` command's reply carries exactly the requested subtree and
    /// budget; other commands reply with the default whole-tree observation.
    pub fn execute(&self, runtime: &mut UiRuntime, command: &Command) -> Reply {
        let result = apply(runtime, command);
        let _ = runtime.ensure_layout();
        let (root, max_nodes) = match &result {
            Ok(Outcome::Observe { root, max_nodes }) => (*root, *max_nodes),
            _ => (None, DEFAULT_MAX_NODES),
        };
        let snapshot = Snapshot::capture(&runtime.semantic_tree(), root, max_nodes);
        match result {
            Ok(outcome) => Reply {
                ok: true,
                handled: matches!(outcome, Outcome::Acted(true)),
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

/// What one command did, plus the observation its reply should carry.
enum Outcome {
    /// An interaction or clock command; the flag is its `handled` result.
    Acted(bool),
    /// A pure observation request with its own scope and budget.
    Observe {
        root: Option<WidgetId>,
        max_nodes: usize,
    },
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
    /// action, for example a disabled button, or that `wait_until_idle` timed
    /// out before the animations settled.
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

fn apply(runtime: &mut UiRuntime, command: &Command) -> Result<Outcome, String> {
    match command {
        Command::Observe { target, max_nodes } => {
            let budget = max_nodes.unwrap_or(DEFAULT_MAX_NODES);
            if budget == 0 {
                return Err("max_nodes must be at least 1".to_owned());
            }
            let root = match target {
                Some(target) => Some(resolve(&runtime.semantic_tree(), target)?),
                None => None,
            };
            Ok(Outcome::Observe {
                root,
                max_nodes: budget,
            })
        }
        Command::Advance { ms } => advance(runtime, *ms).map(Outcome::Acted),
        Command::WaitUntilIdle { timeout_ms } => {
            wait_until_idle(runtime, *timeout_ms).map(Outcome::Acted)
        }
        _ => apply_action(runtime, command).map(Outcome::Acted),
    }
}

fn apply_action(runtime: &mut UiRuntime, command: &Command) -> Result<bool, String> {
    match command {
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
        Command::Observe { .. } | Command::Advance { .. } | Command::WaitUntilIdle { .. } => {
            unreachable!("observation and clock commands are handled by apply")
        }
    }
}

/// Steps the animation clock deterministically in 60 Hz frames.
fn advance(runtime: &mut UiRuntime, ms: f64) -> Result<bool, String> {
    if !ms.is_finite() || !(0.0..=MAX_CLOCK_MS).contains(&ms) {
        return Err(format!("advance ms must be within 0..=60000, got {ms}"));
    }
    let mut remaining = ms;
    while remaining > 0.0 {
        let step = remaining.min(FRAME_MS);
        let _ = runtime.tick(Duration::from_secs_f64(step / 1000.0));
        remaining -= step;
    }
    Ok(true)
}

/// Advances time until no animation frames remain scheduled, or the budget
/// runs out. A live loop (an indeterminate spinner, playing media) never
/// settles; the timeout reports that instead of spinning forever.
fn wait_until_idle(runtime: &mut UiRuntime, timeout_ms: Option<f64>) -> Result<bool, String> {
    let budget = timeout_ms.unwrap_or(5_000.0);
    if !budget.is_finite() || !(0.0..=MAX_CLOCK_MS).contains(&budget) {
        return Err(format!(
            "wait_until_idle timeout_ms must be within 0..=60000, got {budget}"
        ));
    }
    let _ = runtime.ensure_layout();
    let mut remaining = budget;
    loop {
        if runtime.next_frame_in().is_none() {
            return Ok(true);
        }
        if remaining <= 0.0 {
            return Ok(false);
        }
        let step = remaining.min(FRAME_MS);
        let _ = runtime.tick(Duration::from_secs_f64(step / 1000.0));
        remaining -= step;
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
                    .is_some_and(|label| label.to_lowercase() == wanted.to_lowercase())
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

    #[test]
    fn observe_subtree_returns_only_that_branch() {
        let mut demo = demo_app();
        let agent = Agent::new();
        let reply = agent.execute(
            &mut demo.runtime,
            &Command::Observe {
                target: Some(Target::Name("Save".to_owned())),
                max_nodes: None,
            },
        );
        assert!(reply.ok, "{:?}", reply.error);
        assert!(!reply.handled);
        let snapshot = reply.snapshot.expect("observation");
        assert_eq!(snapshot.total, 2); // the button and its label
        assert_eq!(snapshot.nodes.len(), 2);
        assert_eq!(snapshot.nodes[0].role, "button");
        assert_eq!(snapshot.nodes[0].parent, None);
        assert!(!snapshot.truncated);
    }

    #[test]
    fn observe_respects_an_explicit_budget() {
        let mut demo = demo_app();
        let agent = Agent::new();
        let reply = agent.execute(
            &mut demo.runtime,
            &Command::Observe {
                target: None,
                max_nodes: Some(2),
            },
        );
        assert!(reply.ok, "{:?}", reply.error);
        let snapshot = reply.snapshot.expect("observation");
        assert_eq!(snapshot.nodes.len(), 2);
        assert!(snapshot.truncated);
        assert!(snapshot.total > 2);

        let zero = agent.execute(
            &mut demo.runtime,
            &Command::Observe {
                target: None,
                max_nodes: Some(0),
            },
        );
        assert!(!zero.ok);
    }

    #[test]
    fn focus_on_a_disabled_button_is_declined() {
        let mut runtime = UiRuntime::new(DrawenBox::new());
        let button = Button::new().with_enabled(false);
        let button = runtime
            .append_child(runtime.root(), button)
            .expect("root exists");
        runtime
            .append_child(
                button,
                Text::styled("Disabled", TextStyle::new(14.0, Color::BLACK)),
            )
            .expect("button exists");
        runtime
            .layout(
                Constraints::tight(Size::new(320.0, 200.0).expect("valid size")).expect("valid"),
            )
            .expect("layout succeeds");

        let reply = Agent::new().execute(
            &mut runtime,
            &Command::Focus {
                target: Target::Name("Disabled".to_owned()),
            },
        );
        assert!(reply.ok, "{:?}", reply.error);
        assert!(!reply.handled, "a disabled widget must decline focus");
    }

    #[test]
    fn advance_settles_the_focus_transition() {
        let mut demo = demo_app();
        let agent = Agent::new();
        let focus = agent.execute(
            &mut demo.runtime,
            &Command::Focus {
                target: Target::Name("Save".to_owned()),
            },
        );
        assert!(focus.ok && focus.handled);
        assert!(
            demo.runtime.next_frame_in().is_some(),
            "the focus transition should be in flight"
        );
        let reply = agent.execute(&mut demo.runtime, &Command::Advance { ms: 1000.0 });
        assert!(reply.ok && reply.handled, "{:?}", reply.error);
        assert!(
            demo.runtime.next_frame_in().is_none(),
            "the transition should have settled"
        );
    }

    #[test]
    fn advance_rejects_bad_durations() {
        let mut demo = demo_app();
        let agent = Agent::new();
        for ms in [-1.0, f64::NAN, f64::INFINITY, 60_001.0] {
            let reply = agent.execute(&mut demo.runtime, &Command::Advance { ms });
            assert!(!reply.ok, "ms={ms} should be rejected");
        }
    }

    #[test]
    fn wait_until_idle_settles_a_static_tree() {
        let mut demo = demo_app();
        let reply = Agent::new().execute(
            &mut demo.runtime,
            &Command::WaitUntilIdle { timeout_ms: None },
        );
        assert!(reply.ok && reply.handled, "{:?}", reply.error);
    }

    /// A widget whose animation loop never ends, like an indeterminate spinner.
    struct LiveLoop;

    impl drawen::Widget for LiveLoop {
        fn layout(
            &mut self,
            _: &mut drawen::LayoutContext<'_>,
            constraints: Constraints,
        ) -> drawen::LayoutResult {
            drawen::LayoutResult::new(
                constraints.constrain(Size::new(10.0, 10.0).expect("valid size")),
            )
        }

        fn is_animating(&self) -> bool {
            true
        }
    }

    #[test]
    fn wait_until_idle_times_out_on_a_live_loop() {
        let mut runtime = UiRuntime::new(DrawenBox::new());
        runtime
            .append_child(runtime.root(), LiveLoop)
            .expect("root exists");
        runtime
            .layout(
                Constraints::tight(Size::new(320.0, 200.0).expect("valid size")).expect("valid"),
            )
            .expect("layout succeeds");
        let reply = Agent::new().execute(
            &mut runtime,
            &Command::WaitUntilIdle {
                timeout_ms: Some(50.0),
            },
        );
        assert!(reply.ok, "{:?}", reply.error);
        assert!(!reply.handled, "a live loop never settles");
    }

    #[test]
    fn unicode_names_match_case_insensitively() {
        let mut runtime = UiRuntime::new(DrawenBox::new());
        let clicks = Rc::new(Cell::new(0_u32));
        let button = Button::new();
        let subscription = button.activated().subscribe({
            let clicks = Rc::clone(&clicks);
            move |()| clicks.set(clicks.get() + 1)
        });
        let button = runtime
            .append_child(runtime.root(), button)
            .expect("root exists");
        runtime
            .append_child(
                button,
                Text::styled("Сохранить", TextStyle::new(14.0, Color::BLACK)),
            )
            .expect("button exists");
        runtime
            .layout(
                Constraints::tight(Size::new(320.0, 200.0).expect("valid size")).expect("valid"),
            )
            .expect("layout succeeds");

        let reply = Agent::new().execute(
            &mut runtime,
            &Command::Click {
                target: Target::Name("сохранить".to_owned()),
            },
        );
        assert!(reply.ok && reply.handled, "{:?}", reply.error);
        assert_eq!(clicks.get(), 1);
        drop(subscription);
    }
}
