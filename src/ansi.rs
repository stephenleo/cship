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
        _ => None,
    }
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
}
