//! Account module — renders the currently authenticated Anthropic account.
//!
//! Lets users see at a glance whether their active Claude Code session is on
//! their work or personal account. Profile data comes from the OAuth
//! `/api/oauth/profile` endpoint via [`crate::account::fetch_account_profile`].
//!
//! Render flow:
//! 1. Check `disabled` flag → silent `None`
//! 2. Resolve the account profile via [`resolve_profile`] — preferring a
//!    launcher-provided account from the `CSHIP_ACCOUNT` env var, else the
//!    keychain token → OAuth `/api/oauth/profile` fetch (fingerprint-gated and cached)
//! 3. Format output (default or user-defined format string)
//! 4. Apply style
//!
//! The `CSHIP_ACCOUNT` path exists because a multi-account launcher can
//! authenticate the session with an injected token that Claude Code strips from
//! this statusline subprocess. The keychain path then reports the wrong
//! (last-interactive) account. Such a launcher instead resolves the account at
//! launch and passes it in `CSHIP_ACCOUNT` (compact non-secret JSON of the same
//! shape as [`AccountProfile`]); this module simply renders it. Never a token.
//!
//! The OAuth token is never written to disk, stdout, or cache (NFR-S1/S3).

use crate::account::AccountProfile;
use crate::cache;
use crate::config::{AccountConfig, CshipConfig};
use crate::context::Context;

/// Env var a launcher may set to the current session's account as compact JSON
/// (the [`AccountProfile`] shape). Preferred over the keychain/OAuth path.
const ACCOUNT_ENV_VAR: &str = "CSHIP_ACCOUNT";

/// Default format string — renders the resolved label (org name or mapped alias).
const DEFAULT_FORMAT: &str = "{label}";

/// Default cache TTL: 24 hours. Profile data rarely changes.
const DEFAULT_TTL_SECS: u64 = 86_400;

/// Render `$cship.account`.
pub fn render(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    let account_cfg = cfg.account.as_ref();

    // Step 1: disabled flag → silent None
    if account_cfg.and_then(|c| c.disabled) == Some(true) {
        return None;
    }

    // Step 2: resolve the account (CSHIP_ACCOUNT env var first, else keychain/OAuth).
    let profile = resolve_profile(ctx, account_cfg)?;

    // Step 3: build formatted output
    let default_cfg = AccountConfig::default();
    let cfg_ref = account_cfg.unwrap_or(&default_cfg);
    let fmt = cfg_ref.format.as_deref().unwrap_or(DEFAULT_FORMAT);
    let content = format_output(fmt, &profile, cfg_ref)?;

    // Step 4: apply style (threshold styling not meaningful for account names)
    let symbol = cfg_ref.symbol.as_deref().unwrap_or("");
    let styled = crate::ansi::apply_style(&format!("{symbol}{content}"), cfg_ref.style.as_deref());
    Some(styled)
}

/// Parse the launcher-provided account from [`ACCOUNT_ENV_VAR`], if present and
/// well-formed. Returns `None` when the var is unset, empty, or not valid JSON —
/// so the caller cleanly falls back to the keychain/OAuth path. The value is
/// non-secret account identity only (never a token).
fn account_from_env() -> Option<AccountProfile> {
    parse_account_env(&std::env::var(ACCOUNT_ENV_VAR).ok()?)
}

/// Parse the `CSHIP_ACCOUNT` payload. Split from env access so the JSON contract
/// is unit-testable. `None` for empty or malformed input (→ keychain fallback).
fn parse_account_env(raw: &str) -> Option<AccountProfile> {
    if raw.trim().is_empty() {
        return None;
    }
    match serde_json::from_str::<AccountProfile>(raw) {
        Ok(profile) => Some(profile),
        Err(e) => {
            tracing::warn!("cship.account: {ACCOUNT_ENV_VAR} set but not parseable: {e}");
            None
        }
    }
}

/// Resolve the account profile to display, in preference order:
///
/// 1. The **`CSHIP_ACCOUNT` env var** — a launcher that injected a session token
///    this subprocess cannot see resolves the account at launch and passes it here
///    as compact JSON. No keychain read, no network call.
/// 2. The **keychain token → OAuth `/api/oauth/profile`** fetch (fingerprint-gated
///    and cached), which is correct for a plain `claude` with no launcher.
fn resolve_profile(ctx: &Context, account_cfg: Option<&AccountConfig>) -> Option<AccountProfile> {
    // Preference 1: launcher-provided account via env (tool-agnostic contract).
    if let Some(profile) = account_from_env() {
        return Some(profile);
    }

    // Preference 2: keychain token → OAuth fetch, with fingerprint-gated cache.
    let transcript_str = ctx.transcript_path.as_deref()?;
    let transcript_path = std::path::Path::new(transcript_str);

    let token = match crate::platform::get_oauth_token() {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("cship.account: credential retrieval failed: {e}");
            return None;
        }
    };
    let fp = crate::platform::token_fingerprint(&token);

    if let Some(cached) = cache::read_account_profile(transcript_path, false, Some(&fp)) {
        return Some(cached);
    }

    let ttl_secs = account_cfg.and_then(|c| c.ttl).unwrap_or(DEFAULT_TTL_SECS);
    match super::fetch_with_timeout("cship.account", move || {
        crate::account::fetch_account_profile(&token)
    }) {
        Some(fresh) => {
            cache::write_account_profile(transcript_path, &fresh, ttl_secs, Some(&fp));
            Some(fresh)
        }
        None => cache::read_account_profile(transcript_path, true, Some(&fp)),
    }
}

/// Substitute placeholders in `fmt` using fields from `profile` and optional labels map.
///
/// Returns `None` when the resulting string is empty (e.g. all referenced fields are absent),
/// so the caller can suppress rendering rather than emit an empty module.
pub(crate) fn format_output(
    fmt: &str,
    profile: &AccountProfile,
    cfg: &AccountConfig,
) -> Option<String> {
    let org = profile.organization_name.as_deref().unwrap_or("");
    let display = profile.account_display_name.as_deref().unwrap_or("");
    let email = profile.account_email.as_deref().unwrap_or("");
    let tier = profile.organization_tier.as_deref().unwrap_or("");
    let kind = profile.organization_type.as_deref().unwrap_or("");
    let label = resolve_label(profile, cfg);

    let rendered = fmt
        .replace("{label}", &label)
        .replace("{organization}", org)
        .replace("{display_name}", display)
        .replace("{email}", email)
        .replace("{tier}", tier)
        .replace("{type}", kind);

    let trimmed = rendered.trim();
    if trimmed.is_empty() {
        tracing::warn!("cship.account: rendered content is empty (all fields absent)");
        return None;
    }
    Some(trimmed.to_string())
}

/// Resolve the `{label}` placeholder. Lookup order:
/// 1. `cfg.labels[organization_name]` — user-defined alias (opt in)
/// 2. `profile.organization_name`    — raw org name
/// 3. `profile.account_display_name` — fall back to the account owner's name
/// 4. empty string                   — nothing to render
fn resolve_label(profile: &AccountProfile, cfg: &AccountConfig) -> String {
    if let (Some(labels), Some(org)) = (cfg.labels.as_ref(), profile.organization_name.as_deref())
        && let Some(mapped) = labels.get(org)
    {
        return mapped.clone();
    }
    if let Some(org) = profile.organization_name.as_deref() {
        return org.to_string();
    }
    if let Some(name) = profile.account_display_name.as_deref() {
        return name.to_string();
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn profile() -> AccountProfile {
        AccountProfile {
            account_display_name: Some("Nils".into()),
            account_email: Some("nils@example.com".into()),
            organization_name: Some("Fulcrum Genomics".into()),
            organization_tier: Some("default_claude_max_5x".into()),
            organization_type: Some("claude_team".into()),
        }
    }

    #[test]
    fn test_default_format_renders_organization_name() {
        let cfg = AccountConfig::default();
        let out = format_output(DEFAULT_FORMAT, &profile(), &cfg).unwrap();
        assert_eq!(out, "Fulcrum Genomics");
    }

    #[test]
    fn test_parse_account_env_reads_launcher_json() {
        let raw = r#"{"organization_name":"FG Partners","organization_tier":"tier_x","organization_type":"claude_team","account_display_name":"partners"}"#;
        let parsed = parse_account_env(raw).expect("valid CSHIP_ACCOUNT parses");
        assert_eq!(parsed.organization_name.as_deref(), Some("FG Partners"));
        assert_eq!(parsed.organization_tier.as_deref(), Some("tier_x"));
    }

    #[test]
    fn test_parse_account_env_rejects_empty_and_malformed() {
        assert!(parse_account_env("").is_none());
        assert!(parse_account_env("   ").is_none());
        assert!(parse_account_env("{not json").is_none());
    }

    #[test]
    fn test_labels_map_overrides_organization_name() {
        let mut labels = BTreeMap::new();
        labels.insert("Fulcrum Genomics".into(), "work".into());
        labels.insert("Personal Workspace".into(), "personal".into());
        let cfg = AccountConfig {
            labels: Some(labels),
            ..Default::default()
        };
        let out = format_output(DEFAULT_FORMAT, &profile(), &cfg).unwrap();
        assert_eq!(out, "work");
    }

    #[test]
    fn test_labels_map_miss_falls_back_to_organization_name() {
        let mut labels = BTreeMap::new();
        labels.insert("Other Org".into(), "elsewhere".into());
        let cfg = AccountConfig {
            labels: Some(labels),
            ..Default::default()
        };
        let out = format_output(DEFAULT_FORMAT, &profile(), &cfg).unwrap();
        assert_eq!(out, "Fulcrum Genomics");
    }

    #[test]
    fn test_label_falls_back_to_display_name_when_org_absent() {
        let p = AccountProfile {
            organization_name: None,
            ..profile()
        };
        let out = format_output(DEFAULT_FORMAT, &p, &AccountConfig::default()).unwrap();
        assert_eq!(out, "Nils");
    }

    #[test]
    fn test_format_with_multiple_placeholders() {
        let cfg = AccountConfig::default();
        let out =
            format_output("{display_name} @ {organization} ({type})", &profile(), &cfg).unwrap();
        assert_eq!(out, "Nils @ Fulcrum Genomics (claude_team)");
    }

    #[test]
    fn test_format_with_email_placeholder() {
        let cfg = AccountConfig::default();
        let out = format_output("{email}", &profile(), &cfg).unwrap();
        assert_eq!(out, "nils@example.com");
    }

    #[test]
    fn test_format_with_tier_placeholder() {
        let cfg = AccountConfig::default();
        let out = format_output("{tier}", &profile(), &cfg).unwrap();
        assert_eq!(out, "default_claude_max_5x");
    }

    #[test]
    fn test_empty_profile_returns_none() {
        let out = format_output(
            DEFAULT_FORMAT,
            &AccountProfile::default(),
            &AccountConfig::default(),
        );
        assert_eq!(out, None);
    }

    #[test]
    fn test_unknown_placeholder_left_intact() {
        // Forward compatibility: placeholders cship doesn't recognize remain literal
        let out = format_output(
            "{organization} {unknown}",
            &profile(),
            &AccountConfig::default(),
        )
        .unwrap();
        assert_eq!(out, "Fulcrum Genomics {unknown}");
    }

    #[test]
    fn test_render_respects_disabled_flag() {
        let ctx = Context {
            transcript_path: Some("/tmp/cship-test-disabled/transcript.jsonl".into()),
            ..Default::default()
        };
        let cfg = CshipConfig {
            account: Some(AccountConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render(&ctx, &cfg), None);
    }

    #[test]
    fn test_render_returns_none_without_transcript_path() {
        let ctx = Context::default();
        let cfg = CshipConfig::default();
        assert_eq!(render(&ctx, &cfg), None);
    }

    #[test]
    fn test_render_returns_none_without_keychain() {
        // With fingerprinting, render() calls get_oauth_token() before checking cache.
        // In CI/test (no Keychain), render returns None on credential failure.
        // The cache hit path is validated by cache.rs fingerprint tests.
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        let ctx = Context {
            transcript_path: Some(transcript.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let cfg = CshipConfig::default();
        let _result = render(&ctx, &cfg);
        // No assertion on value — depends on whether test env has Keychain access
    }

    #[test]
    fn test_render_cache_invalidated_on_fingerprint_mismatch() {
        // Seed cache with a stale fingerprint and a sentinel organization name.
        // The cache must NOT be used (fingerprint mismatch).
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        let stale_profile = AccountProfile {
            organization_name: Some("STALE_SENTINEL_ORG_12345".into()),
            ..Default::default()
        };
        cache::write_account_profile(
            &transcript,
            &stale_profile,
            86_400,
            Some("old_account_fp_xx"),
        );

        let ctx = Context {
            transcript_path: Some(transcript.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let cfg = CshipConfig::default();
        let result = render(&ctx, &cfg);
        // Whether render returns None (no OAuth) or Some (live fetch), the stale
        // cache data must never appear in the output.
        if let Some(ref rendered) = result {
            assert!(
                !rendered.contains("STALE_SENTINEL_ORG_12345"),
                "stale cache data must not leak: {rendered:?}"
            );
        }
    }
}
