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
    use std::path::Path;

    /// Walk self → parent → … (up to 16 hops) and return the column count of the
    /// first ancestor whose controlling terminal we can read.
    pub(super) fn detect() -> Option<u16> {
        let mut pid = std::process::id() as i32;
        for _ in 0..16 {
            let (ppid, cols) = os::proc_info(pid)?;
            if let Some(cols) = cols
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

    /// Open a tty device by path *without* acquiring it as our controlling
    /// terminal (`O_NOCTTY`) and read its column count via `TIOCGWINSZ`.
    fn cols_for_path(path: &Path) -> Option<u16> {
        let f = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOCTTY)
            .open(path)
            .ok()?;
        let (terminal_size::Width(cols), terminal_size::Height(_)) =
            terminal_size::terminal_size_of(f.as_fd())?;
        Some(cols)
    }

    /// macOS: read a process's parent pid and controlling-tty device via a
    /// single `proc_pidinfo(PROC_PIDTBSDINFO)` call — the same source the
    /// `libproc` crate wrapped — then resolve the device path with `devname(3)`.
    /// (`libc` exposes the compact `proc_bsdinfo` struct but not the sprawling
    /// `kinfo_proc` that `sysctl(KERN_PROC_PID)` would need hand-declared.)
    #[cfg(target_os = "macos")]
    mod os {
        use std::ffi::CStr;
        use std::path::PathBuf;

        /// `(parent pid, controlling-tty columns)` for `pid`. Columns are `None`
        /// when `pid` has no controlling tty or its winsize can't be read.
        pub(super) fn proc_info(pid: i32) -> Option<(i32, Option<u16>)> {
            let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
            // SAFETY: `info`/`size` are a correctly sized, writable out-buffer for
            // one `proc_bsdinfo`, which is plain C data (so zeroed is a valid start).
            let written = unsafe {
                libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, (&raw mut info).cast(), size)
            };
            // `proc_pidinfo` returns the byte count written; a short read means the
            // process is gone or otherwise inaccessible.
            if written != size {
                return None;
            }
            Some((info.pbi_ppid as i32, tty_cols(info.e_tdev)))
        }

        /// Columns of the tty identified by `dev`, or `None` when `dev` is not a
        /// controlling terminal or its winsize can't be read.
        fn tty_cols(dev: u32) -> Option<u16> {
            // A process with no controlling tty reports `e_tdev == 0` or `NODEV`
            // (`(dev_t)-1`, which surfaces here as `u32::MAX`).
            if dev == 0 || dev == u32::MAX {
                return None;
            }
            // SAFETY: `devname` returns a pointer into a static buffer (valid
            // until the next call) or null; we copy the string out immediately.
            let name = unsafe {
                let p = libc::devname(dev as libc::dev_t, libc::S_IFCHR);
                if p.is_null() {
                    return None;
                }
                CStr::from_ptr(p).to_str().ok()?.to_owned()
            };
            super::cols_for_path(&PathBuf::from("/dev").join(name))
        }
    }

    /// Linux: read a process's parent pid and controlling-tty width from
    /// `/proc/<pid>/stat` + `/proc/<pid>/fd`, using only `std` (no extra crates).
    #[cfg(target_os = "linux")]
    mod os {
        use std::path::{Path, PathBuf};

        /// `(parent pid, controlling-tty columns)` for `pid`. Columns are `None`
        /// when `pid` has no controlling tty (`tty_nr == 0`) or its winsize can't
        /// be read.
        pub(super) fn proc_info(pid: i32) -> Option<(i32, Option<u16>)> {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            // Format: `pid (comm) state ppid pgrp session tty_nr …`. `comm` is
            // parenthesised and may itself contain spaces or parens, so the fixed
            // fields are parsed *after the last ')'*.
            let mut fields = stat[stat.rfind(')')? + 1..].split_whitespace();
            let _state = fields.next()?;
            let ppid: i32 = fields.next()?.parse().ok()?;
            let _pgrp = fields.next()?;
            let _session = fields.next()?;
            let tty_nr: i64 = fields.next()?.parse().ok()?;

            let cols = (tty_nr != 0)
                .then(|| tty_path(pid).and_then(|p| super::cols_for_path(&p)))
                .flatten();
            Some((ppid, cols))
        }

        /// The controlling terminal's `/dev` path for `pid`, found by following
        /// its standard descriptors to the first that points at a tty device.
        /// `readlink` avoids the `/dev` scan a `tty_nr`→path mapping would need.
        fn tty_path(pid: i32) -> Option<PathBuf> {
            [0, 1, 2].into_iter().find_map(|fd| {
                let target = std::fs::read_link(format!("/proc/{pid}/fd/{fd}")).ok()?;
                is_tty_dev(&target).then_some(target)
            })
        }

        fn is_tty_dev(path: &Path) -> bool {
            matches!(path.to_str(), Some(s) if s.starts_with("/dev/pts/") || s.starts_with("/dev/tty"))
        }
    }

    /// Other unixes (FreeBSD, etc.): no proc walk yet → fall back to config/80.
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    mod os {
        pub(super) fn proc_info(_pid: i32) -> Option<(i32, Option<u16>)> {
            None
        }
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
