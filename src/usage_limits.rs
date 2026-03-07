//! Fetch current usage limits from the Anthropic API.
//!
//! This is the ONLY file in the codebase that makes external HTTP calls (architectural boundary).
//! OAuth token is held only for the duration of the HTTP call — never written to disk (NFR-S1).
//!
//! Endpoint: `https://api.anthropic.com/api/oauth/usage`
//! Auth: `Authorization: Bearer {token}` + `anthropic-beta: oauth-2025-04-20`

/// Parsed usage limits returned by the Anthropic API.
/// Field names use the project's flat convention; serde mapping is handled via
/// an intermediate [`ApiResponse`] struct during deserialization.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UsageLimitsData {
    pub five_hour_pct: f64,
    pub seven_day_pct: f64,
    pub five_hour_resets_at: String, // ISO 8601; empty string when API returns null
    pub seven_day_resets_at: String, // ISO 8601; empty string when API returns null
}

/// Intermediate struct matching the raw API response structure.
#[derive(serde::Deserialize)]
struct ApiResponse {
    five_hour: UsagePeriod,
    seven_day: UsagePeriod,
}

#[derive(serde::Deserialize)]
struct UsagePeriod {
    utilization: f64,
    resets_at: Option<String>,
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

    let api: ApiResponse = response
        .body_mut()
        .read_json()
        .map_err(|e| format!("unexpected response format: {e}"))?;

    Ok(UsageLimitsData {
        five_hour_pct: api.five_hour.utilization * 100.0,
        seven_day_pct: api.seven_day.utilization * 100.0,
        five_hour_resets_at: api.five_hour.resets_at.unwrap_or_default(),
        seven_day_resets_at: api.seven_day.resets_at.unwrap_or_default(),
    })
}
