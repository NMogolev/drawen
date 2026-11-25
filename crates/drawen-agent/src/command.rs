//! Commands an agent sends to observe or drive the widget tree.
//!
//! Every command is plain data. With feature `json` the types serialize as
//! compact JSON objects tagged by `op`, one per line on the wire:
//!
//! ```json
//! {"op":"observe"}
//! {"op":"click","target":"Save"}
//! {"op":"click","target":{"role":"button","label":"Save"}}
//! {"op":"set_value","target":3,"value":"Grace"}
//! {"op":"press_key","key":"enter","modifiers":["ctrl"]}
//! {"op":"type_text","text":"hello"}
//! {"op":"click_at","x":120.0,"y":90.0}
//! ```

use drawen_core::{Key, Modifiers, NamedKey};

/// How a command names its target widget.
///
/// Resolution prefers an exact accessible name, then a unique case-insensitive
/// substring. An ambiguous query fails with the candidate list instead of
/// guessing, so the agent can disambiguate by id.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "json", serde(untagged))]
pub enum Target {
    /// Arena index shown as `#id` in snapshots.
    Id(u32),
    /// Accessible-name substring, any role.
    Name(String),
    /// Role and/or accessible-name query.
    Query {
        /// Role name, such as `button`; matched forgivingly (`TextField` works).
        role: Option<String>,
        /// Accessible-name substring, case-insensitive.
        label: Option<String>,
    },
}

/// A keyboard modifier held during a synthesized key press.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "lowercase"))]
pub enum Modifier {
    /// Either control key.
    Ctrl,
    /// Either shift key.
    Shift,
    /// Either alt/option key.
    Alt,
    /// Either command/super key.
    Meta,
}

/// One observation or interaction request.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "json", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "json", serde(tag = "op", rename_all = "snake_case"))]
pub enum Command {
    /// Return the current snapshot without changing anything.
    Observe,
    /// Activate a control as if it were clicked.
    Click {
        /// Widget to activate.
        target: Target,
    },
    /// Toggle a checkbox, switch, or toggle button.
    Toggle {
        /// Widget to toggle.
        target: Target,
    },
    /// Move keyboard focus to a widget.
    Focus {
        /// Widget to focus.
        target: Target,
    },
    /// Select a widget or its text content.
    Select {
        /// Widget to select.
        target: Target,
    },
    /// Replace the text value of a field or combo box.
    SetValue {
        /// Widget whose value is replaced.
        target: Target,
        /// New text value.
        value: String,
    },
    /// Increment a numeric value by one step.
    Increment {
        /// Widget to increment.
        target: Target,
    },
    /// Decrement a numeric value by one step.
    Decrement {
        /// Widget to decrement.
        target: Target,
    },
    /// Expand a disclosure, combo box, or tree item.
    Expand {
        /// Widget to expand.
        target: Target,
    },
    /// Collapse a disclosure, combo box, or tree item.
    Collapse {
        /// Widget to collapse.
        target: Target,
    },
    /// Scroll a widget's content by a logical-pixel delta.
    Scroll {
        /// Widget to scroll.
        target: Target,
        /// Horizontal delta; positive scrolls right.
        dx: f32,
        /// Vertical delta; positive scrolls down.
        dy: f32,
    },
    /// Press and release a key on the focused widget.
    ///
    /// Named keys: `backspace`, `enter`, `escape`, `tab`, `space`, `arrowleft`
    /// (or `left`), `arrowright`, `arrowup`, `arrowdown`, `home`, `end`,
    /// `pageup`, `pagedown`, `delete`. Any single character is a text key.
    PressKey {
        /// Key name or single character.
        key: String,
        /// Modifiers held during the press.
        #[cfg_attr(feature = "json", serde(default))]
        modifiers: Vec<Modifier>,
    },
    /// Commit text into the focused widget, as an input method would.
    TypeText {
        /// Text to commit.
        text: String,
    },
    /// Synthesize a primary pointer click at window coordinates.
    ///
    /// The fallback for custom widgets that expose no semantic actions; prefer
    /// [`Self::Click`] whenever the snapshot lists a `click` action.
    ClickAt {
        /// Logical x coordinate in the window's content area.
        x: f32,
        /// Logical y coordinate in the window's content area.
        y: f32,
    },
}

/// Parses a key name into a logical key value.
pub(crate) fn parse_key(key: &str) -> Option<Key> {
    let normalized = super::snapshot::normalize(key);
    let named = match normalized.as_str() {
        "backspace" => NamedKey::Backspace,
        "enter" | "return" => NamedKey::Enter,
        "escape" | "esc" => NamedKey::Escape,
        "tab" => NamedKey::Tab,
        "space" => NamedKey::Space,
        "arrowleft" | "left" => NamedKey::ArrowLeft,
        "arrowright" | "right" => NamedKey::ArrowRight,
        "arrowup" | "up" => NamedKey::ArrowUp,
        "arrowdown" | "down" => NamedKey::ArrowDown,
        "home" => NamedKey::Home,
        "end" => NamedKey::End,
        "pageup" => NamedKey::PageUp,
        "pagedown" => NamedKey::PageDown,
        "delete" | "del" => NamedKey::Delete,
        _ => return (key.chars().count() == 1).then(|| Key::Character(key.to_owned())),
    };
    Some(Key::Named(named))
}

/// Combines modifier words into a modifier set.
pub(crate) fn modifier_flags(modifiers: &[Modifier]) -> Modifiers {
    let mut flags = Modifiers::NONE;
    for modifier in modifiers {
        flags |= match modifier {
            Modifier::Ctrl => Modifiers::CONTROL,
            Modifier::Shift => Modifiers::SHIFT,
            Modifier::Alt => Modifiers::ALT,
            Modifier::Meta => Modifiers::META,
        };
    }
    flags
}

#[cfg(test)]
mod tests {
    use drawen_core::{Key, NamedKey};

    use super::{Modifier, modifier_flags, parse_key};

    #[test]
    fn key_names_are_forgiving() {
        assert_eq!(parse_key("Enter"), Some(Key::Named(NamedKey::Enter)));
        assert_eq!(
            parse_key("ARROW_DOWN"),
            Some(Key::Named(NamedKey::ArrowDown))
        );
        assert_eq!(parse_key("esc"), Some(Key::Named(NamedKey::Escape)));
        assert_eq!(parse_key("a"), Some(Key::Character("a".to_owned())));
        assert_eq!(parse_key("F5"), None);
    }

    #[test]
    fn modifiers_combine() {
        let flags = modifier_flags(&[Modifier::Ctrl, Modifier::Shift]);
        assert!(flags.contains(drawen_core::Modifiers::CONTROL));
        assert!(flags.contains(drawen_core::Modifiers::SHIFT));
        assert!(!flags.contains(drawen_core::Modifiers::ALT));
    }
}

#[cfg(all(test, feature = "json"))]
mod json_tests {
    use super::{Command, Target};

    #[test]
    fn commands_round_trip_through_json() {
        let command = Command::Click {
            target: Target::Name("Save".to_owned()),
        };
        let json = serde_json::to_string(&command).expect("command serializes");
        assert_eq!(json, r#"{"op":"click","target":"Save"}"#);
        let parsed: Command = serde_json::from_str(&json).expect("command parses");
        assert_eq!(parsed, command);

        let by_id: Command =
            serde_json::from_str(r#"{"op":"click","target":4}"#).expect("id target parses");
        assert_eq!(
            by_id,
            Command::Click {
                target: Target::Id(4)
            }
        );

        let query: Command = serde_json::from_str(
            r#"{"op":"set_value","target":{"role":"text_field","label":"Name"},"value":"Ada"}"#,
        )
        .expect("query target parses");
        assert_eq!(
            query,
            Command::SetValue {
                target: Target::Query {
                    role: Some("text_field".to_owned()),
                    label: Some("Name".to_owned()),
                },
                value: "Ada".to_owned(),
            }
        );
    }
}
