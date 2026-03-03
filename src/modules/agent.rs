/// Render the `[cship.agent]` module.
///
/// `$cship.agent` — convenience alias for `$cship.agent.name`.
/// `$cship.agent.name` — raw agent name string (e.g., "claude-code", "security-reviewer").
///
/// [Source: epics.md#Story 2.3, architecture.md#Module System Architecture]
use crate::config::CshipConfig;
use crate::context::Context;

/// Renders `$cship.agent` — convenience alias for agent name.
pub fn render(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    render_name(ctx, cfg)
}

/// Renders `$cship.agent.name` — raw agent name string with optional symbol and style.
pub fn render_name(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    // Disabled check — SILENT (no warn, no log)
    if cfg.agent.as_ref().and_then(|a| a.disabled).unwrap_or(false) {
        return None;
    }

    // Extract value — WARN before returning None (AC8 requirement)
    let name = match ctx.agent.as_ref().and_then(|a| a.name.as_ref()) {
        Some(n) => n,
        None => {
            tracing::warn!("cship.agent: name absent from context");
            return None;
        }
    };

    let agent_cfg = cfg.agent.as_ref();
    let raw_value: &str = name;
    let symbol = agent_cfg.and_then(|a| a.symbol.as_deref());
    let style = agent_cfg.and_then(|a| a.style.as_deref());

    // Format string takes priority if configured (AC1–4)
    if let Some(fmt) = agent_cfg.and_then(|a| a.format.as_deref()) {
        return crate::format::apply_module_format(fmt, Some(raw_value), symbol, style);
    }

    // Default behavior — unchanged (AC5)
    let symbol_str = symbol.unwrap_or("");
    let content = format!("{symbol_str}{raw_value}");
    Some(crate::ansi::apply_style(&content, style))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentConfig, CshipConfig};
    use crate::context::{Agent, Context};

    fn ctx_with_agent(name: &str) -> Context {
        Context {
            agent: Some(Agent {
                name: Some(name.to_string()),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_agent_renders_name_string() {
        let ctx = ctx_with_agent("claude-code");
        let result = render(&ctx, &CshipConfig::default());
        assert_eq!(result, Some("claude-code".to_string()));
    }

    #[test]
    fn test_agent_name_alias_identical_to_render() {
        let ctx = ctx_with_agent("security-reviewer");
        let r1 = render(&ctx, &CshipConfig::default());
        let r2 = render_name(&ctx, &CshipConfig::default());
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_agent_disabled_returns_none() {
        let ctx = ctx_with_agent("claude-code");
        let cfg = CshipConfig {
            agent: Some(AgentConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render(&ctx, &cfg), None);
    }

    #[test]
    fn test_agent_absent_returns_none() {
        let ctx = Context::default(); // no agent field
        assert_eq!(render(&ctx, &CshipConfig::default()), None);
    }

    #[test]
    fn test_agent_applies_symbol_and_style() {
        let ctx = ctx_with_agent("security-reviewer");
        let cfg = CshipConfig {
            agent: Some(AgentConfig {
                symbol: Some("🤖 ".to_string()),
                style: Some("bold cyan".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            result.contains("security-reviewer"),
            "should contain name: {result:?}"
        );
        assert!(result.contains("🤖 "), "should contain symbol: {result:?}");
        assert!(
            result.contains('\x1b'),
            "should contain ANSI codes: {result:?}"
        );
    }
}
