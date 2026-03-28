//! `cship uninstall` — removes binary, settings.json statusline entry, and cache dirs.
//! Intentionally leaves `[cship.*]` sections in `starship.toml` intact (FR44).

pub fn run() {
    let home = match crate::platform::home_dir() {
        Some(h) => h,
        None => {
            println!("Cannot determine home directory — set CLAUDE_HOME. Aborting uninstall.");
            return;
        }
    };
    remove_binary(&home);
    remove_statusline_from_settings(&home);
    remove_cache_directories(&home);
}

fn remove_binary(home: &std::path::Path) {
    #[cfg(not(target_os = "windows"))]
    let candidates = vec![home.join(".local/bin/cship"), home.join(".cargo/bin/cship")];
    #[cfg(target_os = "windows")]
    let candidates = {
        let mut v: Vec<std::path::PathBuf> = Vec::new();
        match std::env::var("LOCALAPPDATA") {
            Ok(local_app_data) => {
                v.push(
                    std::path::Path::new(&local_app_data)
                        .join("Programs")
                        .join("cship")
                        .join("cship.exe"),
                );
            }
            Err(_) => {
                tracing::warn!(
                    "LOCALAPPDATA env var not set; skipping %LOCALAPPDATA%\\Programs\\cship\\cship.exe candidate"
                );
            }
        }
        v.push(home.join(".cargo/bin/cship.exe"));
        v.push(home.join(r".local\bin\cship.exe"));
        v
    };
    for bin in candidates {
        if bin.exists() {
            match std::fs::remove_file(&bin) {
                Ok(()) => println!("Removed: {}", bin.display()),
                Err(e) => println!("Could not remove {}: {e}", bin.display()),
            }
        } else {
            println!("Binary not found at {} — skipping.", bin.display());
        }
    }
}

fn remove_statusline_from_settings(home: &std::path::Path) {
    let path = home.join(".claude/settings.json");

    if !path.exists() {
        println!("settings.json not found at {} — skipping.", path.display());
        return;
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            println!("Could not read {}: {e}", path.display());
            return;
        }
    };
    let mut map: serde_json::Map<String, serde_json::Value> = match serde_json::from_str(&raw) {
        Ok(m) => m,
        Err(e) => {
            println!("Could not parse {}: {e}", path.display());
            return;
        }
    };
    if map.remove("statusLine").is_some() {
        let updated = match serde_json::to_string_pretty(&map) {
            Ok(s) => s,
            Err(e) => {
                println!("Could not serialize {}: {e}", path.display());
                return;
            }
        };
        match std::fs::write(&path, updated + "\n") {
            Ok(()) => println!("Removed \"statusLine\" from {}", path.display()),
            Err(e) => println!("Could not write {}: {e}", path.display()),
        }
    } else {
        println!("\"statusLine\" not found in {} — skipping.", path.display());
    }
}

fn remove_cache_directories(home: &std::path::Path) {
    let projects = home.join(".claude/projects");
    if !projects.exists() {
        println!("No .claude/projects directory found — skipping cache cleanup.");
        return;
    }
    // Walk one level deep: ~/.claude/projects/{hash}/cship/
    let Ok(entries) = std::fs::read_dir(&projects) else {
        println!("Could not read .claude/projects directory — skipping cache cleanup.");
        return;
    };
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let cache_dir = entry.path().join("cship");
        if cache_dir.is_dir() && std::fs::remove_dir_all(&cache_dir).is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        println!(
            "Removed {removed} cship cache director{}.",
            if removed == 1 { "y" } else { "ies" }
        );
    } else {
        println!("No cship cache directories found — skipping.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static HOME_MUTEX: Mutex<()> = Mutex::new(());

    fn with_tempdir<F: FnOnce(&std::path::Path)>(f: F) {
        let _guard = HOME_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        f(dir.path());
    }

    #[test]
    fn test_remove_binary_present() {
        with_tempdir(|home| {
            let bin_name = if cfg!(target_os = "windows") {
                "cship.exe"
            } else {
                "cship"
            };

            let local_bin = home.join(".local/bin");
            std::fs::create_dir_all(&local_bin).unwrap();
            let local_path = local_bin.join(bin_name);
            std::fs::write(&local_path, b"fake binary").unwrap();

            let cargo_bin = home.join(".cargo/bin");
            std::fs::create_dir_all(&cargo_bin).unwrap();
            let cargo_path = cargo_bin.join(bin_name);
            std::fs::write(&cargo_path, b"fake binary").unwrap();

            // On Windows, also test the LOCALAPPDATA candidate path
            #[cfg(target_os = "windows")]
            let (tmp_local, localappdata_bin_path) = {
                let tmp = tempfile::tempdir().unwrap();
                let programs_dir = tmp.path().join("Programs").join("cship");
                std::fs::create_dir_all(&programs_dir).unwrap();
                let bin_path = programs_dir.join("cship.exe");
                std::fs::write(&bin_path, b"fake binary").unwrap();
                // Point LOCALAPPDATA to our temp dir so remove_binary finds it
                // SAFETY: guarded by HOME_MUTEX; no other threads read LOCALAPPDATA concurrently.
                unsafe { std::env::set_var("LOCALAPPDATA", tmp.path()) };
                (tmp, bin_path)
            };

            remove_binary(home);

            assert!(!local_path.exists());
            assert!(!cargo_path.exists());

            #[cfg(target_os = "windows")]
            {
                assert!(
                    !localappdata_bin_path.exists(),
                    "LOCALAPPDATA binary should be removed on Windows"
                );
                // SAFETY: guarded by HOME_MUTEX; no other threads read LOCALAPPDATA concurrently.
                unsafe { std::env::remove_var("LOCALAPPDATA") };
                drop(tmp_local);
            }
        });
    }

    #[test]
    fn test_remove_binary_absent() {
        with_tempdir(|home| {
            // No binary created — should not panic
            remove_binary(home);
        });
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn test_remove_statusline_present() {
        with_tempdir(|home| {
            let claude_dir = home.join(".claude");
            std::fs::create_dir_all(&claude_dir).unwrap();
            let settings_path = claude_dir.join("settings.json");
            std::fs::write(
                &settings_path,
                r#"{"statusLine":{"type":"command","command":"cship"},"otherKey":"value"}"#,
            )
            .unwrap();
            remove_statusline_from_settings(home);
            let content = std::fs::read_to_string(&settings_path).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
            assert!(
                parsed.get("statusLine").is_none(),
                "statusLine key should be removed"
            );
            assert_eq!(
                parsed.get("otherKey").and_then(|v| v.as_str()),
                Some("value"),
                "other keys should be preserved"
            );
        });
    }

    #[test]
    fn test_remove_statusline_absent_key() {
        with_tempdir(|home| {
            let claude_dir = home.join(".claude");
            std::fs::create_dir_all(&claude_dir).unwrap();
            let settings_path = claude_dir.join("settings.json");
            let original = r#"{"otherKey":"value"}"#;
            std::fs::write(&settings_path, original).unwrap();
            remove_statusline_from_settings(home);
            // File should still be parseable and unchanged in content
            let content = std::fs::read_to_string(&settings_path).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
            assert!(parsed.get("statusLine").is_none());
            assert_eq!(
                parsed.get("otherKey").and_then(|v| v.as_str()),
                Some("value")
            );
        });
    }

    #[test]
    fn test_remove_statusline_no_file() {
        with_tempdir(|home| {
            // No settings.json created — should not panic
            remove_statusline_from_settings(home);
        });
    }

    #[test]
    fn test_remove_statusline_malformed_json() {
        with_tempdir(|home| {
            let claude_dir = home.join(".claude");
            std::fs::create_dir_all(&claude_dir).unwrap();
            let settings_path = claude_dir.join("settings.json");
            std::fs::write(&settings_path, b"not valid json {{{").unwrap();
            // Should not panic
            remove_statusline_from_settings(home);
        });
    }

    #[test]
    fn test_remove_cache_dirs_present() {
        with_tempdir(|home| {
            let hash_dir = home.join(".claude/projects/abc123def456");
            let cache_dir = hash_dir.join("cship");
            std::fs::create_dir_all(&cache_dir).unwrap();
            std::fs::write(cache_dir.join("transcript-starship-git_branch"), b"data").unwrap();
            assert!(cache_dir.exists());
            remove_cache_directories(home);
            assert!(!cache_dir.exists(), "cship cache dir should be removed");
        });
    }

    #[test]
    fn test_remove_cache_dirs_absent() {
        with_tempdir(|home| {
            // No .claude/projects directory — should not panic
            remove_cache_directories(home);
        });
    }

    #[test]
    fn test_run_with_empty_home_does_not_panic() {
        let _guard = HOME_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        // SAFETY: guarded by HOME_MUTEX; no other threads read these env vars concurrently.
        unsafe {
            std::env::set_var("HOME", "");
            std::env::set_var("USERPROFILE", "");
            std::env::set_var("CLAUDE_HOME", "");
        };
        // Should print message and return, not panic or touch root paths
        run();
        // Restore to avoid poisoning other tests
        // SAFETY: guarded by HOME_MUTEX; no other threads read CLAUDE_HOME concurrently.
        unsafe { std::env::remove_var("CLAUDE_HOME") };
    }
}
