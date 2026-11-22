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
//! At most one render runs at a time. One that is still running after
//! [`STUCK_AFTER`] (a hung subprocess) is abandoned so the footer can recover.
//! A fresh thread per render also keeps per-render thread-local memos
//! (`modules::usage_limits`) from going stale in this long-lived process.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::snapshot::StatuslineState;
use crate::context::Context;

pub(crate) const RESOURCE_URI: &str = "ui://cship/footer";
const TOOL_NAME: &str = "statusline";
const STATE_META_KEY: &str = "com.openai/codex-statusline-state";
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
/// Leaves headroom under Codex's 1 second read budget for I/O and JSON work.
const RENDER_DEADLINE: Duration = Duration::from_millis(800);
/// A render still running after this long is treated as hung and abandoned.
const STUCK_AFTER: Duration = Duration::from_secs(10);

/// Renders a context to a UI tree. Injectable so tests can simulate slow,
/// hung or panicking renders.
type RenderFn = dyn Fn(&Context, Option<&Path>) -> Value + Send + Sync;

/// Tracks the render that currently owns the worker slot.
#[derive(Default)]
struct Slot {
    /// Bumped for every render started. A worker only clears the slot or
    /// publishes its tree while its generation is still current, so an
    /// abandoned worker that finishes late cannot disturb a newer one.
    generation: u64,
    /// When the current render started; `None` when no render is running.
    started: Option<Instant>,
}

struct Shared {
    slot: Mutex<Slot>,
    last_good: Mutex<Value>,
}

impl Shared {
    fn last_tree(&self) -> Value {
        self.last_good
            .lock()
            .map(|v| v.clone())
            .unwrap_or(Value::Null)
    }
}

/// Clears the slot when the worker ends, including by panic (test and debug
/// builds unwind; release builds abort the whole process on panic).
struct SlotGuard {
    shared: Arc<Shared>,
    generation: u64,
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.shared.slot.lock()
            && slot.generation == self.generation
        {
            slot.started = None;
        }
    }
}

pub(crate) struct Server {
    config_path: Option<PathBuf>,
    shared: Arc<Shared>,
    renderer: Arc<RenderFn>,
    deadline: Duration,
    stuck_after: Duration,
}

impl Server {
    pub(crate) fn new(config_path: Option<PathBuf>) -> Self {
        Self::with_renderer(
            config_path,
            Arc::new(render_tree),
            RENDER_DEADLINE,
            STUCK_AFTER,
        )
    }

    fn with_renderer(
        config_path: Option<PathBuf>,
        renderer: Arc<RenderFn>,
        deadline: Duration,
        stuck_after: Duration,
    ) -> Self {
        Self {
            config_path,
            shared: Arc::new(Shared {
                slot: Mutex::new(Slot::default()),
                last_good: Mutex::new(Value::Null),
            }),
            renderer,
            deadline,
            stuck_after,
        }
    }

    /// Serve requests from `reader` until EOF, writing one JSON line per response.
    /// Invalid UTF-8 is replaced rather than ending the server; the JSON parser
    /// then rejects the line with a parse error.
    pub(crate) fn run(
        &self,
        mut reader: impl BufRead,
        mut writer: impl Write,
    ) -> std::io::Result<()> {
        let mut buf = Vec::new();
        loop {
            buf.clear();
            if reader.read_until(b'\n', &mut buf)? == 0 {
                return Ok(());
            }
            let line = String::from_utf8_lossy(&buf);
            if line.trim().is_empty() {
                continue;
            }
            if let Some(reply) = self.handle_line(&line) {
                writeln!(writer, "{reply}")?;
                writer.flush()?;
            }
        }
    }

    /// Handle one JSON-RPC message. Returns `None` for notifications.
    pub(crate) fn handle_line(&self, line: &str) -> Option<Value> {
        let request: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("cship codex: malformed JSON-RPC message: {e}");
                return Some(error(Value::Null, -32700, "Parse error"));
            }
        };
        let id = request.get("id").cloned();
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            tracing::warn!("cship codex: JSON-RPC message without a method");
            return Some(error(id.unwrap_or(Value::Null), -32600, "Invalid request"));
        };
        // Notifications (e.g. `notifications/initialized`) carry no id and get no reply.
        let id = id?;
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
            Some(raw) => StatuslineState::from_value(raw),
            None => {
                tracing::warn!("cship codex: resources/read carried no statusline snapshot");
                StatuslineState::default()
            }
        };

        let generation = {
            let Ok(mut slot) = self.shared.slot.lock() else {
                return self.shared.last_tree();
            };
            if let Some(started) = slot.started {
                if started.elapsed() < self.stuck_after {
                    tracing::warn!(
                        "cship codex: previous render still running; showing last output"
                    );
                    return self.shared.last_tree();
                }
                tracing::warn!(
                    "cship codex: render running for over {}s; abandoning it and starting a new one",
                    self.stuck_after.as_secs()
                );
            }
            slot.generation += 1;
            slot.started = Some(Instant::now());
            slot.generation
        };

        let ctx = state.into_context();
        let config_path = self.config_path.clone();
        let renderer = Arc::clone(&self.renderer);
        let guard = SlotGuard {
            shared: Arc::clone(&self.shared),
            generation,
        };
        // Keep the server's log routing (see `super::log`) on the worker thread.
        let dispatch = tracing::dispatcher::get_default(|d| d.clone());
        let (tx, rx) = mpsc::channel();

        let spawned = std::thread::Builder::new()
            .name("cship-codex-render".into())
            .spawn(move || {
                tracing::dispatcher::with_default(&dispatch, || {
                    let tree = renderer(&ctx, config_path.as_deref());
                    if let Ok(slot) = guard.shared.slot.lock()
                        && slot.generation == guard.generation
                        && let Ok(mut last) = guard.shared.last_good.lock()
                    {
                        *last = tree.clone();
                    }
                    drop(guard);
                    let _ = tx.send(tree);
                })
            });
        if let Err(e) = spawned {
            // The closure (and its guard) was dropped, which already freed the slot.
            tracing::warn!("cship codex: could not start render thread: {e}");
            return self.shared.last_tree();
        }

        match rx.recv_timeout(self.deadline) {
            Ok(tree) => tree,
            Err(RecvTimeoutError::Timeout) => {
                let last = self.shared.last_tree();
                if last.is_null() {
                    tracing::warn!(
                        "cship codex: render exceeded deadline; footer stays empty until one finishes"
                    );
                } else {
                    tracing::warn!("cship codex: render exceeded deadline; reusing last output");
                }
                last
            }
            Err(RecvTimeoutError::Disconnected) => {
                tracing::warn!("cship codex: render ended without a result; reusing last output");
                self.shared.last_tree()
            }
        }
    }
}

/// Load config for the thread's workspace and render it to a UI tree.
fn render_tree(ctx: &Context, config_path: Option<&Path>) -> Value {
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

    /// A server whose renderer is `f`, with short deadlines for fast tests.
    fn server_with(f: impl Fn(&Context, Option<&Path>) -> Value + Send + Sync + 'static) -> Server {
        Server::with_renderer(
            None,
            Arc::new(f),
            Duration::from_millis(100),
            Duration::from_millis(400),
        )
    }

    fn wait_until_idle(s: &Server) {
        for _ in 0..200 {
            if s.shared.slot.lock().unwrap().started.is_none() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("render never finished");
    }

    #[test]
    fn test_fast_render_returns_tree_and_records_last_good() {
        let s = server_with(|_, _| json!("fresh"));
        assert_eq!(s.render(&json!({})), json!("fresh"));
        wait_until_idle(&s);
        assert_eq!(s.shared.last_tree(), json!("fresh"));
    }

    #[test]
    fn test_cold_start_deadline_miss_returns_null() {
        let s = server_with(|_, _| {
            std::thread::sleep(Duration::from_millis(250));
            json!("late")
        });
        assert_eq!(s.render(&json!({})), Value::Null, "nothing rendered yet");
        wait_until_idle(&s);
        assert_eq!(s.shared.last_tree(), json!("late"), "late result is kept");
    }

    #[test]
    fn test_deadline_miss_reuses_last_good_tree() {
        let slow = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&slow);
        let s = server_with(move |_, _| {
            if flag.load(std::sync::atomic::Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(250));
                json!("slow")
            } else {
                json!("fast")
            }
        });
        assert_eq!(s.render(&json!({})), json!("fast"));
        wait_until_idle(&s);
        slow.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(s.render(&json!({})), json!("fast"));
    }

    #[test]
    fn test_render_in_flight_returns_last_good_without_new_worker() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let s = server_with(move |_, _| {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(300));
            json!("done")
        });
        *s.shared.last_good.lock().unwrap() = json!("previous");
        assert_eq!(s.render(&json!({})), json!("previous")); // misses deadline
        assert_eq!(s.render(&json!({})), json!("previous")); // still in flight
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn test_hung_render_is_abandoned_and_cannot_overwrite_newer_tree() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let s = server_with(move |_, _| {
            if counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                std::thread::sleep(Duration::from_millis(900)); // "hung"
                json!("stale")
            } else {
                json!("recovered")
            }
        });
        assert_eq!(s.render(&json!({})), Value::Null);
        std::thread::sleep(Duration::from_millis(450)); // past stuck_after
        assert_eq!(s.render(&json!({})), json!("recovered"));
        std::thread::sleep(Duration::from_millis(600)); // hung worker finishes
        assert_eq!(s.shared.last_tree(), json!("recovered"));
        assert!(s.shared.slot.lock().unwrap().started.is_none());
    }

    #[test]
    fn test_panicking_render_frees_slot() {
        let s = server_with(|ctx, _| {
            if ctx.session_id.as_deref() == Some("boom") {
                panic!("simulated module panic");
            }
            json!("ok")
        });
        let boom = json!({"_meta": {STATE_META_KEY: {"thread": {"id": "boom"}}}});
        assert_eq!(s.render(&boom), Value::Null);
        wait_until_idle(&s);
        assert_eq!(s.render(&json!({})), json!("ok"), "next read renders again");
    }

    #[test]
    fn test_request_without_method_echoes_id() {
        let s = Server::new(None);
        let reply = s.handle_line(r#"{"jsonrpc":"2.0","id":9}"#).unwrap();
        assert_eq!(reply["id"], json!(9));
        assert_eq!(reply["error"]["code"], json!(-32600));
    }

    #[test]
    fn test_invalid_utf8_line_does_not_stop_server() {
        let s = Server::new(None);
        let mut input = b"\xff\xfe garbage\n".to_vec();
        input.extend_from_slice(br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
        input.push(b'\n');
        let mut out = Vec::new();
        s.run(input.as_slice(), &mut out).unwrap();
        let lines: Vec<Value> = std::str::from_utf8(&out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["error"]["code"], json!(-32700));
        assert_eq!(lines[1]["result"], json!({}));
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
