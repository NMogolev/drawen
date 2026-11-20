use super::DssError;
use super::selector::{PseudoClass, Selector};
use super::style::DssStyle;
use super::value::set_property;

pub(super) struct Parser<'a> {
    source: &'a str,
    cursor: usize,
}

impl<'a> Parser<'a> {
    pub(super) const fn new(source: &'a str) -> Self {
        Self { source, cursor: 0 }
    }

    pub(super) fn parse_rules(mut self) -> Result<Vec<(Selector, DssStyle)>, DssError> {
        let mut rules = Vec::new();
        self.skip_ignored()?;
        while !self.is_eof() {
            let selector_position = self.cursor;
            let selector_source = self.read_until_open_brace()?;
            let selectors = parse_selectors(selector_source, selector_position)?;
            self.advance_char();
            let style = self.parse_declarations()?;
            rules.extend(
                selectors
                    .into_iter()
                    .map(|selector| (selector, style.clone())),
            );
            self.skip_ignored()?;
        }
        Ok(rules)
    }

    fn parse_declarations(&mut self) -> Result<DssStyle, DssError> {
        let mut style = DssStyle::default();
        loop {
            self.skip_ignored()?;
            if self.consume_char('}') {
                return Ok(style);
            }
            if self.is_eof() {
                return Err(self.error("expected `}` to close declaration block"));
            }

            let property_position = self.cursor;
            let property = self.read_identifier()?;
            self.skip_ignored()?;
            if !self.consume_char(':') {
                return Err(self.error("expected `:` after property name"));
            }
            self.skip_ignored()?;
            let value_position = self.cursor;
            let value = self.read_declaration_value()?;
            if value.trim().is_empty() {
                return Err(DssError::new(value_position, "expected a property value"));
            }
            set_property(&mut style, &property, value.trim(), value_position)?;

            if self.consume_char(';') {
                continue;
            }
            if self.consume_char('}') {
                return Ok(style);
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

    fn read_identifier(&mut self) -> Result<String, DssError> {
        let start = self.cursor;
        while self.peek_char().is_some_and(is_identifier_character) {
            self.advance_char();
        }
        if start == self.cursor {
            return Err(self.error("expected identifier"));
        }
        Ok(self.source[start..self.cursor].to_ascii_lowercase())
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

fn parse_selectors(source: &str, position: usize) -> Result<Vec<Selector>, DssError> {
    let mut selectors = Vec::new();
    let mut offset = 0;
    for selector_source in source.split(',') {
        let selector = selector_source.trim();
        let leading_whitespace = selector_source.len() - selector_source.trim_start().len();
        selectors.push(parse_selector(
            selector,
            position + offset + leading_whitespace,
        )?);
        offset += selector_source.len() + 1;
    }
    Ok(selectors)
}

fn parse_selector(source: &str, position: usize) -> Result<Selector, DssError> {
    if source.is_empty() {
        return Err(DssError::new(position, "expected selector"));
    }
    if source == ":root" {
        return Ok(Selector {
            is_root: true,
            ..Selector::default()
        });
    }

    let mut selector = Selector::default();
    let mut cursor = 0;
    let mut saw_universal = false;
    while cursor < source.len() {
        let character = source[cursor..]
            .chars()
            .next()
            .expect("cursor is within selector source");
        match character {
            '*' if cursor == 0 => {
                saw_universal = true;
                cursor += 1;
            }
            '.' => {
                cursor += 1;
                let (class, next) = read_selector_identifier(source, cursor, position)?;
                if !selector.classes.contains(&class) {
                    selector.classes.push(class);
                }
                cursor = next;
            }
            '#' => {
                if selector.id.is_some() {
                    return Err(selector_error(
                        position + cursor,
                        "selector may contain only one id",
                    ));
                }
                cursor += 1;
                let (id, next) = read_selector_identifier(source, cursor, position)?;
                selector.id = Some(id);
                cursor = next;
            }
            ':' => {
                cursor += 1;
                let (pseudo_class, next) = read_selector_identifier(source, cursor, position)?;
                let state = match pseudo_class.as_str() {
                    "hover" => PseudoClass::Hover,
                    "pressed" => PseudoClass::Pressed,
                    "focus" => PseudoClass::Focus,
                    "disabled" => PseudoClass::Disabled,
                    _ => {
                        return Err(selector_error(
                            position + cursor,
                            format!("unsupported pseudo-class `:{pseudo_class}`"),
                        ));
                    }
                };
                if !selector.pseudo_classes.contains(&state) {
                    selector.pseudo_classes.push(state);
                }
                cursor = next;
            }
            _ if is_identifier_character(character)
                && selector.type_name.is_none()
                && !saw_universal =>
            {
                let (type_name, next) = read_selector_identifier(source, cursor, position)?;
                selector.type_name = Some(type_name.to_ascii_lowercase());
                cursor = next;
            }
            _ => {
                return Err(selector_error(
                    position + cursor,
                    "unsupported selector syntax",
                ));
            }
        }
    }
    Ok(selector)
}

fn read_selector_identifier(
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

fn is_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
}

fn selector_error(position: usize, message: impl Into<String>) -> DssError {
    DssError::new(position, message)
}
