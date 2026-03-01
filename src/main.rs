use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "cship", about = "Claude Code statusline renderer")]
struct Cli {
    /// Path to starship.toml config file. Bypasses automatic discovery.
    #[arg(long, value_name = "PATH")]
    config: Option<String>,
}

fn main() {
    // Initialize tracing subscriber — stderr ONLY.
    // Must be called before any tracing:: macro. Respects RUST_LOG env var.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Parse CLI args — must happen before any fallible operations.
    let cli = Cli::parse();

    let ctx = match cship::context::from_stdin() {
        Ok(ctx) => ctx,
        Err(e) => {
            tracing::error!("cship: failed to parse Claude Code session JSON: {e}");
            std::process::exit(1);
        }
    };

    let workspace_dir = ctx
        .workspace
        .as_ref()
        .and_then(|w| w.current_dir.as_deref());

    let _cfg = match cship::config::discover_and_load(workspace_dir, cli.config.as_deref()) {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::error!("cship: failed to load config: {e}");
            std::process::exit(1);
        }
    };

    // Rendering pipeline added in Story 1.4.
    // No stdout output in this story — main.rs is the sole stdout owner (Story 1.4).
}
