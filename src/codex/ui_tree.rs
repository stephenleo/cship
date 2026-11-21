//! Convert cship's rendered (ANSI-styled) pieces into a Codex statusline UI tree.
//!
//! Codex renders string leaves literally, so ANSI escapes cannot pass through.
//! Instead, SGR sequences are parsed into styled runs and emitted as `text`
//! elements with `color` / `weight` / `italic` / `underline` props. Working on
//! the final ANSI output (rather than inside each module) means Starship
//! passthrough modules are converted too.
//!
//! Layout rules:
//! - One line: a root `row`. `$fill` maps to row `justify` (`between`, `end` or
//!   `center`), which gives the same right-alignment as the terminal path.
//! - Several lines: each line becomes a nested row inside a wrapping root row.
//!   Nested rows take their content width, so `$fill` collapses there.
//!
//! Codex limits a tree to 256 non-null nodes, depth 16, 64 KiB of JSON and
//! 16 KiB of leaf text. A tree over any limit is replaced by a single plain
//! string (truncated to fit) so the footer still shows something.

use serde_json::{Value, json};

use crate::renderer::Piece;

const MAX_NODES: usize = 256;
const MAX_JSON_BYTES: usize = 64 * 1024;
const MAX_TEXT_BYTES: usize = 16 * 1024;
/// Columns between lines when several lines share the footer.
const LINE_GAP: u64 = 2;

#[derive(Debug, Clone, Default, PartialEq)]
struct Style {
    color: Option<String>,
    bold: bool,
    italic: bool,
    underline: bool,
}

impl Style {
    fn is_plain(&self) -> bool {
        *self == Style::default()
    }
}

#[derive(Debug, PartialEq)]
struct Run {
    style: Style,
    text: String,
}

/// Build the UI tree for rendered lines. Returns `Value::Null` (clears the
/// footer) when there is nothing to show.
pub fn build(lines: &[Vec<Piece>]) -> Value {
    let tree = match lines {
        [] => return Value::Null,
        [line] => single_line(line),
        _ => multi_line(lines),
    };
    if within_limits(&tree) {
        tree
    } else {
        tracing::warn!("cship codex: UI tree exceeds Codex limits; falling back to plain text");
        plain_fallback(lines)
    }
}

fn single_line(pieces: &[Piece]) -> Value {
    // Split on fills into groups; keep empty groups to know where fills sat.
    let mut groups: Vec<Vec<Run>> = vec![Vec::new()];
    for piece in pieces {
        match piece {
            Piece::Text(s) => append_runs(groups.last_mut().unwrap(), parse_ansi(s)),
            Piece::Fill => groups.push(Vec::new()),
        }
    }

    if groups.len() == 1 {
        let runs = groups.pop().unwrap_or_default();
        return row(json!({"gap": 0}), runs.into_iter().map(run_node).collect());
    }

    let leading_fill = groups.first().is_some_and(Vec::is_empty);
    let trailing_fill = groups.last().is_some_and(Vec::is_empty);
    let filled: Vec<Vec<Run>> = groups.into_iter().filter(|g| !g.is_empty()).collect();

    let justify = match (filled.len(), leading_fill, trailing_fill) {
        (1, true, true) => Some("center"),
        (1, true, false) => Some("end"),
        (1, false, _) => None,
        _ => Some("between"),
    };

    match justify {
        None => row(
            json!({"gap": 0}),
            filled.into_iter().flatten().map(run_node).collect(),
        ),
        Some(justify) => row(
            json!({"gap": 0, "justify": justify}),
            filled
                .into_iter()
                .map(|g| row(json!({"gap": 0}), g.into_iter().map(run_node).collect()))
                .collect(),
        ),
    }
}

fn multi_line(lines: &[Vec<Piece>]) -> Value {
    let children = lines
        .iter()
        .map(|pieces| {
            let mut runs = Vec::new();
            for piece in pieces {
                if let Piece::Text(s) = piece {
                    append_runs(&mut runs, parse_ansi(s));
                }
            }
            row(json!({"gap": 0}), runs.into_iter().map(run_node).collect())
        })
        .collect();
    row(json!({"gap": LINE_GAP, "wrap": "wrap"}), children)
}

fn row(props: Value, children: Vec<Value>) -> Value {
    json!({"type": "row", "props": props, "children": children})
}

/// A plain run is a bare string; a styled run is a `text` element.
fn run_node(run: Run) -> Value {
    if run.style.is_plain() {
        return Value::String(run.text);
    }
    let mut props = serde_json::Map::new();
    if let Some(color) = run.style.color {
        props.insert("color".into(), Value::String(color));
    }
    if run.style.bold {
        props.insert("weight".into(), json!("bold"));
    }
    if run.style.italic {
        props.insert("italic".into(), json!(true));
    }
    if run.style.underline {
        props.insert("underline".into(), json!(true));
    }
    json!({"type": "text", "props": props, "children": [run.text]})
}

/// Append runs, merging with the previous run when the style is unchanged.
fn append_runs(dst: &mut Vec<Run>, src: Vec<Run>) {
    for run in src {
        match dst.last_mut() {
            Some(last) if last.style == run.style => last.text.push_str(&run.text),
            _ => dst.push(run),
        }
    }
}

/// Parse ANSI-styled text into runs. SGR sequences update the style; every other
/// escape sequence and control character is dropped.
fn parse_ansi(s: &str) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut style = Style::default();
    let mut text = String::new();
    let mut chars = s.chars().peekable();

    let flush = |runs: &mut Vec<Run>, text: &mut String, style: &Style| {
        if !text.is_empty() {
            append_runs(
                runs,
                vec![Run {
                    style: style.clone(),
                    text: std::mem::take(text),
                }],
            );
        }
    };

    while let Some(c) = chars.next() {
        if c != '\x1b' {
            if !c.is_control() {
                text.push(c);
            }
            continue;
        }
        match chars.next() {
            // CSI: parameters then a final byte in 0x40..=0x7E.
            Some('[') => {
                let mut params = String::new();
                let mut final_byte = None;
                for c2 in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c2) {
                        final_byte = Some(c2);
                        break;
                    }
                    params.push(c2);
                }
                if final_byte == Some('m') {
                    flush(&mut runs, &mut text, &style);
                    apply_sgr(&mut style, &params);
                }
            }
            // OSC: terminated by BEL or ST (ESC \).
            Some(']') => {
                while let Some(c2) = chars.next() {
                    if c2 == '\x07' {
                        break;
                    }
                    if c2 == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            // Any other two-byte escape: drop both bytes.
            _ => {}
        }
    }
    flush(&mut runs, &mut text, &style);
    runs
}

fn apply_sgr(style: &mut Style, params: &str) {
    let codes: Vec<u16> = if params.is_empty() {
        vec![0]
    } else {
        params
            .split([';', ':'])
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let mut i = 0;
    while i < codes.len() {
        match codes[i] {
            0 => *style = Style::default(),
            1 => style.bold = true,
            22 => style.bold = false,
            3 => style.italic = true,
            23 => style.italic = false,
            4 => style.underline = true,
            24 => style.underline = false,
            n @ 30..=37 => style.color = Some(indexed_color((n - 30) as u8)),
            n @ 90..=97 => style.color = Some(indexed_color((n - 90 + 8) as u8)),
            39 => style.color = None,
            38 | 48 => {
                let is_fg = codes[i] == 38;
                let (color, used) = extended_color(&codes[i + 1..]);
                if is_fg && let Some(c) = color {
                    style.color = Some(c);
                }
                i += used;
            }
            // Background, dim, blink, reverse, etc. have no Codex text equivalent.
            _ => {}
        }
        i += 1;
    }
}

/// Parse the arguments after 38/48. Returns the color and how many codes it used.
fn extended_color(args: &[u16]) -> (Option<String>, usize) {
    match args {
        [5, n, ..] => (Some(indexed_color((*n).min(255) as u8)), 2),
        [2, r, g, b, ..] => (Some(hex(*r as u8, *g as u8, *b as u8)), 4),
        [5] | [2, ..] => (None, args.len()),
        _ => (None, 0),
    }
}

/// Map an xterm-256 index to a Codex color. The 16 base colors use ANSI names so
/// they follow the user's theme; bright variants share the base name, except
/// bright black (often used for dim text), which becomes gray.
fn indexed_color(n: u8) -> String {
    const NAMES: [&str; 8] = [
        "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    ];
    match n {
        8 => "#808080".to_string(),
        0..=15 => NAMES[(n % 8) as usize].to_string(),
        16..=231 => {
            const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
            let i = n - 16;
            hex(
                LEVELS[(i / 36) as usize],
                LEVELS[((i / 6) % 6) as usize],
                LEVELS[(i % 6) as usize],
            )
        }
        232..=255 => {
            let v = 8 + 10 * (n - 232);
            hex(v, v, v)
        }
    }
}

fn hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn within_limits(tree: &Value) -> bool {
    let (nodes, text_bytes, depth) = measure(tree, 0);
    nodes <= MAX_NODES
        && text_bytes <= MAX_TEXT_BYTES
        && depth <= 16
        && serde_json::to_string(tree).is_ok_and(|s| s.len() <= MAX_JSON_BYTES)
}

/// Returns (non-null node count, leaf text bytes, max depth).
fn measure(v: &Value, depth: usize) -> (usize, usize, usize) {
    match v {
        Value::Null => (0, 0, 0),
        Value::String(s) => (1, s.len(), depth),
        Value::Object(o) => {
            let mut acc = (1, 0, depth);
            if let Some(Value::Array(children)) = o.get("children") {
                for child in children {
                    let (n, t, d) = measure(child, depth + 1);
                    acc.0 += n;
                    acc.1 += t;
                    acc.2 = acc.2.max(d);
                }
            }
            acc
        }
        _ => (1, 0, depth),
    }
}

fn plain_fallback(lines: &[Vec<Piece>]) -> Value {
    let joined = lines
        .iter()
        .map(|pieces| {
            pieces
                .iter()
                .filter_map(|p| match p {
                    Piece::Text(s) => Some(crate::ansi::strip_ansi(s)),
                    Piece::Fill => None,
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("  ");
    let mut end = joined.len().min(MAX_TEXT_BYTES);
    while !joined.is_char_boundary(end) {
        end -= 1;
    }
    Value::String(joined[..end].chars().filter(|c| !c.is_control()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::apply_style;

    fn text(s: &str) -> Piece {
        Piece::Text(s.to_string())
    }

    #[test]
    fn test_empty_lines_clear_footer() {
        assert_eq!(build(&[]), Value::Null);
    }

    #[test]
    fn test_plain_text_is_bare_string() {
        let tree = build(&[vec![text("hello")]]);
        assert_eq!(
            tree,
            json!({"type":"row","props":{"gap":0},"children":["hello"]})
        );
    }

    #[test]
    fn test_named_color_and_bold() {
        let styled = apply_style("Opus", Some("bold green"));
        let tree = build(&[vec![text(&styled), text(" | ")]]);
        assert_eq!(
            tree["children"],
            json!([
                {"type":"text","props":{"color":"green","weight":"bold"},"children":["Opus"]},
                " | "
            ])
        );
    }

    #[test]
    fn test_hex_and_256_colors() {
        let rgb = apply_style("a", Some("#ff8800"));
        let fixed = apply_style("b", Some("196"));
        let tree = build(&[vec![text(&rgb), text(&fixed)]]);
        assert_eq!(tree["children"][0]["props"]["color"], json!("#ff8800"));
        assert_eq!(tree["children"][1]["props"]["color"], json!("#ff0000"));
    }

    #[test]
    fn test_italic_underline_and_background_dropped() {
        let styled = apply_style("x", Some("italic underline bg:blue"));
        let tree = build(&[vec![text(&styled)]]);
        assert_eq!(
            tree["children"][0]["props"],
            json!({"italic": true, "underline": true})
        );
    }

    #[test]
    fn test_bright_black_maps_to_gray() {
        let tree = build(&[vec![text("\x1b[90mdim\x1b[0m")]]);
        assert_eq!(tree["children"][0]["props"]["color"], json!("#808080"));
    }

    #[test]
    fn test_adjacent_same_style_runs_merge() {
        let a = apply_style("ab", Some("red"));
        let b = apply_style("cd", Some("red"));
        let tree = build(&[vec![text(&a), text(&b)]]);
        assert_eq!(tree["children"].as_array().unwrap().len(), 1);
        assert_eq!(tree["children"][0]["children"][0], json!("abcd"));
    }

    #[test]
    fn test_non_sgr_escapes_and_controls_are_stripped() {
        let s = "a\x1b]0;title\x07b\x1b[2Kc\x07d";
        let tree = build(&[vec![text(s)]]);
        assert_eq!(tree["children"], json!(["abcd"]));
    }

    #[test]
    fn test_fill_between_two_groups_uses_justify_between() {
        let tree = build(&[vec![text("left"), Piece::Fill, text("right")]]);
        assert_eq!(tree["props"]["justify"], json!("between"));
        assert_eq!(tree["children"][0]["children"], json!(["left"]));
        assert_eq!(tree["children"][1]["children"], json!(["right"]));
    }

    #[test]
    fn test_leading_fill_right_aligns() {
        let tree = build(&[vec![Piece::Fill, text("right")]]);
        assert_eq!(tree["props"]["justify"], json!("end"));
    }

    #[test]
    fn test_fill_on_both_sides_centers() {
        let tree = build(&[vec![Piece::Fill, text("mid"), Piece::Fill]]);
        assert_eq!(tree["props"]["justify"], json!("center"));
    }

    #[test]
    fn test_trailing_fill_is_plain_row() {
        let tree = build(&[vec![text("left"), Piece::Fill]]);
        assert!(tree["props"].get("justify").is_none());
        assert_eq!(tree["children"], json!(["left"]));
    }

    #[test]
    fn test_multi_line_nests_rows_and_drops_fill() {
        let tree = build(&[
            vec![text("one"), Piece::Fill, text("two")],
            vec![text("three")],
        ]);
        assert_eq!(tree["props"], json!({"gap": 2, "wrap": "wrap"}));
        assert_eq!(tree["children"][0]["children"], json!(["onetwo"]));
        assert_eq!(tree["children"][1]["children"], json!(["three"]));
    }

    #[test]
    fn test_oversized_tree_falls_back_to_plain_string() {
        // 300 alternating-colour runs exceed the 256-node limit.
        let line: Vec<Piece> = (0..300)
            .map(|i| {
                text(&apply_style(
                    "x",
                    Some(if i % 2 == 0 { "red" } else { "blue" }),
                ))
            })
            .collect();
        let tree = build(&[line]);
        assert_eq!(tree, Value::String("x".repeat(300)));
    }

    #[test]
    fn test_oversized_text_is_truncated() {
        let big = "é".repeat(MAX_TEXT_BYTES); // 2 bytes each
        let tree = build(&[vec![text(&big)]]);
        let s = tree.as_str().unwrap();
        assert!(s.len() <= MAX_TEXT_BYTES);
    }
}
