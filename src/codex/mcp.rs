//! Minimal stdio MCP server for Codex's custom status line.
//!
//! Protocol (from Codex's `create-statusline` sample): newline-delimited
//! JSON-RPC 2.0. `tools/list` advertises one tool whose `_meta["openai/ui"]`
//! carries a `statusline` entrypoint; Codex then polls `resources/read` on that
//! URI with the thread snapshot in `params._meta`, and expects one
//! `application/json` text resource holding the UI tree.
//!
//! Codex stops polling after a single error or a read slower than one second, so
//! `resources/read` never returns a JSON-RPC error for render problems. Rendering
//! runs on a worker thread with a deadline; if it misses the deadline (for
//! example a cold Starship passthrough), the last good tree is returned instead.
//! A fresh thread per render also keeps per-render thread-local memos
//! (`modules::usage_limits`) from going stale in this long-lived process.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use serde_json::{Value, json};

use super::snapshot::StatuslineState;

pub const RESOURCE_URI: &str = "ui://cship/footer";
const TOOL_NAME: &str = "statusline";
const STATE_META_KEY: &str = "com.openai/codex-statusline-state";
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
/// Leaves headroom under Codex's 1 second read budget for I/O and JSON work.
const RENDER_DEADLINE: Duration = Duration::from_millis(800);

pub struct Server {
    config_path: Option<PathBuf>,
    last_good: Arc<Mutex<Value>>,
    in_flight: Arc<AtomicBool>,
    deadline: Duration,
}

impl Server {
    pub fn new(config_path: Option<PathBuf>) -> Self {
        Self {
            config_path,
            last_good: Arc::new(Mutex::new(Value::Null)),
            in_flight: Arc::new(AtomicBool::new(false)),
            deadline: RENDER_DEADLINE,
        }
    }

    /// Serve requests from `reader` until EOF, writing one JSON line per response.
    pub fn run(&self, reader: impl BufRead, mut writer: impl Write) -> std::io::Result<()> {
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Some(reply) = self.handle_line(&line) {
                writeln!(writer, "{reply}")?;
                writer.flush()?;
            }
        }
        Ok(())
    }

    /// Handle one JSON-RPC message. Returns `None` for notifications.
    pub fn handle_line(&self, line: &str) -> Option<Value> {
        let request: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => return Some(error(Value::Null, -32700, "Parse error")),
        };
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            return Some(error(Value::Null, -32600, "Invalid request"));
        };
        // Notifications (e.g. `notifications/initialized`) carry no id and get no reply.
        let id = request.get("id")?.clone();
        let params = request.get("params").cloned().unwrap_or(Value::Null);

        Some(match self.respond(method, &params) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, message)) => error(id, code, message),
        })
    }

    fn respond(&self, method: &str, params: &Value) -> Result<Value, (i64, &'static str)> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or(DEFAULT_PROTOCOL_VERSION),
                "capabilities": {"tools": {}, "resources": {}},
                "serverInfo": {"name": "cship", "version": env!("CARGO_PKG_VERSION")},
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": [tool()]})),
            "tools/call" => {
                if params.get("name").and_then(Value::as_str) != Some(TOOL_NAME) {
                    return Err((-32602, "Unknown tool name"));
                }
                Ok(json!({"content": [{
                    "type": "text",
                    "text": format!("The cship statusline resource is {RESOURCE_URI}."),
                }]}))
            }
            "resources/list" => Ok(json!({"resources": [{
                "name": "footer",
                "uri": RESOURCE_URI,
                "mimeType": "application/json",
            }]})),
            "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
            "resources/read" => {
                if params.get("uri").and_then(Value::as_str) != Some(RESOURCE_URI) {
                    return Err((-32602, "Unknown resource URI"));
                }
                let tree = self.render(params);
                Ok(json!({"contents": [{
                    "uri": RESOURCE_URI,
                    "mimeType": "application/json",
                    "text": tree.to_string(),
                }]}))
            }
            _ => Err((-32601, "Method not found")),
        }
    }

    /// Render the snapshot in `params._meta` to a UI tree, within the deadline.
    fn render(&self, params: &Value) -> Value {
        let state = match params.get("_meta").and_then(|m| m.get(STATE_META_KEY)) {
            Some(raw) => match serde_json::from_value::<StatuslineState>(raw.clone()) {
                Ok(state) => state,
                Err(e) => {
                    tracing::warn!("cship codex: could not parse statusline snapshot: {e}");
                    StatuslineState::default()
                }
            },
            None => {
                tracing::warn!("cship codex: resources/read carried no statusline snapshot");
                StatuslineState::default()
            }
        };

        // A previous render is still running (it overran its deadline): don't pile
        // up workers, show the last good tree.
        if self.in_flight.swap(true, Ordering::SeqCst) {
            return self.last_tree();
        }

        let ctx = state.into_context();
        let config_path = self.config_path.clone();
        let last_good = Arc::clone(&self.last_good);
        let in_flight = Arc::clone(&self.in_flight);
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let tree = render_tree(&ctx, config_path.as_deref());
            if let Ok(mut slot) = last_good.lock() {
                *slot = tree.clone();
            }
            in_flight.store(false, Ordering::SeqCst);
            let _ = tx.send(tree);
        });

        match rx.recv_timeout(self.deadline) {
            Ok(tree) => tree,
            Err(_) => {
                tracing::warn!("cship codex: render exceeded deadline; reusing last output");
                self.last_tree()
            }
        }
    }

    fn last_tree(&self) -> Value {
        self.last_good
            .lock()
            .map(|v| v.clone())
            .unwrap_or(Value::Null)
    }
}

/// Load config for the thread's workspace and render it to a UI tree.
fn render_tree(ctx: &crate::context::Context, config_path: Option<&std::path::Path>) -> Value {
    let workspace_dir = ctx
        .workspace
        .as_ref()
        .and_then(|w| w.current_dir.as_deref());
    let cfg =
        match crate::config::discover_and_load(workspace_dir, config_path.and_then(|p| p.to_str()))
        {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::warn!("cship codex: failed to load config: {e}");
                return Value::String("cship: config error (run with RUST_LOG=warn)".into());
            }
        };
    let lines = cfg.lines.as_deref().unwrap_or(&[]);
    if cfg.format.is_none() && lines.is_empty() {
        return Value::Null;
    }
    let pieces = crate::renderer::render_pieces(lines, ctx, &cfg);
    super::ui_tree::build(&pieces)
}

fn tool() -> Value {
    json!({
        "name": TOOL_NAME,
        "title": "cship",
        "description": "Render your cship statusline in the Codex footer.",
        "inputSchema": {"type": "object", "properties": {}},
        "_meta": {"openai/ui": {"entrypoints": [
            {"type": "statusline", "resourceUri": RESOURCE_URI}
        ]}},
    })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server_with_config(toml: &str) -> (Server, tempfile::NamedTempFile) {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(toml.as_bytes()).unwrap();
        (Server::new(Some(f.path().to_path_buf())), f)
    }

    fn read_request(snapshot: Value) -> String {
        json!({
            "jsonrpc": "2.0", "id": 7, "method": "resources/read",
            "params": {"uri": RESOURCE_URI, "_meta": {STATE_META_KEY: snapshot}},
        })
        .to_string()
    }

    fn tree_of(reply: &Value) -> Value {
        let text = reply["result"]["contents"][0]["text"].as_str().unwrap();
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn test_initialize_echoes_protocol_version() {
        let s = Server::new(None);
        let reply = s
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25"}}"#)
            .unwrap();
        assert_eq!(reply["id"], json!(1));
        assert_eq!(reply["result"]["protocolVersion"], json!("2025-11-25"));
        assert_eq!(reply["result"]["serverInfo"]["name"], json!("cship"));
    }

    #[test]
    fn test_notification_gets_no_reply() {
        let s = Server::new(None);
        assert!(
            s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .is_none()
        );
    }

    #[test]
    fn test_tools_list_advertises_statusline_entrypoint() {
        let s = Server::new(None);
        let reply = s
            .handle_line(r#"{"jsonrpc":"2.0","id":"a","method":"tools/list"}"#)
            .unwrap();
        let tool = &reply["result"]["tools"][0];
        assert_eq!(
            tool["_meta"]["openai/ui"]["entrypoints"][0],
            json!({"type": "statusline", "resourceUri": RESOURCE_URI})
        );
    }

    #[test]
    fn test_unknown_method_and_parse_errors() {
        let s = Server::new(None);
        let reply = s
            .handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"nope"}"#)
            .unwrap();
        assert_eq!(reply["error"]["code"], json!(-32601));
        let reply = s.handle_line("{not json").unwrap();
        assert_eq!(reply["error"]["code"], json!(-32700));
    }

    #[test]
    fn test_resources_read_renders_snapshot() {
        let (s, _f) = server_with_config(
            "[cship]\nlines = [\"$cship.model\"]\n[cship.model]\nstyle = \"bold green\"\n",
        );
        let snapshot =
            json!({"thread": {"model": {"display_name": "gpt-5.5", "provider": "openai"}}});
        let reply = s.handle_line(&read_request(snapshot)).unwrap();
        assert_eq!(reply["id"], json!(7));
        assert_eq!(
            reply["result"]["contents"][0]["mimeType"],
            json!("application/json")
        );
        let tree = tree_of(&reply);
        assert_eq!(tree["type"], json!("text"));
        let node = &tree["children"][0];
        assert_eq!(node["props"]["color"], json!("green"));
        assert_eq!(node["props"]["weight"], json!("bold"));
        assert!(node["children"][0].as_str().unwrap().contains("gpt-5.5"));
    }

    #[test]
    fn test_resources_read_bad_snapshot_is_not_an_error() {
        let (s, _f) = server_with_config("[cship]\nlines = [\"$cship.model\"]\n");
        let reply = s.handle_line(&read_request(json!("garbage"))).unwrap();
        assert!(reply.get("error").is_none());
        assert!(reply["result"]["contents"][0]["text"].is_string());
    }

    #[test]
    fn test_resources_read_bad_config_is_not_an_error() {
        let (s, _f) = server_with_config("[cship\nnot toml");
        let reply = s.handle_line(&read_request(json!({}))).unwrap();
        assert!(reply.get("error").is_none());
        assert!(tree_of(&reply).as_str().unwrap().contains("config error"));
    }

    #[test]
    fn test_resources_read_wrong_uri_is_rejected() {
        let s = Server::new(None);
        let reply = s
            .handle_line(r#"{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"ui://other"}}"#)
            .unwrap();
        assert_eq!(reply["error"]["code"], json!(-32602));
    }

    #[test]
    fn test_render_in_flight_returns_last_good_tree() {
        let s = Server::new(None);
        *s.last_good.lock().unwrap() = json!("previous");
        s.in_flight.store(true, Ordering::SeqCst);
        assert_eq!(s.render(&json!({})), json!("previous"));
    }

    #[test]
    fn test_run_loop_writes_one_line_per_reply() {
        let s = Server::new(None);
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
            "\n",
        );
        let mut out = Vec::new();
        s.run(input.as_bytes(), &mut out).unwrap();
        let lines: Vec<&str> = std::str::from_utf8(&out).unwrap().lines().collect();
        assert_eq!(lines.len(), 2);
    }
}
