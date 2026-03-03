//! Workspace modules: workspace.current_dir, workspace.project_dir.
//!
//! Both fields share ONE `WorkspaceConfig` (disabled/symbol/style).
//! [Source: epics.md#Story 2.4, architecture.md#Module System Architecture]

/// Renders `$cship.workspace.current_dir` — workspace current directory from Context.
pub fn render_current_dir(
    ctx: &crate::context::Context,
    cfg: &crate::config::CshipConfig,
) -> Option<String> {
    if cfg
        .workspace
        .as_ref()
        .and_then(|w| w.disabled)
        .unwrap_or(false)
    {
        return None;
    }
    let value = match ctx
        .workspace
        .as_ref()
        .and_then(|w| w.current_dir.as_deref())
    {
        Some(v) => v,
        None => {
            tracing::warn!("cship.workspace.current_dir: field absent from context");
            return None;
        }
    };
    let ws_cfg = cfg.workspace.as_ref();
    let symbol = ws_cfg.and_then(|w| w.symbol.as_deref()).unwrap_or("");
    let content = format!("{symbol}{value}");
    let style = ws_cfg.and_then(|w| w.style.as_deref());
    Some(crate::ansi::apply_style(&content, style))
}

/// Renders `$cship.workspace.project_dir` — workspace project directory from Context.
pub fn render_project_dir(
    ctx: &crate::context::Context,
    cfg: &crate::config::CshipConfig,
) -> Option<String> {
    if cfg
        .workspace
        .as_ref()
        .and_then(|w| w.disabled)
        .unwrap_or(false)
    {
        return None;
    }
    let value = match ctx
        .workspace
        .as_ref()
        .and_then(|w| w.project_dir.as_deref())
    {
        Some(v) => v,
        None => {
            tracing::warn!("cship.workspace.project_dir: field absent from context");
            return None;
        }
    };
    let ws_cfg = cfg.workspace.as_ref();
    let symbol = ws_cfg.and_then(|w| w.symbol.as_deref()).unwrap_or("");
    let content = format!("{symbol}{value}");
    let style = ws_cfg.and_then(|w| w.style.as_deref());
    Some(crate::ansi::apply_style(&content, style))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CshipConfig, WorkspaceConfig};
    use crate::context::{Context, Workspace};

    fn ctx_with_workspace(current_dir: &str, project_dir: &str) -> Context {
        Context {
            workspace: Some(Workspace {
                current_dir: Some(current_dir.to_string()),
                project_dir: Some(project_dir.to_string()),
            }),
            ..Default::default()
        }
    }

    // ── current_dir ───────────────────────────────────────────────────────

    #[test]
    fn test_render_current_dir_renders_value() {
        let ctx = ctx_with_workspace("/home/user/projects/myapp", "/home/user/projects/myapp");
        let result = render_current_dir(&ctx, &CshipConfig::default());
        assert_eq!(result, Some("/home/user/projects/myapp".to_string()));
    }

    #[test]
    fn test_render_current_dir_disabled_returns_none() {
        let ctx = ctx_with_workspace("/home/user/projects/myapp", "/home/user/projects/myapp");
        let cfg = CshipConfig {
            workspace: Some(WorkspaceConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render_current_dir(&ctx, &cfg), None);
    }

    #[test]
    fn test_render_current_dir_absent_returns_none() {
        let ctx = Context::default();
        assert_eq!(render_current_dir(&ctx, &CshipConfig::default()), None);
    }

    #[test]
    fn test_render_current_dir_field_none_returns_none() {
        let ctx = Context {
            workspace: Some(Workspace {
                current_dir: None,
                project_dir: Some("/p".to_string()),
            }),
            ..Default::default()
        };
        assert_eq!(render_current_dir(&ctx, &CshipConfig::default()), None);
    }

    // ── project_dir ───────────────────────────────────────────────────────

    #[test]
    fn test_render_project_dir_renders_value() {
        let ctx = ctx_with_workspace("/home/user/projects/myapp", "/home/user/projects/myapp");
        let result = render_project_dir(&ctx, &CshipConfig::default());
        assert_eq!(result, Some("/home/user/projects/myapp".to_string()));
    }

    #[test]
    fn test_render_project_dir_disabled_returns_none() {
        let ctx = ctx_with_workspace("/home/user/projects/myapp", "/home/user/projects/myapp");
        let cfg = CshipConfig {
            workspace: Some(WorkspaceConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render_project_dir(&ctx, &cfg), None);
    }

    #[test]
    fn test_render_project_dir_absent_returns_none() {
        let ctx = Context::default();
        assert_eq!(render_project_dir(&ctx, &CshipConfig::default()), None);
    }

    #[test]
    fn test_render_project_dir_field_none_returns_none() {
        let ctx = Context {
            workspace: Some(Workspace {
                current_dir: Some("/c".to_string()),
                project_dir: None,
            }),
            ..Default::default()
        };
        assert_eq!(render_project_dir(&ctx, &CshipConfig::default()), None);
    }
}
