fn main() {
    // Initialize tracing subscriber — stderr ONLY.
    // Must be called before any tracing:: macro. Respects RUST_LOG env var.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    match cship::context::from_stdin() {
        Ok(_ctx) => {
            // Context successfully parsed.
            // Rendering pipeline added in Story 1.3 (config loading) and Story 1.4 (renderer).
            // No stdout output in this story — main.rs is the sole stdout owner (Story 1.4).
        }
        Err(e) => {
            tracing::error!("cship: failed to parse Claude Code session JSON: {e}");
            std::process::exit(1);
        }
    }
}
