//! Starship-compatible per-module format string parser.
//!
//! Supported syntax (simplified subset):
//! - `$value`  — module's computed raw value (absent = empty string in renders, None in conditionals)
//! - `$symbol` — module's configured symbol (defaults to empty string if None)
//! - `[content]($style)` — style span: render `content` with module's configured style
//! - `[content](bold red)` — style span: render `content` with the literal style string
//! - `(content)` — conditional group: renders as empty if `$value` is None
//! - Literal text — preserved verbatim
//!
//! [Source: architecture.md#Core Architectural Decisions, epics.md#Story 2.5]

/// Apply a per-module format string.
///
/// # Arguments
/// - `fmt`: Format template (e.g. `"[ ctx: $value% ]($style)"`)
/// - `value`: Module's computed raw value; `None` = field absent from context
/// - `symbol`: Module's configured `symbol` field; `None` = no symbol configured
/// - `style`: Module's configured `style` string; used when format references `$style`
///
/// # Returns
/// `None` when the rendered result is empty (conditional group whose `$value` is None).
/// `Some(rendered)` otherwise.
pub fn apply_module_format(
    fmt: &str,
    value: Option<&str>,
    symbol: Option<&str>,
    style: Option<&str>,
) -> Option<String> {
    let rendered = render_fmt(fmt, value, symbol, style);
    if rendered.is_empty() {
        None
    } else {
        Some(rendered)
    }
}

fn render_fmt(s: &str, value: Option<&str>, symbol: Option<&str>, style: Option<&str>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pos = 0;

    while pos < s.len() {
        let remaining = &s[pos..];

        if remaining.starts_with('[')
            && let Some((content, style_spec, len)) = parse_style_span(remaining)
        {
            let rendered_content = render_fmt(&content, value, symbol, style);
            let applied = if style_spec == "$style" {
                crate::ansi::apply_style(&rendered_content, style)
            } else {
                crate::ansi::apply_style(&rendered_content, Some(style_spec.as_str()))
            };
            out.push_str(&applied);
            pos += len;
            continue;
        }

        if remaining.starts_with('(')
            && let Some((content, len)) = parse_paren_group(remaining)
        {
            let conditional_result = render_conditional(&content, value, symbol, style);
            out.push_str(&conditional_result);
            pos += len;
            continue;
        }

        if let Some(rest) = remaining.strip_prefix('$') {
            let var_end = rest
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            let var_name = &rest[..var_end];
            let subst = match var_name {
                "value" => value.unwrap_or(""),
                "symbol" => symbol.unwrap_or(""),
                _ => "",
            };
            out.push_str(subst);
            pos += 1 + var_end; // skip '$' + var_name bytes
            continue;
        }

        // Literal character — advance by one Unicode scalar
        let ch = remaining.chars().next().unwrap();
        out.push(ch);
        pos += ch.len_utf8();
    }

    out
}

/// Conditional group: renders as empty string if `$value` is referenced AND is `None`.
fn render_conditional(
    content: &str,
    value: Option<&str>,
    symbol: Option<&str>,
    style: Option<&str>,
) -> String {
    if content.contains("$value") && value.is_none() {
        return String::new();
    }
    render_fmt(content, value, symbol, style)
}

/// Parse `[content]($style_spec)` from the start of `s`.
/// Returns `(content, style_spec, bytes_consumed)` or `None` if not a valid span.
/// Handles nested brackets via depth tracking (e.g., `[[nested]](style)` works correctly).
fn parse_style_span(s: &str) -> Option<(String, String, usize)> {
    debug_assert!(s.starts_with('['));
    let close_bracket = find_matching_close(s, 1, '[', ']')?;
    let content = s[1..close_bracket].to_string();
    let after = &s[close_bracket + 1..];
    if !after.starts_with('(') {
        return None;
    }
    let close_paren = find_matching_close(after, 1, '(', ')')?;
    let style_spec = after[1..close_paren].to_string();
    let total_len = close_bracket + 1 + close_paren + 1;
    Some((content, style_spec, total_len))
}

/// Parse `(content)` from the start of `s`.
/// Returns `(content, bytes_consumed)` or `None` if not a valid group.
/// Handles nested parentheses via depth tracking (e.g., `(cost: $value (USD))` works correctly).
fn parse_paren_group(s: &str) -> Option<(String, usize)> {
    debug_assert!(s.starts_with('('));
    let close_paren = find_matching_close(s, 1, '(', ')')?;
    let content = s[1..close_paren].to_string();
    Some((content, close_paren + 1))
}

/// Find the byte offset of the matching closing delimiter, respecting nesting depth.
/// `start` is the byte offset to begin scanning (after the opening delimiter).
fn find_matching_close(s: &str, start: usize, open: char, close: char) -> Option<usize> {
    let mut depth: u32 = 1;
    for (i, ch) in s[start..].char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(start + i);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dollar_value_substituted() {
        let result = apply_module_format("$value", Some("35"), None, None);
        assert_eq!(result, Some("35".to_string()));
    }

    #[test]
    fn test_dollar_symbol_substituted() {
        let result = apply_module_format("$symbol$value", Some("35"), Some("🔷"), None);
        assert_eq!(result, Some("🔷35".to_string()));
    }

    #[test]
    fn test_literal_text_preserved() {
        let result = apply_module_format("ctx: $value%", Some("8"), None, None);
        assert_eq!(result, Some("ctx: 8%".to_string()));
    }

    #[test]
    fn test_style_span_applies_ansi() {
        let result = apply_module_format("[$value]($style)", Some("35"), None, Some("bold green"));
        let s = result.unwrap();
        assert!(s.contains("35"), "value present: {s:?}");
        assert!(s.contains('\x1b'), "ANSI codes present: {s:?}");
    }

    #[test]
    fn test_style_span_dollar_style_uses_module_style() {
        // $style in format means "use the module's configured style"
        let result = apply_module_format("[$value]($style)", Some("OK"), None, Some("bold red"));
        let s = result.unwrap();
        assert!(s.contains("OK"));
        assert!(s.contains('\x1b'));
    }

    #[test]
    fn test_style_span_literal_style_string() {
        // literal style in format (not $style)
        let result = apply_module_format("[text](bold cyan)", Some("x"), None, None);
        let s = result.unwrap();
        assert!(s.contains("text"));
        assert!(s.contains('\x1b'));
    }

    #[test]
    fn test_conditional_value_none_returns_none() {
        // AC3: ($value) with value=None → entire group omitted → module returns None
        let result = apply_module_format("($value)", None, None, None);
        assert_eq!(result, None);
    }

    #[test]
    fn test_conditional_value_present_renders_without_parens() {
        // AC4: ($value) with value="NORMAL" → "NORMAL"
        let result = apply_module_format("($value)", Some("NORMAL"), None, None);
        assert_eq!(result, Some("NORMAL".to_string()));
    }

    #[test]
    fn test_complex_format_ac1_style() {
        // AC1 pattern: '[ ctx: $value% ]($style)'
        let result = apply_module_format("[ ctx: $value% ]($style)", Some("8"), None, None);
        // No style configured → no ANSI codes, just literal text
        assert_eq!(result, Some(" ctx: 8% ".to_string()));
    }

    #[test]
    fn test_complex_format_ac1_with_style() {
        let result = apply_module_format(
            "[ ctx: $value% ]($style)",
            Some("8"),
            None,
            Some("bold green"),
        );
        let s = result.unwrap();
        assert!(s.contains("8"), "value in output: {s:?}");
        assert!(s.contains('\x1b'), "ANSI codes: {s:?}");
    }

    #[test]
    fn test_no_variables_returns_literal() {
        let result = apply_module_format("fixed text", None, None, None);
        assert_eq!(result, Some("fixed text".to_string()));
    }

    #[test]
    fn test_empty_format_returns_none() {
        let result = apply_module_format("", None, None, None);
        assert_eq!(result, None);
    }

    #[test]
    fn test_value_none_outside_conditional_renders_empty_string() {
        // $value with None outside conditional → treated as "" (not conditional)
        let result = apply_module_format("prefix $value suffix", None, None, None);
        assert_eq!(result, Some("prefix  suffix".to_string()));
    }

    #[test]
    fn test_nested_parens_in_conditional_group() {
        // "(cost: $value (USD))" — outer group contains $value; inner (USD) is a nested
        // conditional group with no $value ref → renders "USD" (parens are syntax, not output).
        let result = apply_module_format("(cost: $value (USD))", Some("5.00"), None, None);
        assert_eq!(result, Some("cost: 5.00 USD".to_string()));
    }

    #[test]
    fn test_nested_parens_conditional_absent_omits_entire_group() {
        // Outer group references $value which is None → entire group omitted
        let result = apply_module_format("(cost: $value (USD))", None, None, None);
        assert_eq!(result, None);
    }

    #[test]
    fn test_ac2_pattern_value_symbol_style_combined() {
        // AC2 pattern: "[$value $symbol]($style)" with all three variables
        let result = apply_module_format(
            "[$value $symbol]($style)",
            Some("bar"),
            Some("🧠"),
            Some("bold cyan"),
        );
        let s = result.unwrap();
        assert!(s.contains("bar"), "value present: {s:?}");
        assert!(s.contains("🧠"), "symbol present: {s:?}");
        assert!(s.contains('\x1b'), "ANSI codes present: {s:?}");
    }
}
