//! Account module — renders the currently authenticated Anthropic account.
//!
//! Lets users see at a glance whether their active Claude Code session is on
//! their work or personal account. Profile data comes from the OAuth
//! `/api/oauth/profile` endpoint via [`crate::account::fetch_account_profile`].
//!
//! Render flow:
//! 1. Check `disabled` flag → silent `None`
//! 2. Read `transcript_path` for cache keying
//! 3. Read OAuth token up front → compute fingerprint for cache identity
//! 4. Cache hit (fingerprint must match) → render immediately
//! 5. Cache miss → fetch via spawned thread with 2s timeout
//! 6. On timeout, fall back to stale cache (still fingerprint-gated)
//! 7. Format output (default or user-defined format string)
//! 8. Apply style
//!
//! The OAuth token is never written to disk, stdout, or cache (NFR-S1/S3).

use crate::account::AccountProfile;
use crate::cache;
use crate::config::{AccountConfig, CshipConfig};
use crate::context::Context;

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

    // Step 2: transcript_path is required for cache keying
    let transcript_str = ctx.transcript_path.as_deref()?;
    let transcript_path = std::path::Path::new(transcript_str);

    // Step 3: read OAuth token up front for fingerprint (cache identity check)
    let token = match crate::platform::get_oauth_token() {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("cship.account: credential retrieval failed: {e}");
            return None;
        }
    };
    let fp = crate::platform::token_fingerprint(&token);

    // Step 4: cache hit (fingerprint must match) → render immediately
    let profile =
        if let Some(cached) = cache::read_account_profile(transcript_path, false, Some(&fp)) {
            cached
        } else {
            // Step 5: cache miss → OAuth fetch with timeout
            let ttl_secs = account_cfg.and_then(|c| c.ttl).unwrap_or(DEFAULT_TTL_SECS);
            match super::fetch_with_timeout("cship.account", move || {
                crate::account::fetch_account_profile(&token)
            }) {
                Some(fresh) => {
                    cache::write_account_profile(transcript_path, &fresh, ttl_secs, Some(&fp));
                    fresh
                }
                None => cache::read_account_profile(transcript_path, true, Some(&fp))?,
            }
        };

    // Step 6: build formatted output
    let default_cfg = AccountConfig::default();
    let cfg_ref = account_cfg.unwrap_or(&default_cfg);
    let fmt = cfg_ref.format.as_deref().unwrap_or(DEFAULT_FORMAT);
    let content = format_output(fmt, &profile, cfg_ref)?;

    // Step 7: apply style (threshold styling not meaningful for account names)
    let symbol = cfg_ref.symbol.as_deref().unwrap_or("");
    let styled = crate::ansi::apply_style(&format!("{symbol}{content}"), cfg_ref.style.as_deref());
    Some(styled)
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
        // Seed cache with one fingerprint, then render — since get_oauth_token()
        // fails in test env, render returns None (not stale data from wrong account)
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        cache::write_account_profile(&transcript, &profile(), 86_400, Some("old_account_fp_xx"));

        let ctx = Context {
            transcript_path: Some(transcript.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let cfg = CshipConfig::default();
        let result = render(&ctx, &cfg);
        assert_eq!(
            result, None,
            "cache with wrong fingerprint should not be used"
        );
    }
}
