//! The `#rrggbb` color of a request.

use alacritty_terminal::vte::ansi::Rgb;
use poimen_protocol::ErrorCode;

/// `#rrggbb` (the `#` optional).
pub(crate) fn parse_hex(value: &str) -> Result<Rgb, (ErrorCode, String)> {
    let bad = || {
        (
            ErrorCode::InvalidParams,
            format!("{value:?} is not a #rrggbb color"),
        )
    };
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() != 6 || !hex.is_ascii() {
        return Err(bad());
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| bad());
    Ok(Rgb {
        r: byte(0)?,
        g: byte(2)?,
        b: byte(4)?,
    })
}
