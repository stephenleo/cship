//! Convert cship's rendered (ANSI-styled) pieces into a Codex statusline UI tree.
//!
//! Codex renders string leaves literally, so ANSI escapes cannot pass through.
//! Instead, SGR sequences are parsed into styled runs and emitted as `text`
//! elements with `color` / `weight` / `italic` / `underline` props. Working on
//! the final ANSI output (rather than inside each module) means Starship
//! passthrough modules are converted too.
//!
//! Layout: everything goes into one root `text` element so the runs flow inline.
//! Codex lays out each child of a `row` as a separate item and drops its leading
//! whitespace, so rows would glue modules together. Inside a `text` element,
//! whitespace between words survives (collapsed to one space, as in HTML) and a
//! `\n` leaf starts a new footer line. `$fill` becomes a single space: Codex
//! 0.171.0 has no row alignment to right-align with.
//!
//! Codex limits a tree to 256 non-null nodes, depth 16, 64 KiB of JSON and
//! 16 KiB of leaf text. A tree over any limit is replaced by a single plain
//! string (truncated to fit) so the footer still shows something.

use serde_json::{Value, json};

use crate::renderer::Piece;

const MAX_NODES: usize = 256;
const MAX_JSON_BYTES: usize = 64 * 1024;
const MAX_TEXT_BYTES: usize = 16 * 1024;
/// Maximum depth, counting the root as 0.
const MAX_DEPTH: usize = 16;

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
    if lines.is_empty() {
        return Value::Null;
    }
    let mut runs: Vec<Run> = Vec::new();
    for (i, pieces) in lines.iter().enumerate() {
        if i > 0 {
            append_runs(&mut runs, vec![plain("\n")]);
        }
        for piece in pieces {
            match piece {
                Piece::Text(s) => append_runs(&mut runs, parse_ansi(s.trim_end_matches('\n'))),
                Piece::Fill => append_runs(&mut runs, vec![plain(" ")]),
            }
        }
    }
    let tree = json!({
        "type": "text",
        "children": runs.into_iter().map(run_node).collect::<Vec<_>>(),
    });
    if within_limits(&tree) {
        tree
    } else {
        tracing::warn!("cship codex: UI tree exceeds Codex limits; falling back to plain text");
        plain_fallback(lines)
    }
}

fn plain(text: &str) -> Run {
    Run {
        style: Style::default(),
        text: text.to_string(),
    }
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
            if c == '\n' || !c.is_control() {
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

/// Apply one SGR sequence's parameters. Handles both the legacy semicolon form
/// (`38;2;r;g;b`) and ISO 8613-6 colon sub-parameters (`38:2::r:g:b`, `4:3`).
/// A parameter that is not a number is ignored rather than treated as a reset.
fn apply_sgr(style: &mut Style, params: &str) {
    let groups: Vec<&str> = params.split(';').collect();
    let mut i = 0;
    while i < groups.len() {
        let group = groups[i];
        i += 1;
        if group.contains(':') {
            apply_colon_group(style, group);
            continue;
        }
        // An empty parameter means 0 (so a bare `ESC[m` resets).
        let code = if group.is_empty() {
            Some(0)
        } else {
            group.parse::<u16>().ok()
        };
        match code {
            Some(c @ (38 | 48)) => {
                let args: Vec<Option<u16>> =
                    groups[i..].iter().take(4).map(|a| a.parse().ok()).collect();
                let (color, used) = match args.as_slice() {
                    [Some(5), Some(n), ..] => (Some(indexed_color(clamp_u8(*n))), 2),
                    [Some(2), Some(r), Some(g), Some(b), ..] => {
                        (Some(hex(clamp_u8(*r), clamp_u8(*g), clamp_u8(*b))), 4)
                    }
                    _ => (None, 0),
                };
                if c == 38
                    && let Some(color) = color
                {
                    style.color = Some(color);
                }
                i += used;
            }
            Some(c) => apply_code(style, c),
            None => {}
        }
    }
}

/// A colon group carries its own sub-parameters, e.g. `38:5:208`, `38:2::r:g:b`
/// (empty colour-space id) or `4:3` (curly underline).
fn apply_colon_group(style: &mut Style, group: &str) {
    let parts: Vec<Option<u16>> = group.split(':').map(|p| p.parse().ok()).collect();
    let sub = &parts[1..];
    match parts[0] {
        Some(38) => {
            let color = match sub {
                [Some(5), Some(n), ..] => Some(indexed_color(clamp_u8(*n))),
                [Some(2), rest @ ..] => {
                    // `38:2:<colour-space>:r:g:b` or the shorter `38:2:r:g:b`.
                    let rgb = if rest.len() >= 4 { &rest[1..4] } else { rest };
                    match rgb {
                        [Some(r), Some(g), Some(b)] => {
                            Some(hex(clamp_u8(*r), clamp_u8(*g), clamp_u8(*b)))
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(color) = color {
                style.color = Some(color);
            }
        }
        // Underline style: `4:0` turns it off, any other style turns it on.
        Some(4) => style.underline = sub.first().copied().flatten() != Some(0),
        Some(c) => apply_code(style, c),
        None => {}
    }
}

/// Apply a single SGR code that takes no arguments.
fn apply_code(style: &mut Style, code: u16) {
    match code {
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
        // Background, dim, blink, reverse, etc. have no Codex text equivalent.
        _ => {}
    }
}

fn clamp_u8(n: u16) -> u8 {
    n.min(255) as u8
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
        && depth <= MAX_DEPTH
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
                .map(|p| match p {
                    Piece::Text(s) => crate::ansi::strip_ansi(s),
                    Piece::Fill => " ".to_string(),
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut end = joined.len().min(MAX_TEXT_BYTES);
    while !joined.is_char_boundary(end) {
        end -= 1;
    }
    Value::String(
        joined[..end]
            .chars()
            .filter(|c| *c == '\n' || !c.is_control())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::apply_style;

    fn text(s: &str) -> Piece {
        Piece::Text(s.to_string())
    }

    fn children(tree: &Value) -> &Value {
        assert_eq!(
            tree["type"],
            json!("text"),
            "root must be an inline text element"
        );
        &tree["children"]
    }

    #[test]
    fn test_empty_lines_clear_footer() {
        assert_eq!(build(&[]), Value::Null);
    }

    #[test]
    fn test_plain_text_is_bare_string_in_text_root() {
        let tree = build(&[vec![text("hello")]]);
        assert_eq!(tree, json!({"type": "text", "children": ["hello"]}));
    }

    #[test]
    fn test_named_color_and_bold_keep_following_space_inline() {
        let styled = apply_style("Opus", Some("bold green"));
        let tree = build(&[vec![text(&styled), text(" | ")]]);
        assert_eq!(
            children(&tree),
            &json!([
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
        assert_eq!(children(&tree)[0]["props"]["color"], json!("#ff8800"));
        assert_eq!(children(&tree)[1]["props"]["color"], json!("#ff0000"));
    }

    #[test]
    fn test_italic_underline_and_background_dropped() {
        let styled = apply_style("x", Some("italic underline bg:blue"));
        let tree = build(&[vec![text(&styled)]]);
        assert_eq!(
            children(&tree)[0]["props"],
            json!({"italic": true, "underline": true})
        );
    }

    #[test]
    fn test_bright_black_maps_to_gray() {
        let tree = build(&[vec![text("\x1b[90mdim\x1b[0m")]]);
        assert_eq!(children(&tree)[0]["props"]["color"], json!("#808080"));
    }

    #[test]
    fn test_adjacent_same_style_runs_merge() {
        let a = apply_style("ab", Some("red"));
        let b = apply_style("cd", Some("red"));
        let tree = build(&[vec![text(&a), text(&b)]]);
        assert_eq!(children(&tree).as_array().unwrap().len(), 1);
        assert_eq!(children(&tree)[0]["children"][0], json!("abcd"));
    }

    #[test]
    fn test_non_sgr_escapes_and_controls_are_stripped() {
        let s = "a\x1b]0;title\x07b\x1b[2Kc\x07d";
        let tree = build(&[vec![text(s)]]);
        assert_eq!(children(&tree), &json!(["abcd"]));
    }

    #[test]
    fn test_colon_truecolor_with_empty_colour_space() {
        let tree = build(&[vec![text("\x1b[38:2::255:136:0mx\x1b[0m")]]);
        assert_eq!(children(&tree)[0]["props"], json!({"color": "#ff8800"}));
    }

    #[test]
    fn test_colon_truecolor_without_colour_space_and_256() {
        let tree = build(&[vec![
            text("\x1b[38:2:1:2:3ma"),
            text("\x1b[38:5:196mb\x1b[0m"),
        ]]);
        assert_eq!(children(&tree)[0]["props"]["color"], json!("#010203"));
        assert_eq!(children(&tree)[1]["props"]["color"], json!("#ff0000"));
    }

    #[test]
    fn test_colon_underline_style_is_underline_not_italic() {
        let tree = build(&[vec![text("\x1b[4:3mcurly\x1b[4:0m plain")]]);
        assert_eq!(
            children(&tree),
            &json!([
                {"type":"text","props":{"underline":true},"children":["curly"]},
                " plain"
            ])
        );
    }

    #[test]
    fn test_malformed_parameter_does_not_reset_style() {
        let tree = build(&[vec![text("\x1b[1mbold\x1b[99999;31mstill bold\x1b[0m")]]);
        assert_eq!(
            children(&tree)[1]["props"],
            json!({"color": "red", "weight": "bold"})
        );
    }

    #[test]
    fn test_embedded_newline_is_kept_and_trailing_newline_dropped() {
        let tree = build(&[vec![text("one\ntwo\n")], vec![text("three")]]);
        assert_eq!(children(&tree), &json!(["one\ntwo\nthree"]));
    }

    #[test]
    fn test_fill_becomes_single_space() {
        let tree = build(&[vec![text("left"), Piece::Fill, text("right")]]);
        assert_eq!(children(&tree), &json!(["left right"]));
    }

    #[test]
    fn test_multi_line_separated_by_newline_leaf() {
        let red = apply_style("two", Some("red"));
        let tree = build(&[vec![text("one")], vec![text(&red)], vec![text("three")]]);
        assert_eq!(
            children(&tree),
            &json!([
                "one\n",
                {"type":"text","props":{"color":"red"},"children":["two"]},
                "\nthree"
            ])
        );
    }

    #[test]
    fn test_no_row_elements_emitted() {
        // Codex lays out row children as separate items and trims their leading
        // whitespace; the inline text root must never contain rows.
        let tree = build(&[vec![text("a"), Piece::Fill, text("b")], vec![text("c")]]);
        assert!(!tree.to_string().contains("\"row\""));
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

    #[test]
    fn test_fallback_keeps_line_breaks() {
        let lines: Vec<Vec<Piece>> = vec![
            (0..300)
                .map(|i| {
                    text(&apply_style(
                        "x",
                        Some(if i % 2 == 0 { "red" } else { "blue" }),
                    ))
                })
                .collect(),
            vec![text("y")],
        ];
        let tree = build(&lines);
        assert_eq!(tree, Value::String(format!("{}\ny", "x".repeat(300))));
    }
}
