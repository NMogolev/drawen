use super::DssError;
use super::selector::{Combinator, CompoundSelector, Condition, NthChild, PseudoClass, Selector};

/// A declaration before custom properties are expanded and values are typed.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RawDeclaration {
    pub(super) property: String,
    pub(super) value: String,
    pub(super) important: bool,
    pub(super) position: usize,
}

/// One selector and its raw declaration block, in source order.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ParsedRule {
    pub(super) selector: Selector,
    pub(super) declarations: Vec<RawDeclaration>,
}

pub(super) struct Parser<'a> {
    source: &'a str,
    cursor: usize,
}

impl<'a> Parser<'a> {
    pub(super) const fn new(source: &'a str) -> Self {
        Self { source, cursor: 0 }
    }

    pub(super) fn parse_rules(mut self) -> Result<Vec<ParsedRule>, DssError> {
        let mut rules = Vec::new();
        self.skip_ignored()?;
        while !self.is_eof() {
            let selector_position = self.cursor;
            let selector_source = self.read_until_open_brace()?;
            let selectors = parse_selectors(selector_source, selector_position)?;
            self.advance_char();
            let declarations = self.parse_declarations()?;
            rules.extend(selectors.into_iter().map(|selector| ParsedRule {
                selector,
                declarations: declarations.clone(),
            }));
            self.skip_ignored()?;
        }
        Ok(rules)
    }

    fn parse_declarations(&mut self) -> Result<Vec<RawDeclaration>, DssError> {
        let mut declarations = Vec::new();
        loop {
            self.skip_ignored()?;
            if self.consume_char('}') {
                return Ok(declarations);
            }
            if self.is_eof() {
                return Err(self.error("expected `}` to close declaration block"));
            }

            let property_position = self.cursor;
            let property = self.read_property_name()?;
            self.skip_ignored()?;
            if !self.consume_char(':') {
                return Err(self.error("expected `:` after property name"));
            }
            self.skip_ignored()?;
            let value_position = self.cursor;
            let value = self.read_declaration_value()?;
            let (value, important) = split_important(value);
            let value = value.trim();
            if value.is_empty() {
                return Err(DssError::new(value_position, "expected a property value"));
            }
            declarations.push(RawDeclaration {
                property,
                value: value.to_owned().replace(['\r', '\n'], " "),
                important,
                position: value_position,
            });

            if self.consume_char(';') {
                continue;
            }
            if self.consume_char('}') {
                return Ok(declarations);
            }
            return Err(DssError::new(
                property_position,
                "expected `;` or `}` after property value",
            ));
        }
    }

    fn skip_ignored(&mut self) -> Result<(), DssError> {
        loop {
            while self.peek_char().is_some_and(char::is_whitespace) {
                self.advance_char();
            }
            if !self.remaining().starts_with("/*") {
                return Ok(());
            }
            let comment_position = self.cursor;
            self.cursor += 2;
            let Some(end) = self.remaining().find("*/") else {
                return Err(DssError::new(comment_position, "unterminated comment"));
            };
            self.cursor += end + 2;
        }
    }

    fn read_until_open_brace(&mut self) -> Result<&'a str, DssError> {
        let start = self.cursor;
        while let Some(character) = self.peek_char() {
            match character {
                '{' => return Ok(&self.source[start..self.cursor]),
                '}' => {
                    return Err(self.error("unexpected `}` outside declaration block"));
                }
                _ => self.advance_char(),
            }
        }
        Err(self.error("expected `{` after selector"))
    }

    /// Reads a property name. Custom properties are case-sensitive, standard
    /// properties are normalized so `BACKGROUND` and `background` share a slot.
    fn read_property_name(&mut self) -> Result<String, DssError> {
        let start = self.cursor;
        while self.peek_char().is_some_and(is_identifier_character) {
            self.advance_char();
        }
        if start == self.cursor {
            return Err(self.error("expected identifier"));
        }
        let property = &self.source[start..self.cursor];
        if property.starts_with("--") {
            Ok(property.to_owned())
        } else {
            Ok(property.to_ascii_lowercase())
        }
    }

    fn read_declaration_value(&mut self) -> Result<&'a str, DssError> {
        let start = self.cursor;
        let mut parentheses = 0_usize;
        while let Some(character) = self.peek_char() {
            match character {
                '(' => {
                    parentheses += 1;
                    self.advance_char();
                }
                ')' if parentheses > 0 => {
                    parentheses -= 1;
                    self.advance_char();
                }
                ')' => return Err(self.error("unexpected `)` in property value")),
                ';' | '}' if parentheses == 0 => return Ok(&self.source[start..self.cursor]),
                _ => self.advance_char(),
            }
        }
        if parentheses > 0 {
            return Err(self.error("unterminated function in property value"));
        }
        Err(self.error("expected `;` or `}` after property value"))
    }

    fn consume_char(&mut self, expected: char) -> bool {
        if self.peek_char() == Some(expected) {
            self.advance_char();
            true
        } else {
            false
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    fn advance_char(&mut self) {
        if let Some(character) = self.peek_char() {
            self.cursor += character.len_utf8();
        }
    }

    const fn is_eof(&self) -> bool {
        self.cursor == self.source.len()
    }

    fn remaining(&self) -> &str {
        &self.source[self.cursor..]
    }

    fn error(&self, message: impl Into<String>) -> DssError {
        DssError::new(self.cursor, message)
    }
}

/// Splits a selector list on commas that are not nested inside functions.
fn split_selector_list(source: &str) -> Vec<(usize, &str)> {
    let mut parts = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0_usize;
    for (index, character) in source.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push((start, &source[start..index]));
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push((start, &source[start..]));
    parts
}

fn parse_selectors(source: &str, position: usize) -> Result<Vec<Selector>, DssError> {
    let mut selectors = Vec::new();
    for (offset, part) in split_selector_list(source) {
        let trimmed = part.trim();
        let leading_whitespace = part.len() - part.trim_start().len();
        selectors.push(parse_selector(
            trimmed,
            position + offset + leading_whitespace,
        )?);
    }
    Ok(selectors)
}

fn parse_selector(source: &str, position: usize) -> Result<Selector, DssError> {
    if source.is_empty() {
        return Err(selector_error(position, "expected selector"));
    }
    if source == ":root" {
        return Ok(Selector {
            is_root: true,
            specificity: (0, 1, 0),
            ..Selector::default()
        });
    }

    let mut compounds = Vec::new();
    let mut combinators = Vec::new();
    let mut cursor = 0;
    skip_whitespace(source, &mut cursor);
    loop {
        let (compound, next) = parse_compound(source, cursor, position)?;
        compounds.push(compound);
        cursor = next;
        let after_compound = cursor;
        skip_whitespace(source, &mut cursor);
        if cursor == source.len() {
            break;
        }
        if source[cursor..].starts_with('>') {
            cursor += 1;
            skip_whitespace(source, &mut cursor);
            if cursor == source.len() {
                return Err(selector_error(
                    position + cursor,
                    "expected a selector after `>`",
                ));
            }
            combinators.push(Combinator::Child);
        } else if cursor > after_compound {
            combinators.push(Combinator::Descendant);
        } else {
            return Err(selector_error(
                position + cursor,
                "unsupported selector syntax",
            ));
        }
    }

    let specificity = compounds.iter().fold((0, 0, 0), |total, compound| {
        let extra = compound.specificity();
        (total.0 + extra.0, total.1 + extra.1, total.2 + extra.2)
    });
    Ok(Selector {
        compounds,
        combinators,
        is_root: false,
        specificity,
    })
}

fn parse_compound(
    source: &str,
    start: usize,
    position: usize,
) -> Result<(CompoundSelector, usize), DssError> {
    let mut compound = CompoundSelector::default();
    let mut cursor = start;
    let mut saw_universal = false;
    let mut saw_any = false;
    while let Some(character) = peek_char(source, cursor) {
        match character {
            '*' if cursor == start => {
                saw_universal = true;
                saw_any = true;
                cursor += 1;
            }
            '.' => {
                saw_any = true;
                cursor += 1;
                let (class, next) = read_identifier(source, cursor, position)?;
                if !compound.classes.contains(&class) {
                    compound.classes.push(class);
                }
                cursor = next;
            }
            '#' => {
                saw_any = true;
                if compound.id.is_some() {
                    return Err(selector_error(
                        position + cursor,
                        "selector may contain only one id",
                    ));
                }
                cursor += 1;
                let (id, next) = read_identifier(source, cursor, position)?;
                compound.id = Some(id);
                cursor = next;
            }
            ':' => {
                saw_any = true;
                cursor += 1;
                let (name, next) = read_identifier(source, cursor, position)?;
                cursor = next;
                let condition = if peek_char(source, cursor) == Some('(') {
                    let (arguments, after) = read_arguments(source, cursor, position)?;
                    cursor = after;
                    parse_functional(&name, arguments, position + cursor)?
                } else {
                    simple_condition(&name).ok_or_else(|| {
                        selector_error(
                            position + cursor - name.len(),
                            format!("unsupported pseudo-class `:{name}`"),
                        )
                    })?
                };
                if !compound.conditions.contains(&condition) {
                    compound.conditions.push(condition);
                }
            }
            character
                if is_identifier_character(character)
                    && compound.type_name.is_none()
                    && !saw_universal =>
            {
                saw_any = true;
                let (type_name, next) = read_identifier(source, cursor, position)?;
                compound.type_name = Some(type_name.to_ascii_lowercase());
                cursor = next;
            }
            character if is_identifier_character(character) => {
                return Err(selector_error(
                    position + cursor,
                    "unsupported selector syntax",
                ));
            }
            _ => break,
        }
    }
    if !saw_any {
        return Err(selector_error(position + cursor, "expected selector"));
    }
    Ok((compound, cursor))
}

fn parse_compound_list(source: &str, position: usize) -> Result<Vec<CompoundSelector>, DssError> {
    let mut compounds = Vec::new();
    for (offset, part) in split_selector_list(source) {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            return Err(selector_error(
                position + offset,
                "expected a selector inside a pseudo-class function",
            ));
        }
        let leading_whitespace = part.len() - part.trim_start().len();
        let (compound, cursor) =
            parse_compound(trimmed, 0, position + offset + leading_whitespace)?;
        let mut remainder = cursor;
        skip_whitespace(trimmed, &mut remainder);
        if remainder != trimmed.len() {
            return Err(selector_error(
                position + offset + leading_whitespace + remainder,
                "combinators are not allowed inside `:not()` or `:is()`",
            ));
        }
        compounds.push(compound);
    }
    Ok(compounds)
}

fn simple_condition(name: &str) -> Option<Condition> {
    Some(match name {
        "hover" => Condition::State(PseudoClass::Hover),
        "pressed" | "active" => Condition::State(PseudoClass::Pressed),
        "focus" => Condition::State(PseudoClass::Focus),
        "disabled" => Condition::State(PseudoClass::Disabled),
        "checked" => Condition::State(PseudoClass::Checked),
        "selected" => Condition::State(PseudoClass::Selected),
        "first-child" => Condition::FirstChild,
        "last-child" => Condition::LastChild,
        "only-child" => Condition::OnlyChild,
        _ => return None,
    })
}

fn parse_functional(name: &str, arguments: &str, position: usize) -> Result<Condition, DssError> {
    match name {
        "not" => Ok(Condition::Not(parse_compound_list(arguments, position)?)),
        "is" => Ok(Condition::Is(parse_compound_list(arguments, position)?)),
        "nth-child" => Ok(Condition::NthChild(parse_nth_child(arguments, position)?)),
        _ => Err(selector_error(
            position,
            format!("unsupported pseudo-class `:{name}()`"),
        )),
    }
}

fn parse_nth_child(arguments: &str, position: usize) -> Result<NthChild, DssError> {
    let compact: String = arguments
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    let (a, b) = match compact.as_str() {
        "odd" => (2, 1),
        "even" => (2, 0),
        "" => {
            return Err(selector_error(
                position,
                "`:nth-child()` requires an An+B expression",
            ));
        }
        compact => {
            if let Some((a, b)) = compact.split_once('n') {
                let a = match a {
                    "" | "+" => 1,
                    "-" => -1,
                    value => value.parse::<i32>().map_err(|_| {
                        selector_error(
                            position,
                            format!("invalid `:nth-child()` expression `{compact}`"),
                        )
                    })?,
                };
                let b = if b.is_empty() {
                    0
                } else {
                    b.parse::<i32>().map_err(|_| {
                        selector_error(
                            position,
                            format!("invalid `:nth-child()` expression `{compact}`"),
                        )
                    })?
                };
                (a, b)
            } else {
                let b = compact.parse::<i32>().map_err(|_| {
                    selector_error(
                        position,
                        format!("invalid `:nth-child()` expression `{compact}`"),
                    )
                })?;
                (0, b)
            }
        }
    };
    Ok(NthChild::new(a, b))
}

fn read_arguments(source: &str, cursor: usize, position: usize) -> Result<(&str, usize), DssError> {
    let start = cursor + 1;
    let mut depth = 1_usize;
    let mut index = start;
    while index < source.len() {
        let character = peek_char(source, index).expect("index is within source");
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok((&source[start..index], index + 1));
                }
            }
            _ => {}
        }
        index += character.len_utf8();
    }
    Err(selector_error(
        position + cursor,
        "unterminated pseudo-class arguments",
    ))
}

fn read_identifier(
    source: &str,
    cursor: usize,
    position: usize,
) -> Result<(String, usize), DssError> {
    let remainder = &source[cursor..];
    let length = remainder
        .char_indices()
        .take_while(|(_, character)| is_identifier_character(*character))
        .map(|(index, character)| index + character.len_utf8())
        .last()
        .unwrap_or(0);
    if length == 0 {
        return Err(selector_error(
            position + cursor,
            "expected selector identifier",
        ));
    }
    Ok((remainder[..length].to_owned(), cursor + length))
}

fn skip_whitespace(source: &str, cursor: &mut usize) {
    while peek_char(source, *cursor).is_some_and(char::is_whitespace) {
        let character = peek_char(source, *cursor).expect("character was checked");
        *cursor += character.len_utf8();
    }
}

fn peek_char(source: &str, cursor: usize) -> Option<char> {
    source.get(cursor..)?.chars().next()
}

/// Splits a trailing `!important` flag from a declaration value.
fn split_important(value: &str) -> (&str, bool) {
    let trimmed = value.trim_end();
    let lower = trimmed.to_ascii_lowercase();
    if lower.ends_with("!important") {
        (&trimmed[..trimmed.len() - "!important".len()], true)
    } else {
        (trimmed, false)
    }
}

pub(super) fn is_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
}

fn selector_error(position: usize, message: impl Into<String>) -> DssError {
    DssError::new(position, message)
}
