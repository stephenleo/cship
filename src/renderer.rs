use crate::config::CshipConfig;
use crate::context::Context;

enum Token {
    Native(String),
    Passthrough(String),
}

fn parse_line(line: &str) -> Vec<Token> {
    line.split_whitespace()
        .filter_map(|word| {
            word.strip_prefix('$').map(|name| {
                if name.starts_with("cship.") {
                    Token::Native(name.to_string())
                } else {
                    Token::Passthrough(name.to_string())
                }
            })
        })
        .collect()
}

fn render_line(line: &str, ctx: &Context, cfg: &CshipConfig) -> String {
    parse_line(line)
        .into_iter()
        .filter_map(|token| match token {
            Token::Native(name) => crate::modules::render_module(&name, ctx, cfg),
            Token::Passthrough(name) => {
                tracing::debug!(
                    "cship: passthrough module '{name}' not yet implemented — skipping"
                );
                None
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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
    fn test_parse_line_ignores_non_dollar_words() {
        let tokens = parse_line("literal text without dollar");
        assert!(tokens.is_empty());
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
}
