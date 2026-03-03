//! Session identity modules: cwd, session_id, transcript_path, version, output_style.
//!
//! All 5 fields share ONE `SessionConfig` (disabled/symbol/style).
//! [Source: epics.md#Story 2.4, architecture.md#Module System Architecture]

/// Renders `$cship.cwd` — current working directory from Context.
pub fn render_cwd(
    ctx: &crate::context::Context,
    cfg: &crate::config::CshipConfig,
) -> Option<String> {
    if cfg
        .session
        .as_ref()
        .and_then(|s| s.disabled)
        .unwrap_or(false)
    {
        return None;
    }
    let value = match ctx.cwd.as_deref() {
        Some(v) => v,
        None => {
            tracing::warn!("cship.cwd: field absent from context");
            return None;
        }
    };
    let sess_cfg = cfg.session.as_ref();
    let symbol = sess_cfg.and_then(|s| s.symbol.as_deref());
    let style = sess_cfg.and_then(|s| s.style.as_deref());
    if let Some(fmt) = sess_cfg.and_then(|s| s.format.as_deref()) {
        return crate::format::apply_module_format(fmt, Some(value), symbol, style);
    }
    let symbol_str = symbol.unwrap_or("");
    let content = format!("{symbol_str}{value}");
    Some(crate::ansi::apply_style(&content, style))
}

/// Renders `$cship.session_id` — session ID string from Context.
pub fn render_session_id(
    ctx: &crate::context::Context,
    cfg: &crate::config::CshipConfig,
) -> Option<String> {
    if cfg
        .session
        .as_ref()
        .and_then(|s| s.disabled)
        .unwrap_or(false)
    {
        return None;
    }
    let value = match ctx.session_id.as_deref() {
        Some(v) => v,
        None => {
            tracing::warn!("cship.session_id: field absent from context");
            return None;
        }
    };
    let sess_cfg = cfg.session.as_ref();
    let symbol = sess_cfg.and_then(|s| s.symbol.as_deref());
    let style = sess_cfg.and_then(|s| s.style.as_deref());
    if let Some(fmt) = sess_cfg.and_then(|s| s.format.as_deref()) {
        return crate::format::apply_module_format(fmt, Some(value), symbol, style);
    }
    let symbol_str = symbol.unwrap_or("");
    let content = format!("{symbol_str}{value}");
    Some(crate::ansi::apply_style(&content, style))
}

/// Renders `$cship.transcript_path` — transcript file path from Context.
pub fn render_transcript_path(
    ctx: &crate::context::Context,
    cfg: &crate::config::CshipConfig,
) -> Option<String> {
    if cfg
        .session
        .as_ref()
        .and_then(|s| s.disabled)
        .unwrap_or(false)
    {
        return None;
    }
    let value = match ctx.transcript_path.as_deref() {
        Some(v) => v,
        None => {
            tracing::warn!("cship.transcript_path: field absent from context");
            return None;
        }
    };
    let sess_cfg = cfg.session.as_ref();
    let symbol = sess_cfg.and_then(|s| s.symbol.as_deref());
    let style = sess_cfg.and_then(|s| s.style.as_deref());
    if let Some(fmt) = sess_cfg.and_then(|s| s.format.as_deref()) {
        return crate::format::apply_module_format(fmt, Some(value), symbol, style);
    }
    let symbol_str = symbol.unwrap_or("");
    let content = format!("{symbol_str}{value}");
    Some(crate::ansi::apply_style(&content, style))
}

/// Renders `$cship.version` — Claude Code version string from Context.
pub fn render_version(
    ctx: &crate::context::Context,
    cfg: &crate::config::CshipConfig,
) -> Option<String> {
    if cfg
        .session
        .as_ref()
        .and_then(|s| s.disabled)
        .unwrap_or(false)
    {
        return None;
    }
    let value = match ctx.version.as_deref() {
        Some(v) => v,
        None => {
            tracing::warn!("cship.version: field absent from context");
            return None;
        }
    };
    let sess_cfg = cfg.session.as_ref();
    let symbol = sess_cfg.and_then(|s| s.symbol.as_deref());
    let style = sess_cfg.and_then(|s| s.style.as_deref());
    if let Some(fmt) = sess_cfg.and_then(|s| s.format.as_deref()) {
        return crate::format::apply_module_format(fmt, Some(value), symbol, style);
    }
    let symbol_str = symbol.unwrap_or("");
    let content = format!("{symbol_str}{value}");
    Some(crate::ansi::apply_style(&content, style))
}

/// Renders `$cship.output_style` — output style name from Context (nested field).
pub fn render_output_style(
    ctx: &crate::context::Context,
    cfg: &crate::config::CshipConfig,
) -> Option<String> {
    if cfg
        .session
        .as_ref()
        .and_then(|s| s.disabled)
        .unwrap_or(false)
    {
        return None;
    }
    let value = match ctx.output_style.as_ref().and_then(|o| o.name.as_deref()) {
        Some(v) => v,
        None => {
            tracing::warn!("cship.output_style: field absent from context");
            return None;
        }
    };
    let sess_cfg = cfg.session.as_ref();
    let symbol = sess_cfg.and_then(|s| s.symbol.as_deref());
    let style = sess_cfg.and_then(|s| s.style.as_deref());
    if let Some(fmt) = sess_cfg.and_then(|s| s.format.as_deref()) {
        return crate::format::apply_module_format(fmt, Some(value), symbol, style);
    }
    let symbol_str = symbol.unwrap_or("");
    let content = format!("{symbol_str}{value}");
    Some(crate::ansi::apply_style(&content, style))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CshipConfig, SessionConfig};
    use crate::context::{Context, OutputStyle};

    fn ctx_with_cwd(cwd: &str) -> Context {
        Context {
            cwd: Some(cwd.to_string()),
            ..Default::default()
        }
    }

    // ── cwd ───────────────────────────────────────────────────────────────

    #[test]
    fn test_render_cwd_renders_value() {
        let ctx = ctx_with_cwd("/home/leo/projects/cship");
        let result = render_cwd(&ctx, &CshipConfig::default());
        assert_eq!(result, Some("/home/leo/projects/cship".to_string()));
    }

    #[test]
    fn test_render_cwd_disabled_returns_none() {
        let ctx = ctx_with_cwd("/home/leo/projects/cship");
        let cfg = CshipConfig {
            session: Some(SessionConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render_cwd(&ctx, &cfg), None);
    }

    #[test]
    fn test_render_cwd_absent_returns_none() {
        let ctx = Context::default();
        assert_eq!(render_cwd(&ctx, &CshipConfig::default()), None);
    }

    // ── session_id ────────────────────────────────────────────────────────

    #[test]
    fn test_render_session_id_renders_value() {
        let ctx = Context {
            session_id: Some("test-session-id".to_string()),
            ..Default::default()
        };
        let result = render_session_id(&ctx, &CshipConfig::default());
        assert_eq!(result, Some("test-session-id".to_string()));
    }

    #[test]
    fn test_render_session_id_disabled_returns_none() {
        let ctx = Context {
            session_id: Some("s".to_string()),
            ..Default::default()
        };
        let cfg = CshipConfig {
            session: Some(SessionConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render_session_id(&ctx, &cfg), None);
    }

    #[test]
    fn test_render_session_id_absent_returns_none() {
        let ctx = Context::default();
        assert_eq!(render_session_id(&ctx, &CshipConfig::default()), None);
    }

    // ── transcript_path ───────────────────────────────────────────────────

    #[test]
    fn test_render_transcript_path_renders_value() {
        let ctx = Context {
            transcript_path: Some("/home/user/.claude/projects/myapp/transcript.jsonl".to_string()),
            ..Default::default()
        };
        let result = render_transcript_path(&ctx, &CshipConfig::default());
        assert_eq!(
            result,
            Some("/home/user/.claude/projects/myapp/transcript.jsonl".to_string())
        );
    }

    #[test]
    fn test_render_transcript_path_disabled_returns_none() {
        let ctx = Context {
            transcript_path: Some("/tmp/t.jsonl".to_string()),
            ..Default::default()
        };
        let cfg = CshipConfig {
            session: Some(SessionConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render_transcript_path(&ctx, &cfg), None);
    }

    #[test]
    fn test_render_transcript_path_absent_returns_none() {
        let ctx = Context::default();
        assert_eq!(render_transcript_path(&ctx, &CshipConfig::default()), None);
    }

    // ── version ───────────────────────────────────────────────────────────

    #[test]
    fn test_render_version_renders_value() {
        let ctx = Context {
            version: Some("1.0.80".to_string()),
            ..Default::default()
        };
        let result = render_version(&ctx, &CshipConfig::default());
        assert_eq!(result, Some("1.0.80".to_string()));
    }

    #[test]
    fn test_render_version_disabled_returns_none() {
        let ctx = Context {
            version: Some("1.0.80".to_string()),
            ..Default::default()
        };
        let cfg = CshipConfig {
            session: Some(SessionConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render_version(&ctx, &cfg), None);
    }

    #[test]
    fn test_render_version_absent_returns_none() {
        let ctx = Context::default();
        assert_eq!(render_version(&ctx, &CshipConfig::default()), None);
    }

    // ── output_style ──────────────────────────────────────────────────────

    #[test]
    fn test_render_output_style_renders_name() {
        let ctx = Context {
            output_style: Some(OutputStyle {
                name: Some("explanatory".to_string()),
            }),
            ..Default::default()
        };
        let result = render_output_style(&ctx, &CshipConfig::default());
        assert_eq!(result, Some("explanatory".to_string()));
    }

    #[test]
    fn test_render_output_style_disabled_returns_none() {
        let ctx = Context {
            output_style: Some(OutputStyle {
                name: Some("default".to_string()),
            }),
            ..Default::default()
        };
        let cfg = CshipConfig {
            session: Some(SessionConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render_output_style(&ctx, &cfg), None);
    }

    #[test]
    fn test_render_output_style_absent_returns_none() {
        let ctx = Context::default();
        assert_eq!(render_output_style(&ctx, &CshipConfig::default()), None);
    }

    #[test]
    fn test_render_output_style_name_none_returns_none() {
        let ctx = Context {
            output_style: Some(OutputStyle { name: None }),
            ..Default::default()
        };
        assert_eq!(render_output_style(&ctx, &CshipConfig::default()), None);
    }
}
