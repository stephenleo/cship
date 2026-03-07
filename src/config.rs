use serde::Deserialize;

/// Root configuration for CShip, loaded from the `[cship]` section of `starship.toml`.
#[derive(Debug, Deserialize, Default)]
pub struct CshipConfig {
    /// `lines` array — each element is a format string for one statusline row.
    /// Example: `["$cship.model $git_branch", "$cship.cost"]`
    pub lines: Option<Vec<String>>,
    /// Configuration for the `[cship.model]` section.
    pub model: Option<ModelConfig>,
    pub cost: Option<CostConfig>,
    pub context_bar: Option<ContextBarConfig>,
    pub context_window: Option<ContextWindowConfig>,
    pub vim: Option<VimConfig>,
    pub agent: Option<AgentConfig>,
    pub session: Option<SessionConfig>,
    pub workspace: Option<WorkspaceConfig>,
    pub usage_limits: Option<UsageLimitsConfig>,
}

/// Per-module config fields shared by all native CShip modules.
/// These map to `[cship.model]` in `starship.toml`.
#[derive(Debug, Deserialize, Default)]
pub struct ModelConfig {
    pub style: Option<String>,
    pub symbol: Option<String>,
    pub disabled: Option<bool>,
    /// When `true`, prepends the module name as a label.
    pub label: Option<bool>,
    pub format: Option<String>,
}

/// Configuration for `[cship.cost]` — convenience alias for total cost display.
#[derive(Debug, Deserialize, Default)]
pub struct CostConfig {
    pub style: Option<String>,
    pub symbol: Option<String>,
    pub disabled: Option<bool>,
    /// Reserved — not yet rendered; included for config schema consistency with other modules.
    pub label: Option<String>,
    pub warn_threshold: Option<f64>,
    pub warn_style: Option<String>,
    pub critical_threshold: Option<f64>,
    pub critical_style: Option<String>,
    pub format: Option<String>,
    // Sub-field per-display configs (map to [cship.cost.total_cost_usd] etc.)
    pub total_cost_usd: Option<CostSubfieldConfig>,
    pub total_duration_ms: Option<CostSubfieldConfig>,
    pub total_api_duration_ms: Option<CostSubfieldConfig>,
    pub total_lines_added: Option<CostSubfieldConfig>,
    pub total_lines_removed: Option<CostSubfieldConfig>,
}

/// Configuration for individual `[cship.cost.*]` sub-field modules.
#[derive(Debug, Deserialize, Default)]
pub struct CostSubfieldConfig {
    pub style: Option<String>,
    /// Reserved — not yet rendered; included for config schema consistency.
    pub symbol: Option<String>,
    pub disabled: Option<bool>,
    /// Reserved — not yet rendered; included for config schema consistency.
    pub label: Option<String>,
    pub format: Option<String>,
}

/// Configuration for `[cship.context_bar]` — visual progress bar with thresholds.
/// Implemented in Story 2.2. Defined here so all Epic 2 config is available.
#[derive(Debug, Deserialize, Default)]
pub struct ContextBarConfig {
    pub style: Option<String>,
    pub symbol: Option<String>,
    pub disabled: Option<bool>,
    pub label: Option<String>,
    pub warn_threshold: Option<f64>,
    pub warn_style: Option<String>,
    pub critical_threshold: Option<f64>,
    pub critical_style: Option<String>,
    pub width: Option<u32>,
    pub format: Option<String>,
}

/// Configuration for `[cship.context_window]` sub-field modules.
/// Implemented in Story 2.2. Defined here so all Epic 2 config is available.
#[derive(Debug, Deserialize, Default)]
pub struct ContextWindowConfig {
    pub style: Option<String>,
    pub symbol: Option<String>,
    pub disabled: Option<bool>,
    pub label: Option<String>,
    pub format: Option<String>,
}

/// Configuration for `[cship.vim]` — vim mode display.
/// Implemented in Story 2.3. Defined here so all Epic 2 config is available.
#[derive(Debug, Deserialize, Default)]
pub struct VimConfig {
    pub style: Option<String>,
    pub symbol: Option<String>,
    pub disabled: Option<bool>,
    pub label: Option<String>,
    pub normal_style: Option<String>,
    pub insert_style: Option<String>,
    pub format: Option<String>,
}

/// Configuration for `[cship.agent]` — agent name display.
/// Implemented in Story 2.3. Defined here so all Epic 2 config is available.
#[derive(Debug, Deserialize, Default)]
pub struct AgentConfig {
    pub style: Option<String>,
    pub symbol: Option<String>,
    pub disabled: Option<bool>,
    pub label: Option<String>,
    pub format: Option<String>,
}

/// Configuration for session identity modules (cwd, session_id, transcript_path, etc.).
/// Implemented in Story 2.4. Defined here so all Epic 2 config is available.
#[derive(Debug, Deserialize, Default)]
pub struct SessionConfig {
    pub style: Option<String>,
    pub symbol: Option<String>,
    pub disabled: Option<bool>,
    pub label: Option<String>,
    pub format: Option<String>,
}

/// Configuration for workspace modules (workspace.current_dir, workspace.project_dir).
/// Implemented in Story 2.4. Defined here so all Epic 2 config is available.
#[derive(Debug, Deserialize, Default)]
pub struct WorkspaceConfig {
    pub style: Option<String>,
    pub symbol: Option<String>,
    pub disabled: Option<bool>,
    pub label: Option<String>,
    pub format: Option<String>,
}

/// Configuration for `[cship.usage_limits]`.
/// Story 5.1 defines the struct; Stories 5.2 and 5.3 implement the render logic.
#[derive(Debug, Deserialize, Default)]
pub struct UsageLimitsConfig {
    pub disabled: Option<bool>,
    pub warn_threshold: Option<f64>,
    pub warn_style: Option<String>,
    pub critical_threshold: Option<f64>,
    pub critical_style: Option<String>,
    pub format: Option<String>,
}

/// Result of a config load operation — includes the loaded config and its source.
pub struct ConfigLoadResult {
    pub config: CshipConfig,
    pub source: ConfigSource,
}

/// Describes where the config was loaded from.
pub enum ConfigSource {
    ProjectLocal(std::path::PathBuf),
    Global(std::path::PathBuf),
    Override(std::path::PathBuf),
    Default,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigSource::ProjectLocal(p) | ConfigSource::Global(p) | ConfigSource::Override(p) => {
                write!(f, "{}", p.display())
            }
            ConfigSource::Default => write!(f, "(default — no starship.toml found)"),
        }
    }
}

/// Discover and load config, returning both the config and where it was loaded from.
/// Used by `explain.rs` to show the user which config was loaded.
/// Implements the same 4-step discovery chain as `discover_and_load` (AC1).
pub fn load_with_source(
    override_path: Option<&std::path::Path>,
    workspace_dir: Option<&str>,
) -> ConfigLoadResult {
    // Step 1: --config flag override
    if let Some(path) = override_path {
        let config = load_from_path(path).unwrap_or_else(|e| {
            tracing::warn!("cship: failed to load config from {}: {e}", path.display());
            CshipConfig::default()
        });
        return ConfigLoadResult {
            config,
            source: ConfigSource::Override(path.to_path_buf()),
        };
    }

    // Step 2: Walk up from workspace_dir
    if let Some(dir) = workspace_dir {
        let mut current = std::path::Path::new(dir);
        loop {
            let candidate = current.join("starship.toml");
            if candidate.exists() {
                let config = load_from_path(&candidate).unwrap_or_else(|e| {
                    tracing::warn!("cship: failed to load project-local config: {e}");
                    CshipConfig::default()
                });
                return ConfigLoadResult {
                    config,
                    source: ConfigSource::ProjectLocal(candidate),
                };
            }
            match current.parent() {
                Some(parent) => current = parent,
                None => break,
            }
        }
    }

    // Step 3: Global fallback ~/.config/starship.toml
    if let Ok(home) = std::env::var("HOME") {
        let global = std::path::Path::new(&home)
            .join(".config")
            .join("starship.toml");
        if global.exists() {
            let config = load_from_path(&global).unwrap_or_else(|e| {
                tracing::warn!("cship: failed to load global config: {e}");
                CshipConfig::default()
            });
            return ConfigLoadResult {
                config,
                source: ConfigSource::Global(global),
            };
        }
    }

    // Step 4: No config found — use defaults
    tracing::debug!("no starship.toml found; using default CshipConfig");
    ConfigLoadResult {
        config: CshipConfig::default(),
        source: ConfigSource::Default,
    }
}

/// Private wrapper so `toml::from_str` can extract `[cship]` sections
/// from a full `starship.toml` that contains many other sections.
/// Serde silently ignores all non-`cship` top-level keys.
#[derive(Debug, Deserialize, Default)]
struct StarshipToml {
    cship: Option<CshipConfig>,
}

/// Load `CshipConfig` from a file at `path`.
/// Returns an error if the file cannot be read OR if the TOML is malformed.
/// Returns default `CshipConfig` if `[cship]` section is absent (not an error).
fn load_from_path(path: &std::path::Path) -> anyhow::Result<CshipConfig> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("cannot read config file {}: {e}", path.display()))?;
    let wrapper: StarshipToml = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("malformed TOML in {}: {e}", path.display()))?;
    tracing::debug!("loaded config from {}", path.display());
    Ok(wrapper.cship.unwrap_or_default())
}

/// Discover and load `CshipConfig` using the 4-step discovery chain.
///
/// Priority order:
/// 1. If `config_path` is `Some`, load that file directly (bypasses discovery).
/// 2. Walk up the directory tree from `workspace_dir`, return first `starship.toml` found.
/// 3. Fall back to `$HOME/.config/starship.toml`.
/// 4. If nothing found, return `CshipConfig::default()` without error.
///
/// Returns `Err` only if a file is found but fails to parse (malformed TOML or unreadable).
pub fn discover_and_load(
    workspace_dir: Option<&str>,
    config_path: Option<&str>,
) -> anyhow::Result<CshipConfig> {
    // Step 1: explicit override — propagate parse errors (caller handles exit)
    if let Some(path) = config_path {
        return load_from_path(std::path::Path::new(path));
    }
    // Steps 2–4: delegate to load_with_source (workspace walk-up → global → default)
    Ok(load_with_source(None, workspace_dir).config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const VALID_TOML: &str = include_str!("../tests/fixtures/sample_starship.toml");

    #[test]
    fn test_parse_valid_config() {
        let wrapper: StarshipToml = toml::from_str(VALID_TOML).unwrap();
        let cfg = wrapper.cship.unwrap();
        let lines = cfg.lines.as_ref().unwrap();
        assert!(!lines.is_empty(), "lines should be populated");
        let model = cfg.model.as_ref().unwrap();
        assert!(model.style.is_some(), "model.style should be present");
        assert_eq!(model.disabled, Some(false));
    }

    #[test]
    fn test_no_cship_section_returns_default() {
        let toml_without_cship = "[git_branch]\nstyle = \"bold green\"\n";
        let wrapper: StarshipToml = toml::from_str(toml_without_cship).unwrap();
        assert!(wrapper.cship.is_none());
        let cfg = wrapper.cship.unwrap_or_default();
        assert!(cfg.lines.is_none());
        assert!(cfg.model.is_none());
    }

    #[test]
    fn test_malformed_toml_returns_error() {
        let result: Result<StarshipToml, _> = toml::from_str("lines = [unclosed");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_from_nonexistent_path_returns_error() {
        let result = load_from_path(std::path::Path::new("/nonexistent/path/starship.toml"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("cannot read config file"),
            "error message: {msg}"
        );
    }

    #[test]
    fn test_discover_config_override_bypasses_discovery() {
        // Write a temp toml file
        let dir = std::env::temp_dir();
        let path = dir.join("cship_test_override.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "[cship]\nlines = [\"$cship.model\"]").unwrap();

        let cfg = discover_and_load(None, path.to_str()).unwrap();
        assert_eq!(cfg.lines.as_ref().unwrap()[0], "$cship.model");

        std::fs::remove_file(&path).ok();
    }

    // NOTE: "no config found → returns default" is NOT unit-tested here because it
    // depends on HOME env var. If the dev's ~/.config/starship.toml exists, the
    // global fallback fires and the test would fail. This scenario is covered by
    // the integration test `test_no_config_file_found_uses_default_and_exits_zero`
    // which uses a JSON fixture with a non-existent workspace path and does not
    // depend on env vars. Mutating HOME in unit tests is unsafe with parallel
    // test execution (Rust 2024 marks set_var as unsafe for this reason).

    #[test]
    fn test_discover_walks_up_directory_tree() {
        // Create a temp dir hierarchy: /tmp/cship_test_walk/subdir/
        // Put starship.toml in the parent, workspace_dir is subdir
        let parent = std::env::temp_dir().join("cship_test_walk");
        let subdir = parent.join("subdir");
        std::fs::create_dir_all(&subdir).unwrap();
        let toml_path = parent.join("starship.toml");
        let mut f = std::fs::File::create(&toml_path).unwrap();
        writeln!(f, "[cship]\nlines = [\"$cship.model\"]").unwrap();

        let cfg = discover_and_load(subdir.to_str(), None).unwrap();
        assert_eq!(cfg.lines.as_ref().unwrap()[0], "$cship.model");

        // cleanup
        std::fs::remove_dir_all(&parent).ok();
    }
}
