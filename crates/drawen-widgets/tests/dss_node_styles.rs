//! Integration tests for per-node DSS styles on standard widgets.

use drawen_core::{
    Color, Constraints, InputEvent, Modifiers, Point, PointerButtons, PointerEvent, PointerId, Size,
};
use drawen_render::{
    DisplayCommand, DisplayList, FontDatabase, FontDescriptor, FontFace, FontStyle, FontWeight,
    FontdueTextShaper, PaintContext, TextStyle,
};
use drawen_ui::{ChildPosition, LightTheme, StyleTarget, Styled, UiEnvironment, UiRuntime};
use drawen_widgets::{Box as BoxWidget, Button, Checkbox, Stack, Text};

fn environment(source: &str) -> UiEnvironment {
    UiEnvironment::from_dss(LightTheme, source).expect("valid DSS")
}

/// An environment whose shaper has distinct normal and bold-italic faces so
/// font matching is observable in the shaped output.
fn font_environment(source: &str) -> UiEnvironment {
    let mut environment = environment(source);
    let mut database = FontDatabase::with_default(FontFace::ubuntu_light());
    database.add_face(
        FontFace::from_bytes_with_descriptor(
            include_bytes!("../../drawen-render/assets/Ubuntu-Light.ttf").as_slice(),
            FontDescriptor::new("Ubuntu", FontWeight::BOLD, FontStyle::Italic),
        )
        .expect("bundled font is valid"),
    );
    environment.set_text_shaper(FontdueTextShaper::from_database(database));
    environment
}

fn pointer_move(position: Point) -> InputEvent {
    InputEvent::PointerMove(PointerEvent {
        pointer_id: PointerId(1),
        position,
        button: None,
        buttons: PointerButtons::NONE,
        modifiers: Modifiers::NONE,
    })
}

fn pointer_down(position: Point) -> InputEvent {
    InputEvent::PointerDown(PointerEvent {
        pointer_id: PointerId(1),
        position,
        button: Some(drawen_core::PointerButton::Primary),
        buttons: PointerButtons::PRIMARY,
        modifiers: Modifiers::NONE,
    })
}

#[test]
fn type_rules_style_only_matching_widgets() {
    let mut runtime =
        UiRuntime::with_environment(Stack::new(), environment("button { background: #112233; }"));
    let button = runtime
        .append_child(runtime.root(), Button::new())
        .expect("root exists");
    let checkbox = runtime
        .append_child(runtime.root(), Checkbox::new())
        .expect("root exists");
    runtime
        .layout(Constraints::UNBOUNDED)
        .expect("layout succeeds");

    assert_eq!(
        runtime
            .computed_style(button)
            .expect("button is live")
            .background,
        Some(Color::rgba8(17, 34, 51, 255))
    );
    assert_eq!(
        runtime
            .computed_style(checkbox)
            .expect("checkbox is live")
            .background,
        None
    );
}

#[test]
fn id_and_class_rules_apply_through_styled() {
    let mut runtime = UiRuntime::with_environment(
        Stack::new(),
        environment(
            "#save { background: #00ff00; } \
             .primary { border-radius: 7px; } \
             button.primary { color: #0000ff; }",
        ),
    );
    let plain = runtime
        .append_child(runtime.root(), Button::new())
        .expect("root exists");
    let styled = runtime
        .append_child(
            runtime.root(),
            Styled::new(Button::new())
                .with_id("save")
                .with_class("primary"),
        )
        .expect("root exists");
    runtime
        .layout(Constraints::UNBOUNDED)
        .expect("layout succeeds");

    let style = runtime
        .computed_style(styled)
        .expect("styled button is live");
    assert_eq!(style.background, Some(Color::rgba8(0, 255, 0, 255)));
    assert_eq!(style.corner_radius, Some(7.0));
    assert_eq!(style.foreground, Some(Color::rgba8(0, 0, 255, 255)));
    assert_eq!(
        runtime.computed_style(plain).expect("plain button is live"),
        drawen_ui::DssStyle::default()
    );
    let stack = StyleTarget::new("stack");
    let expected_target = StyleTarget::new("button")
        .with_id("save")
        .with_class("primary")
        .with_child_position(1, 2)
        .with_parent(&stack);
    assert_eq!(runtime.style_target(styled), Some(&expected_target));
}

#[test]
fn descendant_and_child_selectors_follow_the_retained_tree() {
    let mut runtime = UiRuntime::with_environment(
        Stack::new(),
        environment(
            "stack > box { border-radius: 4px; } \
             stack box button { background: #010203; } \
             stack > button { background: #040506; } \
             box > button:first-child { color: #070809; }",
        ),
    );
    let card = runtime
        .append_child(
            runtime.root(),
            Styled::new(BoxWidget::new()).with_class("card"),
        )
        .expect("root exists");
    let nested = runtime
        .append_child(card, Button::new())
        .expect("card exists");
    let direct = runtime
        .append_child(runtime.root(), Button::new())
        .expect("root exists");
    runtime
        .layout(Constraints::UNBOUNDED)
        .expect("layout succeeds");

    assert_eq!(
        runtime
            .computed_style(card)
            .expect("card is live")
            .corner_radius,
        Some(4.0)
    );
    assert_eq!(
        runtime
            .computed_style(nested)
            .expect("nested button is live")
            .foreground,
        Some(Color::rgba8(7, 8, 9, 255))
    );
    assert_eq!(
        runtime
            .computed_style(nested)
            .expect("nested button is live")
            .background,
        Some(Color::rgba8(1, 2, 3, 255))
    );
    assert_eq!(
        runtime
            .computed_style(direct)
            .expect("direct button is live")
            .background,
        Some(Color::rgba8(4, 5, 6, 255))
    );
    let target = runtime
        .style_target(nested)
        .expect("nested button has a target");
    assert_eq!(target.child_position().map(ChildPosition::index), Some(0));
    assert_eq!(target.ancestors().len(), 2);
    assert_eq!(target.ancestors()[0].type_name(), Some("box"));
    assert_eq!(target.ancestors()[1].type_name(), Some("stack"));
}

#[test]
fn size_properties_bound_layout() {
    let mut runtime = UiRuntime::with_environment(
        Stack::new(),
        environment(
            ".fixed { width: 40px; height: 12px; } \
             .bounded { min-width: 30px; max-width: 50px; height: 9px; }",
        ),
    );
    let fixed = runtime
        .append_child(
            runtime.root(),
            Styled::new(BoxWidget::new()).with_class("fixed"),
        )
        .expect("root exists");
    let bounded = runtime
        .append_child(
            runtime.root(),
            Styled::new(BoxWidget::new()).with_class("bounded"),
        )
        .expect("root exists");
    runtime
        .layout(Constraints::UNBOUNDED)
        .expect("layout succeeds");

    let fixed = runtime.bounds(fixed).expect("fixed child is live");
    assert!((fixed.size.width() - 40.0).abs() < f32::EPSILON);
    assert!((fixed.size.height() - 12.0).abs() < f32::EPSILON);
    let bounded = runtime.bounds(bounded).expect("bounded child is live");
    assert!((bounded.size.width() - 30.0).abs() < f32::EPSILON);
    assert!((bounded.size.height() - 9.0).abs() < f32::EPSILON);
}

#[test]
fn structural_and_semantic_pseudo_classes_style_children() {
    let mut runtime = UiRuntime::with_environment(
        Stack::new(),
        environment(
            "checkbox:checked { accent-color: #102030; } \
             checkbox:last-child { border-radius: 9px; } \
             button:nth-child(2) { background: #0a0b0c; }",
        ),
    );
    let checked = runtime
        .append_child(runtime.root(), Checkbox::with_checked(true))
        .expect("root exists");
    let button = runtime
        .append_child(runtime.root(), Button::new())
        .expect("root exists");
    runtime
        .layout(Constraints::UNBOUNDED)
        .expect("layout succeeds");

    let checked_style = runtime.computed_style(checked).expect("checkbox is live");
    assert_eq!(checked_style.accent, Some(Color::rgba8(16, 32, 48, 255)));
    assert_eq!(
        checked_style.corner_radius, None,
        "the first child is not the last child"
    );
    assert_eq!(
        runtime
            .computed_style(button)
            .expect("button is live")
            .background,
        Some(Color::rgba8(10, 11, 12, 255))
    );
}

#[test]
fn hover_and_focus_states_participate_in_the_cascade() {
    let mut runtime = UiRuntime::with_environment(
        Button::new(),
        environment("button:hover { background: #101112; } button:focus { border-radius: 5px; }"),
    );
    let button = runtime.root();
    runtime
        .layout(
            Constraints::tight(Size::new(40.0, 40.0).expect("valid size"))
                .expect("valid constraints"),
        )
        .expect("layout succeeds");

    let _ = runtime.dispatch_event(&pointer_move(Point::new(20.0, 20.0)));
    let mut list = DisplayList::new();
    runtime
        .paint(&mut PaintContext::new(&mut list))
        .expect("hover paint succeeds");
    assert_eq!(
        runtime
            .computed_style(button)
            .expect("button is live")
            .background,
        Some(Color::rgba8(16, 17, 18, 255))
    );

    let _ = runtime.dispatch_event(&pointer_down(Point::new(20.0, 20.0)));
    let mut list = DisplayList::new();
    runtime
        .paint(&mut PaintContext::new(&mut list))
        .expect("focus paint succeeds");
    assert_eq!(runtime.focused_widget(), Some(button));
    assert_eq!(
        runtime
            .computed_style(button)
            .expect("button is live")
            .corner_radius,
        Some(5.0)
    );
}

#[test]
fn inherited_text_style_reaches_descendants() {
    let mut runtime = UiRuntime::with_environment(
        Button::new(),
        environment("button { color: #112233; font-size: 22px; }"),
    );
    let button = runtime.root();
    let label = FontdueTextShaper::ubuntu_light().layout(
        "Inherit",
        &TextStyle::new(10.0, Color::BLACK),
        None,
    );
    let text = runtime
        .append_child(button, Text::new(label))
        .expect("button exists");
    runtime
        .layout(Constraints::UNBOUNDED)
        .expect("layout succeeds");

    let style = runtime.computed_style(text).expect("text is live");
    assert_eq!(style.foreground, Some(Color::rgba8(17, 34, 51, 255)));
    assert!((style.font_size_or(0.0) - 22.0).abs() < f32::EPSILON);

    let mut list = DisplayList::new();
    runtime
        .paint(&mut PaintContext::new(&mut list))
        .expect("paint succeeds");
    let drawn = list
        .commands()
        .iter()
        .find_map(|command| match command {
            DisplayCommand::DrawText { layout, .. } => Some(layout),
            _ => None,
        })
        .expect("text is painted");
    assert_eq!(drawn.color(), Color::rgba8(17, 34, 51, 255));
}

#[test]
fn dss_font_properties_reach_shaped_text() {
    let mut runtime = UiRuntime::with_environment(
        Button::new(),
        font_environment(
            r#"button {
                font-family: "Ubuntu";
                font-size: 24px;
                font-weight: 700;
                font-style: italic;
            }"#,
        ),
    );
    runtime
        .append_child(
            runtime.root(),
            Text::styled("Font", TextStyle::new(10.0, Color::BLACK)),
        )
        .expect("root exists");
    runtime
        .layout(Constraints::UNBOUNDED)
        .expect("layout succeeds");

    let mut list = DisplayList::new();
    runtime
        .paint(&mut PaintContext::new(&mut list))
        .expect("paint succeeds");
    let layout = list
        .commands()
        .iter()
        .find_map(|command| match command {
            DisplayCommand::DrawText { layout, .. } => Some(layout),
            _ => None,
        })
        .expect("text is painted");
    let run = layout.glyph_runs().first().expect("text has glyphs");

    assert_eq!(run.font().family().as_str(), "Ubuntu");
    assert_eq!(run.font().weight(), FontWeight::BOLD);
    assert_eq!(run.font().style(), FontStyle::Italic);
    assert!((run.font_size() - 24.0).abs() < f32::EPSILON);
}

#[test]
fn rgba_borders_parse_and_apply_to_standard_widgets() {
    let mut runtime = UiRuntime::with_environment(
        Button::new(),
        environment("button { border: 2px solid rgba(12, 34, 56, 0.5); }"),
    );
    let button = runtime.root();
    runtime
        .layout(
            Constraints::tight(Size::new(30.0, 30.0).expect("valid size"))
                .expect("valid constraints"),
        )
        .expect("layout succeeds");

    let border = runtime
        .computed_style(button)
        .expect("button is live")
        .border
        .expect("border is resolved");
    assert!((border.width - 2.0).abs() < f32::EPSILON);
    assert_eq!(border.color, Color::rgba8(12, 34, 56, 128));

    let mut list = DisplayList::new();
    runtime
        .paint(&mut PaintContext::new(&mut list))
        .expect("paint succeeds");
    assert!(list.commands().iter().any(|command| matches!(
        command,
        DisplayCommand::StrokeRoundedRect {
            width: 2.0,
            color,
            ..
        } if *color == border.color
    )));
}

#[test]
fn disabled_state_rules_match_only_disabled_buttons() {
    let mut runtime = UiRuntime::with_environment(
        Stack::new(),
        environment("button:disabled { background: #445566; }"),
    );
    let enabled = runtime
        .append_child(runtime.root(), Button::new())
        .expect("root exists");
    let disabled = runtime
        .append_child(runtime.root(), Button::new().with_enabled(false))
        .expect("root exists");
    runtime
        .layout(Constraints::UNBOUNDED)
        .expect("layout succeeds");

    assert_eq!(
        runtime
            .computed_style(disabled)
            .expect("button is live")
            .background,
        Some(Color::rgba8(68, 85, 102, 255))
    );
    assert_eq!(
        runtime
            .computed_style(enabled)
            .expect("button is live")
            .background,
        None
    );
}
