//! Best-effort terminal-width detection for `$fill` right-alignment.
//!
//! A Claude Code statusline command is a piped child process with no controlling
//! terminal of its own, and Claude Code passes neither `$COLUMNS` nor a width
//! field in its JSON (see <https://github.com/anthropics/claude-code/issues/22115>).
//! So we recover the width by walking up the parent process chain to the first
//! ancestor that *does* have a controlling tty (typically `claude` itself) and
//! reading that tty's window size via `ioctl(TIOCGWINSZ)`.
//!
//! This is a deliberate workaround, not a clean solution — it depends on the
//! statusline being spawned under a tty-holding ancestor, which is true in a
//! terminal but not in the web/desktop app or on Windows. Every failure path
//! falls back gracefully (see [`statusline_width`]).

use crate::config::CshipConfig;

/// Fallback width when nothing else is known. 80 is the universal terminal default.
const DEFAULT_WIDTH: u16 = 80;
/// Columns Claude Code reserves around the statusline (~2 left + 1 right), subtracted
/// from the raw terminal width. Overridable via `[cship] width_offset`.
const DEFAULT_OFFSET: u16 = 3;

/// The usable statusline width in columns.
///
/// Resolution order (per the project's stated hierarchy):
/// detected terminal width → `$COLUMNS` → `[cship] width` → 80,
/// then minus `[cship] width_offset` (default 3). Never returns 0.
pub fn statusline_width(cfg: &CshipConfig) -> u16 {
    let raw = detect_columns()
        .or_else(env_columns)
        .or(cfg.width)
        .unwrap_or(DEFAULT_WIDTH);
    apply_offset(raw, cfg.width_offset.unwrap_or(DEFAULT_OFFSET))
}

/// Subtract the reserved-margin offset from a raw terminal width, clamped to ≥1.
/// Pure (no environment lookup) so it's unit-testable independent of the terminal.
fn apply_offset(raw: u16, offset: u16) -> u16 {
    raw.saturating_sub(offset).max(1)
}

fn env_columns() -> Option<u16> {
    std::env::var("COLUMNS").ok()?.parse().ok()
}

/// Detect the terminal width by walking the parent chain to a tty-holding
/// ancestor. Returns `None` when no ancestor has a readable controlling tty
/// (e.g. Windows, the web/desktop app, or a detached process tree).
#[cfg(unix)]
pub fn detect_columns() -> Option<u16> {
    unix_impl::detect()
}

#[cfg(not(unix))]
pub fn detect_columns() -> Option<u16> {
    None
}

#[cfg(unix)]
mod unix_impl {
    use std::fs::OpenOptions;
    use std::os::fd::AsFd;
    use std::os::unix::fs::OpenOptionsExt;

    /// Walk self → parent → … (including pid 1) and return the width of the first
    /// ancestor's controlling tty we can read.
    pub(super) fn detect() -> Option<u16> {
        let mut pid = std::process::id() as i32;
        for _ in 0..16 {
            let (ppid, dev) = proc_info(pid)?;
            // 0 and u32::MAX (== (dev_t)-1 / NODEV) both mean "no controlling tty".
            if dev != 0
                && dev != u64::from(u32::MAX)
                && let Some(cols) = cols_for_dev(dev)
                && cols > 0
            {
                tracing::debug!("cship: detected terminal width {cols} via pid {pid}");
                return Some(cols);
            }
            if pid <= 1 || ppid <= 0 || ppid == pid {
                break;
            }
            pid = ppid;
        }
        None
    }

    /// Open a tty device by its `dev_t` (without acquiring it as our controlling
    /// terminal) and read its column count.
    fn cols_for_dev(dev: u64) -> Option<u16> {
        let path = ctty::get_path_for_dev(dev).ok()?;
        let f = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOCTTY)
            .open(&path)
            .ok()?;
        let (terminal_size::Width(cols), terminal_size::Height(_)) =
            terminal_size::terminal_size_of(f.as_fd())?;
        Some(cols)
    }

    /// `(parent pid, controlling-tty dev_t)` for `pid`.
    #[cfg(target_os = "macos")]
    fn proc_info(pid: i32) -> Option<(i32, u64)> {
        use libproc::libproc::bsd_info::BSDInfo;
        use libproc::libproc::proc_pid::pidinfo;
        let info = pidinfo::<BSDInfo>(pid, 0).ok()?;
        Some((info.pbi_ppid as i32, info.e_tdev as u64))
    }

    #[cfg(target_os = "linux")]
    fn proc_info(pid: i32) -> Option<(i32, u64)> {
        let stat = procfs::process::Process::new(pid).ok()?.stat().ok()?;
        Some((stat.ppid, stat.tty_nr as u64))
    }

    // Other unixes (FreeBSD, etc.): no proc walk yet → fall back to config/80.
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    fn proc_info(_pid: i32) -> Option<(i32, u64)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CshipConfig;

    #[test]
    fn test_apply_offset_subtracts() {
        assert_eq!(apply_offset(100, 3), 97);
        assert_eq!(apply_offset(50, 3), 47);
    }

    #[test]
    fn test_apply_offset_clamps_to_one() {
        // Offset larger than width never yields 0.
        assert_eq!(apply_offset(2, 10), 1);
        assert_eq!(apply_offset(1, 1), 1);
    }

    #[test]
    fn test_default_offset_constant_is_three() {
        assert_eq!(DEFAULT_OFFSET, 3);
    }

    #[test]
    fn test_statusline_width_applies_offset_when_detection_absent() {
        // Only meaningful when no ancestor tty is found (e.g. CI); pinned via config.
        // The offset math itself is covered unconditionally by `test_apply_offset_*`.
        if detect_columns().is_none() && env_columns().is_none() {
            let cfg = CshipConfig {
                width: Some(100),
                width_offset: Some(5),
                ..Default::default()
            };
            assert_eq!(statusline_width(&cfg), 95);
        }
    }
}
