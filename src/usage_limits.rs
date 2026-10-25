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
    /// Whether the source actually carried a `five_hour` window (vs. it being
    /// `null`/absent, e.g. on Enterprise plans). A supported-but-idle window
    /// (`{utilization: 0, resets_at: null}`) is still `true` — only a missing
    /// key/`null` object is `false`. Distinguishes "no data" from "zero data"
    /// so `lacks_standard_signal` doesn't mistake one for the other.
    #[serde(default)]
    pub five_hour_available: bool,
    /// Same as `five_hour_available`, for the `seven_day` window.
    #[serde(default)]
    pub seven_day_available: bool,
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
    five_hour: Option<UsagePeriod>,
    seven_day: Option<UsagePeriod>,
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

    let five_hour_available = api.five_hour.is_some();
    let seven_day_available = api.seven_day.is_some();

    let (five_h_pct, five_h_reset) = api
        .five_hour
        .map(|p| (p.utilization, p.resets_at.unwrap_or_default()))
        .unwrap_or((0.0, String::new()));
    let (seven_d_pct, seven_d_reset) = api
        .seven_day
        .map(|p| (p.utilization, p.resets_at.unwrap_or_default()))
        .unwrap_or((0.0, String::new()));

    Ok(UsageLimitsData {
        five_hour_pct: five_h_pct,
        seven_day_pct: seven_d_pct,
        five_hour_resets_at: five_h_reset,
        seven_day_resets_at: seven_d_reset,
        five_hour_resets_at_epoch: None,
        seven_day_resets_at_epoch: None,
        five_hour_available,
        seven_day_available,
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

/// Outcome of a single OAuth usage-limits fetch attempt. The caller
/// (`usage_limits_state`) owns all retry/backoff scheduling — this module
/// only classifies what happened on the wire.
#[derive(Debug)]
pub enum UsageFetchOutcome {
    Success(Box<UsageLimitsData>),
    Unauthorized,
    RateLimited { retry_after_seconds: Option<u64> },
    ServerError { status: u16 },
    NetworkError,
    InvalidResponse,
}

/// Parse an HTTP `Retry-After` header value as integer seconds. A malformed
/// value is treated as absent (caller falls back to exponential backoff).
fn parse_retry_after(header: Option<&str>) -> Option<u64> {
    header.and_then(|h| h.parse::<u64>().ok())
}

/// Maps a completed response's status code (and `Retry-After` header, if any)
/// to a terminal outcome, or `None` if the caller should proceed to read the
/// success body (status 200).
fn classify_status(status: u16, retry_after_header: Option<&str>) -> Option<UsageFetchOutcome> {
    match status {
        200 => None,
        401 => Some(UsageFetchOutcome::Unauthorized),
        429 => Some(UsageFetchOutcome::RateLimited {
            retry_after_seconds: parse_retry_after(retry_after_header),
        }),
        s => Some(UsageFetchOutcome::ServerError { status: s }),
    }
}

/// Fetch current usage limits from the Anthropic API.
/// This is the ONLY file in the codebase that makes external HTTP calls.
/// Does not retry — the caller (`usage_limits_state`) owns retry/backoff scheduling.
pub fn fetch_usage_limits(token: &str) -> UsageFetchOutcome {
    use std::time::Duration;

    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_millis(1500)))
            // ureq 3 defaults to treating any 4xx/5xx status as `Err`, which
            // would make our status-code branches below unreachable — we need
            // to inspect the response ourselves via `classify_status`.
            .http_status_as_error(false)
            .build(),
    );
    let user_agent = concat!("cship/", env!("CARGO_PKG_VERSION"));
    let result = agent
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", &format!("Bearer {token}"))
        .header("User-Agent", user_agent)
        .header("anthropic-beta", "oauth-2025-04-20")
        .call();

    let mut response = match result {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("cship.usage_limits: usage request failed: {e}");
            return UsageFetchOutcome::NetworkError;
        }
    };

    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get("Retry-After")
        .and_then(|v| v.to_str().ok());
    if let Some(outcome) = classify_status(status, retry_after) {
        tracing::warn!("cship.usage_limits: usage API returned HTTP {status}");
        return outcome;
    }

    let body = match response.body_mut().read_to_string() {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("cship.usage_limits: could not read usage response body: {e}");
            return UsageFetchOutcome::InvalidResponse;
        }
    };
    match parse_api_response(&body) {
        Ok(data) => UsageFetchOutcome::Success(Box::new(data)),
        Err(e) => {
            tracing::warn!("cship.usage_limits: could not parse usage response: {e}");
            UsageFetchOutcome::InvalidResponse
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_retry_after_seconds_present() {
        assert_eq!(parse_retry_after(Some("120")), Some(120));
    }

    #[test]
    fn test_parse_retry_after_malformed_is_none() {
        assert_eq!(parse_retry_after(Some("not-a-number")), None);
    }

    #[test]
    fn test_parse_retry_after_absent_is_none() {
        assert_eq!(parse_retry_after(None), None);
    }

    #[test]
    fn test_classify_status_200_returns_none() {
        assert!(classify_status(200, None).is_none());
    }

    #[test]
    fn test_classify_status_401_returns_unauthorized() {
        assert!(matches!(
            classify_status(401, None),
            Some(UsageFetchOutcome::Unauthorized)
        ));
    }

    #[test]
    fn test_classify_status_429_with_retry_after_returns_rate_limited_with_seconds() {
        match classify_status(429, Some("120")) {
            Some(UsageFetchOutcome::RateLimited {
                retry_after_seconds,
            }) => assert_eq!(retry_after_seconds, Some(120)),
            other => panic!("expected RateLimited with retry_after, got {other:?}"),
        }
    }

    #[test]
    fn test_classify_status_429_without_retry_after_returns_rate_limited_with_none() {
        match classify_status(429, None) {
            Some(UsageFetchOutcome::RateLimited {
                retry_after_seconds,
            }) => assert_eq!(retry_after_seconds, None),
            other => panic!("expected RateLimited with no retry_after, got {other:?}"),
        }
    }

    #[test]
    fn test_classify_status_500_returns_server_error() {
        match classify_status(500, None) {
            Some(UsageFetchOutcome::ServerError { status }) => assert_eq!(status, 500),
            other => panic!("expected ServerError{{500}}, got {other:?}"),
        }
    }

    #[test]
    fn test_classify_status_other_non_success_returns_server_error() {
        // 403/418 aren't 401 or 429 but still aren't success — must not be
        // silently treated as a NetworkError or dropped.
        match classify_status(403, None) {
            Some(UsageFetchOutcome::ServerError { status }) => assert_eq!(status, 403),
            other => panic!("expected ServerError{{403}}, got {other:?}"),
        }
        match classify_status(418, None) {
            Some(UsageFetchOutcome::ServerError { status }) => assert_eq!(status, 418),
            other => panic!("expected ServerError{{418}}, got {other:?}"),
        }
    }

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

    #[test]
    fn test_parse_enterprise_response_with_null_standard_fields() {
        // Reproduces the Enterprise API shape: standard fields all null,
        // extra_usage populated. Sourced from issue #173.
        let json = r#"{
            "five_hour": null,
            "seven_day": null,
            "seven_day_oauth_apps": null,
            "seven_day_opus": null,
            "seven_day_sonnet": null,
            "seven_day_cowork": null,
            "seven_day_omelette": null,
            "tangelo": null,
            "iguana_necktie": null,
            "omelette_promotional": {"utilization": 0.0, "resets_at": null},
            "extra_usage": {
                "is_enabled": true,
                "monthly_limit": 20000,
                "used_credits": 19411.0,
                "utilization": 97.055,
                "currency": "USD"
            }
        }"#;
        let data = parse_api_response(json).expect("Enterprise response must parse");
        assert_eq!(data.five_hour_pct, 0.0);
        assert_eq!(data.seven_day_pct, 0.0);
        assert!(data.five_hour_resets_at.is_empty());
        assert!(data.seven_day_resets_at.is_empty());
        assert!(!data.five_hour_available);
        assert!(!data.seven_day_available);
        assert_eq!(data.extra_usage_enabled, Some(true));
        assert_eq!(data.extra_usage_monthly_limit, Some(20000.0));
        assert!((data.extra_usage_used_credits.unwrap() - 19411.0).abs() < f64::EPSILON);
        assert!((data.extra_usage_utilization.unwrap() - 97.055).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_five_hour_idle_after_reset_is_distinguished_from_enterprise_shape() {
        // Real capture taken seconds after a 5h window reset (2026-07-15, session
        // 6318e733). `five_hour` is `Some({utilization: 0.0, resets_at: null})` here,
        // not `None` — same (0.0, "") tuple as the Enterprise `five_hour: null` case
        // above, but `five_hour_available` now records that the window was actually
        // present, so `lacks_standard_signal` won't mistake this idle-but-real window
        // for a plan where the 5h window doesn't exist at all.
        let json = r#"{
            "five_hour": {"utilization": 0.0, "resets_at": null},
            "seven_day": {"utilization": 62.0, "resets_at": "2026-07-17T15:59:59.793231+00:00"},
            "extra_usage": {"is_enabled": false}
        }"#;
        let data = parse_api_response(json).expect("idle five_hour must still parse");
        assert_eq!(data.five_hour_pct, 0.0);
        assert!(data.five_hour_resets_at.is_empty());
        assert!(data.five_hour_available);
        assert!(data.seven_day_available);
        // seven_day is untouched by the reset and still carries real signal.
        assert!((data.seven_day_pct - 62.0).abs() < f64::EPSILON);
        assert_eq!(data.seven_day_resets_at, "2026-07-17T15:59:59.793231+00:00");
    }

    #[test]
    fn test_parse_five_hour_active_window_is_distinguishable() {
        // Contrast case from the same session, ~15 minutes earlier: five_hour
        // mid-window with real utilization and a real reset timestamp.
        let json = r#"{
            "five_hour": {"utilization": 26.0, "resets_at": "2026-07-15T12:09:59.528964+00:00"},
            "seven_day": {"utilization": 62.0, "resets_at": "2026-07-17T15:59:59.528995+00:00"}
        }"#;
        let data = parse_api_response(json).expect("active window must parse");
        assert!((data.five_hour_pct - 26.0).abs() < f64::EPSILON);
        assert_eq!(data.five_hour_resets_at, "2026-07-15T12:09:59.528964+00:00");
    }

    #[test]
    fn test_parse_response_with_missing_standard_fields_keys() {
        // Future-proofing: even if the API drops these keys entirely, parse must succeed.
        let json = r#"{
            "extra_usage": {
                "is_enabled": true,
                "monthly_limit": 5000,
                "used_credits": 1234.5,
                "utilization": 24.69
            }
        }"#;
        let data = parse_api_response(json).expect("missing standard fields must not be fatal");
        assert_eq!(data.five_hour_pct, 0.0);
        assert_eq!(data.seven_day_pct, 0.0);
        assert_eq!(data.extra_usage_enabled, Some(true));
    }
}
