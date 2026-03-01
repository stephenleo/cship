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
}
