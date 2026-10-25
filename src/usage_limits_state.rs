//! Cross-process coordinator for OAuth usage data, shared per Claude config directory.
//!
//! Every `cship` invocation is a separate short-lived process; multiple
//! Claude Code sessions can invoke it at nearly the same time. This module
//! makes those processes share one on-disk state file and one fetch lock so
//! `/api/oauth/usage` is polled at most once per interval (60s by default,
//! user-configurable via `ttl`), globally, regardless of how many sessions or
//! processes are running.
//!
//! This is the ONLY file that reads/writes the shared cross-process cache
//! directory (`platform::cship_shared_cache_dir()`). It does not make HTTP
//! calls itself — it drives `crate::usage_limits::fetch_usage_limits` and
//! persists its typed outcome.

use crate::usage_limits::UsageLimitsData;
use std::path::{Path, PathBuf};
use std::time::Duration;

// The filename's `-vN` suffix must be bumped in lockstep with
// `CURRENT_SCHEMA_VERSION` whenever the schema changes. Different-schema
// binaries then use separate files during a version rollout instead of
// endlessly overwriting each other's state (which would defeat the fetch
// throttle for the whole upgrade window). The compiler can't enforce this —
// keep the two in sync by hand.
const STATE_FILE_NAME: &str = "usage-limits-state-v3.json";
const CURRENT_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SharedUsageState {
    pub schema_version: u32,
    pub token_fingerprint: Option<String>,
    pub usage: Option<UsageLimitsData>,
    pub next_allowed_at_epoch: u64,
    pub rate_limit_until_epoch: u64,
    pub consecutive_errors: u32,
    pub failed_token_fingerprint: Option<String>,
    /// Fingerprint of an as-yet-uncommitted (`token_fingerprint` still holds
    /// the previous token) token change that has already been given its one
    /// out-of-schedule "bypass and probe immediately" attempt. Additive
    /// field — `#[serde(default)]` so state files written before this field
    /// existed still deserialize (as `None`, i.e. "not yet probed").
    #[serde(default)]
    pub token_change_probe_fingerprint: Option<String>,
}

impl SharedUsageState {
    fn default_state() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            token_fingerprint: None,
            usage: None,
            next_allowed_at_epoch: 0,
            rate_limit_until_epoch: 0,
            consecutive_errors: 0,
            failed_token_fingerprint: None,
            token_change_probe_fingerprint: None,
        }
    }
}

fn state_file_path_in(dir: &Path) -> PathBuf {
    dir.join(STATE_FILE_NAME)
}

fn load_state_from_dir(dir: &Path) -> SharedUsageState {
    let path = state_file_path_in(dir);
    let raw = match std::fs::read_to_string(&path) {
        Ok(r) => r,
        Err(_) => return SharedUsageState::default_state(),
    };
    match serde_json::from_str::<SharedUsageState>(&raw) {
        Ok(state) if state.schema_version == CURRENT_SCHEMA_VERSION => state,
        Ok(_) => {
            tracing::debug!("cship.usage_limits_state: unknown schema version, using default");
            SharedUsageState::default_state()
        }
        Err(e) => {
            tracing::debug!("cship.usage_limits_state: invalid state JSON ({e}), using default");
            SharedUsageState::default_state()
        }
    }
}

/// Atomically write `state` to `<dir>/{STATE_FILE_NAME}`: serialize,
/// write to a sibling temp file in the same directory, flush + fsync, then
/// rename over the target. `fs::rename` is atomic on both POSIX and NTFS
/// when source and destination are on the same volume (guaranteed here since
/// both live in `dir`), so a concurrent reader always sees either the
/// complete previous file or the complete new one, never a partial write.
fn persist_state_to_dir(dir: &Path, state: &SharedUsageState) {
    if std::fs::create_dir_all(dir).is_err() {
        tracing::debug!("cship.usage_limits_state: could not create cache dir");
        return;
    }
    let json = match serde_json::to_string(state) {
        Ok(j) => j,
        Err(e) => {
            tracing::debug!("cship.usage_limits_state: failed to serialize state: {e}");
            return;
        }
    };
    let tmp_path = dir.join(format!("{STATE_FILE_NAME}.tmp.{}", std::process::id()));
    let write_result = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut f = private_open_options()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)?;
        f.write_all(json.as_bytes())?;
        f.sync_all()?;
        Ok(())
    })();
    if let Err(e) = write_result {
        tracing::warn!("cship.usage_limits_state: failed to write temp state file: {e}");
        let _ = std::fs::remove_file(&tmp_path);
        return;
    }
    if let Err(e) = std::fs::rename(&tmp_path, state_file_path_in(dir)) {
        tracing::warn!("cship.usage_limits_state: failed to replace state file: {e}");
        let _ = std::fs::remove_file(&tmp_path);
    }
}

/// `OpenOptions` that create files owner-only (0600) on Unix, so the token
/// fingerprint stored in the state and lock files is never briefly readable by
/// other users. Windows relies on the current user's directory ACL; there is no
/// file-mode equivalent to set.
fn private_open_options() -> std::fs::OpenOptions {
    #[allow(unused_mut)]
    let mut opts = std::fs::OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts
}

pub(crate) fn load_state_or_default() -> SharedUsageState {
    match crate::platform::cship_shared_cache_dir() {
        Some(dir) => load_state_from_dir(&dir),
        None => SharedUsageState::default_state(),
    }
}

// =============================================================================
// Cross-process fetch lock — non-blocking OS-level advisory file lock
// =============================================================================

const LOCK_FILE_NAME: &str = "usage-limits-fetch.lock";

/// Holds an OS-level advisory lock (`flock` on Unix, `LockFileEx` on
/// Windows) on the shared lock file for the duration of a fetch. Mutual
/// exclusion is kernel-enforced: only one process can hold the lock at a
/// time, and if the holder crashes or is killed the OS releases the lock
/// the moment its file descriptor closes — no staleness heuristic needed.
/// Dropping the `File` releases the lock; the lock file itself is left in
/// place (it's a permanent rendezvous point, not a token of ownership).
pub(crate) struct FetchLock {
    _file: std::fs::File,
}

impl FetchLock {
    /// Records "a fetch for `fingerprint` was just attempted" in the lock
    /// file itself (contents + mtime), independent of whether `persist_state`
    /// succeeds afterward. See `recently_attempted` for why this matters.
    fn touch(&self, fingerprint: &str) {
        use std::io::{Seek, Write};
        let mut f = &self._file;
        let _ = self._file.set_len(0);
        let _ = f.rewind();
        let _ = f.write_all(fingerprint.as_bytes());
        let _ = self._file.set_modified(std::time::SystemTime::now());
    }

    /// Backstop fetch-eligibility check, independent of `SharedUsageState`:
    /// true if this lock file records an attempt for `fingerprint` within
    /// `interval` seconds of `now`.
    ///
    /// `persist_state` can silently fail to durably record a completed fetch
    /// (read-only/unavailable cache dir, a rename blocked by an AV scanner or
    /// a sync client holding the target file open, ...). When that happens the
    /// state file under-reports how recently a fetch actually ran, so
    /// `fetch_may_be_needed` alone would judge every render eligible again —
    /// hammering the API on every statusline render instead of respecting
    /// `interval`. `touch` updates the lock file whenever a fetch starts,
    /// regardless of persist success; a small in-place write is far more
    /// likely to survive whatever is blocking the state file's atomic rename.
    ///
    /// The fingerprint is part of the record so that (a) a freshly created,
    /// never-touched lock file (empty, mtime = creation time) is not mistaken
    /// for an attempt, and (b) switching accounts is not throttled by the
    /// previous account's attempt. Reads through the already-locked handle:
    /// on Windows a second handle cannot read a byte-range-locked file.
    fn recently_attempted(&self, fingerprint: &str, interval: u64, now: u64) -> bool {
        use std::io::{Read, Seek};
        let mut f = &self._file;
        let mut recorded = String::new();
        if f.rewind().is_err() || f.read_to_string(&mut recorded).is_err() {
            return false;
        }
        if recorded != fingerprint {
            return false;
        }
        let Ok(modified) = self._file.metadata().and_then(|m| m.modified()) else {
            return false;
        };
        let Ok(mtime_epoch) = modified
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
        else {
            return false; // mtime before 1970 — shouldn't happen, don't block on it
        };
        now.saturating_sub(mtime_epoch) < interval
    }
}

fn lock_path_in(dir: &Path) -> PathBuf {
    dir.join(LOCK_FILE_NAME)
}

fn try_acquire_fetch_lock_in(dir: &Path) -> Option<FetchLock> {
    if std::fs::create_dir_all(dir).is_err() {
        return None;
    }
    let lock_path = lock_path_in(dir);
    let file = match private_open_options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(e) => {
            tracing::debug!("cship.usage_limits_state: could not open lock file: {e}");
            return None;
        }
    };
    // Non-blocking: if another process holds the lock, give up immediately —
    // callers are a statusline renderer that must never stall.
    match file.try_lock() {
        Ok(()) => Some(FetchLock { _file: file }),
        Err(_) => None,
    }
}

// =============================================================================
// Fetch eligibility, backoff, and outcome-driven state transitions
// =============================================================================

const DEFAULT_API_INTERVAL_SECS: u64 = 60;
const MAX_RATE_LIMIT_BACKOFF_SECS: u64 = 900;
const TRANSIENT_ERROR_RETRY_SECS: u64 = 30;
/// Poll this long after a known window reset, so the API has rolled the window over.
const RESET_CONFIRMATION_BUFFER_SECS: u64 = 5;
/// Two polls are never scheduled closer together than this (or `ttl`, if shorter).
const MIN_POLL_SPACING_SECS: u64 = 30;

fn fetch_may_be_needed(state: &SharedUsageState, fingerprint: &str, now: u64) -> bool {
    if state.failed_token_fingerprint.as_deref() == Some(fingerprint) {
        return false;
    }
    if now < state.rate_limit_until_epoch {
        return false;
    }
    now >= state.next_allowed_at_epoch
}

/// A token change is "fresh" — eligible for the one-time bypass-and-probe
/// treatment (design points 1/5) — only the first time we see it. Once a
/// probe has been attempted for `fingerprint` (success or failure), further
/// renders for that same still-uncommitted fingerprint fall back to normal
/// `fetch_may_be_needed` scheduling, exactly like a non-token-change render,
/// so a persistently-failing new token doesn't get a live fetch on every
/// render. Success commits `token_fingerprint`, so `fingerprint` no longer
/// reads as "changed" at all — this only governs the failure path.
fn is_fresh_token_change(state: &SharedUsageState, fingerprint: &str) -> bool {
    state.token_fingerprint.as_deref() != Some(fingerprint)
        && state.token_change_probe_fingerprint.as_deref() != Some(fingerprint)
}

/// Defaults to 60s when unset. An explicit `ttl` is honored as-is, including
/// values below 60 — the user has opted into more frequent polling.
///
/// `ttl = 0` would disable the fetch throttle entirely (every render issuing
/// an OAuth request), so it is clamped to 1s rather than rejected: a bad
/// usage_limits value must not take down the whole statusline.
fn effective_interval(configured_ttl: Option<u64>) -> u64 {
    match configured_ttl {
        Some(0) => {
            tracing::warn!("cship.usage_limits.ttl = 0 would disable fetch throttling; using 1s");
            1
        }
        Some(ttl) => ttl,
        None => DEFAULT_API_INTERVAL_SECS,
    }
}

/// Earliest future reset epoch present in a usage snapshot, if any — used to
/// schedule the next poll for just after a known reset, so the new window
/// shows up promptly instead of a full interval later.
fn earliest_future_reset(data: &UsageLimitsData, now: u64) -> Option<u64> {
    [
        crate::cache::iso8601_to_epoch(&data.five_hour_resets_at),
        crate::cache::iso8601_to_epoch(&data.seven_day_resets_at),
    ]
    .into_iter()
    .flatten()
    .filter(|&e| e > now)
    .min()
}

/// Exponential backoff shared by every "keep failing, wait longer" outcome:
/// `base * 2^(consecutive_errors - 1)`, capped at `cap`. One formula, fed
/// different `base`/`cap` pairs per outcome, instead of two near-identical
/// hand-rolled versions (one for 429s, one for 5xx).
fn backoff_secs(base: u64, consecutive_errors: u32, cap: u64) -> u64 {
    let exponent = consecutive_errors.saturating_sub(1).min(10);
    base.saturating_mul(1_u64 << exponent).min(cap)
}

fn apply_fetch_outcome(
    state: &mut SharedUsageState,
    outcome: crate::usage_limits::UsageFetchOutcome,
    now: u64,
    api_interval: u64,
    attempted_fingerprint: &str,
) {
    use crate::usage_limits::UsageFetchOutcome;
    match outcome {
        UsageFetchOutcome::Success(data) => {
            let reset = earliest_future_reset(&data, now);
            state.usage = Some(*data);
            let ordinary_next = now + api_interval;
            let spacing = MIN_POLL_SPACING_SECS.min(api_interval);
            state.next_allowed_at_epoch = match reset {
                // Poll just after the reset instead of the regular poll when the
                // reset comes before it, or lands within `spacing` after it (a
                // regular poll then would only see the old window). Never
                // sooner than `spacing` from now, and a regular poll is never
                // delayed by more than `spacing`.
                Some(r) if r + RESET_CONFIRMATION_BUFFER_SECS < ordinary_next + spacing => {
                    (r + RESET_CONFIRMATION_BUFFER_SECS).max(now + spacing)
                }
                _ => ordinary_next,
            };
            state.rate_limit_until_epoch = 0;
            state.consecutive_errors = 0;
            state.failed_token_fingerprint = None;
        }
        UsageFetchOutcome::RateLimited {
            retry_after_seconds,
        } => {
            state.consecutive_errors = state.consecutive_errors.saturating_add(1);
            let delay = retry_after_seconds
                .map(|retry_after| {
                    retry_after
                        .max(api_interval)
                        .min(MAX_RATE_LIMIT_BACKOFF_SECS)
                })
                .unwrap_or_else(|| {
                    backoff_secs(
                        api_interval,
                        state.consecutive_errors,
                        MAX_RATE_LIMIT_BACKOFF_SECS,
                    )
                });
            // Never poll more often than the configured ttl, even after an error.
            state.rate_limit_until_epoch = now + delay.max(api_interval);
            state.next_allowed_at_epoch = state.rate_limit_until_epoch;
        }
        UsageFetchOutcome::Unauthorized => {
            // Use the fingerprint that was actually attempted in this call,
            // not `state.token_fingerprint` — during a token-change probe
            // that still holds the *previous* (uncommitted) fingerprint, so
            // reading it here would blame the wrong token.
            state.failed_token_fingerprint = Some(attempted_fingerprint.to_string());
            // No retry timer here: `fetch_may_be_needed` unconditionally
            // blocks any fetch while `failed_token_fingerprint` matches the
            // current token, so only a token change (different fingerprint)
            // can unblock — a `next_allowed_at_epoch` bump would be inert.
        }
        UsageFetchOutcome::NetworkError | UsageFetchOutcome::InvalidResponse => {
            state.consecutive_errors = state.consecutive_errors.saturating_add(1);
            state.next_allowed_at_epoch = now + TRANSIENT_ERROR_RETRY_SECS.max(api_interval);
        }
        UsageFetchOutcome::ServerError { .. } => {
            state.consecutive_errors = state.consecutive_errors.saturating_add(1);
            let delay = backoff_secs(TRANSIENT_ERROR_RETRY_SECS, state.consecutive_errors, 300);
            state.next_allowed_at_epoch = now + delay.max(api_interval);
        }
    }
}

pub(crate) struct SharedUsageResolution {
    pub usage: Option<UsageLimitsData>,
}

/// Usage is only displayable when it was recorded under the fingerprint
/// currently in use (token-change points 1/2) — checked at read time, not
/// wiped destructively, so a stale value on disk simply never surfaces
/// under a different token rather than needing to be actively cleared.
pub(crate) fn displayable_usage(
    state: &SharedUsageState,
    fingerprint: &str,
) -> Option<UsageLimitsData> {
    if state.token_fingerprint.as_deref() == Some(fingerprint) {
        state.usage.clone()
    } else {
        None
    }
}

/// Render invocations must never stall waiting on the network — a slow or
/// unreachable API would otherwise block every eligible statusline render.
const RENDER_FETCH_TIMEOUT: Duration = Duration::from_secs(2);

/// Runs `fetch_fn` on a worker thread and waits up to `RENDER_FETCH_TIMEOUT`
/// for it to reply, bounding how long a render can stall regardless of the
/// HTTP client's own (longer) internal timeout. If the worker hasn't replied
/// in time, its result is abandoned — the thread keeps running in the
/// background, but this call reports `NetworkError` so the caller applies
/// the same backoff as any other transient failure (`apply_fetch_outcome`'s
/// `TRANSIENT_ERROR_RETRY_SECS`), instead of re-attempting the same slow
/// endpoint on the very next render. Generic `F` bound allows tests to
/// inject a fast lambda bypassing real HTTP.
/// `lock` is moved into the spawned thread rather than released by the
/// caller: if the real request outlives the render timeout, the OS-level
/// fetch lock must stay held for as long as the request is actually in
/// flight, not just for as long as the render is willing to wait on it —
/// otherwise another process can acquire the lock and start an overlapping
/// request. The lock drops (and the rendezvous file is freed for the next
/// fetch) whenever the thread actually finishes, bounded by the fetch
/// function's own client-side timeout.
fn fetch_with_render_timeout<F>(
    fetch_fn: F,
    lock: FetchLock,
) -> crate::usage_limits::UsageFetchOutcome
where
    F: FnOnce() -> crate::usage_limits::UsageFetchOutcome + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _lock = lock;
        let _ = tx.send(fetch_fn());
    });
    match rx.recv_timeout(RENDER_FETCH_TIMEOUT) {
        Ok(outcome) => outcome,
        Err(_) => {
            tracing::warn!(
                "cship.usage_limits_state: API fetch timed out after {}s",
                RENDER_FETCH_TIMEOUT.as_secs()
            );
            crate::usage_limits::UsageFetchOutcome::NetworkError
        }
    }
}

/// Coordinator's single public entry point: resolve OAuth usage
/// data for the current token, fetching from the Anthropic API at most once
/// per the configured interval (60s default) across all concurrent `cship`
/// processes.
///
/// Non-blocking: a process that can't acquire the fetch lock renders
/// immediately from whatever's already on disk, never waits on another
/// process's in-flight fetch.
pub(crate) fn resolve_shared_usage(
    token: Option<&str>,
    configured_ttl: Option<u64>,
    now: u64,
) -> SharedUsageResolution {
    let dir = crate::platform::cship_shared_cache_dir();
    resolve_shared_usage_in(
        dir.as_deref(),
        |t: String| crate::usage_limits::fetch_usage_limits(&t),
        token,
        configured_ttl,
        now,
    )
}

/// `resolve_shared_usage` with the cache directory and the fetch function
/// injected, so the whole orchestration (eligibility, locking, token-change
/// probe, outcome handling, persistence) can be tested against a temp dir and
/// a fake fetch. `dir = None` (no resolvable cache dir) behaves as before:
/// nothing is read, locked or persisted.
fn resolve_shared_usage_in<F>(
    dir: Option<&Path>,
    fetch: F,
    token: Option<&str>,
    configured_ttl: Option<u64>,
    now: u64,
) -> SharedUsageResolution
where
    F: FnOnce(String) -> crate::usage_limits::UsageFetchOutcome + Send + 'static,
{
    let load = || match dir {
        Some(d) => load_state_from_dir(d),
        None => SharedUsageState::default_state(),
    };
    let Some(token) = token else {
        return SharedUsageResolution { usage: None };
    };

    let fingerprint = crate::platform::token_fingerprint(token);
    let interval = effective_interval(configured_ttl);
    let state = load();
    let fresh_token_change = is_fresh_token_change(&state, &fingerprint);

    if !fresh_token_change && !fetch_may_be_needed(&state, &fingerprint, now) {
        return SharedUsageResolution {
            usage: displayable_usage(&state, &fingerprint),
        };
    }

    let Some(_lock) = dir.and_then(try_acquire_fetch_lock_in) else {
        return SharedUsageResolution {
            usage: displayable_usage(&state, &fingerprint),
        };
    };

    // Re-read: another process may have refreshed while we were acquiring.
    let mut state = load();
    let fresh_token_change = is_fresh_token_change(&state, &fingerprint);

    let recently_attempted = _lock.recently_attempted(&fingerprint, interval, now);

    if recently_attempted
        || (!fresh_token_change && !fetch_may_be_needed(&state, &fingerprint, now))
    {
        // Someone else already refreshed while we waited for the lock, or
        // this fingerprint already burned its one bypass probe and is now
        // just in normal backoff — or the state file under-reports how
        // recently a fetch ran because persisting it is currently failing
        // (see `FetchLock::recently_attempted`). The recently-attempted check applies
        // even to a fresh token change: if persisting the probe's outcome
        // failed, the lock file is still the backstop that stops it from
        // re-firing on every render.
        return SharedUsageResolution {
            usage: displayable_usage(&state, &fingerprint),
        };
    }

    if fresh_token_change {
        // Points 3/4/5: the old failure block no longer applies to this
        // token, the old schedule belonged to the old token, and any
        // existing rate-limit cooldown gets one probe rather than a blind
        // 15-minute wait — `apply_fetch_outcome` re-establishes backoff
        // normally below if the probe itself comes back 429. This bypass is
        // spent exactly once per newly-arrived fingerprint: if the probe
        // fails, `token_change_probe_fingerprint` records that below, so the
        // *next* render for this same fingerprint takes the
        // `fetch_may_be_needed` branch instead, like any ordinary render.
        //
        // `rate_limit_until_epoch` and `consecutive_errors` are cleared too:
        // they describe the *old* token's history. Left alone, a probe that
        // comes back NetworkError/InvalidResponse (rather than another 429)
        // would only bump `next_allowed_at_epoch`, leaving the old token's
        // rate-limit cooldown — up to `MAX_RATE_LIMIT_BACKOFF_SECS` — still
        // in effect and blocking the *new* token via `fetch_may_be_needed`'s
        // separate `rate_limit_until_epoch` check.
        state.failed_token_fingerprint = None;
        state.next_allowed_at_epoch = now;
        state.rate_limit_until_epoch = 0;
        state.consecutive_errors = 0;
    }

    // Recorded unconditionally, even if the state write below fails, so a
    // degraded (but writable-lock-file) cache dir still self-limits fetches
    // to once per `interval` — see `FetchLock::recently_attempted`. Touched now,
    // before the request even starts, since the request may outlive this
    // function's own return (see `fetch_with_render_timeout`).
    _lock.touch(&fingerprint);

    let token_owned = token.to_string();
    let outcome = fetch_with_render_timeout(move || fetch(token_owned), _lock);
    let succeeded = matches!(outcome, crate::usage_limits::UsageFetchOutcome::Success(_));
    apply_fetch_outcome(&mut state, outcome, now, interval, &fingerprint);
    if succeeded {
        // Point 6: only a successful fetch commits the new fingerprint —
        // this is the moment the "switch" is actually recorded.
        state.token_fingerprint = Some(fingerprint.clone());
        state.token_change_probe_fingerprint = None;
    } else if fresh_token_change {
        // Spend this fingerprint's one out-of-schedule probe so subsequent
        // renders for the same still-uncommitted fingerprint respect normal
        // backoff instead of bypassing the schedule again.
        state.token_change_probe_fingerprint = Some(fingerprint.clone());
    }
    if let Some(d) = dir {
        persist_state_to_dir(d, &state);
    }

    SharedUsageResolution {
        usage: displayable_usage(&state, &fingerprint),
    }
    // The OS-level lock itself may still be held by the background thread at
    // this point if the request timed out past the render deadline — it's
    // released whenever that thread actually finishes. The lock file is left
    // in place either way, as a permanent rendezvous point.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage_limits::UsageLimitsData;

    fn default_state_at(now: u64) -> SharedUsageState {
        let mut s = SharedUsageState::default_state();
        s.next_allowed_at_epoch = now; // eligible immediately unless overridden
        s
    }

    #[test]
    fn test_fetch_with_render_timeout_returns_fast_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let lock = try_acquire_fetch_lock_in(dir.path()).unwrap();
        let outcome = fetch_with_render_timeout(
            || crate::usage_limits::UsageFetchOutcome::Success(Box::default()),
            lock,
        );
        assert!(matches!(
            outcome,
            crate::usage_limits::UsageFetchOutcome::Success(_)
        ));
    }

    #[test]
    fn test_fetch_with_render_timeout_abandons_slow_fetch() {
        let dir = tempfile::tempdir().unwrap();
        let lock = try_acquire_fetch_lock_in(dir.path()).unwrap();
        let outcome = fetch_with_render_timeout(
            || {
                std::thread::sleep(Duration::from_secs(5));
                crate::usage_limits::UsageFetchOutcome::Success(Box::default())
            },
            lock,
        );
        assert!(matches!(
            outcome,
            crate::usage_limits::UsageFetchOutcome::NetworkError
        ));
    }

    /// Regression test for the lock-outliving-render-timeout bug: the OS
    /// lock must stay held (blocking a second `try_acquire_fetch_lock_in`)
    /// until the background thread actually finishes, not just until the
    /// render gives up waiting on it.
    #[test]
    fn test_fetch_with_render_timeout_keeps_lock_held_past_render_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let lock = try_acquire_fetch_lock_in(dir.path()).unwrap();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let outcome = fetch_with_render_timeout(
                || {
                    // Longer than RENDER_FETCH_TIMEOUT (2s) but short enough
                    // to keep the test fast.
                    std::thread::sleep(RENDER_FETCH_TIMEOUT + Duration::from_millis(500));
                    crate::usage_limits::UsageFetchOutcome::Success(Box::default())
                },
                lock,
            );
            let _ = done_tx.send(outcome);
        });

        // Wait past the render timeout — the caller above has already
        // "returned" its NetworkError outcome, but the real fetch (and thus
        // the lock) is still alive.
        std::thread::sleep(RENDER_FETCH_TIMEOUT + Duration::from_millis(100));
        assert!(
            try_acquire_fetch_lock_in(dir.path()).is_none(),
            "lock must still be held while the slow fetch is in flight"
        );

        // Let the slow fetch finish and release the lock. The lock drops
        // just after `tx.send` inside the spawned thread, a few
        // instructions after `done_rx` receives — poll briefly rather than
        // racing a fixed sleep against that window.
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let reacquired = (0..50).find_map(|_| {
            let lock = try_acquire_fetch_lock_in(dir.path());
            if lock.is_none() {
                std::thread::sleep(Duration::from_millis(20));
            }
            lock
        });
        assert!(
            reacquired.is_some(),
            "lock must be released once the slow fetch actually completes"
        );
    }

    #[test]
    fn test_effective_interval_defaults_to_60_when_unset() {
        assert_eq!(effective_interval(None), 60);
    }

    #[test]
    fn test_effective_interval_honors_configured_value_below_default() {
        assert_eq!(effective_interval(Some(30)), 30);
    }

    #[test]
    fn test_effective_interval_honors_configured_value_above_default() {
        assert_eq!(effective_interval(Some(600)), 600);
    }

    #[test]
    fn test_fetch_eligible_when_next_allowed_in_past() {
        let now = 2_000_000;
        let mut state = default_state_at(now);
        state.next_allowed_at_epoch = now - 1;
        assert!(fetch_may_be_needed(&state, "fp-1", now));
    }

    #[test]
    fn test_fetch_not_eligible_before_next_allowed() {
        let now = 2_000_000;
        let mut state = default_state_at(now);
        state.next_allowed_at_epoch = now + 100;
        assert!(!fetch_may_be_needed(&state, "fp-1", now));
    }

    #[test]
    fn test_fetch_not_eligible_during_rate_limit_cooldown() {
        let now = 2_000_000;
        let mut state = default_state_at(now);
        state.next_allowed_at_epoch = now - 1;
        state.rate_limit_until_epoch = now + 500;
        assert!(!fetch_may_be_needed(&state, "fp-1", now));
    }

    #[test]
    fn test_fetch_not_eligible_when_token_is_failed_fingerprint() {
        let now = 2_000_000;
        let mut state = default_state_at(now);
        state.next_allowed_at_epoch = now - 1;
        state.failed_token_fingerprint = Some("fp-1".to_string());
        assert!(!fetch_may_be_needed(&state, "fp-1", now));
    }

    #[test]
    fn test_fetch_eligible_when_failed_fingerprint_is_a_different_token() {
        let now = 2_000_000;
        let mut state = default_state_at(now);
        state.next_allowed_at_epoch = now - 1;
        state.failed_token_fingerprint = Some("fp-old".to_string());
        assert!(fetch_may_be_needed(&state, "fp-new", now));
    }

    #[test]
    fn test_apply_success_outcome_clears_backoff_and_sets_next_allowed() {
        let now = 2_000_000;
        let mut state = default_state_at(now);
        state.consecutive_errors = 3;
        state.rate_limit_until_epoch = now + 900;
        let data = UsageLimitsData::default();
        apply_fetch_outcome(
            &mut state,
            crate::usage_limits::UsageFetchOutcome::Success(Box::new(data)),
            now,
            180,
            "fp-1",
        );
        assert_eq!(state.consecutive_errors, 0);
        assert_eq!(state.rate_limit_until_epoch, 0);
        assert_eq!(state.next_allowed_at_epoch, now + 180);
        assert!(state.usage.is_some());
    }

    #[test]
    fn test_apply_rate_limited_outcome_honors_retry_after_within_bounds() {
        let now = 2_000_000;
        let mut state = default_state_at(now);
        apply_fetch_outcome(
            &mut state,
            crate::usage_limits::UsageFetchOutcome::RateLimited {
                retry_after_seconds: Some(400),
            },
            now,
            180,
            "fp-1",
        );
        assert_eq!(state.rate_limit_until_epoch, now + 400);
        assert_eq!(state.next_allowed_at_epoch, now + 400);
    }

    #[test]
    fn test_apply_rate_limited_outcome_clamps_retry_after_below_interval_up_to_interval() {
        let now = 2_000_000;
        let mut state = default_state_at(now);
        apply_fetch_outcome(
            &mut state,
            crate::usage_limits::UsageFetchOutcome::RateLimited {
                retry_after_seconds: Some(30),
            },
            now,
            180,
            "fp-1",
        );
        assert_eq!(state.rate_limit_until_epoch, now + 180);
    }

    #[test]
    fn test_apply_rate_limited_outcome_caps_retry_after_at_900() {
        let now = 2_000_000;
        let mut state = default_state_at(now);
        apply_fetch_outcome(
            &mut state,
            crate::usage_limits::UsageFetchOutcome::RateLimited {
                retry_after_seconds: Some(5000),
            },
            now,
            180,
            "fp-1",
        );
        assert_eq!(state.rate_limit_until_epoch, now + 900);
    }

    #[test]
    fn test_apply_rate_limited_outcome_missing_retry_after_uses_exponential_backoff() {
        let now = 2_000_000;
        let mut state = default_state_at(now);
        state.consecutive_errors = 2; // this will become the 3rd consecutive error
        apply_fetch_outcome(
            &mut state,
            crate::usage_limits::UsageFetchOutcome::RateLimited {
                retry_after_seconds: None,
            },
            now,
            180,
            "fp-1",
        );
        // exponent = consecutive_errors.saturating_sub(1).min(10) using the
        // POST-increment count (3 - 1 = 2) => 180 * 2^2 = 720
        assert_eq!(state.rate_limit_until_epoch, now + 720);
    }

    #[test]
    fn test_apply_network_error_keeps_previous_snapshot_and_retries_after_ttl() {
        let now = 2_000_000;
        let mut state = default_state_at(now);
        state.usage = Some(UsageLimitsData {
            five_hour_pct: 42.0,
            ..Default::default()
        });
        apply_fetch_outcome(
            &mut state,
            crate::usage_limits::UsageFetchOutcome::NetworkError,
            now,
            180,
            "fp-1",
        );
        assert_eq!(state.usage.as_ref().unwrap().five_hour_pct, 42.0);
        // Retry is never sooner than the configured ttl.
        assert_eq!(state.next_allowed_at_epoch, now + 180);
    }

    /// With a ttl shorter than the transient-error retry delay, the longer
    /// retry delay applies.
    #[test]
    fn test_apply_network_error_with_short_ttl_retries_in_30s() {
        let now = 2_000_000;
        let mut state = default_state_at(now);
        apply_fetch_outcome(
            &mut state,
            crate::usage_limits::UsageFetchOutcome::NetworkError,
            now,
            10,
            "fp-1",
        );
        assert_eq!(state.next_allowed_at_epoch, now + 30);
    }

    #[test]
    fn test_apply_unauthorized_outcome_sets_failed_token_fingerprint() {
        let now = 2_000_000;
        let mut state = default_state_at(now);
        state.token_fingerprint = Some("fp-current".to_string());
        apply_fetch_outcome(
            &mut state,
            crate::usage_limits::UsageFetchOutcome::Unauthorized,
            now,
            180,
            "fp-current",
        );
        assert_eq!(
            state.failed_token_fingerprint,
            Some("fp-current".to_string())
        );
    }

    /// Bug 2: during a token-change probe, `state.token_fingerprint` still
    /// holds the *previous* (uncommitted) fingerprint at the point
    /// `apply_fetch_outcome` runs. A 401 for the newly-attempted fingerprint
    /// must blame the fingerprint that was actually attempted, not the
    /// stale previous one, or `failed_token_fingerprint` can never correctly
    /// suppress retries for the actual bad token.
    #[test]
    fn test_apply_unauthorized_outcome_blames_attempted_fingerprint_not_stale_previous() {
        let now = 2_000_000;
        let mut state = default_state_at(now);
        state.token_fingerprint = Some("fp-old-stale".to_string());
        apply_fetch_outcome(
            &mut state,
            crate::usage_limits::UsageFetchOutcome::Unauthorized,
            now,
            180,
            "fp-new-attempted",
        );
        assert_eq!(
            state.failed_token_fingerprint,
            Some("fp-new-attempted".to_string())
        );
        assert_ne!(state.failed_token_fingerprint, state.token_fingerprint);
    }

    // -------------------------------------------------------------------
    // Bug 1: a token-change bypass probe must be spent at most once per
    // newly-arrived fingerprint — a persistently-failing new token must not
    // get a live fetch attempt on every render.
    // -------------------------------------------------------------------

    #[test]
    fn test_is_fresh_token_change_true_for_never_seen_fingerprint() {
        let state = SharedUsageState::default_state();
        assert!(is_fresh_token_change(&state, "fp-new"));
    }

    #[test]
    fn test_is_fresh_token_change_false_once_committed() {
        let mut state = SharedUsageState::default_state();
        state.token_fingerprint = Some("fp-new".to_string());
        assert!(!is_fresh_token_change(&state, "fp-new"));
    }

    #[test]
    fn test_is_fresh_token_change_false_after_probe_already_spent() {
        let mut state = SharedUsageState::default_state();
        state.token_fingerprint = Some("fp-old".to_string());
        state.token_change_probe_fingerprint = Some("fp-new".to_string());
        assert!(!is_fresh_token_change(&state, "fp-new"));
    }

    #[test]
    fn test_is_fresh_token_change_true_for_different_fingerprint_than_spent_probe() {
        let mut state = SharedUsageState::default_state();
        state.token_fingerprint = Some("fp-old".to_string());
        state.token_change_probe_fingerprint = Some("fp-some-other-new".to_string());
        assert!(is_fresh_token_change(&state, "fp-new"));
    }

    // ── resolve_shared_usage_in: the real orchestration, against a temp dir and a
    // fake fetch (no network, no real credential, no global env) ──────────────

    use crate::usage_limits::UsageFetchOutcome;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const OLD_TOKEN: &str = "old-token-aaaaaaaaaaaaaaaa";
    const NEW_TOKEN: &str = "new-token-bbbbbbbbbbbbbbbb";
    const TTL: u64 = 180;

    /// Wall-clock now. The lock file's mtime is real, so the orchestration
    /// tests must use real time (plus offsets), not an arbitrary epoch.
    fn real_now() -> u64 {
        now_epoch_for_test()
    }

    /// Runs one render against `dir` with a fake fetch returning `outcome`,
    /// counting how many times the "network" was hit.
    fn render_with(
        dir: &Path,
        calls: &Arc<AtomicUsize>,
        outcome: impl FnOnce() -> UsageFetchOutcome + Send + 'static,
        token: &str,
        now: u64,
    ) -> SharedUsageResolution {
        let calls = Arc::clone(calls);
        resolve_shared_usage_in(
            Some(dir),
            move |_token| {
                calls.fetch_add(1, Ordering::SeqCst);
                outcome()
            },
            Some(token),
            Some(TTL),
            now,
        )
    }

    fn success_with_pct(pct: f64) -> UsageFetchOutcome {
        UsageFetchOutcome::Success(Box::new(UsageLimitsData {
            five_hour_pct: pct,
            five_hour_available: true,
            ..Default::default()
        }))
    }

    /// Seeds the state file as if `token` had been fetched successfully before.
    fn seed_committed_state(dir: &Path, token: &str, now: u64) {
        let mut state = SharedUsageState::default_state();
        state.token_fingerprint = Some(crate::platform::token_fingerprint(token));
        state.next_allowed_at_epoch = now;
        state.usage = Some(UsageLimitsData {
            five_hour_pct: 11.0,
            five_hour_available: true,
            ..Default::default()
        });
        persist_state_to_dir(dir, &state);
    }

    #[test]
    fn test_resolve_first_run_fetches_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let now = real_now();

        let r = render_with(
            dir.path(),
            &calls,
            || success_with_pct(42.0),
            NEW_TOKEN,
            now,
        );

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "a fresh install must fetch"
        );
        assert_eq!(r.usage.unwrap().five_hour_pct, 42.0);
        let saved = load_state_from_dir(dir.path());
        assert_eq!(
            saved.token_fingerprint,
            Some(crate::platform::token_fingerprint(NEW_TOKEN))
        );
        assert_eq!(saved.usage.unwrap().five_hour_pct, 42.0);
    }

    #[test]
    fn test_resolve_within_ttl_uses_saved_data_without_fetching() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let now = real_now();
        render_with(
            dir.path(),
            &calls,
            || success_with_pct(42.0),
            NEW_TOKEN,
            now,
        );

        let r = render_with(
            dir.path(),
            &calls,
            || success_with_pct(99.0),
            NEW_TOKEN,
            now + 10,
        );

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(r.usage.unwrap().five_hour_pct, 42.0);
    }

    #[test]
    fn test_resolve_after_ttl_fetches_again() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let now = real_now();
        render_with(
            dir.path(),
            &calls,
            || success_with_pct(42.0),
            NEW_TOKEN,
            now,
        );

        let r = render_with(
            dir.path(),
            &calls,
            || success_with_pct(99.0),
            NEW_TOKEN,
            now + TTL + 1,
        );

        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(r.usage.unwrap().five_hour_pct, 99.0);
    }

    #[test]
    fn test_resolve_without_token_never_fetches() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&calls);
        let r = resolve_shared_usage_in(
            Some(dir.path()),
            move |_t| {
                c.fetch_add(1, Ordering::SeqCst);
                success_with_pct(1.0)
            },
            None,
            Some(TTL),
            real_now(),
        );
        assert!(r.usage.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_resolve_without_cache_dir_never_touches_disk_or_locks() {
        let calls = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&calls);
        let r = resolve_shared_usage_in(
            None,
            move |_t| {
                c.fetch_add(1, Ordering::SeqCst);
                success_with_pct(1.0)
            },
            Some(NEW_TOKEN),
            Some(TTL),
            real_now(),
        );
        // No cache dir means no lock can be taken, so no fetch is attempted.
        assert!(r.usage.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    /// Another process holds the fetch lock: render from what's on disk and
    /// never wait or fetch.
    #[test]
    fn test_resolve_when_lock_held_elsewhere_renders_saved_data_without_fetching() {
        let dir = tempfile::tempdir().unwrap();
        let now = real_now();
        seed_committed_state(dir.path(), NEW_TOKEN, now - 1);
        let _other_process = try_acquire_fetch_lock_in(dir.path()).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));

        let r = render_with(
            dir.path(),
            &calls,
            || success_with_pct(99.0),
            NEW_TOKEN,
            now,
        );

        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(r.usage.unwrap().five_hour_pct, 11.0);
    }

    /// The first render for a new token bypasses the old schedule and probes
    /// once; if that fails, the second render must NOT probe again and must
    /// respect the backoff the failure recorded.
    #[test]
    fn test_resolve_second_render_for_same_failing_new_token_does_not_bypass_again() {
        let dir = tempfile::tempdir().unwrap();
        let now = real_now();
        seed_committed_state(dir.path(), OLD_TOKEN, now);
        // The old token's schedule is far in the future; the switch bypasses it.
        let mut s = load_state_from_dir(dir.path());
        s.next_allowed_at_epoch = now + 9_999;
        persist_state_to_dir(dir.path(), &s);
        let calls = Arc::new(AtomicUsize::new(0));
        let rate_limited = || UsageFetchOutcome::RateLimited {
            retry_after_seconds: Some(400),
        };

        render_with(dir.path(), &calls, rate_limited, NEW_TOKEN, now);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "first render probes once");
        let saved = load_state_from_dir(dir.path());
        assert_eq!(saved.rate_limit_until_epoch, now + 400);
        assert_ne!(
            saved.token_fingerprint,
            Some(crate::platform::token_fingerprint(NEW_TOKEN)),
            "a failed probe must never commit the new token"
        );

        render_with(dir.path(), &calls, rate_limited, NEW_TOKEN, now + 10);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "second render must respect the recorded backoff"
        );

        render_with(dir.path(), &calls, rate_limited, NEW_TOKEN, now + 401);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "normal scheduling resumes after the cooldown"
        );
    }

    /// A 401 on the new token blames it: no more fetches until the token changes.
    #[test]
    fn test_resolve_unauthorized_new_token_stays_suppressed() {
        let dir = tempfile::tempdir().unwrap();
        let now = real_now();
        seed_committed_state(dir.path(), OLD_TOKEN, now);
        let calls = Arc::new(AtomicUsize::new(0));

        render_with(
            dir.path(),
            &calls,
            || UsageFetchOutcome::Unauthorized,
            NEW_TOKEN,
            now,
        );
        assert_eq!(
            load_state_from_dir(dir.path()).failed_token_fingerprint,
            Some(crate::platform::token_fingerprint(NEW_TOKEN))
        );

        render_with(
            dir.path(),
            &calls,
            || success_with_pct(1.0),
            NEW_TOKEN,
            now + 10_000,
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "a blamed token stays suppressed, even much later"
        );

        // A different token is not blamed and fetches immediately.
        render_with(
            dir.path(),
            &calls,
            || success_with_pct(7.0),
            OLD_TOKEN,
            now + 10_001,
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// A successful probe commits the new token and clears the probe marker.
    #[test]
    fn test_resolve_success_on_new_token_commits_fingerprint_and_clears_probe_marker() {
        let dir = tempfile::tempdir().unwrap();
        let now = real_now();
        seed_committed_state(dir.path(), OLD_TOKEN, now);
        let calls = Arc::new(AtomicUsize::new(0));

        let r = render_with(dir.path(), &calls, || success_with_pct(5.0), NEW_TOKEN, now);

        let saved = load_state_from_dir(dir.path());
        assert_eq!(
            saved.token_fingerprint,
            Some(crate::platform::token_fingerprint(NEW_TOKEN))
        );
        assert_eq!(saved.token_change_probe_fingerprint, None);
        assert_eq!(r.usage.unwrap().five_hour_pct, 5.0);
    }

    /// The old token's data must not be shown for the new token.
    #[test]
    fn test_resolve_does_not_show_previous_tokens_data_when_probe_fails() {
        let dir = tempfile::tempdir().unwrap();
        let now = real_now();
        seed_committed_state(dir.path(), OLD_TOKEN, now);
        let calls = Arc::new(AtomicUsize::new(0));

        let r = render_with(
            dir.path(),
            &calls,
            || UsageFetchOutcome::NetworkError,
            NEW_TOKEN,
            now,
        );

        assert!(r.usage.is_none());
    }

    /// A token switch must not inherit the old token's rate-limit cooldown,
    /// even when the probe fails with a transient (non-429) error.
    #[test]
    fn test_resolve_token_switch_clears_inherited_rate_limit_on_transient_failure() {
        let dir = tempfile::tempdir().unwrap();
        let now = real_now();
        seed_committed_state(dir.path(), OLD_TOKEN, now);
        let mut s = load_state_from_dir(dir.path());
        s.rate_limit_until_epoch = now + 900;
        s.consecutive_errors = 3;
        persist_state_to_dir(dir.path(), &s);
        let calls = Arc::new(AtomicUsize::new(0));

        render_with(
            dir.path(),
            &calls,
            || UsageFetchOutcome::NetworkError,
            NEW_TOKEN,
            now,
        );

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let saved = load_state_from_dir(dir.path());
        assert_eq!(
            saved.rate_limit_until_epoch, 0,
            "the old token's cooldown must not survive the switch"
        );
        assert_eq!(saved.consecutive_errors, 1);
        // Eligible again once its own transient backoff elapses.
        render_with(
            dir.path(),
            &calls,
            || success_with_pct(3.0),
            NEW_TOKEN,
            now + TTL + 31,
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    /// The old token's 401 blame describes that token only; switching tokens
    /// clears it, so it is not carried into the new token's state.
    #[test]
    fn test_resolve_token_switch_clears_old_tokens_401_blame() {
        let dir = tempfile::tempdir().unwrap();
        let now = real_now();
        seed_committed_state(dir.path(), OLD_TOKEN, now);
        let mut s = load_state_from_dir(dir.path());
        s.failed_token_fingerprint = Some(crate::platform::token_fingerprint(OLD_TOKEN));
        persist_state_to_dir(dir.path(), &s);
        let calls = Arc::new(AtomicUsize::new(0));

        render_with(
            dir.path(),
            &calls,
            || UsageFetchOutcome::NetworkError,
            NEW_TOKEN,
            now,
        );

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            load_state_from_dir(dir.path()).failed_token_fingerprint,
            None
        );
    }

    /// If the state file can never be written, the lock file's record of the
    /// attempt must still stop a fetch on every render.
    #[test]
    fn test_resolve_unwritable_state_file_still_throttles_via_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the state file should be makes every persist fail.
        std::fs::create_dir(state_file_path_in(dir.path())).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let now = real_now();

        render_with(dir.path(), &calls, || success_with_pct(1.0), NEW_TOKEN, now);
        render_with(
            dir.path(),
            &calls,
            || success_with_pct(1.0),
            NEW_TOKEN,
            now + 5,
        );
        render_with(
            dir.path(),
            &calls,
            || success_with_pct(1.0),
            NEW_TOKEN,
            now + 10,
        );

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "persist failures must not turn into a fetch per render"
        );

        render_with(
            dir.path(),
            &calls,
            || success_with_pct(1.0),
            NEW_TOKEN,
            now + TTL + 1,
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "but ttl still re-opens fetching"
        );
    }

    #[test]
    fn test_backoff_secs_doubles_per_error_and_caps() {
        assert_eq!(backoff_secs(180, 1, 900), 180); // 180 * 2^0
        assert_eq!(backoff_secs(180, 2, 900), 360); // 180 * 2^1
        assert_eq!(backoff_secs(180, 3, 900), 720); // 180 * 2^2
        assert_eq!(backoff_secs(180, 4, 900), 900); // 180 * 2^3 = 1440, capped
    }

    #[test]
    fn test_resolve_shared_usage_no_token_returns_no_usage() {
        let resolution = resolve_shared_usage(None, None, 1_000_000);
        assert!(resolution.usage.is_none());
    }

    #[test]
    fn test_load_state_or_default_when_file_missing_returns_schema_v3_default() {
        let dir = tempfile::tempdir().unwrap();
        let state = load_state_from_dir(dir.path());
        assert_eq!(state.schema_version, 3);
        assert!(state.usage.is_none());
        assert_eq!(state.next_allowed_at_epoch, 0);
    }

    #[test]
    fn test_load_state_or_default_when_json_invalid_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("usage-limits-state-v3.json"), "{not json").unwrap();
        let state = load_state_from_dir(dir.path());
        assert_eq!(state.schema_version, 3);
        assert!(state.usage.is_none());
    }

    #[test]
    fn test_load_state_or_default_when_schema_version_unknown_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("usage-limits-state-v3.json"),
            r#"{"schema_version":99,"token_fingerprint":null,"usage":null,"next_allowed_at_epoch":0,"rate_limit_until_epoch":0,"consecutive_errors":0,"failed_token_fingerprint":null}"#,
        )
        .unwrap();
        let state = load_state_from_dir(dir.path());
        assert_eq!(state.schema_version, 3);
        assert!(state.usage.is_none());
    }

    #[test]
    fn test_persist_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = SharedUsageState::default_state();
        state.token_fingerprint = Some("fp-abc123".to_string());
        state.next_allowed_at_epoch = 4_000_000;
        persist_state_to_dir(dir.path(), &state);
        let loaded = load_state_from_dir(dir.path());
        assert_eq!(loaded.token_fingerprint, Some("fp-abc123".to_string()));
        assert_eq!(loaded.next_allowed_at_epoch, 4_000_000);
    }

    #[test]
    fn test_persist_state_produces_no_temp_files_left_behind() {
        let dir = tempfile::tempdir().unwrap();
        let state = SharedUsageState::default_state();
        persist_state_to_dir(dir.path(), &state);
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(entries, vec!["usage-limits-state-v3.json"]);
    }

    #[test]
    fn test_lock_first_process_acquires() {
        let dir = tempfile::tempdir().unwrap();
        let lock = try_acquire_fetch_lock_in(dir.path());
        assert!(lock.is_some());
    }

    #[test]
    fn test_lock_concurrent_acquire_while_held_fails_immediately() {
        let dir = tempfile::tempdir().unwrap();
        let _first = try_acquire_fetch_lock_in(dir.path()).unwrap();
        let second = try_acquire_fetch_lock_in(dir.path());
        assert!(second.is_none());
    }

    #[test]
    fn test_lock_released_on_drop_can_be_immediately_reacquired() {
        let dir = tempfile::tempdir().unwrap();
        {
            let _lock = try_acquire_fetch_lock_in(dir.path()).unwrap();
        } // dropped here — OS releases the advisory lock
        let second = try_acquire_fetch_lock_in(dir.path());
        assert!(second.is_some());
    }

    /// A pre-existing lock *file* with no live lock on it (e.g. left behind
    /// by a previous run — the file is a permanent rendezvous point, never
    /// deleted) must not block acquisition: only a held OS lock blocks.
    #[test]
    fn test_lock_preexisting_unlocked_file_does_not_block() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(LOCK_FILE_NAME), "leftover").unwrap();
        let lock = try_acquire_fetch_lock_in(dir.path());
        assert!(lock.is_some());
    }

    /// Crash recovery: if the holding process dies without running our
    /// `Drop` (simulated by dropping the raw locked `File` directly, never
    /// constructing a `FetchLock`), the OS releases the lock as soon as the
    /// descriptor closes — a new acquire succeeds with no staleness wait.
    #[test]
    fn test_lock_released_by_os_when_holder_crashes() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join(LOCK_FILE_NAME);
        let crashed_holder = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();
        crashed_holder.try_lock().unwrap();

        // While "crashed_holder" is alive and holding the lock, acquisition
        // must fail...
        assert!(try_acquire_fetch_lock_in(dir.path()).is_none());

        // ...and the moment its handle closes, the OS releases the lock.
        drop(crashed_holder);
        assert!(try_acquire_fetch_lock_in(dir.path()).is_some());
    }

    fn now_epoch_for_test() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn test_recently_attempted_true_just_after_touch() {
        let dir = tempfile::tempdir().unwrap();
        let lock = try_acquire_fetch_lock_in(dir.path()).unwrap();
        lock.touch("fp-1");
        assert!(lock.recently_attempted("fp-1", 60, now_epoch_for_test()));
    }

    #[test]
    fn test_recently_attempted_false_once_interval_elapses() {
        let dir = tempfile::tempdir().unwrap();
        let lock = try_acquire_fetch_lock_in(dir.path()).unwrap();
        lock.touch("fp-1");
        assert!(!lock.recently_attempted("fp-1", 60, now_epoch_for_test() + 61));
    }

    /// A freshly created lock file has an mtime of "now" but records no
    /// attempt. It must not be mistaken for "a fetch just ran", or the very
    /// first render (or the first after the cache dir is wiped) would skip
    /// fetching for a whole interval.
    #[test]
    fn test_recently_attempted_false_for_fresh_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        let lock = try_acquire_fetch_lock_in(dir.path()).unwrap();
        assert!(!lock.recently_attempted("fp-1", 60, now_epoch_for_test()));
    }

    /// Persist-failure amplification: even though `state.next_allowed_at_epoch`
    /// never advances (simulating a cache dir where `persist_state` silently
    /// fails), a second render within `interval` of the first must not be
    /// judged eligible, because the lock file records the attempt.
    #[test]
    fn test_recently_attempted_blocks_reeligibility_despite_stale_state() {
        let dir = tempfile::tempdir().unwrap();
        let lock = try_acquire_fetch_lock_in(dir.path()).unwrap();
        lock.touch("fp-1");
        drop(lock);

        let now = now_epoch_for_test();
        let state = SharedUsageState::default_state(); // next_allowed_at_epoch stuck at 0
        let lock = try_acquire_fetch_lock_in(dir.path()).unwrap();

        assert!(fetch_may_be_needed(&state, "fp-1", now)); // state alone says "go"
        assert!(lock.recently_attempted("fp-1", 60, now)); // backstop says "no"
    }

    /// Same amplification for the fresh-token-change bypass: if persisting the
    /// probe's outcome fails, the fingerprint stays "fresh" on every render,
    /// so the lock's record of the attempt must still gate it.
    #[test]
    fn test_recently_attempted_blocks_fresh_token_bypass_despite_stale_state() {
        let dir = tempfile::tempdir().unwrap();
        let lock = try_acquire_fetch_lock_in(dir.path()).unwrap();
        lock.touch("fp-new");
        drop(lock);

        let state = SharedUsageState::default_state();
        let lock = try_acquire_fetch_lock_in(dir.path()).unwrap();

        assert!(is_fresh_token_change(&state, "fp-new")); // state alone says "bypass"
        assert!(lock.recently_attempted("fp-new", 60, now_epoch_for_test()));
    }

    /// Switching accounts right after a fetch must not be throttled by the
    /// previous account's attempt: the new token gets its one probe.
    #[test]
    fn test_recently_attempted_false_for_different_token() {
        let dir = tempfile::tempdir().unwrap();
        let lock = try_acquire_fetch_lock_in(dir.path()).unwrap();
        lock.touch("fp-old");
        assert!(!lock.recently_attempted("fp-new", 60, now_epoch_for_test()));
    }

    /// The state and lock files hold a token fingerprint, so they must be
    /// owner-only from creation (not chmod'd afterwards).
    #[cfg(unix)]
    #[test]
    fn test_state_and_lock_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let lock = try_acquire_fetch_lock_in(dir.path()).unwrap();
        persist_state_to_dir(dir.path(), &SharedUsageState::default_state());
        for path in [lock_path_in(dir.path()), state_file_path_in(dir.path())] {
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{}", path.display());
        }
        drop(lock);
    }

    #[test]
    fn test_effective_interval_clamps_zero_ttl() {
        assert_eq!(effective_interval(Some(0)), 1);
        assert_eq!(effective_interval(Some(5)), 5);
        assert_eq!(effective_interval(None), DEFAULT_API_INTERVAL_SECS);
    }

    // ── next-poll scheduling around window resets, and backoff vs ttl ─────────

    fn iso_at(epoch: u64) -> String {
        chrono::DateTime::from_timestamp(epoch as i64, 0)
            .unwrap()
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    }

    /// Success outcome for a snapshot whose 5h window resets at `reset_epoch`
    /// (None = no known reset), applied with the given ttl; returns the
    /// scheduled `next_allowed_at_epoch`.
    fn next_after_success(now: u64, ttl: u64, reset_epoch: Option<u64>) -> u64 {
        let mut state = default_state_at(now);
        let data = UsageLimitsData {
            five_hour_resets_at: reset_epoch.map(iso_at).unwrap_or_default(),
            ..Default::default()
        };
        apply_fetch_outcome(
            &mut state,
            crate::usage_limits::UsageFetchOutcome::Success(Box::new(data)),
            now,
            ttl,
            "fp-1",
        );
        state.next_allowed_at_epoch
    }

    /// A reset before the regular poll pulls the next poll forward to just
    /// after it, so the new window shows up promptly.
    #[test]
    fn test_success_reset_before_regular_poll_polls_just_after_reset() {
        let now = 2_000_000;
        assert_eq!(next_after_success(now, 600, Some(now + 100)), now + 105);
    }

    /// A reset more than one interval away must not delay the regular poll.
    #[test]
    fn test_success_reset_beyond_regular_poll_does_not_delay_it() {
        let now = 2_000_000;
        // ttl 600: regular poll at +600; reset at +1190 is well beyond it.
        assert_eq!(next_after_success(now, 600, Some(now + 1190)), now + 600);
    }

    /// A reset landing just after the regular poll replaces it: polling at the
    /// regular time would only see the old window.
    #[test]
    fn test_success_reset_just_after_regular_poll_replaces_it() {
        let now = 2_000_000;
        assert_eq!(next_after_success(now, 600, Some(now + 610)), now + 615);
    }

    /// ...but only within the spacing window; a later reset keeps the regular poll.
    #[test]
    fn test_success_reset_after_spacing_window_keeps_regular_poll() {
        let now = 2_000_000;
        // regular poll at +600; reset+5 = +635 is 35s later (> 30s spacing).
        assert_eq!(next_after_success(now, 600, Some(now + 630)), now + 600);
    }

    /// Two polls are never scheduled closer than the minimum spacing, even
    /// when a reset is imminent.
    #[test]
    fn test_success_imminent_reset_respects_min_spacing() {
        let now = 2_000_000;
        assert_eq!(next_after_success(now, 600, Some(now + 3)), now + 30);
    }

    /// The spacing never exceeds a short user-configured ttl.
    #[test]
    fn test_success_min_spacing_is_capped_by_short_ttl() {
        let now = 2_000_000;
        assert_eq!(next_after_success(now, 10, Some(now + 3)), now + 10);
    }

    #[test]
    fn test_success_without_reset_uses_regular_interval() {
        let now = 2_000_000;
        assert_eq!(next_after_success(now, 600, None), now + 600);
    }

    /// Errors must never make polling more frequent than the configured ttl.
    #[test]
    fn test_errors_never_poll_more_often_than_ttl() {
        use crate::usage_limits::UsageFetchOutcome;
        let now = 2_000_000;
        let ttl = 3600;
        let cases = [
            UsageFetchOutcome::RateLimited {
                retry_after_seconds: Some(30),
            },
            UsageFetchOutcome::RateLimited {
                retry_after_seconds: Some(5000),
            },
            UsageFetchOutcome::RateLimited {
                retry_after_seconds: None,
            },
            UsageFetchOutcome::ServerError { status: 503 },
            UsageFetchOutcome::NetworkError,
            UsageFetchOutcome::InvalidResponse,
        ];
        for outcome in cases {
            let label = format!("{outcome:?}");
            let mut state = default_state_at(now);
            apply_fetch_outcome(&mut state, outcome, now, ttl, "fp-1");
            assert!(
                state.next_allowed_at_epoch >= now + ttl,
                "{label}: next poll {}s away, below ttl {ttl}s",
                state.next_allowed_at_epoch - now
            );
        }
    }
}
