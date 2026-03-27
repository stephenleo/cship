use crate::config::CshipConfig;
use crate::context::Context;

const DEFAULT_EXCEEDS_SYMBOL: &str = ">200k";

fn is_disabled(cfg: &CshipConfig) -> bool {
    cfg.context_window
        .as_ref()
        .and_then(|c| c.disabled)
        .unwrap_or(false)
}

fn apply_cw_style(content: &str, cfg: &CshipConfig) -> String {
    crate::ansi::apply_style(
        content,
        cfg.context_window.as_ref().and_then(|c| c.style.as_deref()),
    )
}

/// Renders `$cship.context_window.used_percentage` — integer percentage, no `%` sign.
pub fn render_used_percentage(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let cw_cfg = cfg.context_window.as_ref();
    let sub_cfg = cw_cfg.and_then(|c| c.used_percentage.as_ref());
    if sub_cfg.and_then(|c| c.disabled).unwrap_or(false) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.used_percentage)
    {
        Some(v) => v,
        None => {
            tracing::warn!("cship.context_window.used_percentage: value absent from context");
            return None;
        }
    };
    let val_str = format!("{:.0}", val);
    crate::format::render_styled_value(
        &val_str,
        Some(val),
        sub_cfg,
        cw_cfg.map(|c| c as &dyn crate::config::HasThresholdStyle),
    )
}

/// Renders `$cship.context_window.remaining_percentage` — integer percentage, no `%` sign.
///
/// ## `invert_threshold` contract
///
/// When [`crate::config::SubfieldConfig::invert_threshold`] is `true`:
/// - `warn_threshold`, `warn_style`, `critical_threshold`, and `critical_style` are resolved
///   from the **sub-field config only** (`[cship.context_window.remaining_percentage]`).
///   Parent [`crate::config::ContextWindowConfig`] threshold values are **not** inherited — they live in the
///   non-inverted domain (high = bad), whereas this sub-field treats low values as bad.
///   Inheriting parent thresholds would incorrectly invert the semantics.
/// - Base `style` **still falls back to the parent** [`crate::config::ContextWindowConfig`]`.style` when not
///   set on the sub-field. Style inheritance is domain-independent and safe.
pub fn render_remaining_percentage(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let cw_cfg = cfg.context_window.as_ref();
    let sub_cfg = cw_cfg.and_then(|c| c.remaining_percentage.as_ref());
    if sub_cfg.and_then(|c| c.disabled).unwrap_or(false) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.remaining_percentage)
    {
        Some(v) => v,
        None => {
            tracing::warn!("cship.context_window.remaining_percentage: value absent from context");
            return None;
        }
    };
    let val_str = format!("{:.0}", val);
    crate::format::render_styled_value(
        &val_str,
        Some(val),
        sub_cfg,
        cw_cfg.map(|c| c as &dyn crate::config::HasThresholdStyle),
    )
}

/// Renders `$cship.context_window.used_tokens` — real token count from `current_usage`.
///
/// Computes: `input_tokens + cache_creation_input_tokens + cache_read_input_tokens`
/// from `current_usage`, combined with `used_percentage` and `context_window_size`.
/// Output format: `8%(79k/1000k)`.
/// Returns `None` when `current_usage` is absent (before first API call).
// Explicit `match` arms are intentional per CLAUDE.md convention — all absent-data paths use
// explicit `match`, not `?`. Suppress clippy::question_mark for the silent-None arms.
#[allow(clippy::question_mark)]
pub fn render_used_tokens(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let cw_cfg = cfg.context_window.as_ref();
    let sub_cfg = cw_cfg.and_then(|c| c.used_tokens.as_ref());
    if sub_cfg.and_then(|c| c.disabled).unwrap_or(false) {
        return None;
    }
    let cw = match ctx.context_window.as_ref() {
        Some(cw) => cw,
        None => {
            tracing::warn!("cship.context_window.used_tokens: context_window absent from context");
            return None;
        }
    };
    let cu = match cw.current_usage.as_ref() {
        Some(cu) => cu,
        None => {
            tracing::warn!(
                "cship.context_window.used_tokens: current_usage absent from context_window"
            );
            return None;
        }
    };
    let used = cu.input_tokens.unwrap_or(0)
        + cu.cache_creation_input_tokens.unwrap_or(0)
        + cu.cache_read_input_tokens.unwrap_or(0);
    let size = match cw.context_window_size {
        Some(v) => v,
        None => {
            tracing::warn!(
                "cship.context_window.used_tokens: context_window_size absent from context"
            );
            return None;
        }
    };
    let pct = cw.used_percentage.unwrap_or(0.0);
    let val_str = format!(
        "{:.0}%({}k/{}k)",
        pct,
        (used + 500) / 1000,
        (size + 500) / 1000
    );
    crate::format::render_styled_value(
        &val_str,
        Some(pct),
        sub_cfg,
        cw_cfg.map(|c| c as &dyn crate::config::HasThresholdStyle),
    )
}

/// Renders `$cship.context_window.size` — reads `context_window_size` field (not `size`).
pub fn render_size(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let cw_cfg = cfg.context_window.as_ref();
    let sub_cfg = cw_cfg.and_then(|c| c.size.as_ref());
    if sub_cfg.and_then(|c| c.disabled).unwrap_or(false) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.context_window_size)
    {
        Some(v) => v,
        None => {
            tracing::warn!("cship.context_window.size: context_window_size absent from context");
            return None;
        }
    };
    let val_str = val.to_string();
    crate::format::render_styled_value(
        &val_str,
        Some(val as f64),
        sub_cfg,
        cw_cfg.map(|c| c as &dyn crate::config::HasThresholdStyle),
    )
}

/// Renders `$cship.context_window.total_input_tokens`.
pub fn render_total_input_tokens(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let cw_cfg = cfg.context_window.as_ref();
    let sub_cfg = cw_cfg.and_then(|c| c.total_input_tokens.as_ref());
    if sub_cfg.and_then(|c| c.disabled).unwrap_or(false) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.total_input_tokens)
    {
        Some(v) => v,
        None => {
            tracing::warn!("cship.context_window.total_input_tokens: value absent from context");
            return None;
        }
    };
    let val_str = val.to_string();
    crate::format::render_styled_value(
        &val_str,
        Some(val as f64),
        sub_cfg,
        cw_cfg.map(|c| c as &dyn crate::config::HasThresholdStyle),
    )
}

/// Renders `$cship.context_window.total_output_tokens`.
pub fn render_total_output_tokens(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let cw_cfg = cfg.context_window.as_ref();
    let sub_cfg = cw_cfg.and_then(|c| c.total_output_tokens.as_ref());
    if sub_cfg.and_then(|c| c.disabled).unwrap_or(false) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.total_output_tokens)
    {
        Some(v) => v,
        None => {
            tracing::warn!("cship.context_window.total_output_tokens: value absent from context");
            return None;
        }
    };
    let val_str = val.to_string();
    crate::format::render_styled_value(
        &val_str,
        Some(val as f64),
        sub_cfg,
        cw_cfg.map(|c| c as &dyn crate::config::HasThresholdStyle),
    )
}

/// Renders `$cship.context_window.exceeds_200k`.
///
/// CRITICAL: `exceeds_200k_tokens` is a TOP-LEVEL field on `Context`, NOT inside `context_window`.
/// Returns None when false or absent (no tracing::warn! — false is a valid expected state).
/// When true, renders configurable symbol (default: ">200k").
pub fn render_exceeds_200k(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let exceeds = ctx.exceeds_200k_tokens.unwrap_or(false);
    if !exceeds {
        return None; // false is normal — no warn
    }
    let cw_cfg = cfg.context_window.as_ref();
    let symbol_str = cw_cfg
        .and_then(|c| c.symbol.as_deref())
        .unwrap_or(DEFAULT_EXCEEDS_SYMBOL);
    if let Some(fmt) = cw_cfg.and_then(|c| c.format.as_deref()) {
        let style = cw_cfg.and_then(|c| c.style.as_deref());
        return crate::format::apply_module_format(fmt, Some(symbol_str), Some(symbol_str), style);
    }
    Some(apply_cw_style(symbol_str, cfg))
}

/// Renders `$cship.context_window.current_usage.input_tokens`.
pub fn render_current_usage_input_tokens(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let cw_cfg = cfg.context_window.as_ref();
    let sub_cfg = cw_cfg.and_then(|c| c.current_usage_input_tokens.as_ref());
    if sub_cfg.and_then(|c| c.disabled).unwrap_or(false) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.current_usage.as_ref())
        .and_then(|cu| cu.input_tokens)
    {
        Some(v) => v,
        None => {
            tracing::warn!(
                "cship.context_window.current_usage.input_tokens: value absent from context"
            );
            return None;
        }
    };
    let val_str = val.to_string();
    crate::format::render_styled_value(
        &val_str,
        Some(val as f64),
        sub_cfg,
        cw_cfg.map(|c| c as &dyn crate::config::HasThresholdStyle),
    )
}

/// Renders `$cship.context_window.current_usage.output_tokens`.
pub fn render_current_usage_output_tokens(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let cw_cfg = cfg.context_window.as_ref();
    let sub_cfg = cw_cfg.and_then(|c| c.current_usage_output_tokens.as_ref());
    if sub_cfg.and_then(|c| c.disabled).unwrap_or(false) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.current_usage.as_ref())
        .and_then(|cu| cu.output_tokens)
    {
        Some(v) => v,
        None => {
            tracing::warn!(
                "cship.context_window.current_usage.output_tokens: value absent from context"
            );
            return None;
        }
    };
    let val_str = val.to_string();
    crate::format::render_styled_value(
        &val_str,
        Some(val as f64),
        sub_cfg,
        cw_cfg.map(|c| c as &dyn crate::config::HasThresholdStyle),
    )
}

/// Renders `$cship.context_window.current_usage.cache_creation_input_tokens`.
pub fn render_current_usage_cache_creation_input_tokens(
    ctx: &Context,
    cfg: &CshipConfig,
) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let cw_cfg = cfg.context_window.as_ref();
    let sub_cfg = cw_cfg.and_then(|c| c.current_usage_cache_creation_input_tokens.as_ref());
    if sub_cfg.and_then(|c| c.disabled).unwrap_or(false) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.current_usage.as_ref())
        .and_then(|cu| cu.cache_creation_input_tokens)
    {
        Some(v) => v,
        None => {
            tracing::warn!(
                "cship.context_window.current_usage.cache_creation_input_tokens: value absent from context"
            );
            return None;
        }
    };
    let val_str = val.to_string();
    crate::format::render_styled_value(
        &val_str,
        Some(val as f64),
        sub_cfg,
        cw_cfg.map(|c| c as &dyn crate::config::HasThresholdStyle),
    )
}

/// Renders `$cship.context_window.current_usage.cache_read_input_tokens`.
pub fn render_current_usage_cache_read_input_tokens(
    ctx: &Context,
    cfg: &CshipConfig,
) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let cw_cfg = cfg.context_window.as_ref();
    let sub_cfg = cw_cfg.and_then(|c| c.current_usage_cache_read_input_tokens.as_ref());
    if sub_cfg.and_then(|c| c.disabled).unwrap_or(false) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.current_usage.as_ref())
        .and_then(|cu| cu.cache_read_input_tokens)
    {
        Some(v) => v,
        None => {
            tracing::warn!(
                "cship.context_window.current_usage.cache_read_input_tokens: value absent from context"
            );
            return None;
        }
    };
    let val_str = val.to_string();
    crate::format::render_styled_value(
        &val_str,
        Some(val as f64),
        sub_cfg,
        cw_cfg.map(|c| c as &dyn crate::config::HasThresholdStyle),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ContextWindowConfig, CshipConfig, SubfieldConfig};
    use crate::context::{Context, ContextWindow, CurrentUsage};

    fn ctx_full() -> Context {
        Context {
            exceeds_200k_tokens: Some(false),
            context_window: Some(ContextWindow {
                used_percentage: Some(35.0),
                remaining_percentage: Some(65.0),
                context_window_size: Some(200000),
                total_input_tokens: Some(15234),
                total_output_tokens: Some(4521),
                current_usage: Some(CurrentUsage {
                    input_tokens: Some(8500),
                    output_tokens: Some(1200),
                    cache_creation_input_tokens: Some(5000),
                    cache_read_input_tokens: Some(2000),
                }),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_used_percentage_renders_as_integer_no_percent_sign() {
        let ctx = ctx_full();
        assert_eq!(
            render_used_percentage(&ctx, &CshipConfig::default()),
            Some("35".to_string())
        );
    }

    #[test]
    fn test_remaining_percentage_renders_as_integer() {
        let ctx = ctx_full();
        assert_eq!(
            render_remaining_percentage(&ctx, &CshipConfig::default()),
            Some("65".to_string())
        );
    }

    #[test]
    fn test_size_reads_context_window_size_field() {
        let ctx = ctx_full();
        assert_eq!(
            render_size(&ctx, &CshipConfig::default()),
            Some("200000".to_string())
        );
    }

    #[test]
    fn test_total_input_tokens() {
        let ctx = ctx_full();
        assert_eq!(
            render_total_input_tokens(&ctx, &CshipConfig::default()),
            Some("15234".to_string())
        );
    }

    #[test]
    fn test_total_output_tokens() {
        let ctx = ctx_full();
        assert_eq!(
            render_total_output_tokens(&ctx, &CshipConfig::default()),
            Some("4521".to_string())
        );
    }

    #[test]
    fn test_exceeds_200k_false_returns_none_no_warn() {
        let ctx = ctx_full(); // exceeds_200k_tokens = false
        assert_eq!(render_exceeds_200k(&ctx, &CshipConfig::default()), None);
    }

    #[test]
    fn test_exceeds_200k_absent_treated_as_false() {
        let ctx = Context::default(); // exceeds_200k_tokens = None
        assert_eq!(render_exceeds_200k(&ctx, &CshipConfig::default()), None);
    }

    #[test]
    fn test_exceeds_200k_true_renders_default_symbol() {
        let ctx = Context {
            exceeds_200k_tokens: Some(true),
            ..Default::default()
        };
        let result = render_exceeds_200k(&ctx, &CshipConfig::default());
        assert_eq!(result, Some(">200k".to_string()));
    }

    #[test]
    fn test_current_usage_input_tokens() {
        let ctx = ctx_full();
        assert_eq!(
            render_current_usage_input_tokens(&ctx, &CshipConfig::default()),
            Some("8500".to_string())
        );
    }

    #[test]
    fn test_current_usage_output_tokens() {
        let ctx = ctx_full();
        assert_eq!(
            render_current_usage_output_tokens(&ctx, &CshipConfig::default()),
            Some("1200".to_string())
        );
    }

    #[test]
    fn test_current_usage_cache_creation_tokens() {
        let ctx = ctx_full();
        assert_eq!(
            render_current_usage_cache_creation_input_tokens(&ctx, &CshipConfig::default()),
            Some("5000".to_string())
        );
    }

    #[test]
    fn test_current_usage_cache_read_tokens() {
        let ctx = ctx_full();
        assert_eq!(
            render_current_usage_cache_read_input_tokens(&ctx, &CshipConfig::default()),
            Some("2000".to_string())
        );
    }

    #[test]
    fn test_exceeds_200k_true_renders_custom_symbol() {
        let ctx = Context {
            exceeds_200k_tokens: Some(true),
            ..Default::default()
        };
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                symbol: Some("⚠".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_exceeds_200k(&ctx, &cfg);
        assert_eq!(result, Some("⚠".to_string()));
    }

    #[test]
    fn test_disabled_flag_suppresses_all_renders() {
        let ctx = ctx_full();
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render_used_percentage(&ctx, &cfg), None);
        assert_eq!(render_size(&ctx, &cfg), None);
        assert_eq!(render_exceeds_200k(&ctx, &cfg), None);
    }

    #[test]
    fn test_absent_context_window_returns_none() {
        let ctx = Context::default(); // no context_window
        assert_eq!(render_used_percentage(&ctx, &CshipConfig::default()), None);
        assert_eq!(render_size(&ctx, &CshipConfig::default()), None);
        assert_eq!(
            render_total_input_tokens(&ctx, &CshipConfig::default()),
            None
        );
        // Story 1.2: render_used_tokens must also return None (with tracing::warn!) when context_window absent
        assert_eq!(render_used_tokens(&ctx, &CshipConfig::default()), None);
    }

    // --- AC7: Sub-field threshold tests (Story 9.2) ---

    #[test]
    fn test_subfield_used_percentage_above_warn_applies_warn_style() {
        // AC7: used_percentage = 85 > warn_threshold 80 → warn_style applied
        let ctx = Context {
            context_window: Some(ContextWindow {
                used_percentage: Some(85.0),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                used_percentage: Some(SubfieldConfig {
                    warn_threshold: Some(80.0),
                    warn_style: Some("yellow".to_string()),
                    critical_threshold: Some(95.0),
                    critical_style: Some("bold red".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_used_percentage(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI for warn: {result:?}"
        );
        assert!(result.contains("85"), "expected value: {result:?}");
        // Verify warn style (yellow = \x1b[33m) is distinct from critical
        assert!(
            result.contains("\x1b[33m"),
            "expected yellow ANSI code for warn: {result:?}"
        );
    }

    #[test]
    fn test_subfield_used_percentage_above_critical_applies_critical_style() {
        let ctx = Context {
            context_window: Some(ContextWindow {
                used_percentage: Some(97.0),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                used_percentage: Some(SubfieldConfig {
                    warn_threshold: Some(80.0),
                    warn_style: Some("yellow".to_string()),
                    critical_threshold: Some(95.0),
                    critical_style: Some("bold red".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_used_percentage(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI for critical: {result:?}"
        );
        assert!(result.contains("97"), "expected value: {result:?}");
        // Verify critical style (bold red = \x1b[1;31m combined SGR) is distinct from warn
        assert!(
            result.contains("\x1b[1;31m"),
            "expected bold+red ANSI code for critical: {result:?}"
        );
    }

    #[test]
    fn test_subfield_used_percentage_below_warn_uses_base_style() {
        let ctx = Context {
            context_window: Some(ContextWindow {
                used_percentage: Some(50.0),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                used_percentage: Some(SubfieldConfig {
                    warn_threshold: Some(80.0),
                    warn_style: Some("yellow".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_used_percentage(&ctx, &cfg).unwrap();
        assert!(
            !result.contains('\x1b'),
            "no ANSI expected below warn: {result:?}"
        );
        assert!(result.contains("50"), "expected value: {result:?}");
    }

    #[test]
    fn test_subfield_parent_threshold_used_as_fallback() {
        // AC3: parent warn_threshold applies when no sub-field threshold is set
        let ctx = Context {
            context_window: Some(ContextWindow {
                used_percentage: Some(85.0),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                warn_threshold: Some(80.0),
                warn_style: Some("yellow".to_string()),
                // no per-sub-field config for used_percentage
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_used_percentage(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected parent threshold fallback ANSI: {result:?}"
        );
    }

    #[test]
    fn test_subfield_disabled_flag_suppresses_only_that_subfield() {
        // AC6: sub-field disabled=true suppresses only that sub-field
        let ctx = ctx_full(); // used_percentage=35, remaining_percentage=65
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                used_percentage: Some(SubfieldConfig {
                    disabled: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        // used_percentage disabled → None
        assert_eq!(render_used_percentage(&ctx, &cfg), None);
        // remaining_percentage NOT disabled → still renders
        assert_eq!(
            render_remaining_percentage(&ctx, &cfg),
            Some("65".to_string())
        );
    }

    #[test]
    fn test_subfield_format_with_warn_threshold_uses_warn_style() {
        // AC5: format path + threshold → threshold-resolved style in apply_module_format
        let ctx = Context {
            context_window: Some(ContextWindow {
                used_percentage: Some(85.0),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                used_percentage: Some(SubfieldConfig {
                    format: Some("[$value%]($style)".to_string()),
                    warn_threshold: Some(80.0),
                    warn_style: Some("yellow".to_string()),
                    critical_threshold: Some(95.0),
                    critical_style: Some("bold red".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_used_percentage(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI in format path: {result:?}"
        );
        assert!(
            result.contains("85"),
            "expected value in format: {result:?}"
        );
    }

    #[test]
    fn test_subfield_size_above_warn_applies_warn_style() {
        // u64 cast coverage for render_size
        let ctx = Context {
            context_window: Some(ContextWindow {
                context_window_size: Some(200000),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                size: Some(SubfieldConfig {
                    warn_threshold: Some(150000.0),
                    warn_style: Some("yellow".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_size(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI for size warn: {result:?}"
        );
        assert!(result.contains("200000"), "expected value: {result:?}");
    }

    #[test]
    fn test_subfield_total_input_tokens_above_warn_applies_warn_style() {
        // u64 cast coverage for render_total_input_tokens
        let ctx = Context {
            context_window: Some(ContextWindow {
                total_input_tokens: Some(160000),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                total_input_tokens: Some(SubfieldConfig {
                    warn_threshold: Some(150000.0),
                    warn_style: Some("yellow".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_total_input_tokens(&ctx, &cfg).unwrap();
        assert!(result.contains('\x1b'), "expected ANSI: {result:?}");
    }

    #[test]
    fn test_subfield_style_overrides_parent_style() {
        // AC3: sub-field style takes priority over parent style (no thresholds involved)
        let ctx = Context {
            context_window: Some(ContextWindow {
                used_percentage: Some(50.0),
                ..Default::default()
            }),
            ..Default::default()
        };
        // Parent style = green, sub-field style = blue → blue should win
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                style: Some("green".to_string()),
                used_percentage: Some(SubfieldConfig {
                    style: Some("blue".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_used_percentage(&ctx, &cfg).unwrap();
        // blue = \x1b[34m, green = \x1b[32m
        assert!(
            result.contains("\x1b[34m"),
            "expected blue (sub-field) style, not green (parent): {result:?}"
        );
        assert!(
            !result.contains("\x1b[32m"),
            "parent green style should NOT appear: {result:?}"
        );
    }

    #[test]
    fn test_remaining_percentage_invert_threshold_fires_when_low() {
        // invert_threshold=true: warn fires when remaining < warn_threshold (low = bad)
        let ctx = Context {
            context_window: Some(ContextWindow {
                remaining_percentage: Some(15.0), // 15% remaining — below warn=20
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                remaining_percentage: Some(SubfieldConfig {
                    invert_threshold: Some(true),
                    warn_threshold: Some(20.0),
                    warn_style: Some("yellow".to_string()),
                    critical_threshold: Some(10.0),
                    critical_style: Some("bold red".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_remaining_percentage(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI for low remaining: {result:?}"
        );
        assert!(result.contains("15"), "expected value: {result:?}");
    }

    #[test]
    fn test_remaining_percentage_invert_threshold_no_fire_when_high() {
        // invert_threshold=true: no warn when remaining=85 (healthy)
        let ctx = Context {
            context_window: Some(ContextWindow {
                remaining_percentage: Some(85.0),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                remaining_percentage: Some(SubfieldConfig {
                    invert_threshold: Some(true),
                    warn_threshold: Some(20.0),
                    warn_style: Some("yellow".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_remaining_percentage(&ctx, &cfg).unwrap();
        assert!(
            !result.contains('\x1b'),
            "expected no ANSI for high remaining: {result:?}"
        );
    }

    #[test]
    fn test_remaining_percentage_invert_critical_fires_below_critical_threshold() {
        // invert_threshold=true: critical fires when remaining < critical_threshold (10)
        let ctx = Context {
            context_window: Some(ContextWindow {
                remaining_percentage: Some(5.0), // 5% remaining — critically low
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                remaining_percentage: Some(SubfieldConfig {
                    invert_threshold: Some(true),
                    warn_threshold: Some(20.0),
                    warn_style: Some("yellow".to_string()),
                    critical_threshold: Some(10.0),
                    critical_style: Some("bold red".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_remaining_percentage(&ctx, &cfg).unwrap();
        assert!(
            result.contains("\x1b[1;31m"),
            "expected bold red for critically low remaining: {result:?}"
        );
    }

    #[test]
    fn test_remaining_percentage_invert_does_not_inherit_parent_threshold() {
        // invert_threshold=true: parent warn_threshold=80 must NOT fire for remaining=85
        // (85% remaining is healthy — parent threshold is in non-inverted domain)
        let ctx = Context {
            context_window: Some(ContextWindow {
                remaining_percentage: Some(85.0),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                warn_threshold: Some(80.0), // parent: warn when 80% USED
                warn_style: Some("yellow".to_string()),
                remaining_percentage: Some(SubfieldConfig {
                    invert_threshold: Some(true),
                    // no subfield thresholds set → nothing fires
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_remaining_percentage(&ctx, &cfg).unwrap();
        assert!(
            !result.contains('\x1b'),
            "parent threshold must not fire for remaining_percentage with invert_threshold: {result:?}"
        );
    }

    #[test]
    fn test_subfield_no_threshold_unchanged() {
        // AC8: no threshold fields → output identical to baseline (no regression)
        let ctx = ctx_full();
        let result_default = render_used_percentage(&ctx, &CshipConfig::default());
        let cfg_no_thresh = CshipConfig {
            context_window: Some(ContextWindowConfig {
                used_percentage: Some(SubfieldConfig {
                    ..Default::default() // all None
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result_explicit = render_used_percentage(&ctx, &cfg_no_thresh);
        assert_eq!(result_default, result_explicit);
    }

    // --- Story 1.1: used_tokens threshold styling tests ---

    fn ctx_used_tokens(pct: f64) -> Context {
        // Build a context where used_percentage = pct and current_usage is set
        // used = 8000 + 5000 + 2000 = 15000, size = 200000
        Context {
            context_window: Some(ContextWindow {
                used_percentage: Some(pct),
                remaining_percentage: Some(100.0 - pct),
                context_window_size: Some(200000),
                current_usage: Some(CurrentUsage {
                    input_tokens: Some(8000),
                    cache_creation_input_tokens: Some(5000),
                    cache_read_input_tokens: Some(2000),
                    output_tokens: Some(1200),
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_used_tokens_warn_threshold_applied() {
        // AC1: used_percentage (85) >= warn_threshold (80) → warn_style applied
        let ctx = ctx_used_tokens(85.0);
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                used_tokens: Some(SubfieldConfig {
                    warn_threshold: Some(80.0),
                    warn_style: Some("yellow".to_string()),
                    critical_threshold: Some(95.0),
                    critical_style: Some("bold red".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_used_tokens(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI for warn: {result:?}"
        );
        assert!(
            result.contains("85"),
            "expected percentage value in output: {result:?}"
        );
        assert!(
            result.contains("\x1b[33m"),
            "expected yellow ANSI code for warn: {result:?}"
        );
    }

    #[test]
    fn test_used_tokens_critical_threshold_applied() {
        // AC2: used_percentage (97) >= critical_threshold (95) → critical_style applied
        let ctx = ctx_used_tokens(97.0);
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                used_tokens: Some(SubfieldConfig {
                    warn_threshold: Some(80.0),
                    warn_style: Some("yellow".to_string()),
                    critical_threshold: Some(95.0),
                    critical_style: Some("bold red".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_used_tokens(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI for critical: {result:?}"
        );
        assert!(
            result.contains("97"),
            "expected percentage value in output: {result:?}"
        );
        assert!(
            result.contains("\x1b[1;31m"),
            "expected bold+red ANSI code for critical: {result:?}"
        );
    }

    #[test]
    fn test_used_tokens_no_threshold_config_no_regression() {
        // AC3: no threshold config → output identical to baseline (no ANSI, same string)
        let ctx = ctx_used_tokens(35.0);
        let result_default = render_used_tokens(&ctx, &CshipConfig::default()).unwrap();
        let cfg_no_thresh = CshipConfig {
            context_window: Some(ContextWindowConfig {
                used_tokens: Some(SubfieldConfig {
                    ..Default::default() // all None
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result_explicit = render_used_tokens(&ctx, &cfg_no_thresh).unwrap();
        assert_eq!(
            result_default, result_explicit,
            "no-threshold config must not change output"
        );
        assert!(
            !result_default.contains('\x1b'),
            "no ANSI expected without threshold config: {result_default:?}"
        );
    }

    #[test]
    fn test_used_tokens_threshold_parent_fallback() {
        // AC3 + parent fallback: parent warn_threshold applies when no sub_cfg threshold set
        let ctx = ctx_used_tokens(85.0);
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                warn_threshold: Some(80.0),
                warn_style: Some("yellow".to_string()),
                // no per-sub-field config for used_tokens
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_used_tokens(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected parent threshold fallback ANSI: {result:?}"
        );
        assert!(
            result.contains("\x1b[33m"),
            "expected yellow from parent fallback: {result:?}"
        );
    }

    // --- Story 1.2: explicit match + tracing::warn! for render_used_tokens ---

    #[test]
    fn test_used_tokens_absent_current_usage_returns_none() {
        // AC1: current_usage is None → tracing::warn! emitted, returns None
        let ctx = Context {
            context_window: Some(ContextWindow {
                used_percentage: Some(50.0),
                context_window_size: Some(200000),
                current_usage: None,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render_used_tokens(&ctx, &CshipConfig::default()), None);
    }

    #[test]
    fn test_used_tokens_absent_context_window_size_returns_none() {
        // AC2: context_window_size is None when current_usage exists → tracing::warn! emitted
        // (not assertable in unit tests without a tracing subscriber) but function must return None
        let ctx = Context {
            context_window: Some(ContextWindow {
                used_percentage: Some(50.0),
                context_window_size: None,
                current_usage: Some(CurrentUsage {
                    input_tokens: Some(8000),
                    cache_creation_input_tokens: Some(0),
                    cache_read_input_tokens: Some(0),
                    output_tokens: Some(0),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render_used_tokens(&ctx, &CshipConfig::default()), None);
    }

    #[test]
    fn test_used_tokens_rounding_500_shows_1k() {
        // 500 tokens: (500 + 500) / 1000 = 1 → displays "1k"
        // size 200000: (200000 + 500) / 1000 = 200 → displays "200k"
        let ctx = Context {
            context_window: Some(ContextWindow {
                used_percentage: Some(0.25),
                context_window_size: Some(200000),
                current_usage: Some(CurrentUsage {
                    input_tokens: Some(500),
                    cache_creation_input_tokens: Some(0),
                    cache_read_input_tokens: Some(0),
                    output_tokens: Some(0),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_used_tokens(&ctx, &CshipConfig::default()).unwrap();
        assert!(
            result.contains("1k/200k"),
            "expected '1k/200k' in: {result:?}"
        );
    }

    #[test]
    fn test_used_tokens_rounding_499_shows_0k() {
        // 499 tokens: (499 + 500) / 1000 = 0 → displays "0k" (below 0.5k threshold)
        // size 200000: (200000 + 500) / 1000 = 200 → displays "200k"
        let ctx = Context {
            context_window: Some(ContextWindow {
                used_percentage: Some(0.25),
                context_window_size: Some(200000),
                current_usage: Some(CurrentUsage {
                    input_tokens: Some(499),
                    cache_creation_input_tokens: Some(0),
                    cache_read_input_tokens: Some(0),
                    output_tokens: Some(0),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_used_tokens(&ctx, &CshipConfig::default()).unwrap();
        assert!(
            result.contains("0k/200k"),
            "expected '0k/200k' in: {result:?}"
        );
    }
}
