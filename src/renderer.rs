use crate::config::CshipConfig;
use crate::context::Context;

enum Token {
    Native(String),
    Passthrough(String),
    StarshipPrompt,
    Literal(String), // bare text preserved verbatim
    StyledSpan { content: String, style: String },
    Fill, // `$fill` — expands to fill remaining horizontal space (right-align)
}

fn parse_line(line: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut pos = 0;

    while pos < line.len() {
        let remaining = &line[pos..];

        // Check for styled span: [content](style) — only when '[' is at current position
        if let Some(after_bracket) = remaining.strip_prefix('[') {
            if let Some(close_bracket_offset) = after_bracket.find("](") {
                let content = &after_bracket[..close_bracket_offset];
                let after_open_paren = &after_bracket[close_bracket_offset + 2..]; // skip "]("
                if let Some(close_paren) = after_open_paren.find(')') {
                    let style = &after_open_paren[..close_paren];
                    tokens.push(Token::StyledSpan {
                        content: content.to_string(),
                        style: style.to_string(),
                    });
                    // advance: 1 ('[') + close_bracket_offset + 2 ('](') + close_paren + 1 (')')
                    pos += 1 + close_bracket_offset + 2 + close_paren + 1;
                    continue;
                }
            }
            // No matching ](...) found — emit literal "[" and advance one char
            tokens.push(Token::Literal("[".to_string()));
            pos += 1;
            continue;
        }

        // Find next special character: '$' or '[' — process whichever comes first
        let next_dollar = remaining.find('$');
        let next_bracket = remaining.find('[');
        let next_pos = match (next_dollar, next_bracket) {
            (Some(d), Some(b)) => Some(d.min(b)),
            (Some(d), None) => Some(d),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        match next_pos {
            Some(special_pos) if special_pos > 0 => {
                // Literal text before the next special character
                tokens.push(Token::Literal(remaining[..special_pos].to_string()));
                pos += special_pos;
                // Re-enter loop — next iteration handles '[' or '$' at position 0
            }
            Some(_) => {
                // special_pos == 0 and not '[' (handled above) → must be '$'
                let after_dollar = &remaining[1..];
                let name_end = after_dollar
                    .find(|c: char| c.is_whitespace() || c == '[' || c == '$')
                    .unwrap_or(after_dollar.len());
                let name = &after_dollar[..name_end];
                if !name.is_empty() {
                    if name == "fill" {
                        tokens.push(Token::Fill);
                    } else if name == "starship_prompt" {
                        tokens.push(Token::StarshipPrompt);
                    } else if name.starts_with("cship.") {
                        tokens.push(Token::Native(name.to_string()));
                    } else {
                        tokens.push(Token::Passthrough(name.to_string()));
                    }
                }
                pos += 1 + name_end;
            }
            None => {
                // No more special characters — remainder is all literal text
                if !remaining.is_empty() {
                    tokens.push(Token::Literal(remaining.to_string()));
                }
                break;
            }
        }
    }

    tokens
}

/// A rendered line segment: either literal text or a `$fill` placeholder whose
/// width is computed once all text segments are known.
enum Piece {
    Text(String),
    Fill,
}

fn render_line(line: &str, ctx: &Context, cfg: &CshipConfig) -> String {
    let fill_disabled = cfg.fill.as_ref().and_then(|f| f.disabled).unwrap_or(false);

    let mut pieces: Vec<Piece> = Vec::new();
    for token in parse_line(line) {
        match token {
            Token::Native(name) => {
                if let Some(rendered) = crate::modules::render_module(&name, ctx, cfg) {
                    pieces.push(Piece::Text(rendered));
                }
            }
            Token::Passthrough(name) => {
                if let Some(rendered) = crate::passthrough::render_passthrough(&name, ctx) {
                    pieces.push(Piece::Text(rendered));
                }
            }
            Token::StarshipPrompt => {
                if let Some(rendered) = crate::passthrough::render_starship_prompt(ctx, cfg) {
                    pieces.push(Piece::Text(rendered));
                }
            }
            Token::Literal(text) => pieces.push(Piece::Text(text)),
            Token::StyledSpan { content, style } => {
                pieces.push(Piece::Text(crate::ansi::apply_style(
                    &content,
                    Some(&style),
                )));
            }
            // A disabled fill collapses to nothing; otherwise it's a layout placeholder.
            Token::Fill => {
                if !fill_disabled {
                    pieces.push(Piece::Fill);
                }
            }
        }
    }

    // Fast path: no fills → concatenate (spacing is encoded in Literal tokens).
    // This keeps the common case free of any terminal-width lookup.
    if !pieces.iter().any(|p| matches!(p, Piece::Fill)) {
        return pieces
            .into_iter()
            .map(|p| match p {
                Piece::Text(s) => s,
                Piece::Fill => String::new(),
            })
            .collect();
    }

    // Fill path: distribute the leftover terminal width across the fills.
    let total_width = crate::terminal::statusline_width(cfg) as usize;
    let (fill_char, fill_style) = fill_appearance(cfg);
    build_filled_line(&pieces, total_width, &fill_char, fill_style.as_deref())
}

/// Resolve the fill character and style, defaulting to Starship's `"."` / `"bold black"`.
fn fill_appearance(cfg: &CshipConfig) -> (String, Option<String>) {
    let fill = cfg.fill.as_ref();
    let symbol = fill
        .and_then(|f| f.symbol.clone())
        .unwrap_or_else(|| ".".to_string());
    let style = fill
        .and_then(|f| f.style.clone())
        .or_else(|| Some("bold black".to_string()));
    (symbol, style)
}

/// Lay out `pieces` into a line `total_width` columns wide, growing each `Fill`
/// to share the leftover space evenly. Pure (width is injected) so it's testable
/// without a terminal.
fn build_filled_line(
    pieces: &[Piece],
    total_width: usize,
    fill_char: &str,
    fill_style: Option<&str>,
) -> String {
    let content_width: usize = pieces
        .iter()
        .filter_map(|p| match p {
            Piece::Text(s) => Some(crate::ansi::display_width(s)),
            Piece::Fill => None,
        })
        .sum();

    // A line with no visible content (only fills / absent modules) renders nothing,
    // matching cship's habit of dropping empty lines rather than emitting a bare rule.
    if content_width == 0 {
        return String::new();
    }

    let n_fills = pieces.iter().filter(|p| matches!(p, Piece::Fill)).count();
    let gaps = distribute(total_width.saturating_sub(content_width), n_fills);

    let mut out = String::new();
    let mut gap_idx = 0;
    for piece in pieces {
        match piece {
            Piece::Text(s) => out.push_str(s),
            Piece::Fill => {
                let gap = gaps[gap_idx];
                gap_idx += 1;
                if gap > 0 {
                    out.push_str(&crate::ansi::apply_style(
                        &render_fill(fill_char, gap),
                        fill_style,
                    ));
                }
            }
        }
    }
    out
}

/// Build exactly `gap` columns of fill from `fill_char`, accounting for the
/// char's own display width and padding any leftover columns with spaces. This
/// keeps alignment exact even when `symbol` is more than one column wide.
fn render_fill(fill_char: &str, gap: usize) -> String {
    // A zero-width symbol (e.g. `""`) can't fill anything — repeating it stays
    // empty and silently breaks alignment. Fall back to plain space padding.
    let char_cols = crate::ansi::display_width(fill_char);
    if char_cols == 0 {
        return " ".repeat(gap);
    }
    let reps = gap / char_cols;
    let mut s = fill_char.repeat(reps);
    let pad = gap - reps * char_cols;
    if pad > 0 {
        s.push_str(&" ".repeat(pad));
    }
    s
}

/// Split `remaining` columns across `n` fills as evenly as possible (earlier
/// fills absorb the remainder). The gaps always sum to `remaining`, so the line
/// ends exactly at `total_width` and any content after the last fill is flush-right.
fn distribute(remaining: usize, n: usize) -> Vec<usize> {
    if n == 0 {
        return Vec::new();
    }
    let base = remaining / n;
    let extra = remaining % n;
    (0..n).map(|i| base + usize::from(i < extra)).collect()
}

pub fn render(lines: &[String], ctx: &Context, cfg: &CshipConfig) -> String {
    // cfg.format takes priority over lines; split on "$line_break" to produce rows
    let owned_lines: Vec<String>;
    let effective_lines: &[String] = if let Some(format_str) = &cfg.format {
        if !lines.is_empty() {
            tracing::warn!("cship: format field is set — ignoring lines config");
        }
        owned_lines = format_str
            .split("$line_break")
            .map(|s| s.to_string())
            .collect();
        &owned_lines
    } else {
        lines
    };

    effective_lines
        .iter()
        .map(|line| render_line(line, ctx, cfg))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CshipConfig;
    use crate::context::Context;

    #[test]
    fn test_parse_line_classifies_cship_as_native() {
        let tokens = parse_line("$cship.model");
        assert!(matches!(tokens[0], Token::Native(ref n) if n == "cship.model"));
    }

    #[test]
    fn test_parse_line_classifies_other_as_passthrough() {
        let tokens = parse_line("$git_branch");
        assert!(matches!(tokens[0], Token::Passthrough(ref n) if n == "git_branch"));
    }

    #[test]
    fn test_parse_line_literal_text_produces_literal_token() {
        // REPLACES test_parse_line_ignores_non_dollar_words
        let tokens = parse_line("literal text without dollar");
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0], Token::Literal(ref t) if t == "literal text without dollar"));
    }

    #[test]
    fn test_parse_line_preserves_prefix_literal() {
        let tokens = parse_line("in: $cship.context_window.total_input_tokens");
        assert_eq!(tokens.len(), 2);
        assert!(matches!(tokens[0], Token::Literal(ref t) if t == "in: "));
        assert!(
            matches!(tokens[1], Token::Native(ref n) if n == "cship.context_window.total_input_tokens")
        );
    }

    #[test]
    fn test_parse_line_styled_span_with_content() {
        let tokens = parse_line("[text](bold green)");
        assert_eq!(tokens.len(), 1);
        assert!(
            matches!(&tokens[0], Token::StyledSpan { content, style } if content == "text" && style == "bold green")
        );
    }

    #[test]
    fn test_parse_line_empty_styled_span() {
        let tokens = parse_line("[](fg:#3d414a bg:#5f6366)");
        assert_eq!(tokens.len(), 1);
        assert!(
            matches!(&tokens[0], Token::StyledSpan { content, style } if content.is_empty() && style == "fg:#3d414a bg:#5f6366")
        );
    }

    #[test]
    fn test_parse_line_unclosed_bracket_literal() {
        let tokens = parse_line("[note:");
        // "[" emitted as Literal, then "note:" as Literal
        assert_eq!(tokens.len(), 2);
        assert!(matches!(&tokens[0], Token::Literal(t) if t == "["));
        assert!(matches!(&tokens[1], Token::Literal(t) if t == "note:"));
    }

    #[test]
    fn test_parse_line_mixed_span_and_native() {
        let tokens = parse_line("[bold](bold) $cship.model");
        assert_eq!(tokens.len(), 3);
        assert!(
            matches!(&tokens[0], Token::StyledSpan { content, style } if content == "bold" && style == "bold")
        );
        assert!(matches!(&tokens[1], Token::Literal(t) if t == " "));
        assert!(matches!(&tokens[2], Token::Native(n) if n == "cship.model"));
    }

    #[test]
    fn test_parse_line_spaces_styled_span() {
        // AC6: two spaces with foreground color
        let tokens = parse_line("[  ](fg:#ffc878)");
        assert_eq!(tokens.len(), 1);
        assert!(
            matches!(&tokens[0], Token::StyledSpan { content, style } if content == "  " && style == "fg:#ffc878")
        );
    }

    #[test]
    fn test_parse_line_styled_span_after_literal_text() {
        // Regression: styled spans preceded by literal text must still be parsed
        let tokens = parse_line("prefix [text](bold green)");
        assert_eq!(tokens.len(), 2);
        assert!(matches!(&tokens[0], Token::Literal(t) if t == "prefix "));
        assert!(
            matches!(&tokens[1], Token::StyledSpan { content, style } if content == "text" && style == "bold green")
        );
    }

    #[test]
    fn test_parse_line_styled_span_adjacent_to_token_no_space() {
        // Regression: $token[span](style) without space must not absorb '[' into token name
        let tokens = parse_line("$cship.model[sep](fg:red)");
        assert_eq!(tokens.len(), 2);
        assert!(matches!(&tokens[0], Token::Native(n) if n == "cship.model"));
        assert!(
            matches!(&tokens[1], Token::StyledSpan { content, style } if content == "sep" && style == "fg:red")
        );
    }

    #[test]
    fn test_parse_line_styled_span_after_native_token() {
        // Regression: styled span after $token must be parsed (not eaten as literal)
        let tokens = parse_line("$cship.model [sep](fg:red) $cship.cost");
        assert_eq!(tokens.len(), 5);
        assert!(matches!(&tokens[0], Token::Native(n) if n == "cship.model"));
        assert!(matches!(&tokens[1], Token::Literal(t) if t == " "));
        assert!(
            matches!(&tokens[2], Token::StyledSpan { content, style } if content == "sep" && style == "fg:red")
        );
        assert!(matches!(&tokens[3], Token::Literal(t) if t == " "));
        assert!(matches!(&tokens[4], Token::Native(n) if n == "cship.cost"));
    }

    #[test]
    fn test_render_empty_lines_is_empty() {
        let ctx = Context::default();
        let cfg = CshipConfig::default();
        let result = render(&[], &ctx, &cfg);
        assert_eq!(result, "");
    }

    #[test]
    fn test_render_two_empty_lines_filtered_to_empty() {
        // With default context (no model), both lines render empty and are filtered out
        let ctx = Context::default();
        let cfg = CshipConfig::default();
        let lines = vec!["$cship.model".to_string(), "$cship.model".to_string()];
        let result = render(&lines, &ctx, &cfg);
        // Both tokens render to None → empty strings filtered out → empty result
        assert_eq!(result, "");
    }

    #[test]
    fn test_render_line_literal_and_native_concatenated_without_extra_space() {
        // Verifies AC6 behavior: "in: " + "15234" = "in: 15234" (no double space)
        use crate::context::{Context, ContextWindow};
        let ctx = Context {
            context_window: Some(ContextWindow {
                total_input_tokens: Some(15234),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = CshipConfig::default();
        let result = render_line("in: $cship.context_window.total_input_tokens", &ctx, &cfg);
        assert_eq!(result, "in: 15234");
    }

    #[test]
    fn test_render_format_field_line_break() {
        let ctx = Context::default();
        let cfg = CshipConfig {
            format: Some("line1$line_breakline2".to_string()),
            ..Default::default()
        };
        let result = render(&[], &ctx, &cfg);
        assert_eq!(result, "line1\nline2");
    }

    #[test]
    fn test_render_format_takes_priority_over_lines() {
        let ctx = Context::default();
        let cfg = CshipConfig {
            format: Some("from_format".to_string()),
            lines: Some(vec!["from_lines".to_string()]),
            ..Default::default()
        };
        let lines = cfg.lines.as_deref().unwrap_or(&[]);
        let result = render(lines, &ctx, &cfg);
        assert_eq!(result, "from_format");
    }

    #[test]
    fn test_render_lines_unchanged_when_no_format() {
        let ctx = Context::default();
        let cfg = CshipConfig {
            lines: Some(vec!["hello".to_string()]),
            ..Default::default()
        };
        let lines = cfg.lines.as_deref().unwrap_or(&[]);
        let result = render(lines, &ctx, &cfg);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_parse_line_fill_token() {
        assert!(matches!(parse_line("$fill").as_slice(), [Token::Fill]));
        let toks = parse_line("a $fill b");
        assert_eq!(toks.len(), 3);
        assert!(matches!(toks[0], Token::Literal(ref t) if t == "a "));
        assert!(matches!(toks[1], Token::Fill));
        assert!(matches!(toks[2], Token::Literal(ref t) if t == " b"));
    }

    #[test]
    fn test_distribute_single_fill_gets_all() {
        assert_eq!(distribute(10, 1), vec![10]);
    }

    #[test]
    fn test_distribute_even_split() {
        assert_eq!(distribute(10, 2), vec![5, 5]);
    }

    #[test]
    fn test_distribute_remainder_goes_to_earlier_fills() {
        assert_eq!(distribute(11, 2), vec![6, 5]);
        assert_eq!(distribute(9, 2), vec![5, 4]);
    }

    #[test]
    fn test_distribute_zero_and_none() {
        assert_eq!(distribute(0, 2), vec![0, 0]);
        assert_eq!(distribute(5, 0), Vec::<usize>::new());
    }

    #[test]
    fn test_build_filled_line_single_fill_right_aligns() {
        let pieces = vec![
            Piece::Text("AB".into()),
            Piece::Fill,
            Piece::Text("C".into()),
        ];
        let out = build_filled_line(&pieces, 10, ".", None);
        assert_eq!(out, "AB.......C");
        assert_eq!(out.len(), 10);
    }

    #[test]
    fn test_build_filled_line_multiple_fills_even() {
        let pieces = vec![
            Piece::Text("A".into()),
            Piece::Fill,
            Piece::Text("B".into()),
            Piece::Fill,
            Piece::Text("C".into()),
        ];
        // width 11, content 3 → remaining 8 → gaps 4,4
        let out = build_filled_line(&pieces, 11, ".", None);
        assert_eq!(out, "A....B....C");
    }

    #[test]
    fn test_build_filled_line_overflow_yields_no_gap() {
        let pieces = vec![
            Piece::Text("AB".into()),
            Piece::Fill,
            Piece::Text("C".into()),
        ];
        let out = build_filled_line(&pieces, 2, ".", None);
        assert_eq!(out, "ABC");
    }

    #[test]
    fn test_build_filled_line_uses_display_width_not_bytes() {
        // 💰 is 2 columns but 4 bytes; the gap must be sized by columns.
        let pieces = vec![
            Piece::Text("💰".into()),
            Piece::Fill,
            Piece::Text("X".into()),
        ];
        let out = build_filled_line(&pieces, 10, ".", None);
        assert_eq!(out, "💰.......X");
    }

    #[test]
    fn test_build_filled_line_no_content_renders_empty() {
        // Only fills / empty text → nothing, not a full-width rule.
        assert_eq!(build_filled_line(&[Piece::Fill], 80, ".", None), "");
        let pieces = vec![
            Piece::Text(String::new()),
            Piece::Fill,
            Piece::Text(String::new()),
        ];
        assert_eq!(build_filled_line(&pieces, 80, ".", None), "");
    }

    #[test]
    fn test_build_filled_line_multichar_symbol_is_column_exact() {
        let pieces = vec![
            Piece::Text("A".into()),
            Piece::Fill,
            Piece::Text("B".into()),
        ];
        // width 10, content 2, gap 8; symbol "··" is 2 cols → 4 reps = exactly 8 cols
        let out = build_filled_line(&pieces, 10, "··", None);
        assert_eq!(out, "A········B");
        assert_eq!(crate::ansi::display_width(&out), 10);
    }

    #[test]
    fn test_build_filled_line_multichar_symbol_pads_remainder() {
        let pieces = vec![
            Piece::Text("A".into()),
            Piece::Fill,
            Piece::Text("B".into()),
        ];
        // width 11, content 2, gap 9; "··"(2) → 4 reps = 8 cols, +1 space pad = 9
        let out = build_filled_line(&pieces, 11, "··", None);
        assert_eq!(out, "A········ B");
        assert_eq!(crate::ansi::display_width(&out), 11);
    }

    #[test]
    fn test_build_filled_line_empty_symbol_pads_with_spaces() {
        // A zero-width symbol must not collapse the fill — the gap falls back to spaces.
        let pieces = vec![
            Piece::Text("A".into()),
            Piece::Fill,
            Piece::Text("B".into()),
        ];
        let out = build_filled_line(&pieces, 10, "", None);
        assert_eq!(out, "A        B");
        assert_eq!(crate::ansi::display_width(&out), 10);
    }

    #[test]
    fn test_parse_line_recognizes_starship_prompt_token() {
        let tokens = parse_line("$starship_prompt");
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0], Token::StarshipPrompt));
    }

    #[test]
    fn test_parse_line_starship_prompt_with_literal_text() {
        let tokens = parse_line("prompt: $starship_prompt");
        assert_eq!(tokens.len(), 2);
        assert!(matches!(&tokens[0], Token::Literal(t) if t == "prompt: "));
        assert!(matches!(&tokens[1], Token::StarshipPrompt));
    }
}
