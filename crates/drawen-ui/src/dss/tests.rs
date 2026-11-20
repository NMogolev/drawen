use drawen_core::{Color, Insets};
use drawen_render::{FontFamily, FontSize, FontStyle, FontWeight};

use crate::{Border, LightTheme, PseudoClass, StyleSheet, StyleTarget, Theme};

use super::DssStyle;

fn resolve(source: &str, target: &StyleTarget) -> DssStyle {
    StyleSheet::parse(source)
        .expect("test stylesheet is valid")
        .resolve(target)
}

#[test]
fn resolves_the_most_specific_matching_declarations() {
    let stylesheet = StyleSheet::parse(
        r"
            * { color: #111; font-size: 13px; }
            button { background: #224466; padding: 4px 8px; }
            .primary { background: #336699; }
            #save.primary:hover { background: rgba(12, 34, 56, 0.5); }
        ",
    )
    .expect("valid DSS");
    let target = StyleTarget::new("button")
        .with_id("save")
        .with_class("primary")
        .with_state(PseudoClass::Hover);

    let style = stylesheet.resolve(&target);
    assert_eq!(style.foreground, Some(Color::rgba8(17, 17, 17, 255)));
    assert_eq!(style.background, Some(Color::rgba8(12, 34, 56, 128)));
    assert_eq!(style.padding, Some(Insets::new(4.0, 8.0, 4.0, 8.0)));
    assert_eq!(style.font_size, Some(FontSize::pixels(13.0)));
}

#[test]
fn source_order_breaks_specificity_ties() {
    let target = StyleTarget::new("button")
        .with_class("primary")
        .with_class("wide");
    let style = resolve(
        r"
            button.primary { background: #111111; }
            button.wide { background: #222222; }
        ",
        &target,
    );
    assert_eq!(style.background, Some(Color::rgba8(34, 34, 34, 255)));
}

#[test]
fn root_supplies_theme_tokens_without_rewriting_control_values() {
    let stylesheet = StyleSheet::parse(
        r"
            :root {
                background: #101820;
                color: #f1f3f5;
                accent-color: #2edcb3;
                gap: 12px;
                font-size: 15px;
                border-radius: 6px;
            }
            button { background: #243447; padding: 6px 10px; }
            button:hover { background: #34516b; }
            button:pressed { background: #182838; }
        ",
    )
    .expect("valid DSS");
    let theme = stylesheet.theme(LightTheme);
    let button = stylesheet.resolve(&StyleTarget::new("button"));
    let hovered = stylesheet.resolve(&StyleTarget::new("button").with_state(PseudoClass::Hover));
    let pressed = stylesheet.resolve(&StyleTarget::new("button").with_state(PseudoClass::Pressed));

    assert_eq!(theme.background(), Color::rgba8(16, 24, 32, 255));
    assert_eq!(theme.foreground(), Color::rgba8(241, 243, 245, 255));
    assert_eq!(theme.accent(), Color::rgba8(46, 220, 179, 255));
    assert!((theme.spacing() - 12.0).abs() < f32::EPSILON);
    assert!((theme.font_size() - 15.0).abs() < f32::EPSILON);
    assert!((theme.corner_radius() - 6.0).abs() < f32::EPSILON);
    // Control selectors are node styles, not global theme overrides.
    assert_eq!(theme.control_background(), LightTheme.control_background());
    assert_eq!(button.background, Some(Color::rgba8(36, 52, 71, 255)));
    assert_eq!(hovered.background, Some(Color::rgba8(52, 81, 107, 255)));
    assert_eq!(pressed.background, Some(Color::rgba8(24, 40, 56, 255)));
    assert_eq!(button.padding, Some(Insets::new(6.0, 10.0, 6.0, 10.0)));
}

#[test]
fn supports_comments_and_comma_separated_selectors() {
    let stylesheet = StyleSheet::parse(
        "/* shared state */ button:hover, .preview { border: 2px solid #abcdef; }",
    )
    .expect("valid DSS");
    let hover = stylesheet.resolve(&StyleTarget::new("button").with_state(PseudoClass::Hover));
    let preview = stylesheet.resolve(&StyleTarget::new("image").with_class("preview"));

    assert_eq!(hover.border, preview.border);
    assert!((hover.border.expect("border declaration").width - 2.0).abs() < f32::EPSILON);
}

#[test]
fn parses_border_colors_that_contain_spaces() {
    let style = resolve(
        "button { border: 2px solid rgba(12, 34, 56, 0.5); }",
        &StyleTarget::new("button"),
    );
    assert_eq!(
        style.border,
        Some(Border::new(2.0, Color::rgba8(12, 34, 56, 128)))
    );
}

#[test]
fn parses_all_documented_color_forms() {
    let cases = [
        ("transparent", Color::TRANSPARENT),
        ("black", Color::BLACK),
        ("white", Color::WHITE),
        ("#abc", Color::rgba8(0xaa, 0xbb, 0xcc, 255)),
        ("#abcd", Color::rgba8(0xaa, 0xbb, 0xcc, 0xdd)),
        ("#a1b2c3", Color::rgba8(0xa1, 0xb2, 0xc3, 255)),
        ("#a1b2c3d4", Color::rgba8(0xa1, 0xb2, 0xc3, 0xd4)),
        ("rgb(1, 2, 3)", Color::rgba8(1, 2, 3, 255)),
        ("rgb( 1 , 2 , 3 )", Color::rgba8(1, 2, 3, 255)),
        ("rgba(1, 2, 3, 0.5)", Color::rgba8(1, 2, 3, 128)),
        ("rgba(1, 2, 3, 50%)", Color::rgba8(1, 2, 3, 128)),
        ("rgba(1, 2, 3, 0)", Color::rgba8(1, 2, 3, 0)),
    ];
    for (source, expected) in cases {
        let style = resolve(
            &format!("button {{ background: {source}; }}"),
            &StyleTarget::new("button"),
        );
        assert_eq!(style.background, Some(expected), "color `{source}`");
    }
}

#[test]
fn parses_all_padding_shorthands() {
    let cases = [
        ("1px", Insets::all(1.0)),
        ("1px 2px", Insets::new(1.0, 2.0, 1.0, 2.0)),
        ("1px 2px 3px", Insets::new(1.0, 2.0, 3.0, 2.0)),
        ("1px 2px 3px 4px", Insets::new(1.0, 2.0, 3.0, 4.0)),
        ("0", Insets::all(0.0)),
    ];
    for (source, expected) in cases {
        let style = resolve(
            &format!("button {{ padding: {source}; }}"),
            &StyleTarget::new("button"),
        );
        assert_eq!(style.padding, Some(expected), "padding `{source}`");
    }
}

#[test]
fn parses_font_family_size_weight_and_style() {
    let style = resolve(
        r#"button {
            font-family: "Ubuntu", 'Noto Sans', sans-serif;
            font-size: 18px;
            font-weight: 700;
            font-style: oblique;
        }"#,
        &StyleTarget::new("button"),
    );

    assert_eq!(
        style.font_families.as_deref(),
        Some(
            [
                FontFamily::new("Ubuntu"),
                FontFamily::new("Noto Sans"),
                FontFamily::new("sans-serif"),
            ]
            .as_slice()
        )
    );
    assert_eq!(style.font_size, Some(FontSize::pixels(18.0)));
    assert_eq!(style.font_weight, Some(FontWeight::BOLD));
    assert_eq!(style.font_style, Some(FontStyle::Oblique));
}

#[test]
fn resolves_point_sizes_and_font_weight_keywords() {
    let style = resolve(
        "text { font-size: 12pt; font-weight: normal; font-style: normal; }",
        &StyleTarget::new("text"),
    );
    assert_eq!(style.font_size, Some(FontSize::points(12.0)));
    assert!((style.font_size_pixels(96.0).expect("size") - 16.0).abs() < f32::EPSILON);
    assert_eq!(style.font_weight, Some(FontWeight::NORMAL));
    assert_eq!(style.font_style, Some(FontStyle::Normal));

    let bold = resolve("text { font-weight: bold; }", &StyleTarget::new("text"));
    assert_eq!(bold.font_weight, Some(FontWeight::BOLD));
}

#[test]
fn font_properties_inherit_from_parent_styles() {
    let root = resolve(
        ":root { font-family: Root; font-size: 10px; font-weight: 300; font-style: italic; }",
        &StyleTarget::root(),
    );
    let mut child = DssStyle::default();
    child.inherit_from(&root);

    assert_eq!(child.font_families, root.font_families);
    assert_eq!(child.font_size, root.font_size);
    assert_eq!(child.font_weight, root.font_weight);
    assert_eq!(child.font_style, root.font_style);
}

#[test]
fn rejects_invalid_font_values() {
    for (property, value) in [
        ("font-size", "12"),
        ("font-size", "-1px"),
        ("font-size", "large"),
        ("font-weight", "950"),
        ("font-weight", "heavy"),
        ("font-style", "slanted"),
        ("font-family", "\"\""),
        ("font-family", "Ubuntu,,Sans"),
    ] {
        let source = format!("button {{ {property}: {value}; }}");
        let error = StyleSheet::parse(&source)
            .expect_err(&format!("`{property}: {value}` must be rejected"));
        assert!(
            error.message().contains("font"),
            "unexpected error for `{property}: {value}`: {}",
            error.message()
        );
    }
}

#[test]
fn accepts_crlf_and_unicode_comments() {
    let style = resolve(
        "/* привет 🦀 */\r\nbutton {\r\n  background: #102030;\r\n}\r\n",
        &StyleTarget::new("button"),
    );
    assert_eq!(style.background, Some(Color::rgba8(16, 32, 48, 255)));
}

#[test]
fn rejects_unknown_properties_and_invalid_values() {
    let error = StyleSheet::parse("button { width: 24px; }").expect_err("property is unsupported");
    assert!(error.message().contains("unsupported property `width`"));
    assert_eq!(error.position(), 16);

    let error = StyleSheet::parse("button { padding: -1px; }").expect_err("negative padding");
    assert!(error.message().contains("non-negative"));

    let error = StyleSheet::parse("button { padding: 1px 2px 3px 4px 5px; }")
        .expect_err("five padding values are invalid");
    assert!(error.message().contains("between one and four"));
}

#[test]
fn rejects_universal_selectors_followed_by_a_type_name() {
    let error = StyleSheet::parse("*button { background: #000; }")
        .expect_err("`*button` is not a type selector");
    assert!(error.message().contains("unsupported selector syntax"));

    assert!(StyleSheet::parse("*.primary { background: #000; }").is_ok());
    assert!(StyleSheet::parse("#id.primary:hover { background: #000; }").is_ok());
}

#[test]
fn reports_the_position_of_selector_errors() {
    let error = StyleSheet::parse("button { background: #000; }\n:unknown { color: #fff; }")
        .expect_err("unknown pseudo-class");
    assert!(
        error
            .message()
            .contains("unsupported pseudo-class `:unknown`")
    );
    assert_eq!(error.position(), 30);
}

#[test]
fn empty_sheet_and_universal_selectors_are_valid() {
    assert_eq!(StyleSheet::parse("").expect("empty sheet").rules(), []);
    let style = resolve(
        "* { color: #010203; }",
        &StyleTarget::empty().with_class("anything"),
    );
    assert_eq!(style.foreground, Some(Color::rgba8(1, 2, 3, 255)));
}

fn nested_target() -> StyleTarget {
    let stack = StyleTarget::new("stack");
    let card = StyleTarget::new("box")
        .with_class("card")
        .with_child_position(0, 2)
        .with_parent(&stack);
    StyleTarget::new("button")
        .with_class("primary")
        .with_child_position(1, 2)
        .with_parent(&card)
}

#[test]
fn matches_descendant_and_child_combinators() {
    let target = nested_target();
    let cases = [
        ("button", true),
        ("box button", true),
        ("stack button", true),
        ("stack > button", false),
        ("box > button", true),
        ("stack > box > button", true),
        ("#missing button", false),
        ("stack > box > button.primary", true),
        ("stack > box > button.danger", false),
        ("box .primary", true),
    ];
    for (source, expected) in cases {
        let stylesheet = StyleSheet::parse(&format!("{source} {{ color: #010203; }}"))
            .unwrap_or_else(|error| panic!("`{source}` must parse: {error}"));
        let matched = stylesheet.resolve(&target).foreground.is_some();
        assert_eq!(matched, expected, "selector `{source}`");
    }
}

#[test]
fn matches_structural_pseudo_classes() {
    let first = StyleTarget::new("button").with_child_position(0, 3);
    let middle = StyleTarget::new("button").with_child_position(1, 3);
    let last = StyleTarget::new("button").with_child_position(2, 3);
    let only = StyleTarget::new("button").with_child_position(0, 1);
    let detached = StyleTarget::new("button");
    let cases = [
        (":first-child", &[true, false, false, true, false]),
        (":last-child", &[false, false, true, true, false]),
        (":only-child", &[false, false, false, true, false]),
        (":nth-child(2)", &[false, true, false, false, false]),
        (":nth-child(odd)", &[true, false, true, true, false]),
        (":nth-child(even)", &[false, true, false, false, false]),
        (":nth-child(2n+1)", &[true, false, true, true, false]),
        (":nth-child(-n+2)", &[true, true, false, true, false]),
    ];
    for (source, expected) in cases {
        let stylesheet = StyleSheet::parse(&format!("button{source} {{ color: #010203; }}"))
            .unwrap_or_else(|error| panic!("`{source}` must parse: {error}"));
        for (target, expected) in [&first, &middle, &last, &only, &detached]
            .into_iter()
            .zip(expected)
        {
            assert_eq!(
                stylesheet.resolve(target).foreground.is_some(),
                *expected,
                "selector `{source}`"
            );
        }
    }
}

#[test]
fn matches_negation_and_matches_any_pseudo_classes() {
    let danger = StyleTarget::new("button").with_class("danger");
    let primary = StyleTarget::new("button").with_class("primary");

    let style = StyleSheet::parse(
        r"
            button:not(.danger) { color: #010101; }
            button:is(.primary, .secondary) { background: #020202; }
            button:not(.danger, .primary) { border-radius: 3px; }
        ",
    )
    .expect("valid DSS");

    assert_eq!(
        style.resolve(&primary).foreground,
        Some(Color::rgba8(1, 1, 1, 255))
    );
    assert_eq!(
        style.resolve(&primary).background,
        Some(Color::rgba8(2, 2, 2, 255))
    );
    assert_eq!(style.resolve(&primary).corner_radius, None);
    assert_eq!(
        style.resolve(&danger).background,
        None,
        ":is() must not match an unrelated class"
    );
}

#[test]
fn combinator_rules_keep_css_specificity_and_source_order() {
    let target = nested_target();
    let style = resolve(
        r"
            button { background: #111111; }
            box button { background: #222222; }
            .card .primary { background: #333333; }
            stack > box > button.primary { background: #444444; }
        ",
        &target,
    );
    // `.card .primary` has two class components and outranks the later rule,
    // which only has one class and three type components. Combinators do not
    // change specificity; their compounds add to it like any other selector.
    assert_eq!(style.background, Some(Color::rgba8(0x33, 0x33, 0x33, 255)));
}

#[test]
fn rejects_malformed_extended_selectors() {
    for source in [
        "button > { color: #000; }",
        "button:not() { color: #000; }",
        "button:not(.a > .b) { color: #000; }",
        "button:nth-child(foo) { color: #000; }",
        "button:nth-child() { color: #000; }",
        "button:unknown() { color: #000; }",
        "box > > button { color: #000; }",
    ] {
        let error = StyleSheet::parse(source).expect_err(&format!("`{source}` must be rejected"));
        assert!(!error.message().is_empty(), "`{source}` has a message");
    }
}

#[test]
fn random_utf8_source_never_panics() {
    const ALPHABET: &[char] = &[
        'a', 'Z', '0', '-', '_', '.', '#', ':', '*', '{', '}', ';', ',', '(', ')', '%', '#', ' ',
        '\t', '\r', '\n', '/', '\\', '"', '🦀', 'я', 'é',
    ];
    let alphabet_len = u32::try_from(ALPHABET.len()).expect("alphabet fits u32");
    let mut state = 0x1234_5678_u32;
    for _ in 0..2000 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let length = usize::try_from(state % 64).expect("modulo fits usize");
        let mut source = String::new();
        for _ in 0..length {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let index = usize::try_from(state % alphabet_len).expect("modulo fits usize");
            source.push(ALPHABET[index]);
        }
        let result = std::panic::catch_unwind(|| StyleSheet::parse(&source));
        assert!(result.is_ok(), "parser panicked on source {source:?}");
    }
}
