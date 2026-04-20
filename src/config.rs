use serde::Deserialize;

/// Root configuration for CShip, loaded from the `[cship]` section of `starship.toml`.
#[derive(Debug, Deserialize, Default)]
pub struct CshipConfig {
    /// `lines` array — each element is a format string for one statusline row.
    /// Example: `["$cship.model $git_branch", "$cship.cost"]`
    pub lines: Option<Vec<String>>,
    /// Starship-compatible top-level format string. Split on `$line_break` to produce
    /// multiple rows. Takes priority over `lines` when both are set.
    pub format: Option<String>,
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
    pub peak_usage: Option<PeakUsageConfig>,
    pub starship_prompt: Option<StarshipPromptConfig>,
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
    pub total_cost_usd: Option<SubfieldConfig>,
    pub total_duration_ms: Option<SubfieldConfig>,
    pub total_api_duration_ms: Option<SubfieldConfig>,
    pub total_lines_added: Option<SubfieldConfig>,
    pub total_lines_removed: Option<SubfieldConfig>,
}

/// Unified configuration for individual sub-field modules
/// (e.g. `[cship.cost.total_cost_usd]`, `[cship.context_window.used_percentage]`).
#[derive(Debug, Deserialize, Default)]
pub struct SubfieldConfig {
    pub style: Option<String>,
    pub symbol: Option<String>,
    pub disabled: Option<bool>,
    pub warn_threshold: Option<f64>,
    pub warn_style: Option<String>,
    pub critical_threshold: Option<f64>,
    pub critical_style: Option<String>,
    pub format: Option<String>,
    /// When `true`, fires threshold styles when value is BELOW the threshold.
    /// Use for decreasing-health indicators like `remaining_percentage` (low = bad).
    ///
    /// **Threshold resolution when `invert_threshold = true`:**
    /// - `warn_threshold`, `warn_style`, `critical_threshold`, and `critical_style` are resolved
    ///   from **this sub-field config only** — parent [`ContextWindowConfig`] values are NOT
    ///   inherited. Rationale: parent thresholds live in the non-inverted domain (high = bad),
    ///   while this sub-field treats low as bad. Inheriting parent thresholds would invert the
    ///   semantics incorrectly.
    /// - Base `style` **still falls back to the parent** [`ContextWindowConfig`]`.style` when not
    ///   set on the sub-field. The style fallback is domain-independent and safe to inherit.
    pub invert_threshold: Option<bool>,
}

/// Backwards-compatible type aliases (used by test code).
#[cfg(test)]
pub type CostSubfieldConfig = SubfieldConfig;
#[cfg(test)]
pub type ContextWindowSubfieldConfig = SubfieldConfig;

/// Trait for uniform access to style/threshold fields shared by config types.
/// Used by `render_styled_value()` to resolve sub-field → parent fallback.
///
/// `format_str()` and `symbol_str()` default to `None`. Only parent configs
/// whose sub-fields should inherit format/symbol (i.e., `ContextWindowConfig`)
/// override these.
pub trait HasThresholdStyle {
    fn style(&self) -> Option<&str>;
    fn warn_threshold(&self) -> Option<f64>;
    fn warn_style(&self) -> Option<&str>;
    fn critical_threshold(&self) -> Option<f64>;
    fn critical_style(&self) -> Option<&str>;
    fn format_str(&self) -> Option<&str> {
        None
    }
    fn symbol_str(&self) -> Option<&str> {
        None
    }
}

#[allow(unused_macros)]
macro_rules! impl_has_threshold_style {
    ($t:ty) => {
        impl HasThresholdStyle for $t {
            fn style(&self) -> Option<&str> {
                self.style.as_deref()
            }
            fn warn_threshold(&self) -> Option<f64> {
                self.warn_threshold
            }
            fn warn_style(&self) -> Option<&str> {
                self.warn_style.as_deref()
            }
            fn critical_threshold(&self) -> Option<f64> {
                self.critical_threshold
            }
            fn critical_style(&self) -> Option<&str> {
                self.critical_style.as_deref()
            }
        }
    };
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
    /// Style applied when rendering the bar at 0% due to absent context data.
    /// Example: `"dim"` to visually distinguish the empty state.
    pub empty_style: Option<String>,
}

/// Configuration for `[cship.context_window]` sub-field modules.
/// Implemented in Story 2.2. Defined here so all Epic 2 config is available.
#[derive(Debug, Deserialize, Default)]
pub struct ContextWindowConfig {
    pub style: Option<String>,
    pub symbol: Option<String>,
    pub disabled: Option<bool>,
    pub label: Option<String>,
    pub warn_threshold: Option<f64>,
    pub warn_style: Option<String>,
    pub critical_threshold: Option<f64>,
    pub critical_style: Option<String>,
    pub format: Option<String>,
    // Per-sub-field configs (map to [cship.context_window.used_percentage] etc.)
    pub used_percentage: Option<SubfieldConfig>,
    pub remaining_percentage: Option<SubfieldConfig>,
    pub size: Option<SubfieldConfig>,
    pub total_input_tokens: Option<SubfieldConfig>,
    pub total_output_tokens: Option<SubfieldConfig>,
    pub current_usage_input_tokens: Option<SubfieldConfig>,
    pub current_usage_output_tokens: Option<SubfieldConfig>,
    pub current_usage_cache_creation_input_tokens: Option<SubfieldConfig>,
    pub current_usage_cache_read_input_tokens: Option<SubfieldConfig>,
    pub used_tokens: Option<SubfieldConfig>,
}

impl HasThresholdStyle for ContextWindowConfig {
    fn style(&self) -> Option<&str> {
        self.style.as_deref()
    }
    fn warn_threshold(&self) -> Option<f64> {
        self.warn_threshold
    }
    fn warn_style(&self) -> Option<&str> {
        self.warn_style.as_deref()
    }
    fn critical_threshold(&self) -> Option<f64> {
        self.critical_threshold
    }
    fn critical_style(&self) -> Option<&str> {
        self.critical_style.as_deref()
    }
    fn format_str(&self) -> Option<&str> {
        self.format.as_deref()
    }
    fn symbol_str(&self) -> Option<&str> {
        self.symbol.as_deref()
    }
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
    pub style: Option<String>,
    pub warn_threshold: Option<f64>,
    pub warn_style: Option<String>,
    pub critical_threshold: Option<f64>,
    pub critical_style: Option<String>,
    /// Cache refresh interval in seconds. Default: 60.
    /// Increase to reduce API pressure with many concurrent sessions.
    pub ttl: Option<u64>,
    /// Reserved — not yet rendered. Use `five_hour_format`, `seven_day_format`,
    /// and `separator` for per-section format control.
    pub format: Option<String>,
    pub five_hour_format: Option<String>,
    pub seven_day_format: Option<String>,
    pub separator: Option<String>,
    /// Format string for extra usage display. Shown when extra_usage.is_enabled is true.
    /// Placeholders: {pct}, {used}, {limit}, {remaining}
    /// Default: "extra: {pct}% (${used}/${limit})"
    pub extra_usage_format: Option<String>,
    /// Format string for 7-day Opus breakdown. Shown when API returns non-null data.
    /// Placeholders: {pct}, {reset}, {remaining}
    /// Default: "opus {pct}%"
    pub opus_format: Option<String>,
    /// Format string for 7-day Sonnet breakdown.
    /// Placeholders: {pct}, {reset}, {remaining}
    /// Default: "sonnet {pct}%"
    pub sonnet_format: Option<String>,
    /// Format string for 7-day Cowork breakdown.
    /// Placeholders: {pct}, {reset}, {remaining}
    /// Default: "cowork {pct}%"
    pub cowork_format: Option<String>,
    /// Format string for 7-day OAuth apps breakdown.
    /// Placeholders: {pct}, {reset}, {remaining}
    /// Default: "oauth {pct}%"
    pub oauth_apps_format: Option<String>,
}

/// Configuration for `[cship.peak_usage]` — peak-time indicator.
/// Shows when Anthropic's peak-time rate limiting is likely active
/// based on current time relative to US Pacific business hours.
#[derive(Debug, Deserialize, Default)]
pub struct PeakUsageConfig {
    pub disabled: Option<bool>,
    pub symbol: Option<String>,
    pub style: Option<String>,
    pub format: Option<String>,
    /// Start of peak window in US Pacific time (0–23). Default: 7.
    pub start_hour: Option<u32>,
    /// End of peak window in US Pacific time, exclusive (0–24). Default: 17.
    /// Use 24 to mean "through end of day" (e.g., `start_hour = 0, end_hour = 24` = all day).
    pub end_hour: Option<u32>,
}

/// Configuration for `[cship.starship_prompt]` — renders full starship prompt as a token.
#[derive(Debug, Deserialize, Default)]
pub struct StarshipPromptConfig {
    pub disabled: Option<bool>,
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
    /// Config loaded from a dedicated `cship.toml` file.
    /// Supports both canonical `[cship]` section format and legacy wrapper-free format.
    DedicatedFile(std::path::PathBuf),
    Default,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigSource::ProjectLocal(p) | ConfigSource::Global(p) | ConfigSource::Override(p) => {
                write!(f, "{}", p.display())
            }
            ConfigSource::DedicatedFile(p) => write!(f, "{} (cship.toml)", p.display()),
            ConfigSource::Default => write!(f, "(default — no config file found)"),
        }
    }
}

/// Discover and load config, returning both the config and where it was loaded from.
/// Used by `explain.rs` to show the user which config was loaded.
/// Implements the same discovery chain as `discover_and_load` (AC1).
pub fn load_with_source(
    override_path: Option<&std::path::Path>,
    workspace_dir: Option<&str>,
) -> ConfigLoadResult {
    // Step 1: --config flag override
    if let Some(path) = override_path {
        let config = load_override(path).unwrap_or_else(|e| {
            tracing::warn!("cship: failed to load config from {}: {e}", path.display());
            CshipConfig::default()
        });
        return ConfigLoadResult {
            config,
            source: if path.file_name().map(|n| n == "cship.toml").unwrap_or(false) {
                ConfigSource::DedicatedFile(path.to_path_buf())
            } else {
                ConfigSource::Override(path.to_path_buf())
            },
        };
    }

    // Step 2: Walk up from workspace_dir — check cship.toml before starship.toml
    if let Some(dir) = workspace_dir {
        let mut current = std::path::Path::new(dir);
        loop {
            let cship_candidate = current.join("cship.toml");
            if cship_candidate.exists() {
                let config = load_cship_toml(&cship_candidate).unwrap_or_else(|e| {
                    tracing::warn!("cship: failed to load dedicated config: {e}");
                    CshipConfig::default()
                });
                return ConfigLoadResult {
                    config,
                    source: ConfigSource::DedicatedFile(cship_candidate),
                };
            }
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

    // Step 3: Global fallback — check ~/.config/cship.toml before ~/.config/starship.toml
    if let Some(home) = crate::platform::home_dir() {
        let cship_global = home.join(".config").join("cship.toml");
        if cship_global.exists() {
            let config = load_cship_toml(&cship_global).unwrap_or_else(|e| {
                tracing::warn!("cship: failed to load global dedicated config: {e}");
                CshipConfig::default()
            });
            return ConfigLoadResult {
                config,
                source: ConfigSource::DedicatedFile(cship_global),
            };
        }
        let global = home.join(".config").join("starship.toml");
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
    tracing::debug!("no config file found; using default CshipConfig");
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

/// Load `CshipConfig` from a `starship.toml`-format file at `path`.
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

/// Load `CshipConfig` from a dedicated `cship.toml` file at `path`.
///
/// The canonical format uses a `[cship]` section header (parsed via `StarshipToml` wrapper).
/// Files without a `[cship]` header are treated as legacy wrapper-free format and parsed
/// directly as `CshipConfig` for backwards compatibility.
fn load_cship_toml(path: &std::path::Path) -> anyhow::Result<CshipConfig> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("cannot read config file {}: {e}", path.display()))?;
    let value: toml::Value = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("malformed TOML in {}: {e}", path.display()))?;
    if value.get("cship").is_some() {
        tracing::debug!("loading cship.toml with [cship] section via wrapper");
        let wrapper: StarshipToml = toml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("malformed TOML in {}: {e}", path.display()))?;
        tracing::debug!("loaded config from {} (via wrapper)", path.display());
        return Ok(wrapper.cship.unwrap_or_default());
    }
    tracing::debug!("loading cship.toml in legacy wrapper-free format");
    let config: CshipConfig = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("malformed TOML in {}: {e}", path.display()))?;
    tracing::debug!("loaded config from {}", path.display());
    Ok(config)
}

/// Load `CshipConfig` from an explicit `--config` override path.
/// Routes to `load_cship_toml` if the filename is `cship.toml`, otherwise
/// uses the `StarshipToml` wrapper (standard starship.toml format).
fn load_override(path: &std::path::Path) -> anyhow::Result<CshipConfig> {
    if path.file_name().map(|n| n == "cship.toml").unwrap_or(false) {
        load_cship_toml(path)
    } else {
        load_from_path(path)
    }
}

/// Discover and load `CshipConfig` using the discovery chain.
///
/// Priority order:
/// 1. If `config_path` is `Some`, load that file directly (bypasses discovery).
/// 2. Walk up the directory tree from `workspace_dir`, checking `cship.toml` then `starship.toml`.
/// 3. Fall back to `$HOME/.config/cship.toml`, then `$HOME/.config/starship.toml`.
/// 4. If nothing found, return `CshipConfig::default()` without error.
///
/// Returns `Err` only if a file is found but fails to parse (malformed TOML or unreadable).
pub fn discover_and_load(
    workspace_dir: Option<&str>,
    config_path: Option<&str>,
) -> anyhow::Result<CshipConfig> {
    // Step 1: explicit override — propagate parse errors (caller handles exit)
    if let Some(path) = config_path {
        return load_override(std::path::Path::new(path));
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

    // --- Task 6: cship.toml discovery tests ---
    //
    // NOTE: AC#3 (~/.config/cship.toml global fallback) is not directly unit-tested.
    // The global path uses the same `load_cship_toml` function tested below, and
    // mutating HOME in parallel tests is unsafe (Rust 2024). The workspace walk-up
    // path (AC#4) exercises the same discovery+load logic.  AC#3 correctness is
    // verified by code review of `load_with_source` Step 3.

    #[test]
    fn test_cship_toml_takes_priority_over_starship_toml() {
        // AC#1: cship.toml in workspace dir is loaded instead of starship.toml
        let dir = tempfile::tempdir().unwrap();

        let cship_path = dir.path().join("cship.toml");
        let mut f = std::fs::File::create(&cship_path).unwrap();
        writeln!(f, "lines = [\"$cship.cost\"]").unwrap();

        let starship_path = dir.path().join("starship.toml");
        let mut g = std::fs::File::create(&starship_path).unwrap();
        writeln!(g, "[cship]\nlines = [\"$cship.model\"]").unwrap();

        let cfg = discover_and_load(dir.path().to_str(), None).unwrap();
        // Should load from cship.toml (cost), not starship.toml (model)
        assert_eq!(cfg.lines.as_ref().unwrap()[0], "$cship.cost");
    }

    #[test]
    fn test_cship_toml_absent_falls_through_to_starship_toml() {
        // AC#2: without cship.toml, existing starship.toml discovery is unchanged
        let dir = tempfile::tempdir().unwrap();

        let starship_path = dir.path().join("starship.toml");
        let mut f = std::fs::File::create(&starship_path).unwrap();
        writeln!(f, "[cship]\nlines = [\"$cship.model\"]").unwrap();

        let cfg = discover_and_load(dir.path().to_str(), None).unwrap();
        assert_eq!(cfg.lines.as_ref().unwrap()[0], "$cship.model");
    }

    #[test]
    fn test_cship_toml_walk_up_from_subdirectory() {
        // AC#4: cship.toml in parent directory is discovered via walk-up
        let parent = tempfile::tempdir().unwrap();
        let subdir = parent.path().join("child");
        std::fs::create_dir_all(&subdir).unwrap();

        let cship_path = parent.path().join("cship.toml");
        let mut f = std::fs::File::create(&cship_path).unwrap();
        writeln!(f, "lines = [\"$cship.workspace\"]").unwrap();

        let cfg = discover_and_load(subdir.to_str(), None).unwrap();
        assert_eq!(cfg.lines.as_ref().unwrap()[0], "$cship.workspace");
    }

    #[test]
    fn test_cship_toml_with_wrapper_section_still_parses() {
        // AC#5: canonical [cship] format is parsed correctly via StarshipToml wrapper
        let dir = tempfile::tempdir().unwrap();

        let cship_path = dir.path().join("cship.toml");
        let mut f = std::fs::File::create(&cship_path).unwrap();
        writeln!(f, "[cship]\nlines = [\"$cship.agent\"]").unwrap();

        let cfg = load_cship_toml(&cship_path).unwrap();
        // [cship] is the canonical format — config is extracted correctly
        assert_eq!(cfg.lines.as_ref().unwrap()[0], "$cship.agent");
    }

    #[test]
    fn test_cship_toml_canonical_format_loads_correctly() {
        // AC#5: cship.toml with [cship] header uses the primary (debug) path
        let dir = tempfile::tempdir().unwrap();

        let cship_path = dir.path().join("cship.toml");
        let mut f = std::fs::File::create(&cship_path).unwrap();
        writeln!(
            f,
            "[cship]\nlines = [\"$cship.model $cship.cost $cship.context_bar\"]"
        )
        .unwrap();

        // The warn path has been removed; canonical [cship] format loads successfully
        let cfg = load_cship_toml(&cship_path).unwrap();
        assert_eq!(
            cfg.lines.as_ref().unwrap()[0],
            "$cship.model $cship.cost $cship.context_bar"
        );
    }

    #[test]
    fn test_cship_toml_direct_cshipconfig_parsing() {
        // Legacy wrapper-free format (backwards compatible) — no [cship] section header
        let dir = tempfile::tempdir().unwrap();

        let cship_path = dir.path().join("cship.toml");
        let mut f = std::fs::File::create(&cship_path).unwrap();
        writeln!(
            f,
            "lines = [\"$cship.model\"]\n\n[model]\nstyle = \"bold green\""
        )
        .unwrap();

        let cfg = load_cship_toml(&cship_path).unwrap();
        assert_eq!(cfg.lines.as_ref().unwrap()[0], "$cship.model");
        assert_eq!(
            cfg.model.as_ref().unwrap().style.as_deref(),
            Some("bold green")
        );
    }

    #[test]
    fn test_dedicated_file_config_source_display() {
        // AC#6: DedicatedFile variant displays path with (cship.toml) annotation
        let path = std::path::PathBuf::from("/home/user/.config/cship.toml");
        let source = ConfigSource::DedicatedFile(path);
        let display = source.to_string();
        assert!(display.contains("cship.toml"), "display: {display}");
        assert!(
            display.contains("(cship.toml)"),
            "should have annotation: {display}"
        );
    }

    #[test]
    fn test_override_cship_toml_routes_correctly() {
        // AC#5: --config override pointing to cship.toml parses directly as CshipConfig
        let dir = tempfile::tempdir().unwrap();

        let cship_path = dir.path().join("cship.toml");
        let mut f = std::fs::File::create(&cship_path).unwrap();
        writeln!(f, "lines = [\"$cship.session\"]").unwrap();

        let cfg = discover_and_load(None, cship_path.to_str()).unwrap();
        assert_eq!(cfg.lines.as_ref().unwrap()[0], "$cship.session");
    }

    #[test]
    fn test_load_with_source_returns_dedicated_file_variant() {
        // M1: Verify discovery flow returns ConfigSource::DedicatedFile for cship.toml
        let dir = tempfile::tempdir().unwrap();

        let cship_path = dir.path().join("cship.toml");
        let mut f = std::fs::File::create(&cship_path).unwrap();
        writeln!(f, "lines = [\"$cship.cost\"]").unwrap();

        let result = load_with_source(None, dir.path().to_str());
        assert!(
            matches!(result.source, ConfigSource::DedicatedFile(_)),
            "expected DedicatedFile source, got: {}",
            result.source
        );
        assert_eq!(result.config.lines.as_ref().unwrap()[0], "$cship.cost");
    }

    #[test]
    fn test_load_with_source_override_cship_toml_returns_dedicated_file_variant() {
        // Regression: --config override pointing to cship.toml must return DedicatedFile, not Override
        let dir = tempfile::tempdir().unwrap();

        let cship_path = dir.path().join("cship.toml");
        let mut f = std::fs::File::create(&cship_path).unwrap();
        writeln!(f, "lines = [\"$cship.cost\"]").unwrap();

        let result = load_with_source(Some(&cship_path), None);
        assert!(
            matches!(result.source, ConfigSource::DedicatedFile(_)),
            "expected DedicatedFile source for cship.toml override, got: {}",
            result.source
        );
        assert_eq!(result.config.lines.as_ref().unwrap()[0], "$cship.cost");
    }
}
