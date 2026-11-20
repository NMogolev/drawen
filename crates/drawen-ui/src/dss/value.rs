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
        "width" => style.width = Some(parse_length(value, position)?),
        "height" => style.height = Some(parse_length(value, position)?),
        "min-width" => style.min_width = Some(parse_length(value, position)?),
        "min-height" => style.min_height = Some(parse_length(value, position)?),
        "max-width" => style.max_width = Some(parse_length(value, position)?),
        "max-height" => style.max_height = Some(parse_length(value, position)?),
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
    let value = value.trim();
    if let Some(color) = named_color(value) {
        return Ok(color);
    }
    if let Some(hex) = value.strip_prefix('#') {
        return parse_hex_color(hex, position);
    }
    if let Some(arguments) =
        function_arguments(value, "rgb").or_else(|| function_arguments(value, "rgba"))
    {
        return parse_rgb_color(arguments, position);
    }
    if let Some(arguments) =
        function_arguments(value, "hsl").or_else(|| function_arguments(value, "hsla"))
    {
        return parse_hsl_color(arguments, position);
    }
    Err(DssError::new(position, format!("invalid color `{value}`")))
}

/// Returns the CSS named color table entry for `value`.
fn named_color(value: &str) -> Option<Color> {
    if value.eq_ignore_ascii_case("transparent") {
        return Some(Color::TRANSPARENT);
    }
    NAMED_COLORS
        .iter()
        .find(|(name, _)| value.eq_ignore_ascii_case(name))
        .map(|(_, rgb)| {
            let channel =
                |shift: u32| u8::try_from((rgb >> shift) & 0xff).expect("table channels are bytes");
            Color::rgba8(channel(16), channel(8), channel(0), 255)
        })
}

/// CSS Color Level 4 named colors, without the `transparent` keyword.
#[allow(clippy::unreadable_literal)]
const NAMED_COLORS: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

/// Returns the argument text of a single-argument CSS function.
pub(super) fn function_arguments<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    let value = value.trim();
    if !value.ends_with(')') {
        return None;
    }
    let open = value.find('(')?;
    if !value[..open].trim().eq_ignore_ascii_case(name) {
        return None;
    }
    Some(&value[open + 1..value.len() - 1])
}

fn parse_rgb_color(arguments: &str, position: usize) -> Result<Color, DssError> {
    let (channels, alpha) = split_color_arguments(arguments, position)?;
    let red = parse_rgb_channel(channels[0], position)?;
    let green = parse_rgb_channel(channels[1], position)?;
    let blue = parse_rgb_channel(channels[2], position)?;
    let alpha = match alpha {
        Some(alpha) => parse_alpha(alpha, position)?,
        None => 255,
    };
    Ok(Color::rgba8(red, green, blue, alpha))
}

fn parse_hsl_color(arguments: &str, position: usize) -> Result<Color, DssError> {
    let (channels, alpha) = split_color_arguments(arguments, position)?;
    let hue = parse_hue(channels[0], position)?;
    let saturation = parse_percentage(channels[1], position)?;
    let lightness = parse_percentage(channels[2], position)?;
    let (red, green, blue) = hsl_to_rgb(hue, saturation, lightness);
    let alpha = match alpha {
        Some(alpha) => parse_alpha(alpha, position)?,
        None => 255,
    };
    Ok(Color::rgba8(red, green, blue, alpha))
}

/// Splits `r, g, b[, a]` or the modern `r g b [/ a]` argument syntax.
fn split_color_arguments(
    arguments: &str,
    position: usize,
) -> Result<(Vec<&str>, Option<&str>), DssError> {
    if arguments.contains(',') {
        let parts: Vec<&str> = arguments.split(',').map(str::trim).collect();
        return match parts.as_slice() {
            [red, green, blue] => Ok((vec![red, green, blue], None)),
            [red, green, blue, alpha] => Ok((vec![red, green, blue], Some(alpha))),
            _ => Err(DssError::new(
                position,
                "color functions require three channels and an optional alpha",
            )),
        };
    }

    let mut channels = Vec::new();
    let mut alpha = None;
    let mut after_slash = false;
    for token in arguments.split_whitespace() {
        if token == "/" {
            if after_slash {
                return Err(DssError::new(position, "color alpha may only be set once"));
            }
            after_slash = true;
            continue;
        }
        if let Some((before, after)) = token.split_once('/') {
            if !before.is_empty() {
                if after_slash {
                    return Err(DssError::new(position, "color alpha may only be set once"));
                }
                channels.push(before);
            }
            if after_slash && alpha.is_some() {
                return Err(DssError::new(position, "color alpha may only be set once"));
            }
            after_slash = true;
            if !after.is_empty() {
                alpha = Some(after);
            }
            continue;
        }
        if after_slash {
            if alpha.is_some() {
                return Err(DssError::new(position, "color alpha may only be set once"));
            }
            alpha = Some(token);
        } else {
            channels.push(token);
        }
    }

    if channels.len() != 3 {
        return Err(DssError::new(
            position,
            "color functions require three channels and an optional alpha",
        ));
    }
    if after_slash && alpha.is_none() {
        return Err(DssError::new(
            position,
            "expected an alpha channel after `/`",
        ));
    }
    Ok((channels, alpha))
}

fn parse_rgb_channel(value: &str, position: usize) -> Result<u8, DssError> {
    if let Some(percent) = value.strip_suffix('%') {
        let percent = percent
            .parse::<f32>()
            .map_err(|_| DssError::new(position, format!("invalid color channel `{value}`")))?;
        if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
            return Err(DssError::new(
                position,
                "color percentages must be between 0% and 100%",
            ));
        }
        return Ok(channel(percent / 100.0));
    }
    value
        .parse::<u8>()
        .map_err(|_| DssError::new(position, format!("invalid color channel `{value}`")))
}

fn parse_hue(value: &str, position: usize) -> Result<f32, DssError> {
    let number = value
        .strip_suffix("deg")
        .or_else(|| value.strip_suffix("DEG"))
        .unwrap_or(value)
        .trim();
    let hue = number
        .parse::<f32>()
        .map_err(|_| DssError::new(position, format!("invalid hue `{value}`")))?;
    if !hue.is_finite() {
        return Err(DssError::new(position, format!("invalid hue `{value}`")));
    }
    Ok(hue)
}

fn parse_percentage(value: &str, position: usize) -> Result<f32, DssError> {
    let Some(percent) = value.strip_suffix('%') else {
        return Err(DssError::new(
            position,
            format!("expected a percentage, found `{value}`"),
        ));
    };
    let percent = percent
        .parse::<f32>()
        .map_err(|_| DssError::new(position, format!("invalid percentage `{value}`")))?;
    if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
        return Err(DssError::new(
            position,
            "percentages must be between 0% and 100%",
        ));
    }
    Ok(percent)
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> (u8, u8, u8) {
    let hue = hue.rem_euclid(360.0) / 60.0;
    let saturation = saturation / 100.0;
    let lightness = lightness / 100.0;
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let secondary = chroma * (1.0 - (hue.rem_euclid(2.0) - 1.0).abs());
    let offset = lightness - chroma / 2.0;
    // `hue` is in 0.0..6.0 because of `rem_euclid(360.0) / 60.0`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let sector = hue as u32;
    let (red, green, blue) = match sector {
        0 => (chroma, secondary, 0.0),
        1 => (secondary, chroma, 0.0),
        2 => (0.0, chroma, secondary),
        3 => (0.0, secondary, chroma),
        4 => (secondary, 0.0, chroma),
        _ => (chroma, 0.0, secondary),
    };
    (
        channel(red + offset),
        channel(green + offset),
        channel(blue + offset),
    )
}

fn channel(value: f32) -> u8 {
    let scaled = (value * 255.0).round().clamp(0.0, 255.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        scaled as u8
    }
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
    // Split only the first two top-level tokens. Both the width and the color
    // may contain spaces, as in `calc(1px + 1px) solid rgba(12, 34, 56, 0.5)`.
    let value = value.trim();
    let Some((width, rest)) = split_first_token(value) else {
        return Err(DssError::new(
            position,
            "border must be `<width> solid <color>`",
        ));
    };
    let Some((line_style, color)) = split_first_token(rest) else {
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
    let values: Result<Vec<f32>, DssError> = split_top_level_whitespace(value)
        .into_iter()
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
    if function_arguments(value, "calc").is_some() {
        return Ok(FontSize::pixels(parse_length(value, position)?));
    }
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
        "font-size must be a non-negative `px`, `pt`, or `calc()` length",
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

/// Splits a value on whitespace that is not inside a function's parentheses.
fn split_top_level_whitespace(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0_usize;
    let mut start = None;
    for (index, character) in value.char_indices() {
        match character {
            '(' => {
                depth += 1;
                start.get_or_insert(index);
            }
            ')' => depth = depth.saturating_sub(1),
            character if character.is_whitespace() && depth == 0 => {
                if let Some(start) = start.take() {
                    parts.push(&value[start..index]);
                }
            }
            _ => {
                start.get_or_insert(index);
            }
        }
    }
    if let Some(start) = start {
        parts.push(&value[start..]);
    }
    parts
}

/// Splits `value` at the first top-level whitespace, returning `(token, rest)`.
fn split_first_token(value: &str) -> Option<(&str, &str)> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let parts = split_top_level_whitespace(value);
    let first = parts.first()?;
    let rest = value.get(first.len()..).unwrap_or_default().trim_start();
    Some((first, rest))
}

pub(super) fn parse_length(value: &str, position: usize) -> Result<f32, DssError> {
    let value = value.trim();
    let length = if let Some(arguments) = function_arguments(value, "calc") {
        CalcParser::new(arguments, position).parse()?
    } else if let Some(number) = value.strip_suffix("px") {
        let length = number
            .trim()
            .parse::<f32>()
            .map_err(|_| DssError::new(position, format!("invalid length `{value}`")))?;
        if !length.is_finite() {
            return Err(DssError::new(position, "length must be finite"));
        }
        length
    } else if value == "0" {
        0.0
    } else {
        return Err(DssError::new(
            position,
            "non-zero lengths must use the `px` unit or `calc()`",
        ));
    };
    if !length.is_finite() || length < 0.0 {
        return Err(DssError::new(
            position,
            "length must be finite and non-negative",
        ));
    }
    Ok(length)
}

/// Recursive-descent evaluator for `calc()` expressions in logical pixels.
///
/// Percentages and unknown units are rejected instead of guessed, because DSS
/// values are resolved before layout knows the containing size.
struct CalcParser<'a> {
    value: &'a str,
    cursor: usize,
    position: usize,
}

impl<'a> CalcParser<'a> {
    const fn new(value: &'a str, position: usize) -> Self {
        Self {
            value,
            cursor: 0,
            position,
        }
    }

    fn parse(mut self) -> Result<f32, DssError> {
        let result = self.expression()?;
        self.skip_whitespace();
        if self.cursor != self.value.len() {
            return Err(self.error("unexpected token in `calc()`"));
        }
        Ok(result)
    }

    fn expression(&mut self) -> Result<f32, DssError> {
        let mut result = self.term()?;
        loop {
            self.skip_whitespace();
            if self.consume('+') {
                result += self.term()?;
            } else if self.consume('-') {
                result -= self.term()?;
            } else {
                return Ok(result);
            }
        }
    }

    fn term(&mut self) -> Result<f32, DssError> {
        let mut result = self.factor()?;
        loop {
            self.skip_whitespace();
            if self.consume('*') {
                result *= self.factor()?;
            } else if self.consume('/') {
                let divisor = self.factor()?;
                if divisor == 0.0 {
                    return Err(self.error("division by zero in `calc()`"));
                }
                result /= divisor;
            } else {
                return Ok(result);
            }
        }
    }

    fn factor(&mut self) -> Result<f32, DssError> {
        self.skip_whitespace();
        if self.consume('(') {
            let value = self.expression()?;
            self.skip_whitespace();
            if !self.consume(')') {
                return Err(self.error("expected `)` in `calc()`"));
            }
            return Ok(value);
        }
        if self.consume('-') {
            return Ok(-self.factor()?);
        }
        if self.consume('+') {
            return self.factor();
        }
        self.number()
    }

    fn number(&mut self) -> Result<f32, DssError> {
        let start = self.cursor;
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_digit() || character == '.')
        {
            self.cursor += 1;
        }
        if start == self.cursor {
            return Err(self.error("expected a number in `calc()`"));
        }
        let number = self.value[start..self.cursor]
            .parse::<f32>()
            .map_err(|_| self.error("invalid number in `calc()`"))?;
        if self.rest().starts_with("px") || self.rest().starts_with("PX") {
            self.cursor += 2;
        } else if self.rest().starts_with('%') {
            return Err(self.error("percentages are not supported in `calc()`"));
        } else if self
            .rest()
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic())
        {
            return Err(self.error("`calc()` supports `px` lengths and unitless numbers"));
        }
        Ok(number)
    }

    fn peek(&self) -> Option<char> {
        self.value[self.cursor..].chars().next()
    }

    fn rest(&self) -> &str {
        &self.value[self.cursor..]
    }

    fn consume(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.cursor += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(character) = self.peek() {
            if character.is_whitespace() {
                self.cursor += character.len_utf8();
            } else {
                break;
            }
        }
    }

    fn error(&self, message: impl Into<String>) -> DssError {
        DssError::new(self.position, message)
    }
}
