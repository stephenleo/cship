use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "cship", about = "Claude Code statusline renderer")]
struct Cli {
    /// Path to starship.toml config file. Bypasses automatic discovery.
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show each native module's current rendered value and config source.
    Explain,
    /// Remove cship binary and settings.json entry.
    Uninstall,
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

    match cli.command {
        Some(Commands::Explain) => {
            let output = cship::explain::run(cli.config.as_deref());
            if !output.is_empty() {
                println!("{output}");
            }
        }
        Some(Commands::Uninstall) => {
            cship::uninstall::run();
        }
        None => {
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

            let cfg = match cship::config::discover_and_load(
                workspace_dir,
                cli.config.as_deref().and_then(|p| p.to_str()),
            ) {
                Ok(cfg) => cfg,
                Err(e) => {
                    tracing::error!("cship: failed to load config: {e}");
                    std::process::exit(1);
                }
            };

            // Render and emit — main.rs is the SOLE owner of stdout.
            // println! is the ONLY stdout write in the rendering pipeline.
            let lines = cfg.lines.as_deref().unwrap_or(&[]);
            if !lines.is_empty() {
                let output = cship::renderer::render(lines, &ctx, &cfg);
                if !output.is_empty() {
                    println!("{output}");
                }
            }
        }
    }
}
