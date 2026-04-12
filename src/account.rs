//! Fetch the current Anthropic account profile from the OAuth API.
//!
//! Endpoint: `https://api.anthropic.com/api/oauth/profile`
//! Auth: `Authorization: Bearer {token}` + `anthropic-beta: oauth-2025-04-20`
//!
//! Used by the `cship.account` module to display which Anthropic account
//! (work/personal/etc.) the user is currently authenticated with.
//!
//! The OAuth token is held only for the duration of the HTTP call — never
//! written to disk, cache, stdout, or stderr (NFR-S1). The parsed profile
//! data contains no secrets and is safe to cache.

/// Parsed subset of the `/api/oauth/profile` response.
///
/// Only fields actually used by the `cship.account` module are retained.
/// The API response is larger than this struct — unknown fields are ignored
/// by serde (no `deny_unknown_fields`) so the struct is forward-compatible.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AccountProfile {
    /// Account `display_name` (e.g. `"Nils"`).
    pub account_display_name: Option<String>,
    /// Account `email` (e.g. `"nils@example.com"`). Treat as PII — opt-in to render.
    pub account_email: Option<String>,
    /// Organization `name` (e.g. `"Fulcrum Genomics"` or `"Personal Workspace"`).
    pub organization_name: Option<String>,
    /// Organization `rate_limit_tier` (e.g. `"default_claude_max_5x"`).
    pub organization_tier: Option<String>,
    /// Organization `organization_type` (e.g. `"claude_team"`, `"personal"`).
    pub organization_type: Option<String>,
}

/// Intermediate structs matching the raw API response shape.
#[derive(serde::Deserialize)]
struct ApiResponse {
    account: Option<AccountObject>,
    organization: Option<OrganizationObject>,
}

#[derive(serde::Deserialize)]
struct AccountObject {
    display_name: Option<String>,
    email: Option<String>,
}

#[derive(serde::Deserialize)]
struct OrganizationObject {
    name: Option<String>,
    rate_limit_tier: Option<String>,
    organization_type: Option<String>,
}

/// Parse raw `/api/oauth/profile` JSON into an [`AccountProfile`].
/// Extracted from `fetch_account_profile` so it can be unit-tested without HTTP.
pub fn parse_api_response(json: &str) -> Result<AccountProfile, String> {
    let api: ApiResponse =
        serde_json::from_str(json).map_err(|e| format!("unexpected response format: {e}"))?;
    Ok(AccountProfile {
        account_display_name: api.account.as_ref().and_then(|a| a.display_name.clone()),
        account_email: api.account.as_ref().and_then(|a| a.email.clone()),
        organization_name: api.organization.as_ref().and_then(|o| o.name.clone()),
        organization_tier: api
            .organization
            .as_ref()
            .and_then(|o| o.rate_limit_tier.clone()),
        organization_type: api
            .organization
            .as_ref()
            .and_then(|o| o.organization_type.clone()),
    })
}

const API_ENDPOINT: &str = "https://api.anthropic.com/api/oauth/profile";
const OAUTH_BETA_HEADER: &str = "oauth-2025-04-20";
const HTTP_TIMEOUT_SECS: u64 = 5;

/// Fetch the current account profile from the Anthropic OAuth API.
/// Returns a structured `AccountProfile` or a descriptive `Err`.
pub fn fetch_account_profile(token: &str) -> Result<AccountProfile, String> {
    use std::time::Duration;

    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(HTTP_TIMEOUT_SECS)))
            .build(),
    );
    let mut response = agent
        .get(API_ENDPOINT)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", OAUTH_BETA_HEADER)
        .call()
        .map_err(|e| format!("network error: {e}"))?;

    if response.status() != 200 {
        return Err(format!("API returned {}", response.status()));
    }

    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("failed to read response body: {e}"))?;
    parse_api_response(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RESPONSE: &str = r#"{
        "account": {
            "uuid": "774e599f-cedf-4d64-81b2-fe2f971f2636",
            "full_name": "Nils",
            "display_name": "Nils",
            "email": "nils@example.com",
            "has_claude_max": false,
            "has_claude_pro": false,
            "created_at": "2025-10-21T16:39:02.756615Z"
        },
        "organization": {
            "uuid": "e9993f1a-2e19-42ce-868e-a8daf85c716e",
            "name": "Example Team",
            "organization_type": "claude_team",
            "billing_type": "stripe_subscription",
            "rate_limit_tier": "default_claude_max_5x",
            "has_extra_usage_enabled": true,
            "subscription_status": "active"
        },
        "application": {
            "uuid": "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
            "name": "Claude Code",
            "slug": "claude-code"
        }
    }"#;

    #[test]
    fn test_parse_extracts_account_and_organization_fields() {
        let profile = parse_api_response(SAMPLE_RESPONSE).unwrap();
        assert_eq!(profile.account_display_name.as_deref(), Some("Nils"));
        assert_eq!(profile.account_email.as_deref(), Some("nils@example.com"));
        assert_eq!(profile.organization_name.as_deref(), Some("Example Team"));
        assert_eq!(
            profile.organization_tier.as_deref(),
            Some("default_claude_max_5x")
        );
        assert_eq!(profile.organization_type.as_deref(), Some("claude_team"));
    }

    #[test]
    fn test_parse_handles_missing_optional_fields() {
        let json = r#"{
            "account": {"uuid": "abc"},
            "organization": {"uuid": "def"}
        }"#;
        let profile = parse_api_response(json).unwrap();
        assert_eq!(profile.account_display_name, None);
        assert_eq!(profile.account_email, None);
        assert_eq!(profile.organization_name, None);
        assert_eq!(profile.organization_tier, None);
    }

    #[test]
    fn test_parse_handles_completely_absent_objects() {
        // Both top-level objects missing — should yield an all-None profile, not an error
        let profile = parse_api_response("{}").unwrap();
        assert_eq!(profile, AccountProfile::default());
    }

    #[test]
    fn test_parse_rejects_malformed_json() {
        let result = parse_api_response("not json");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unexpected response format"));
    }

    #[test]
    fn test_parse_ignores_unknown_fields() {
        // Forward compatibility: new API fields must not break parsing
        let json = r#"{
            "account": {"display_name": "N", "surprise_field_2099": "x"},
            "organization": {"name": "O"},
            "future_top_level_field": 42
        }"#;
        let profile = parse_api_response(json).unwrap();
        assert_eq!(profile.account_display_name.as_deref(), Some("N"));
        assert_eq!(profile.organization_name.as_deref(), Some("O"));
    }
}
