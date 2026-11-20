use drawen_core::{Color, Insets};
use drawen_render::{FontFamily, FontSize, FontStyle, FontWeight};

use crate::Border;

use super::DssError;
use super::style::DssStyle;

pub(super) fn set_property(
    style: &mut DssStyle,
    property: &str,
    value: &str,
    position: usize,
) -> Result<(), DssError> {
    match property {
        "background" | "background-color" => style.background = Some(parse_color(value, position)?),
        "color" => style.foreground = Some(parse_color(value, position)?),
        "accent-color" => style.accent = Some(parse_color(value, position)?),
        "border" => style.border = Some(parse_border(value, position)?),
        "padding" => style.padding = Some(parse_padding(value, position)?),
        "gap" => style.spacing = Some(parse_length(value, position)?),
        "font-family" => style.font_families = Some(parse_font_families(value, position)?),
        "font-size" => style.font_size = Some(parse_font_size(value, position)?),
        "font-weight" => style.font_weight = Some(parse_font_weight(value, position)?),
        "font-style" => style.font_style = Some(parse_font_style(value, position)?),
        "border-radius" => style.corner_radius = Some(parse_length(value, position)?),
        _ => {
            return Err(DssError::new(
                position,
                format!("unsupported property `{property}`"),
            ));
        }
    }
    Ok(())
}

pub(super) fn parse_color(value: &str, position: usize) -> Result<Color, DssError> {
    if value.eq_ignore_ascii_case("transparent") {
        return Ok(Color::TRANSPARENT);
    }
    if value.eq_ignore_ascii_case("black") {
        return Ok(Color::BLACK);
    }
    if value.eq_ignore_ascii_case("white") {
        return Ok(Color::WHITE);
    }
    if let Some(hex) = value.strip_prefix('#') {
        return parse_hex_color(hex, position);
    }
    if let Some(arguments) = value
        .strip_prefix("rgb(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let channels = parse_color_channels(arguments, 3, position)?;
        return Ok(Color::rgba8(channels[0], channels[1], channels[2], 255));
    }
    if let Some(arguments) = value
        .strip_prefix("rgba(")
        .and_then(|value| value.strip_suffix(')'))
    {
        let channels = parse_color_channels(arguments, 4, position)?;
        return Ok(Color::rgba8(
            channels[0],
            channels[1],
            channels[2],
            channels[3],
        ));
    }
    Err(DssError::new(position, format!("invalid color `{value}`")))
}

fn parse_hex_color(hex: &str, position: usize) -> Result<Color, DssError> {
    let components = match hex.len() {
        3 => [
            duplicate_hex_digit(hex, 0, position)?,
            duplicate_hex_digit(hex, 1, position)?,
            duplicate_hex_digit(hex, 2, position)?,
            255,
        ],
        4 => [
            duplicate_hex_digit(hex, 0, position)?,
            duplicate_hex_digit(hex, 1, position)?,
            duplicate_hex_digit(hex, 2, position)?,
            duplicate_hex_digit(hex, 3, position)?,
        ],
        6 => [
            parse_hex_pair(hex, 0, position)?,
            parse_hex_pair(hex, 2, position)?,
            parse_hex_pair(hex, 4, position)?,
            255,
        ],
        8 => [
            parse_hex_pair(hex, 0, position)?,
            parse_hex_pair(hex, 2, position)?,
            parse_hex_pair(hex, 4, position)?,
            parse_hex_pair(hex, 6, position)?,
        ],
        _ => {
            return Err(DssError::new(
                position,
                "hex colors must contain 3, 4, 6, or 8 digits",
            ));
        }
    };
    Ok(Color::rgba8(
        components[0],
        components[1],
        components[2],
        components[3],
    ))
}

fn duplicate_hex_digit(hex: &str, index: usize, position: usize) -> Result<u8, DssError> {
    let digit = hex
        .as_bytes()
        .get(index)
        .copied()
        .and_then(hex_value)
        .ok_or_else(|| DssError::new(position + 1 + index, "invalid hexadecimal color digit"))?;
    Ok(digit * 17)
}

fn parse_hex_pair(hex: &str, index: usize, position: usize) -> Result<u8, DssError> {
    u8::from_str_radix(&hex[index..index + 2], 16)
        .map_err(|_| DssError::new(position + 1 + index, "invalid hexadecimal color digit"))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn parse_color_channels(
    arguments: &str,
    expected: usize,
    position: usize,
) -> Result<Vec<u8>, DssError> {
    let channels: Vec<&str> = arguments.split(',').map(str::trim).collect();
    if channels.len() != expected {
        return Err(DssError::new(
            position,
            format!("expected {expected} color channels"),
        ));
    }
    channels
        .into_iter()
        .enumerate()
        .map(|(index, channel)| {
            if expected == 4 && index == 3 {
                parse_alpha(channel, position)
            } else {
                channel.parse::<u8>().map_err(|_| {
                    DssError::new(position, format!("invalid color channel `{channel}`"))
                })
            }
        })
        .collect()
}

fn parse_alpha(value: &str, position: usize) -> Result<u8, DssError> {
    let alpha = if let Some(percent) = value.strip_suffix('%') {
        let percent = percent
            .parse::<f32>()
            .map_err(|_| DssError::new(position, format!("invalid alpha channel `{value}`")))?;
        percent / 100.0
    } else {
        value
            .parse::<f32>()
            .map_err(|_| DssError::new(position, format!("invalid alpha channel `{value}`")))?
    };
    if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
        return Err(DssError::new(
            position,
            "alpha channel must be between 0 and 1",
        ));
    }
    Ok(round_alpha_channel(alpha))
}

fn round_alpha_channel(alpha: f32) -> u8 {
    // `parse_alpha` admits only finite values in the inclusive 0.0..=1.0
    // range, so the rounded result is necessarily an unsigned byte.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        (alpha * 255.0).round() as u8
    }
}

fn parse_border(value: &str, position: usize) -> Result<Border, DssError> {
    // Split only the first two whitespace-delimited tokens. The remainder is
    // the color and may itself contain spaces, as in
    // `2px solid rgba(12, 34, 56, 0.5)`.
    let value = value.trim();
    let Some((width, rest)) = value.split_once(char::is_whitespace) else {
        return Err(DssError::new(
            position,
            "border must be `<width> solid <color>`",
        ));
    };
    let rest = rest.trim_start();
    let Some((line_style, color)) = rest.split_once(char::is_whitespace) else {
        return Err(DssError::new(
            position,
            "border must be `<width> solid <color>`",
        ));
    };
    if !line_style.eq_ignore_ascii_case("solid") {
        return Err(DssError::new(
            position,
            "only `solid` borders are supported",
        ));
    }
    let color = color.trim();
    if color.is_empty() {
        return Err(DssError::new(
            position,
            "border must be `<width> solid <color>`",
        ));
    }
    Ok(Border::new(
        parse_length(width, position)?,
        parse_color(color, position)?,
    ))
}

fn parse_padding(value: &str, position: usize) -> Result<Insets, DssError> {
    let values: Result<Vec<f32>, DssError> = value
        .split_ascii_whitespace()
        .map(|part| parse_length(part, position))
        .collect();
    let values = values?;
    match values.as_slice() {
        [all] => Ok(Insets::all(*all)),
        [vertical, horizontal] => Ok(Insets::new(*vertical, *horizontal, *vertical, *horizontal)),
        [top, horizontal, bottom] => Ok(Insets::new(*top, *horizontal, *bottom, *horizontal)),
        [top, right, bottom, left] => Ok(Insets::new(*top, *right, *bottom, *left)),
        _ => Err(DssError::new(
            position,
            "padding requires between one and four lengths",
        )),
    }
}

fn parse_font_families(value: &str, position: usize) -> Result<Vec<FontFamily>, DssError> {
    let mut families = Vec::new();
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err(DssError::new(
                position,
                "font-family requires non-empty family names",
            ));
        }
        let name = unquote_font_family(part);
        if name.is_empty() {
            return Err(DssError::new(
                position,
                "font-family requires non-empty family names",
            ));
        }
        families.push(FontFamily::new(name));
    }
    if families.is_empty() {
        return Err(DssError::new(
            position,
            "font-family requires at least one family name",
        ));
    }
    Ok(families)
}

fn unquote_font_family(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        let first = bytes[0];
        let last = bytes[value.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn parse_font_size(value: &str, position: usize) -> Result<FontSize, DssError> {
    let value = value.trim();
    if let Some(number) = value.strip_suffix("px") {
        return Ok(FontSize::pixels(parse_font_number(
            number.trim(),
            value,
            position,
        )?));
    }
    if let Some(number) = value.strip_suffix("pt") {
        return Ok(FontSize::points(parse_font_number(
            number.trim(),
            value,
            position,
        )?));
    }
    if value == "0" {
        return Ok(FontSize::pixels(0.0));
    }
    Err(DssError::new(
        position,
        "font-size must be a non-negative `px` or `pt` length",
    ))
}

fn parse_font_number(number: &str, value: &str, position: usize) -> Result<f32, DssError> {
    let number = number
        .parse::<f32>()
        .map_err(|_| DssError::new(position, format!("invalid font length `{value}`")))?;
    if !number.is_finite() || number < 0.0 {
        return Err(DssError::new(
            position,
            "font length must be finite and non-negative",
        ));
    }
    Ok(number)
}

fn parse_font_weight(value: &str, position: usize) -> Result<FontWeight, DssError> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "normal" => Ok(FontWeight::NORMAL),
        "bold" => Ok(FontWeight::BOLD),
        _ => {
            let weight = normalized
                .parse::<u16>()
                .map_err(|_| DssError::new(position, format!("invalid font-weight `{value}`")))?;
            if !(100..=900).contains(&weight) {
                return Err(DssError::new(
                    position,
                    "font-weight must be `normal`, `bold`, or 100 through 900",
                ));
            }
            FontWeight::new(weight).ok_or_else(|| {
                DssError::new(position, "font-weight is outside the supported range")
            })
        }
    }
}

fn parse_font_style(value: &str, position: usize) -> Result<FontStyle, DssError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "normal" => Ok(FontStyle::Normal),
        "italic" => Ok(FontStyle::Italic),
        "oblique" => Ok(FontStyle::Oblique),
        _ => Err(DssError::new(
            position,
            format!("invalid font-style `{value}`"),
        )),
    }
}

pub(super) fn parse_length(value: &str, position: usize) -> Result<f32, DssError> {
    let value = value.trim();
    let (number, unitless) = match value.strip_suffix("px") {
        Some(number) => (number.trim(), false),
        None => (value, true),
    };
    let length = number
        .parse::<f32>()
        .map_err(|_| DssError::new(position, format!("invalid length `{value}`")))?;
    if !length.is_finite() || length < 0.0 {
        return Err(DssError::new(
            position,
            "length must be finite and non-negative",
        ));
    }
    if unitless && length != 0.0 {
        return Err(DssError::new(
            position,
            "non-zero lengths must use the `px` unit",
        ));
    }
    Ok(length)
}
