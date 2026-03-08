use nu_ansi_term::{Color, Style};

/// Apply a Starship-compatible style string to content using nu_ansi_term 0.50.
/// Style string format: space-separated tokens like "bold green" or "fg:red bg:blue".
/// Returns plain content string if style_str is None or empty.
pub fn apply_style(content: &str, style_str: Option<&str>) -> String {
    let Some(style_str) = style_str else {
        return content.to_string();
    };
    if style_str.trim().is_empty() {
        return content.to_string();
    }

    let mut style = Style::new();
    for token in style_str.split_whitespace() {
        match token.to_lowercase().as_str() {
            "bold" => style = style.bold(),
            "italic" => style = style.italic(),
            "underline" => style = style.underline(),
            "dimmed" => style = style.dimmed(),
            "blink" => style = style.blink(),
            "reverse" => style = style.reverse(),
            "hidden" => style = style.hidden(),
            "strikethrough" => style = style.strikethrough(),
            s => {
                let (is_bg, color_name) = if let Some(name) = s.strip_prefix("bg:") {
                    (true, name)
                } else if let Some(name) = s.strip_prefix("fg:") {
                    (false, name)
                } else {
                    (false, s)
                };
                if let Some(color) = parse_color(color_name) {
                    if is_bg {
                        style = style.on(color);
                    } else {
                        style = style.fg(color);
                    }
                }
            }
        }
    }

    style.paint(content).to_string()
}

/// Apply style with optional numeric threshold switching.
/// If `value` >= `critical_threshold` (both Some), applies `critical_style`.
/// If `value` >= `warn_threshold` (both Some), applies `warn_style`.
/// Otherwise applies base `style`. Falls back gracefully if thresholds are None.
///
/// [Source: architecture.md#Epic 1 Retrospective Addenda — ansi.rs Threshold Extension]
pub fn apply_style_with_threshold(
    content: &str,
    value: Option<f64>,
    style: Option<&str>,
    warn_threshold: Option<f64>,
    warn_style: Option<&str>,
    critical_threshold: Option<f64>,
    critical_style: Option<&str>,
) -> String {
    if let (Some(val), Some(thresh), Some(crit_style)) = (value, critical_threshold, critical_style)
        && val >= thresh
    {
        return apply_style(content, Some(crit_style));
    }
    if let (Some(val), Some(thresh), Some(w_style)) = (value, warn_threshold, warn_style)
        && val >= thresh
    {
        return apply_style(content, Some(w_style));
    }
    apply_style(content, style)
}

/// Resolve the effective style string based on numeric thresholds — without painting content.
/// Returns `critical_style` if `value` >= `critical_threshold` (both Some).
/// Returns `warn_style` if `value` >= `warn_threshold` (both Some).
/// Otherwise returns base `style`.
/// Used by modules that apply format strings but still need threshold-escalated style.
///
/// [Source: epics.md#Story 2.6]
pub fn resolve_threshold_style<'a>(
    value: Option<f64>,
    style: Option<&'a str>,
    warn_threshold: Option<f64>,
    warn_style: Option<&'a str>,
    critical_threshold: Option<f64>,
    critical_style: Option<&'a str>,
) -> Option<&'a str> {
    if let (Some(val), Some(thresh), Some(crit)) = (value, critical_threshold, critical_style)
        && val >= thresh
    {
        return Some(crit);
    }
    if let (Some(val), Some(thresh), Some(warn)) = (value, warn_threshold, warn_style)
        && val >= thresh
    {
        return Some(warn);
    }
    style
}

/// Strip ANSI escape sequences from a string, returning plain text.
/// Used by `cship explain` to display module values in a readable table.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c2 in chars.by_ref() {
                    if c2.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_color(name: &str) -> Option<Color> {
    match name {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "purple" | "magenta" => Some(Color::Purple),
        "cyan" => Some(Color::Cyan),
        "white" => Some(Color::White),
        _ => {
            // #RRGGBB — 6-digit hex
            if name.starts_with('#') && name.len() == 7 {
                let r = u8::from_str_radix(&name[1..3], 16).ok()?;
                let g = u8::from_str_radix(&name[3..5], 16).ok()?;
                let b = u8::from_str_radix(&name[5..7], 16).ok()?;
                return Some(Color::Rgb(r, g, b));
            }
            // #RGB — 3-digit short-form (each nibble doubled)
            if name.starts_with('#') && name.len() == 4 {
                let mut chars = name[1..].chars();
                let r = hex_nibble(chars.next()?)? * 17;
                let g = hex_nibble(chars.next()?)? * 17;
                let b = hex_nibble(chars.next()?)? * 17;
                return Some(Color::Rgb(r, g, b));
            }
            // Numeric 256-color palette index (0–255)
            name.parse::<u8>().ok().map(Color::Fixed)
        }
    }
}

fn hex_nibble(c: char) -> Option<u8> {
    c.to_digit(16).map(|d| d as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_style_returns_plain_text() {
        assert_eq!(apply_style("Opus", None), "Opus");
    }

    #[test]
    fn test_empty_style_returns_plain_text() {
        assert_eq!(apply_style("Opus", Some("")), "Opus");
    }

    #[test]
    fn test_bold_green_style_contains_ansi_codes() {
        let result = apply_style("Opus", Some("bold green"));
        // Must contain an ANSI escape sequence
        assert!(
            result.contains('\x1b'),
            "expected ANSI escape in: {result:?}"
        );
        // Must contain the original content
        assert!(result.contains("Opus"), "expected 'Opus' in: {result:?}");
    }

    #[test]
    fn test_unknown_style_tokens_ignored_content_preserved() {
        // Unknown tokens are silently skipped; content is still output
        let result = apply_style("Opus", Some("nonexistent_color"));
        assert!(result.contains("Opus"));
    }

    #[test]
    fn test_fg_prefix_applies_foreground_color() {
        let result = apply_style("text", Some("fg:red"));
        assert!(result.contains('\x1b'), "expected ANSI in: {result:?}");
    }

    #[test]
    fn test_bg_prefix_applies_background_color() {
        let result = apply_style("text", Some("bg:blue"));
        assert!(result.contains('\x1b'), "expected ANSI in: {result:?}");
    }

    #[test]
    fn test_threshold_below_warn_uses_base_style() {
        // value 3.0 below warn 5.0 → base style (no ANSI when style is None)
        let result = apply_style_with_threshold(
            "$3.00",
            Some(3.0),
            None,
            Some(5.0),
            Some("yellow"),
            Some(10.0),
            Some("red"),
        );
        assert_eq!(result, "$3.00");
    }

    #[test]
    fn test_threshold_above_warn_uses_warn_style() {
        let result = apply_style_with_threshold(
            "$6.00",
            Some(6.0),
            None,
            Some(5.0),
            Some("yellow"),
            Some(10.0),
            Some("red"),
        );
        assert!(
            result.contains('\x1b'),
            "expected ANSI codes for warn: {result:?}"
        );
    }

    #[test]
    fn test_threshold_above_critical_uses_critical_style() {
        let result = apply_style_with_threshold(
            "$12.00",
            Some(12.0),
            None,
            Some(5.0),
            Some("yellow"),
            Some(10.0),
            Some("bold red"),
        );
        assert!(
            result.contains('\x1b'),
            "expected ANSI codes for critical: {result:?}"
        );
    }

    #[test]
    fn test_threshold_no_thresholds_uses_base() {
        let result =
            apply_style_with_threshold("text", Some(100.0), Some("green"), None, None, None, None);
        assert!(result.contains('\x1b'), "expected base style ANSI");
    }

    #[test]
    fn test_resolve_threshold_below_warn_returns_base() {
        let result = resolve_threshold_style(
            Some(3.0),
            Some("green"),
            Some(5.0),
            Some("yellow"),
            Some(10.0),
            Some("red"),
        );
        assert_eq!(result, Some("green"));
    }

    #[test]
    fn test_resolve_threshold_above_warn_returns_warn() {
        let result = resolve_threshold_style(
            Some(6.0),
            Some("green"),
            Some(5.0),
            Some("yellow"),
            Some(10.0),
            Some("red"),
        );
        assert_eq!(result, Some("yellow"));
    }

    #[test]
    fn test_resolve_threshold_above_critical_returns_critical() {
        let result = resolve_threshold_style(
            Some(12.0),
            Some("green"),
            Some(5.0),
            Some("yellow"),
            Some(10.0),
            Some("red"),
        );
        assert_eq!(result, Some("red"));
    }

    #[test]
    fn test_resolve_threshold_value_none_returns_base() {
        let result = resolve_threshold_style(
            None,
            Some("green"),
            Some(5.0),
            Some("yellow"),
            Some(10.0),
            Some("red"),
        );
        assert_eq!(result, Some("green"));
    }

    #[test]
    fn test_resolve_threshold_no_thresholds_returns_base() {
        let result = resolve_threshold_style(Some(100.0), Some("green"), None, None, None, None);
        assert_eq!(result, Some("green"));
    }

    #[test]
    fn test_resolve_threshold_warn_style_none_falls_through_to_base() {
        // warn_threshold set but warn_style is None → cannot escalate, returns base
        let result = resolve_threshold_style(Some(6.0), Some("green"), Some(5.0), None, None, None);
        assert_eq!(result, Some("green"));
    }

    #[test]
    fn test_resolve_threshold_at_exact_boundary_triggers() {
        // value == warn_threshold → >= fires, returns warn_style
        let result = resolve_threshold_style(
            Some(5.0),
            Some("green"),
            Some(5.0),
            Some("yellow"),
            Some(10.0),
            Some("red"),
        );
        assert_eq!(result, Some("yellow"));
    }

    // --- Hex & 256-color tests (Story 7.1) ---

    #[test]
    fn test_hex_6digit_fg_applies_rgb() {
        // fg:#c3e88d → R=195, G=232, B=141 (AC1)
        let result = apply_style("text", Some("fg:#c3e88d"));
        assert!(
            result.contains("38;2;195;232;141"),
            "expected 24-bit RGB 195,232,141 in: {result:?}"
        );
        assert!(result.contains("text"), "content preserved: {result:?}");
    }

    #[test]
    fn test_hex_3digit_expands_to_rgb() {
        // fg:#fff → expanded to #ffffff → Color::Rgb(255, 255, 255) (AC2)
        let result = apply_style("text", Some("fg:#fff"));
        assert!(
            result.contains("38;2;255;255;255"),
            "expected 24-bit RGB 255,255,255 in: {result:?}"
        );
        assert!(result.contains("text"), "content preserved: {result:?}");
    }

    #[test]
    fn test_hex_3digit_mid_value_expands_correctly() {
        // fg:#80f → R=0x88=136, G=0x00=0, B=0xFF=255
        let result = apply_style("text", Some("fg:#80f"));
        assert!(
            result.contains("38;2;136;0;255"),
            "expected 24-bit RGB 136,0,255 in: {result:?}"
        );
    }

    #[test]
    fn test_256_palette_index_fixed() {
        // fg:220 → Color::Fixed(220) (AC3)
        let result = apply_style("text", Some("fg:220"));
        assert!(
            result.contains("38;5;220"),
            "expected 256-color index 220 in: {result:?}"
        );
        assert!(result.contains("text"), "content preserved: {result:?}");
    }

    #[test]
    fn test_256_palette_index_zero() {
        let result = apply_style("text", Some("fg:0"));
        assert!(
            result.contains("38;5;0"),
            "expected 256-color index 0 in: {result:?}"
        );
    }

    #[test]
    fn test_256_palette_index_max() {
        let result = apply_style("text", Some("fg:255"));
        assert!(
            result.contains("38;5;255"),
            "expected 256-color index 255 in: {result:?}"
        );
    }

    #[test]
    fn test_hex_bg_applies_background() {
        // bg:#1e1e2e → applied as background (AC4)
        let result = apply_style("text", Some("bg:#1e1e2e"));
        // 48;2;R;G;B is the ANSI sequence for 24-bit background color
        assert!(
            result.contains("48;2;30;30;46"),
            "expected 24-bit bg RGB 30,30,46 in: {result:?}"
        );
        assert!(result.contains("text"), "content preserved: {result:?}");
    }

    #[test]
    fn test_unknown_color_token_silent_ignore_regression() {
        // Unrecognized token → content returned unchanged, no ANSI (AC5)
        let result = apply_style("text", Some("fg:notacolor"));
        assert!(!result.contains('\x1b'), "unexpected ANSI in: {result:?}");
        assert_eq!(result, "text");
    }

    #[test]
    fn test_hex_3digit_invalid_chars_ignored() {
        // #xyz contains non-hex chars → must be silently ignored (AC5)
        let result = apply_style("text", Some("fg:#xyz"));
        assert!(
            !result.contains('\x1b'),
            "unexpected ANSI for #xyz: {result:?}"
        );
        assert_eq!(result, "text");
    }

    #[test]
    fn test_numeric_out_of_u8_range_ignored() {
        // 256 is out of u8 range → silently ignored
        let result = apply_style("text", Some("fg:256"));
        assert!(
            !result.contains('\x1b'),
            "unexpected ANSI for 256: {result:?}"
        );
        assert_eq!(result, "text");
    }
}
