use crate::config::CshipConfig;
use crate::context::Context;

enum Token {
    Native(String),
    Passthrough(String),
    Literal(String), // bare text preserved verbatim
}

fn parse_line(line: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut pos = 0;

    while pos < line.len() {
        let remaining = &line[pos..];

        if let Some(dollar_pos) = remaining.find('$') {
            // Text before the '$' token is a literal (if non-empty)
            if dollar_pos > 0 {
                tokens.push(Token::Literal(remaining[..dollar_pos].to_string()));
            }
            // Read the token name (from after '$' to next whitespace or end)
            let after_dollar = &remaining[dollar_pos + 1..];
            let name_end = after_dollar
                .find(char::is_whitespace)
                .unwrap_or(after_dollar.len());
            let name = &after_dollar[..name_end];
            if !name.is_empty() {
                if name.starts_with("cship.") {
                    tokens.push(Token::Native(name.to_string()));
                } else {
                    tokens.push(Token::Passthrough(name.to_string()));
                }
            }
            pos += dollar_pos + 1 + name_end;
        } else {
            // No more '$' — remainder is all literal text
            if !remaining.is_empty() {
                tokens.push(Token::Literal(remaining.to_string()));
            }
            break;
        }
    }

    tokens
}

fn render_line(line: &str, ctx: &Context, cfg: &CshipConfig) -> String {
    let mut parts: Vec<String> = Vec::new();

    for token in parse_line(line) {
        match token {
            Token::Native(name) => {
                if let Some(rendered) = crate::modules::render_module(&name, ctx, cfg) {
                    parts.push(rendered);
                }
            }
            Token::Passthrough(name) => {
                tracing::debug!(
                    "cship: passthrough module '{name}' not yet implemented — skipping"
                );
            }
            Token::Literal(text) => {
                parts.push(text);
            }
        }
    }

    parts.join("") // No separator — spacing is encoded in Literal tokens
}

pub fn render(lines: &[String], ctx: &Context, cfg: &CshipConfig) -> String {
    lines
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
}
