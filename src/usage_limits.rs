//! Fetch current usage limits from the Anthropic API.
//!
//! This is the ONLY file in the codebase that makes external HTTP calls (architectural boundary).
//! OAuth token is held only for the duration of the HTTP call — never written to disk (NFR-S1).
//!
//! Endpoint: `https://api.anthropic.com/api/oauth/usage`
//! Auth: `Authorization: Bearer {token}` + `anthropic-beta: oauth-2025-04-20`

/// Parsed usage limits returned by the Anthropic API.
/// Field names use the project's flat convention; serde mapping is handled via
/// an intermediate `ApiResponse` struct during deserialization.
///
/// The `*_epoch` fields are set only on the stdin path (Claude Code sends `resets_at` as a
/// Unix epoch directly). On the OAuth/cache path these fields are `None` and the ISO 8601
/// string fields are used instead. Serde serialises `None` as `null`, which is ignored by
/// old cache readers (backward-compatible).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct UsageLimitsData {
    pub five_hour_pct: f64,
    pub seven_day_pct: f64,
    pub five_hour_resets_at: String, // ISO 8601; empty string when API returns null
    pub seven_day_resets_at: String, // ISO 8601; empty string when API returns null
    /// Unix epoch seconds for the five-hour window reset; `Some` only on the stdin path.
    #[serde(default)]
    pub five_hour_resets_at_epoch: Option<u64>,
    /// Unix epoch seconds for the seven-day window reset; `Some` only on the stdin path.
    #[serde(default)]
    pub seven_day_resets_at_epoch: Option<u64>,
    // Extra usage (OAuth API only; absent on stdin path)
    #[serde(default)]
    pub extra_usage_enabled: Option<bool>,
    #[serde(default)]
    pub extra_usage_monthly_limit: Option<f64>,
    #[serde(default)]
    pub extra_usage_used_credits: Option<f64>,
    #[serde(default)]
    pub extra_usage_utilization: Option<f64>,
    // Per-model 7-day breakdowns (OAuth API only)
    #[serde(default)]
    pub seven_day_opus_pct: Option<f64>,
    #[serde(default)]
    pub seven_day_opus_resets_at: Option<String>,
    #[serde(default)]
    pub seven_day_sonnet_pct: Option<f64>,
    #[serde(default)]
    pub seven_day_sonnet_resets_at: Option<String>,
    #[serde(default)]
    pub seven_day_cowork_pct: Option<f64>,
    #[serde(default)]
    pub seven_day_cowork_resets_at: Option<String>,
    #[serde(default)]
    pub seven_day_oauth_apps_pct: Option<f64>,
    #[serde(default)]
    pub seven_day_oauth_apps_resets_at: Option<String>,
}

/// Intermediate struct matching the raw API response structure.
#[derive(serde::Deserialize)]
struct ApiResponse {
    five_hour: UsagePeriod,
    seven_day: UsagePeriod,
    seven_day_opus: Option<UsagePeriod>,
    seven_day_sonnet: Option<UsagePeriod>,
    seven_day_cowork: Option<UsagePeriod>,
    seven_day_oauth_apps: Option<UsagePeriod>,
    extra_usage: Option<ExtraUsageResponse>,
}

#[derive(serde::Deserialize)]
struct UsagePeriod {
    utilization: f64,
    resets_at: Option<String>,
}

/// Intermediate struct matching the `extra_usage` object in the API response.
#[derive(serde::Deserialize)]
struct ExtraUsageResponse {
    is_enabled: Option<bool>,
    monthly_limit: Option<f64>,
    used_credits: Option<f64>,
    utilization: Option<f64>,
}

/// Parse a raw JSON string into `UsageLimitsData`.
/// Extracted from `fetch_usage_limits` for unit-testability without HTTP.
fn parse_api_response(json: &str) -> Result<UsageLimitsData, String> {
    let api: ApiResponse =
        serde_json::from_str(json).map_err(|e| format!("unexpected response format: {e}"))?;

    let map_period = |p: &Option<UsagePeriod>| -> (Option<f64>, Option<String>) {
        match p {
            Some(period) => (Some(period.utilization), period.resets_at.clone()),
            None => (None, None),
        }
    };

    let (opus_pct, opus_reset) = map_period(&api.seven_day_opus);
    let (sonnet_pct, sonnet_reset) = map_period(&api.seven_day_sonnet);
    let (cowork_pct, cowork_reset) = map_period(&api.seven_day_cowork);
    let (oauth_apps_pct, oauth_apps_reset) = map_period(&api.seven_day_oauth_apps);

    Ok(UsageLimitsData {
        five_hour_pct: api.five_hour.utilization,
        seven_day_pct: api.seven_day.utilization,
        five_hour_resets_at: api.five_hour.resets_at.unwrap_or_default(),
        seven_day_resets_at: api.seven_day.resets_at.unwrap_or_default(),
        five_hour_resets_at_epoch: None,
        seven_day_resets_at_epoch: None,
        extra_usage_enabled: api.extra_usage.as_ref().and_then(|e| e.is_enabled),
        extra_usage_monthly_limit: api.extra_usage.as_ref().and_then(|e| e.monthly_limit),
        extra_usage_used_credits: api.extra_usage.as_ref().and_then(|e| e.used_credits),
        extra_usage_utilization: api.extra_usage.as_ref().and_then(|e| e.utilization),
        seven_day_opus_pct: opus_pct,
        seven_day_opus_resets_at: opus_reset,
        seven_day_sonnet_pct: sonnet_pct,
        seven_day_sonnet_resets_at: sonnet_reset,
        seven_day_cowork_pct: cowork_pct,
        seven_day_cowork_resets_at: cowork_reset,
        seven_day_oauth_apps_pct: oauth_apps_pct,
        seven_day_oauth_apps_resets_at: oauth_apps_reset,
    })
}

/// Fetch current usage limits from the Anthropic API.
/// Returns structured usage data or a descriptive Err.
/// This is the ONLY file in the codebase that makes external HTTP calls.
pub fn fetch_usage_limits(token: &str) -> Result<UsageLimitsData, String> {
    use std::time::Duration;

    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(5)))
            .build(),
    );
    let mut response = agent
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
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

    #[test]
    fn test_fetch_parses_extra_usage_fields() {
        let json = r#"{
            "five_hour": {"utilization": 100.0, "resets_at": "2099-01-01T00:00:00+00:00"},
            "seven_day": {"utilization": 47.0, "resets_at": "2099-01-01T00:00:00+00:00"},
            "seven_day_opus": {"utilization": 12.0, "resets_at": "2099-02-01T00:00:00+00:00"},
            "seven_day_sonnet": {"utilization": 3.0, "resets_at": "2099-03-01T00:00:00+00:00"},
            "seven_day_cowork": null,
            "seven_day_oauth_apps": null,
            "extra_usage": {
                "is_enabled": true,
                "monthly_limit": 20000,
                "used_credits": 6195.0,
                "utilization": 30.975
            },
            "iguana_necktie": null
        }"#;
        let data = parse_api_response(json).unwrap();
        assert_eq!(data.extra_usage_enabled, Some(true));
        assert_eq!(data.extra_usage_monthly_limit, Some(20000.0));
        assert!((data.extra_usage_used_credits.unwrap() - 6195.0).abs() < f64::EPSILON);
        assert!((data.extra_usage_utilization.unwrap() - 30.975).abs() < f64::EPSILON);
        assert!((data.seven_day_opus_pct.unwrap() - 12.0).abs() < f64::EPSILON);
        assert_eq!(
            data.seven_day_opus_resets_at.as_deref(),
            Some("2099-02-01T00:00:00+00:00")
        );
        assert!((data.seven_day_sonnet_pct.unwrap() - 3.0).abs() < f64::EPSILON);
        assert_eq!(
            data.seven_day_sonnet_resets_at.as_deref(),
            Some("2099-03-01T00:00:00+00:00")
        );
        assert!(data.seven_day_cowork_pct.is_none());
        assert!(data.seven_day_oauth_apps_pct.is_none());
    }

    #[test]
    fn test_fetch_parses_null_extra_usage() {
        let json = r#"{
            "five_hour": {"utilization": 50.0, "resets_at": "2099-01-01T00:00:00+00:00"},
            "seven_day": {"utilization": 20.0, "resets_at": "2099-01-01T00:00:00+00:00"}
        }"#;
        let data = parse_api_response(json).unwrap();
        assert!(data.extra_usage_enabled.is_none());
        assert!(data.extra_usage_monthly_limit.is_none());
        assert!(data.seven_day_opus_pct.is_none());
        assert!(data.seven_day_sonnet_pct.is_none());
    }
}
