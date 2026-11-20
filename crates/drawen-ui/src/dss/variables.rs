use std::collections::HashMap;

use super::DssError;

/// Global custom properties collected while parsing a stylesheet.
pub(super) type CustomProperties = HashMap<String, String>;

/// Maximum number of nested `var()` expansions before reporting a cycle.
const MAX_EXPANSION_DEPTH: usize = 32;

/// Replaces every `var(--name[, fallback])` in `value`.
pub(super) fn substitute(
    value: &str,
    properties: &CustomProperties,
    position: usize,
) -> Result<String, DssError> {
    substitute_inner(value, properties, position, 0)
}

fn substitute_inner(
    value: &str,
    properties: &CustomProperties,
    position: usize,
    depth: usize,
) -> Result<String, DssError> {
    if depth > MAX_EXPANSION_DEPTH {
        return Err(DssError::new(
            position,
            "custom property expansion exceeds 32 levels; check for a reference cycle",
        ));
    }

    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while cursor < value.len() {
        let remainder = &value[cursor..];
        let lower = remainder.to_ascii_lowercase();
        let Some(start) = find_function(&lower, "var(") else {
            output.push_str(remainder);
            break;
        };
        output.push_str(&remainder[..start]);
        let open = cursor + start + "var(".len();
        let Some(close) = find_matching_parenthesis(value, open) else {
            return Err(DssError::new(
                position,
                "unterminated `var()` in property value",
            ));
        };
        let arguments = &value[open..close];
        output.push_str(&resolve_var(arguments, properties, position, depth)?);
        cursor = close + 1;
    }
    Ok(output)
}

fn resolve_var(
    arguments: &str,
    properties: &CustomProperties,
    position: usize,
    depth: usize,
) -> Result<String, DssError> {
    let (name, fallback) = split_fallback(arguments);
    let name = name.trim();
    if !name.starts_with("--") || name.len() == 2 {
        return Err(DssError::new(
            position,
            format!("`var()` requires a custom property name, found `{name}`"),
        ));
    }
    if let Some(value) = properties.get(name) {
        return substitute_inner(value, properties, position, depth + 1);
    }
    if let Some(fallback) = fallback {
        return substitute_inner(fallback.trim(), properties, position, depth + 1);
    }
    Err(DssError::new(
        position,
        format!("undefined custom property `{name}`"),
    ))
}

/// Splits `--name, fallback` at the first top-level comma.
fn split_fallback(arguments: &str) -> (&str, Option<&str>) {
    let mut depth = 0_usize;
    for (index, character) in arguments.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => return (&arguments[..index], Some(&arguments[index + 1..])),
            _ => {}
        }
    }
    (arguments, None)
}

/// Finds `needle` in an already lowercased string that is not part of a longer
/// identifier.
fn find_function(haystack: &str, needle: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(offset) = haystack[from..].find(needle) {
        let index = from + offset;
        let preceded_by_identifier = haystack[..index]
            .chars()
            .next_back()
            .is_some_and(|character| character.is_ascii_alphanumeric() || character == '-');
        if !preceded_by_identifier {
            return Some(index);
        }
        from = index + needle.len();
    }
    None
}

/// Returns the index of the `)` matching the `(` that ends just before `open`.
fn find_matching_parenthesis(value: &str, open: usize) -> Option<usize> {
    let mut depth = 1_usize;
    for (offset, character) in value[open..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}
