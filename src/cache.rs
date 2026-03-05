//! File-based cache for Starship passthrough module results.
//!
//! Cache path: {dirname(transcript_path)}/cship/{transcript_stem}-starship-{module_name}
//! TTL: 5 seconds (compared against file mtime — no metadata file needed).
//! Format: raw UTF-8 text (exact `starship module` stdout output, trimmed for storage).
//!
//! Story 4.2: passthrough cache only.
//! TODO: Story 5.2 adds `read_usage_limits`/`write_usage_limits`.

use std::path::Path;
use std::time::{Duration, SystemTime};

const PASSTHROUGH_TTL: Duration = Duration::from_secs(5);

/// Derive the cache file path for a passthrough module.
/// Sanitizes `module_name` by replacing `/` and space with `_`.
fn passthrough_cache_path(module_name: &str, transcript_path: &Path) -> Option<std::path::PathBuf> {
    let dir = transcript_path.parent()?;
    let stem = transcript_path.file_stem()?.to_str()?;
    let safe_name = module_name.replace(['/', ' '], "_");
    Some(
        dir.join("cship")
            .join(format!("{stem}-starship-{safe_name}")),
    )
}

/// Read a cached passthrough value if it exists and is < 5 seconds old.
/// Returns None on cache miss, stale entry, or any I/O error.
pub fn read_passthrough(module_name: &str, transcript_path: &Path) -> Option<String> {
    let path = passthrough_cache_path(module_name, transcript_path)?;
    let metadata = std::fs::metadata(&path).ok()?;
    let modified = metadata.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    if age >= PASSTHROUGH_TTL {
        return None; // stale
    }
    std::fs::read_to_string(&path).ok()
}

/// Write a passthrough value to the cache file, creating the cache directory if needed.
/// Silently no-ops on any I/O error — cache write failure must never surface to the user.
pub fn write_passthrough(module_name: &str, transcript_path: &Path, content: &str) {
    if let Some(path) = passthrough_cache_path(module_name, transcript_path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_transcript(subdir: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join(subdir).join("test_transcript.jsonl");
        (dir, transcript)
    }

    #[test]
    fn test_cache_miss_returns_none_for_nonexistent_file() {
        let (_dir, transcript) = temp_transcript("session1");
        let result = read_passthrough("git_branch", &transcript);
        assert!(result.is_none());
    }

    #[test]
    fn test_cache_hit_returns_content_within_ttl() {
        let (dir, transcript) = temp_transcript("session2");
        // Write directly to the expected cache path
        write_passthrough("git_branch", &transcript, "main");
        // Immediately read back — should be within TTL
        let result = read_passthrough("git_branch", &transcript);
        assert_eq!(result, Some("main".to_string()));
        drop(dir);
    }

    #[test]
    fn test_write_creates_directory_if_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        // transcript in a subdir that doesn't exist yet
        let transcript = dir
            .path()
            .join("deep")
            .join("nested")
            .join("transcript.jsonl");
        write_passthrough("directory", &transcript, "/home/user");
        // Verify the cache file was created
        let cache_file = dir
            .path()
            .join("deep")
            .join("nested")
            .join("cship")
            .join("transcript-starship-directory");
        assert!(cache_file.exists(), "cache file should have been created");
        let content = std::fs::read_to_string(&cache_file).unwrap();
        assert_eq!(content, "/home/user");
    }

    #[test]
    fn test_path_derivation() {
        // Verify the derived path matches the expected scheme
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        write_passthrough("git_branch", &transcript, "main");
        let expected = dir
            .path()
            .join("cship")
            .join("transcript-starship-git_branch");
        assert!(
            expected.exists(),
            "cache file at expected path: {expected:?}"
        );
    }

    #[test]
    fn test_module_name_sanitization() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        // Module name with slash and space
        write_passthrough("node/js lang", &transcript, "v20");
        let expected = dir
            .path()
            .join("cship")
            .join("transcript-starship-node_js_lang");
        assert!(
            expected.exists(),
            "sanitized path should exist: {expected:?}"
        );
        let content = std::fs::read_to_string(&expected).unwrap();
        assert_eq!(content, "v20");
    }

    #[test]
    fn test_stale_cache_returns_none() {
        use std::time::{Duration, SystemTime};

        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        write_passthrough("git_branch", &transcript, "main");

        // Manually set the file mtime to 10 seconds in the past
        let cache_file = dir
            .path()
            .join("cship")
            .join("transcript-starship-git_branch");
        let stale_time = SystemTime::now() - Duration::from_secs(10);
        filetime::set_file_mtime(
            &cache_file,
            filetime::FileTime::from_system_time(stale_time),
        )
        .expect("set mtime");

        let result = read_passthrough("git_branch", &transcript);
        assert!(result.is_none(), "stale cache should return None");
    }
}
