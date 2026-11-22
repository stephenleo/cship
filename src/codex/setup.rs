//! `cship codex-setup`: register cship as Codex's status line provider.
//!
//! Merges three settings into Codex's `config.toml` (`$CODEX_HOME/config.toml`,
//! default `~/.codex/config.toml`), touching only keys that are not set yet:
//!
//! ```toml
//! [mcp_servers.cship]
//! command = "cship"
//! args = ["codex-mcp"]
//!
//! [features]
//! tui_extensions_statusline = true
//!
//! [tui]
//! status_line_provider = { server = "cship", tool = "statusline" }
//! ```
//!
//! Writing the file directly (rather than relying on Codex's `/experimental`
//! menu) is the setup that persists reliably. Editing goes through `toml_edit`,
//! so the user's comments, ordering and formatting survive, existing tables are
//! extended rather than duplicated, and inline tables or dotted keys are
//! handled. A user's existing values, including a different
//! `status_line_provider`, are never overwritten. The original file is backed
//! up before the first change. The installers call this after detecting Codex.

use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, InlineTable, Item, Table, TableLike, value};

const SERVER_NAME: &str = "cship";
const TOOL_NAME: &str = "statusline";
const FEATURE_KEY: &str = "tui_extensions_statusline";

/// What `codex-setup` did, for the summary it prints.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Report {
    pub(crate) config_path: PathBuf,
    /// Settings added, as `table.key` strings.
    pub(crate) added: Vec<String>,
    /// Settings left alone because they already had a value, with a note.
    pub(crate) kept: Vec<String>,
    /// Backup of the original file, when one was written.
    pub(crate) backup: Option<PathBuf>,
}

/// Run the subcommand: merge the settings and print what changed. Returns the
/// process exit code (`0` on success, `1` when the file could not be updated).
pub fn run(codex_home: Option<&Path>) -> i32 {
    let Some(dir) = codex_home
        .map(Path::to_path_buf)
        .or_else(default_codex_home)
    else {
        println!(
            "cship codex-setup: cannot locate the Codex home directory; pass --codex-home or set CODEX_HOME."
        );
        return 1;
    };
    match setup(&dir, unix_now()) {
        Ok(report) => {
            print_report(&report);
            0
        }
        Err(e) => {
            println!("cship codex-setup: {e}");
            println!("Add the settings by hand instead: https://cship.dev/codex");
            1
        }
    }
}

/// `$CODEX_HOME`, else `~/.codex`, matching Codex's own lookup.
fn default_codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::platform::home_dir().map(|h| h.join(".codex")))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn print_report(report: &Report) {
    let path = report.config_path.display();
    if report.added.is_empty() {
        println!("Codex is already set up for cship in {path}; nothing changed.");
    } else {
        println!("Updated {path}:");
        for key in &report.added {
            println!("  + {key}");
        }
    }
    for note in &report.kept {
        println!("  = {note}");
    }
    if let Some(backup) = &report.backup {
        println!("Backup of the previous file: {}", backup.display());
    }
    if !report.added.is_empty() {
        println!("Restart Codex for the cship status line to take effect.");
    }
}

/// Merge the settings into `<codex_home>/config.toml`. `now` names the backup.
pub(crate) fn setup(codex_home: &Path, now: u64) -> Result<Report, String> {
    let config_path = codex_home.join("config.toml");
    let original = match std::fs::read_to_string(&config_path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("cannot read {}: {e}", config_path.display())),
    };

    let mut doc: DocumentMut = original.as_deref().unwrap_or("").parse().map_err(|e| {
        format!(
            "{} is not valid TOML, so it was left unchanged: {e}",
            config_path.display()
        )
    })?;

    let mut report = Report {
        config_path: config_path.clone(),
        ..Default::default()
    };
    merge(&mut doc, &mut report)?;

    if report.added.is_empty() {
        return Ok(report);
    }

    std::fs::create_dir_all(codex_home)
        .map_err(|e| format!("cannot create {}: {e}", codex_home.display()))?;
    if let Some(text) = &original {
        let backup = codex_home.join(format!("config.toml.cship-backup-{now}"));
        std::fs::write(&backup, text)
            .map_err(|e| format!("cannot write backup {}: {e}", backup.display()))?;
        report.backup = Some(backup);
    }
    // Write to a sibling temp file and rename, so a failure never leaves a
    // half-written config behind.
    let tmp = codex_home.join("config.toml.cship-tmp");
    std::fs::write(&tmp, doc.to_string())
        .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &config_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot replace {}: {e}", config_path.display())
    })?;
    Ok(report)
}

/// Add each missing setting to `doc`, recording what happened in `report`.
fn merge(doc: &mut DocumentMut, report: &mut Report) -> Result<(), String> {
    // [mcp_servers.cship]
    let servers = table_like_mut(doc.as_table_mut(), "mcp_servers")?;
    if servers.contains_key(SERVER_NAME) {
        report
            .kept
            .push(format!("mcp_servers.{SERVER_NAME} (already defined)"));
    } else {
        let mut server = Table::new();
        server.insert("command", value("cship"));
        let mut args = toml_edit::Array::new();
        args.push("codex-mcp");
        server.insert("args", value(args));
        servers.insert(SERVER_NAME, Item::Table(server));
        report.added.push(format!("mcp_servers.{SERVER_NAME}"));
    }

    // [features] tui_extensions_statusline = true
    let features = table_like_mut(doc.as_table_mut(), "features")?;
    match features.get(FEATURE_KEY).and_then(Item::as_bool) {
        None if !features.contains_key(FEATURE_KEY) => {
            features.insert(FEATURE_KEY, value(true));
            report.added.push(format!("features.{FEATURE_KEY}"));
        }
        Some(true) => report.kept.push(format!("features.{FEATURE_KEY} (already true)")),
        _ => report.kept.push(format!(
            "features.{FEATURE_KEY} (already set but not true; enable \"Custom status line\" in Codex's /experimental)"
        )),
    }

    // [tui] status_line_provider = { server = "cship", tool = "statusline" }
    let tui = table_like_mut(doc.as_table_mut(), "tui")?;
    if let Some(existing) = tui.get("status_line_provider") {
        let ours = existing
            .as_table_like()
            .and_then(|t| t.get("server"))
            .and_then(Item::as_str)
            == Some(SERVER_NAME);
        report.kept.push(if ours {
            "tui.status_line_provider (already cship)".to_string()
        } else {
            "tui.status_line_provider (another provider is selected; pick cship in Codex's /statusline to switch)".to_string()
        });
    } else {
        let mut provider = InlineTable::new();
        provider.insert("server", SERVER_NAME.into());
        provider.insert("tool", TOOL_NAME.into());
        tui.insert("status_line_provider", value(provider));
        report.added.push("tui.status_line_provider".to_string());
    }
    Ok(())
}

/// Get `key` under `parent` as an editable table, creating an implicit table
/// (so no empty `[key]` header is written) when it is missing. A key holding a
/// non-table value is an error: the user's value is never replaced.
fn table_like_mut<'a>(parent: &'a mut Table, key: &str) -> Result<&'a mut dyn TableLike, String> {
    if !parent.contains_key(key) {
        let mut table = Table::new();
        table.set_implicit(true);
        parent.insert(key, Item::Table(table));
    }
    parent
        .get_mut(key)
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| format!("`{key}` in config.toml is not a table, so it was left unchanged"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_on(original: Option<&str>) -> (Report, String, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        if let Some(text) = original {
            std::fs::write(dir.path().join("config.toml"), text).unwrap();
        }
        let report = setup(dir.path(), 1_700_000_000).unwrap();
        let out = std::fs::read_to_string(dir.path().join("config.toml")).unwrap_or_default();
        (report, out, dir)
    }

    fn parsed(text: &str) -> toml::Table {
        toml::from_str(text).expect("result must be valid TOML")
    }

    #[test]
    fn test_creates_config_when_missing() {
        let (report, out, dir) = run_on(None);
        let t = parsed(&out);
        assert_eq!(t["mcp_servers"]["cship"]["command"].as_str(), Some("cship"));
        assert_eq!(
            t["mcp_servers"]["cship"]["args"].as_array().unwrap()[0].as_str(),
            Some("codex-mcp")
        );
        assert_eq!(
            t["features"]["tui_extensions_statusline"].as_bool(),
            Some(true)
        );
        assert_eq!(
            t["tui"]["status_line_provider"]["server"].as_str(),
            Some("cship")
        );
        assert_eq!(
            t["tui"]["status_line_provider"]["tool"].as_str(),
            Some("statusline")
        );
        assert_eq!(report.added.len(), 3);
        assert!(report.backup.is_none(), "nothing to back up");
        assert!(!dir.path().join("config.toml.cship-tmp").exists());
    }

    #[test]
    fn test_extends_existing_tables_without_duplicate_headers() {
        let original = "# my codex config\nmodel = \"gpt-5.5\"\n\n[features]\nweb_search = true\n\n[tui]\nanimations = false # keep me\n\n[mcp_servers.other]\ncommand = \"other\"\n";
        let (_, out, _) = run_on(Some(original));
        assert_eq!(out.matches("[features]").count(), 1, "{out}");
        assert_eq!(out.matches("[tui]").count(), 1, "{out}");
        assert!(out.contains("# my codex config"), "comments kept: {out}");
        assert!(out.contains("animations = false # keep me"), "{out}");
        let t = parsed(&out);
        assert_eq!(t["features"]["web_search"].as_bool(), Some(true));
        assert_eq!(
            t["features"]["tui_extensions_statusline"].as_bool(),
            Some(true)
        );
        assert_eq!(t["mcp_servers"]["other"]["command"].as_str(), Some("other"));
        assert_eq!(t["mcp_servers"]["cship"]["command"].as_str(), Some("cship"));
        assert_eq!(t["model"].as_str(), Some("gpt-5.5"));
    }

    #[test]
    fn test_never_overwrites_existing_values() {
        let original = "[features]\ntui_extensions_statusline = false\n\n[tui]\nstatus_line_provider = { server = \"work\", tool = \"usage\" }\n\n[mcp_servers.cship]\ncommand = \"/opt/cship\"\nargs = [\"--config\", \"/x.toml\", \"codex-mcp\"]\n";
        let (report, out, dir) = run_on(Some(original));
        assert!(report.added.is_empty());
        assert_eq!(out, original, "file untouched");
        assert!(report.backup.is_none(), "no backup when nothing changes");
        assert_eq!(report.kept.len(), 3);
        assert!(report.kept.iter().any(|k| k.contains("another provider")));
        assert!(report.kept.iter().any(|k| k.contains("not true")));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn test_is_idempotent() {
        let (_, first, dir) = run_on(Some("model = \"x\"\n"));
        let report = setup(dir.path(), 1_700_000_001).unwrap();
        assert!(report.added.is_empty());
        let second = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert_eq!(first, second);
        assert!(report.kept.iter().any(|k| k.contains("already cship")));
    }

    #[test]
    fn test_backs_up_original_before_changing() {
        let original = "model = \"x\"\n";
        let (report, _, dir) = run_on(Some(original));
        let backup = report.backup.unwrap();
        assert_eq!(
            backup,
            dir.path().join("config.toml.cship-backup-1700000000")
        );
        assert_eq!(std::fs::read_to_string(backup).unwrap(), original);
    }

    #[test]
    fn test_handles_inline_and_dotted_tables() {
        let original = "features = { web_search = true }\ntui.animations = false\n";
        let (_, out, _) = run_on(Some(original));
        let t = parsed(&out);
        assert_eq!(t["features"]["web_search"].as_bool(), Some(true));
        assert_eq!(
            t["features"]["tui_extensions_statusline"].as_bool(),
            Some(true)
        );
        assert_eq!(t["tui"]["animations"].as_bool(), Some(false));
        assert_eq!(
            t["tui"]["status_line_provider"]["server"].as_str(),
            Some("cship")
        );
    }

    #[test]
    fn test_invalid_toml_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[features\nbroken").unwrap();
        let err = setup(dir.path(), 0).unwrap_err();
        assert!(err.contains("not valid TOML"), "{err}");
        assert_eq!(std::fs::read_to_string(path).unwrap(), "[features\nbroken");
    }

    #[test]
    fn test_non_table_key_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "tui = \"compact\"\n").unwrap();
        let err = setup(dir.path(), 0).unwrap_err();
        assert!(err.contains("`tui`"), "{err}");
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            "tui = \"compact\"\n"
        );
    }

    #[test]
    fn test_creates_codex_home_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("nested").join(".codex");
        let report = setup(&home, 0).unwrap();
        assert_eq!(report.added.len(), 3);
        assert!(home.join("config.toml").exists());
    }
}
