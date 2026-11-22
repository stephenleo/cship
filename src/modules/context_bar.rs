use crate::config::CshipConfig;
use crate::context::Context;

const DEFAULT_BAR_WIDTH: u32 = 10;
const DEFAULT_FILLED_CHAR: &str = "█";
const DEFAULT_EMPTY_CHAR: &str = "░";

/// Renders `$cship.context_bar` — visual Unicode progress bar with threshold color escalation.
/// Format: `{bar} {used_percentage:.0}%` e.g. `███░░░░░░░ 35%`
/// The bar has `width` slots (default 10), or one slot per character of `filled_text` when set.
/// Fill characters are configurable via `filled_char` (default `"█"`) and `empty_char` (default `"░"`).
/// `filled_text` reveals text one character per slot instead of repeating `filled_char`.
/// `full_at` (default 100) is the percentage drawn as a full bar.
/// `filled_style` / `track_style` style the filled slots and the track on their own.
///
/// Source: epics.md#Story 2.2, prd.md#FR8
pub fn render(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    let bar_cfg = cfg.context_bar.as_ref();

    if bar_cfg.and_then(|c| c.disabled).unwrap_or(false) {
        return None;
    }

    let (used_pct, is_empty) = match ctx.context_window.as_ref() {
        Some(cw) => match cw.used_percentage {
            Some(v) => (v, false),
            None => {
                tracing::debug!(
                    "cship.context_bar: context_window present but used_percentage absent; rendering empty bar"
                );
                (0.0f64, true)
            }
        },
        None => (0.0f64, true),
    };

    let slots = bar_cfg
        .and_then(|c| c.filled_text.as_deref())
        .and_then(filled_text_slots);
    let width = match &slots {
        Some(slots) => slots.len(),
        None => bar_cfg.and_then(|c| c.width).unwrap_or(DEFAULT_BAR_WIDTH) as usize,
    };
    let full_at = match bar_cfg.and_then(|c| c.full_at) {
        Some(pct) if pct.is_finite() && pct > 0.0 => {
            if pct > 100.0 {
                // used_percentage tops out at 100, so this bar can never fill
                crate::warn_once!(
                    "cship.context_bar: full_at is above 100, so the bar never fills"
                );
            }
            pct
        }
        Some(_) => {
            // 0, negative, NaN or infinite values can't scale the bar: fall back to 100
            crate::warn_once!(
                "cship.context_bar: full_at must be a finite number above 0; ignoring it"
            );
            100.0
        }
        None => 100.0,
    };
    // Floor via `as usize` truncation — NOT round. The percentage text uses {:.0} (round).
    // At boundary values like 99.5%, bar shows 9/10 filled while text shows "100%".
    // This is intentional: the bar is a visual approximation, the number is canonical.
    let filled = ((used_pct / full_at) * width as f64) as usize;
    let filled = filled.min(width); // cap once used_pct reaches full_at
    let empty = width - filled;

    let filled_char = bar_cfg
        .and_then(|c| c.filled_char.as_deref())
        .unwrap_or(DEFAULT_FILLED_CHAR);
    let empty_char = bar_cfg
        .and_then(|c| c.empty_char.as_deref())
        .unwrap_or(DEFAULT_EMPTY_CHAR);
    let (filled_part, empty_part) = match &slots {
        Some(slots) => {
            // The track covers the hidden text's display width, so wide characters
            // don't make the bar change width as it fills.
            let track_width = slots[filled..]
                .iter()
                .map(|s| crate::ansi::display_width(s))
                .sum();
            (slots[..filled].concat(), track(empty_char, track_width))
        }
        None => (filled_char.repeat(filled), empty_char.repeat(empty)),
    };
    let percentage = format!("{:.0}%", used_pct);
    let bar_content = format!("{filled_part}{empty_part} {percentage}");

    let symbol = bar_cfg.and_then(|c| c.symbol.as_deref());
    let format = bar_cfg.and_then(|c| c.format.as_deref());

    // Absent context: render 0% bar styled with empty_style (AC: 1, 2, 3)
    if is_empty {
        let empty_style = bar_cfg.and_then(|c| c.empty_style.as_deref());
        if let Some(fmt) = format {
            return crate::format::apply_module_format(
                fmt,
                Some(&bar_content),
                symbol,
                empty_style,
            );
        }
        return Some(crate::ansi::apply_style(&bar_content, empty_style));
    }

    let threshold_style = crate::ansi::resolve_threshold_style(
        Some(used_pct),
        bar_cfg.and_then(|c| c.style.as_deref()),
        bar_cfg.and_then(|c| c.warn_threshold),
        bar_cfg.and_then(|c| c.warn_style.as_deref()),
        bar_cfg.and_then(|c| c.critical_threshold),
        bar_cfg.and_then(|c| c.critical_style.as_deref()),
    );

    // Per-part styles paint the filled slots, the track and the percentage on their own,
    // with the threshold style on the percentage. An outer style's attributes (e.g. bold)
    // would leak into any part that doesn't reset them, so `$style` is left empty and a
    // style written in `format` is cancelled with a reset before the bar.
    // A blank part style counts as unset, so it can't turn off the threshold style.
    let filled_style = bar_cfg
        .and_then(|c| c.filled_style.as_deref())
        .filter(|s| !s.trim().is_empty());
    let track_style = bar_cfg
        .and_then(|c| c.track_style.as_deref())
        .filter(|s| !s.trim().is_empty());
    if filled_style.is_some() || track_style.is_some() {
        // An empty part (the text at 0%, the track at 100%) is left out rather than
        // painted as a bare pair of escape codes.
        let paint = |part: &str, style: Option<&str>| match part {
            "" => String::new(),
            _ => crate::ansi::apply_style(part, style),
        };
        let painted = format!(
            "{}{} {}",
            paint(&filled_part, filled_style.or(threshold_style)),
            paint(&empty_part, track_style.or(threshold_style)),
            crate::ansi::apply_style(&percentage, threshold_style),
        );
        // `$symbol` takes the threshold style itself.
        let symbol = symbol.map(|s| paint(s, threshold_style));
        return match format {
            Some(fmt) => {
                if fmt.contains("$style") {
                    crate::warn_once!(
                        "cship.context_bar: $style is empty while filled_style or track_style \
                         is set; text styled with it renders unstyled"
                    );
                }
                // Start the bar from a reset, so a style written directly in `format`
                // (e.g. `[$value](bold)`) doesn't carry into the parts.
                let painted = crate::ansi::after_reset(&painted);
                crate::format::apply_module_format(fmt, Some(&painted), symbol.as_deref(), None)
            }
            None => Some(painted),
        };
    }

    // Format string takes priority if configured (AC2)
    if let Some(fmt) = format {
        return crate::format::apply_module_format(
            fmt,
            Some(&bar_content),
            symbol,
            threshold_style,
        );
    }

    // Default behavior — unchanged (AC5): threshold-style logic
    Some(crate::ansi::apply_style(&bar_content, threshold_style))
}

/// Splits `filled_text` into slots after removing control characters, which would
/// otherwise leak escape sequences or line breaks into the status line. Returns `None`,
/// so the bar falls back to `filled_char`, when nothing with display width is left.
fn filled_text_slots(text: &str) -> Option<Vec<String>> {
    let clean = crate::ansi::sanitize_control(text);
    if clean.len() != text.len() {
        crate::warn_once!(
            "cship.context_bar: filled_text contains control characters; removing them"
        );
    }
    let slots = text_slots(&clean);
    if slots.is_empty() {
        crate::warn_once!("cship.context_bar: filled_text has zero display width; ignoring it");
        return None;
    }
    Some(slots)
}

/// Splits `text` into one slot per visible character: zero-width characters, such as
/// combining accents, join the character before them. A zero-width character with no
/// character before it is dropped, since it would combine with whatever precedes the
/// bar. Note: emoji made of several characters (skin tones, flags, joined emoji like
/// 👨‍👩‍👦) aren't supported: they split into several slots, so they can reveal in pieces
/// and shift the bar's width. Splitting into grapheme clusters would need a new dependency.
fn text_slots(text: &str) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;
    let mut slots: Vec<String> = Vec::new();
    for c in text.chars() {
        if c.width() == Some(0) {
            if let Some(last) = slots.last_mut() {
                last.push(c);
            }
        } else {
            slots.push(c.to_string());
        }
    }
    slots
}

/// Fills `width` columns with `empty_char`, padding with spaces when its display width
/// doesn't divide `width` evenly, so the track always spans exactly `width` columns.
fn track(empty_char: &str, width: usize) -> String {
    match crate::ansi::display_width(empty_char) {
        0 => empty_char.repeat(width),
        char_width => empty_char.repeat(width / char_width) + &" ".repeat(width % char_width),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi::apply_style;
    use crate::config::{ContextBarConfig, CshipConfig};
    use crate::context::{Context, ContextWindow};

    fn ctx_with_pct(pct: f64) -> Context {
        Context {
            context_window: Some(ContextWindow {
                used_percentage: Some(pct),
                remaining_percentage: Some(100.0 - pct),
                context_window_size: Some(200000),
                total_input_tokens: Some(15234),
                total_output_tokens: Some(4521),
                current_usage: None,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_context_bar_35_percent_3_filled_7_empty() {
        let ctx = ctx_with_pct(35.0);
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        // 35% of 10 = 3.5 → floor → 3 filled, 7 empty → "███░░░░░░░ 35%"
        let filled: usize = result.chars().filter(|&c| c == '█').count();
        let empty: usize = result.chars().filter(|&c| c == '░').count();
        assert_eq!(filled, 3, "expected 3 filled chars: {result:?}");
        assert_eq!(empty, 7, "expected 7 empty chars: {result:?}");
        assert!(result.contains("35%"), "expected '35%' in: {result:?}");
    }

    #[test]
    fn test_context_bar_disabled_returns_none() {
        let ctx = ctx_with_pct(50.0);
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render(&ctx, &cfg), None);
    }

    #[test]
    fn test_context_bar_absent_context_window_renders_empty_bar() {
        let ctx = Context::default(); // context_window is None
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        let empty_count: usize = result.chars().filter(|&c| c == '░').count();
        assert_eq!(
            empty_count, DEFAULT_BAR_WIDTH as usize,
            "all chars should be empty: {result:?}"
        );
        assert!(result.contains("0%"), "should show 0%: {result:?}");
        assert!(!result.contains('█'), "no filled chars: {result:?}");
    }

    #[test]
    fn test_context_bar_absent_with_empty_style_applies_ansi() {
        let ctx = Context::default(); // context_window is None
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                empty_style: Some("dimmed".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI codes for empty_style dimmed: {result:?}"
        );
    }

    #[test]
    fn test_context_bar_absent_no_empty_style_no_ansi() {
        let ctx = Context::default(); // context_window is None
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        assert!(
            !result.contains('\x1b'),
            "expected no ANSI codes when no style configured: {result:?}"
        );
    }

    #[test]
    fn test_context_bar_disabled_absent_returns_none() {
        let ctx = Context::default(); // context_window is None
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render(&ctx, &cfg), None);
    }

    #[test]
    fn test_context_bar_custom_width_5() {
        let ctx = ctx_with_pct(40.0);
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                width: Some(5),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        // 40% of 5 = 2.0 → 2 filled, 3 empty
        let total_bar: usize = result.chars().filter(|&c| c == '█' || c == '░').count();
        assert_eq!(total_bar, 5, "expected total bar width 5: {result:?}");
        assert_eq!(
            result.chars().filter(|&c| c == '█').count(),
            2,
            "expected 2 filled: {result:?}"
        );
    }

    #[test]
    fn test_context_bar_warn_threshold_applies_ansi() {
        let ctx = ctx_with_pct(75.0);
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                warn_threshold: Some(70.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(85.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI codes for warn: {result:?}"
        );
    }

    #[test]
    fn test_context_bar_critical_threshold_applies_ansi() {
        let ctx = ctx_with_pct(90.0);
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                warn_threshold: Some(70.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(85.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI codes for critical: {result:?}"
        );
    }

    #[test]
    fn test_context_bar_100_percent_all_filled() {
        let ctx = ctx_with_pct(100.0);
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        assert!(
            !result.contains('░'),
            "expected no empty chars at 100%: {result:?}"
        );
        assert!(result.contains("100%"));
    }

    #[test]
    fn test_context_bar_boundary_15_percent_floors_to_1_filled() {
        let ctx = ctx_with_pct(15.0);
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        // 15% of 10 = 1.5 → floor → 1 filled, 9 empty (NOT round → 2)
        let filled: usize = result.chars().filter(|&c| c == '█').count();
        let empty: usize = result.chars().filter(|&c| c == '░').count();
        assert_eq!(filled, 1, "15% should floor to 1 filled: {result:?}");
        assert_eq!(empty, 9, "15% should leave 9 empty: {result:?}");
        assert!(result.contains("15%"), "expected '15%' in: {result:?}");
    }

    #[test]
    fn test_context_bar_boundary_99_5_percent_floors_to_9_filled() {
        let ctx = ctx_with_pct(99.5);
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        // 99.5% of 10 = 9.95 → floor → 9 filled, 1 empty; text rounds to "100%"
        let filled: usize = result.chars().filter(|&c| c == '█').count();
        let empty: usize = result.chars().filter(|&c| c == '░').count();
        assert_eq!(filled, 9, "99.5% should floor to 9 filled: {result:?}");
        assert_eq!(empty, 1, "99.5% should leave 1 empty: {result:?}");
        // {:.0} rounds 99.5 to "100" (banker's rounding) — this is expected
        assert!(
            result.contains("100%") || result.contains("99%"),
            "expected rounded percentage in: {result:?}"
        );
    }

    #[test]
    fn test_context_bar_0_percent_all_empty() {
        let ctx = ctx_with_pct(0.0);
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        assert!(
            !result.contains('█'),
            "expected no filled chars at 0%: {result:?}"
        );
        assert!(result.contains("0%"));
    }

    #[test]
    fn test_context_bar_format_below_threshold_uses_base_style() {
        // AC2: format + value below all thresholds → base style (None) used
        let ctx = ctx_with_pct(50.0); // below warn_threshold of 70.0
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                format: Some("[$value]($style)".to_string()),
                warn_threshold: Some(70.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(85.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            !result.contains('\x1b'),
            "expected NO ANSI codes below threshold with no base style: {result:?}"
        );
    }

    #[test]
    fn test_context_bar_format_with_warn_threshold_uses_warn_style() {
        // AC2: format + warn_threshold → warn_style flows into format renderer
        let ctx = ctx_with_pct(75.0); // above warn_threshold of 70.0
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                format: Some("[$value]($style)".to_string()),
                warn_threshold: Some(70.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(85.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI codes for warn style: {result:?}"
        );
    }

    #[test]
    fn test_context_bar_format_with_critical_threshold_uses_critical_style() {
        // AC2: format + critical_threshold → critical_style flows into format renderer
        let ctx = ctx_with_pct(90.0); // above critical_threshold of 85.0
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                format: Some("[$value]($style)".to_string()),
                warn_threshold: Some(70.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(85.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI codes for critical style: {result:?}"
        );
    }

    #[test]
    fn test_context_bar_format_warn_and_critical_produce_different_styles() {
        // M1 fix: verify warn and critical styles are distinguishable
        let warn_cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                format: Some("[$value]($style)".to_string()),
                warn_threshold: Some(70.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(100.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let crit_cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                format: Some("[$value]($style)".to_string()),
                warn_threshold: Some(70.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(70.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let ctx = ctx_with_pct(75.0);
        let warn_result = render(&ctx, &warn_cfg).unwrap();
        let crit_result = render(&ctx, &crit_cfg).unwrap();
        assert_ne!(
            warn_result, crit_result,
            "warn and critical styles must produce different output"
        );
    }

    #[test]
    fn test_context_bar_custom_filled_and_empty_chars() {
        let ctx = ctx_with_pct(40.0);
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                filled_char: Some("●".to_string()),
                empty_char: Some("○".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        // 40% of 10 = 4 filled, 6 empty
        let filled: usize = result.chars().filter(|&c| c == '●').count();
        let empty: usize = result.chars().filter(|&c| c == '○').count();
        assert_eq!(filled, 4, "expected 4 filled circles: {result:?}");
        assert_eq!(empty, 6, "expected 6 empty circles: {result:?}");
        assert!(result.contains("40%"), "expected '40%' in: {result:?}");
    }

    #[test]
    fn test_context_bar_only_filled_char_overridden() {
        let ctx = ctx_with_pct(50.0);
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                filled_char: Some("●".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        let filled: usize = result.chars().filter(|&c| c == '●').count();
        let empty: usize = result.chars().filter(|&c| c == '░').count();
        assert_eq!(filled, 5, "expected 5 filled circles: {result:?}");
        assert_eq!(empty, 5, "expected 5 default empty chars: {result:?}");
    }

    fn render_bar(pct: f64, bar: ContextBarConfig) -> String {
        let cfg = CshipConfig {
            context_bar: Some(bar),
            ..Default::default()
        };
        render(&ctx_with_pct(pct), &cfg).unwrap()
    }

    #[test]
    fn test_context_bar_filled_text_reveals_one_char_per_slot() {
        let bar = || ContextBarConfig {
            filled_text: Some("CONTEXT WINDOW".to_string()),
            width: Some(4), // ignored: the bar is as wide as the text
            ..Default::default()
        };
        assert_eq!(render_bar(0.0, bar()), "░░░░░░░░░░░░░░ 0%");
        assert_eq!(render_bar(50.0, bar()), "CONTEXT░░░░░░░ 50%");
        assert_eq!(render_bar(100.0, bar()), "CONTEXT WINDOW 100%");
    }

    #[test]
    fn test_context_bar_filled_text_keeps_its_width_with_wide_chars() {
        let bar = || ContextBarConfig {
            filled_text: Some("日本語テキスト".to_string()),
            ..Default::default()
        };
        // 7 characters, each two columns wide: the track stands in for the hidden ones
        assert_eq!(render_bar(0.0, bar()), format!("{} 0%", "░".repeat(14)));
        assert_eq!(
            render_bar(50.0, bar()),
            format!("日本語{} 50%", "░".repeat(8))
        );
        assert_eq!(render_bar(100.0, bar()), "日本語テキスト 100%");
    }

    #[test]
    fn test_context_bar_filled_text_joins_combining_marks() {
        let bar = || ContextBarConfig {
            filled_text: Some("cafe\u{301} bar".to_string()),
            ..Default::default()
        };
        // 8 slots: the accent joins its "e"
        assert_eq!(render_bar(50.0, bar()), "cafe\u{301}░░░░ 50%");
        assert_eq!(render_bar(100.0, bar()), "cafe\u{301} bar 100%");
    }

    #[test]
    fn test_context_bar_invisible_filled_text_keeps_default_bar() {
        for text in ["", "\u{301}"] {
            let bar = ContextBarConfig {
                filled_text: Some(text.to_string()),
                ..Default::default()
            };
            assert_eq!(
                render_bar(50.0, bar),
                "█████░░░░░ 50%",
                "filled_text {text:?}"
            );
        }
    }

    #[test]
    fn test_context_bar_filled_text_with_format_uses_the_threshold_style() {
        let bar = ContextBarConfig {
            filled_text: Some("CONTEXT WINDOW".to_string()),
            format: Some("[$value]($style)".to_string()),
            warn_threshold: Some(50.0),
            warn_style: Some("yellow".to_string()),
            ..Default::default()
        };
        let expected = apply_style("CONTEXT░░░░░░░ 50%", Some("yellow"));
        assert_eq!(render_bar(50.0, bar), expected);
    }

    #[test]
    fn test_context_bar_full_at_fills_the_bar_early() {
        let bar = || ContextBarConfig {
            full_at: Some(95.0),
            ..Default::default()
        };
        assert_eq!(render_bar(90.0, bar()), "█████████░ 90%"); // 90/95 of 10 → 9
        assert_eq!(render_bar(95.0, bar()), "██████████ 95%");
        assert_eq!(render_bar(100.0, bar()), "██████████ 100%"); // capped at the width
    }

    #[test]
    fn test_context_bar_invalid_full_at_is_ignored() {
        for full_at in [0.0, -5.0, f64::NAN, f64::INFINITY] {
            let bar = ContextBarConfig {
                full_at: Some(full_at),
                ..Default::default()
            };
            assert_eq!(render_bar(50.0, bar), "█████░░░░░ 50%", "full_at {full_at}");
        }
    }

    #[test]
    fn test_context_bar_full_at_above_100_is_honoured() {
        let bar = ContextBarConfig {
            full_at: Some(150.0),
            ..Default::default()
        };
        assert_eq!(render_bar(100.0, bar), "██████░░░░ 100%"); // 100/150 of 10 → 6
    }

    #[test]
    fn test_context_bar_full_at_does_not_move_the_thresholds() {
        let bar = || ContextBarConfig {
            full_at: Some(95.0),
            critical_threshold: Some(98.0),
            critical_style: Some("red".to_string()),
            ..Default::default()
        };
        assert_eq!(render_bar(96.0, bar()), "██████████ 96%"); // full, not yet critical
        let expected = apply_style("██████████ 98%", Some("red"));
        assert_eq!(render_bar(98.0, bar()), expected);
    }

    #[test]
    fn test_context_bar_part_styles_paint_each_part_separately() {
        let bar = ContextBarConfig {
            filled_style: Some("bold".to_string()),
            track_style: Some("dimmed".to_string()),
            style: Some("red".to_string()),
            ..Default::default()
        };
        let expected = format!(
            "{}{} {}",
            apply_style("█████", Some("bold")),
            apply_style("░░░░░", Some("dimmed")),
            apply_style("50%", Some("red")),
        );
        assert_eq!(render_bar(50.0, bar), expected);
    }

    #[test]
    fn test_context_bar_unset_part_style_falls_back_to_threshold_style() {
        let bar = ContextBarConfig {
            filled_style: Some("bold".to_string()),
            critical_threshold: Some(90.0),
            critical_style: Some("red".to_string()),
            ..Default::default()
        };
        let red = |s: &str| apply_style(s, Some("red"));
        let expected = format!(
            "{}{} {}",
            apply_style("█████████", Some("bold")),
            red("░"),
            red("95%")
        );
        assert_eq!(render_bar(95.0, bar), expected);
    }

    #[test]
    fn test_context_bar_format_style_does_not_wrap_part_styles() {
        let bar = ContextBarConfig {
            track_style: Some("dimmed".to_string()),
            style: Some("bold".to_string()),
            format: Some("[$value]($style)".to_string()),
            ..Default::default()
        };
        let bold = |s: &str| apply_style(s, Some("bold"));
        let parts = format!(
            "{}{} {}",
            bold("█████"),
            apply_style("░░░░░", Some("dimmed")),
            bold("50%")
        );
        assert_eq!(render_bar(50.0, bar), crate::ansi::after_reset(&parts));
    }

    #[test]
    fn test_context_bar_part_styles_give_the_symbol_the_threshold_style() {
        let bar = ContextBarConfig {
            filled_style: Some("bold".to_string()),
            symbol: Some("CTX ".to_string()),
            format: Some("[$symbol$value]($style)".to_string()),
            critical_threshold: Some(90.0),
            critical_style: Some("red".to_string()),
            ..Default::default()
        };
        let red = |s: &str| apply_style(s, Some("red"));
        let bold = apply_style("█████████", Some("bold"));
        let parts = format!("{bold}{} {}", red("░"), red("95%"));
        let expected = format!("{}{}", red("CTX "), crate::ansi::after_reset(&parts));
        assert_eq!(render_bar(95.0, bar), expected);
    }

    #[test]
    fn test_context_bar_part_styles_survive_a_style_written_in_format() {
        let bar = ContextBarConfig {
            filled_style: Some("bold".to_string()),
            track_style: Some("dimmed".to_string()),
            format: Some("[$value](fg:#FFFFFF)".to_string()),
            ..Default::default()
        };
        let result = render_bar(50.0, bar);
        assert!(
            result.contains(&apply_style("█████", Some("bold"))),
            "{result:?}"
        );
        assert!(
            result.contains(&apply_style("░░░░░", Some("dimmed"))),
            "{result:?}"
        );
    }

    /// The visible characters of `s` a terminal would draw bold. Handles the plain SGR
    /// codes these tests use (`1` bold, `0` reset, named colours).
    fn bold_chars(s: &str) -> String {
        let (mut bold, mut out, mut rest) = (false, String::new(), s);
        while let Some(c) = rest.chars().next() {
            if let Some(seq) = rest.strip_prefix("\x1b[") {
                let end = seq.find('m').unwrap();
                for code in seq[..end].split(';') {
                    match code {
                        "0" => bold = false,
                        "1" => bold = true,
                        _ => {}
                    }
                }
                rest = &seq[end + 1..];
            } else {
                if bold {
                    out.push(c);
                }
                rest = &rest[c.len_utf8()..];
            }
        }
        out
    }

    #[test]
    fn test_context_bar_style_written_in_format_does_not_reach_the_parts() {
        let bar = || ContextBarConfig {
            filled_text: Some("CONTEXT WINDOW".to_string()),
            filled_style: Some("red".to_string()),
            format: Some("[$value](bold)".to_string()),
            ..Default::default()
        };
        for pct in [0.0, 50.0, 100.0] {
            assert_eq!(bold_chars(&render_bar(pct, bar())), "", "at {pct}%");
        }
    }

    #[test]
    fn test_context_bar_part_styles_leave_out_empty_parts() {
        let bar = || ContextBarConfig {
            filled_style: Some("bold".to_string()),
            track_style: Some("dimmed".to_string()),
            ..Default::default()
        };
        let empty = format!("{} 0%", apply_style("░░░░░░░░░░", Some("dimmed")));
        let full = format!("{} 100%", apply_style("██████████", Some("bold")));
        assert_eq!(render_bar(0.0, bar()), empty);
        assert_eq!(render_bar(100.0, bar()), full);
    }

    #[test]
    fn test_context_bar_part_styles_not_used_without_context_data() {
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                track_style: Some("bold".to_string()),
                empty_style: Some("dimmed".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&Context::default(), &cfg).unwrap();
        assert_eq!(result, apply_style("░░░░░░░░░░ 0%", Some("dimmed")));
    }

    #[test]
    fn test_context_bar_filled_text_strips_control_chars() {
        let bar = || ContextBarConfig {
            filled_text: Some("\x1b[1mAB\nC".to_string()),
            ..Default::default()
        };
        for pct in [0.0, 30.0, 50.0, 100.0] {
            let result = render_bar(pct, bar());
            assert!(
                !result.contains(['\x1b', '\n']),
                "control char leaked at {pct}%: {result:?}"
            );
        }
        // ESC and the newline are gone; the printable remnant stays as plain text
        assert_eq!(render_bar(100.0, bar()), "[1mABC 100%");
    }

    #[test]
    fn test_context_bar_filled_text_of_only_control_chars_keeps_default_bar() {
        let bar = ContextBarConfig {
            filled_text: Some("\n\t".to_string()),
            ..Default::default()
        };
        assert_eq!(render_bar(50.0, bar), "█████░░░░░ 50%");
    }

    #[test]
    fn test_context_bar_filled_text_drops_a_leading_zero_width_char() {
        let bar = || ContextBarConfig {
            filled_text: Some("\u{301}AB".to_string()),
            ..Default::default()
        };
        // 2 slots, not 3: the first fill step shows "A"
        assert_eq!(render_bar(0.0, bar()), "░░ 0%");
        assert_eq!(render_bar(50.0, bar()), "A░ 50%");
        assert_eq!(render_bar(100.0, bar()), "AB 100%");
    }

    #[test]
    fn test_context_bar_filled_text_keeps_its_width_with_a_wide_empty_char() {
        let bar = |text: &str| ContextBarConfig {
            filled_text: Some(text.to_string()),
            empty_char: Some("⬜".to_string()),
            ..Default::default()
        };
        assert_eq!(render_bar(0.0, bar("ABCD")), "⬜⬜ 0%");
        assert_eq!(render_bar(50.0, bar("ABCD")), "AB⬜ 50%");
        assert_eq!(render_bar(100.0, bar("ABCD")), "ABCD 100%");
        // An odd column left over is padded with a space
        assert_eq!(render_bar(0.0, bar("ABC")), "⬜  0%");
        assert_eq!(render_bar(34.0, bar("ABC")), "A⬜ 34%");
        for pct in [0.0, 34.0, 67.0, 100.0] {
            let bar_width = crate::ansi::display_width(&render_bar(pct, bar("ABC")))
                - crate::ansi::display_width(&format!(" {pct:.0}%"));
            assert_eq!(bar_width, 3, "at {pct}%");
        }
    }

    #[test]
    fn test_context_bar_blank_part_style_keeps_the_threshold_style() {
        let bar = ContextBarConfig {
            filled_style: Some("  ".to_string()),
            critical_threshold: Some(90.0),
            critical_style: Some("red".to_string()),
            ..Default::default()
        };
        assert_eq!(
            render_bar(95.0, bar),
            apply_style("█████████░ 95%", Some("red"))
        );
    }

    #[test]
    fn test_context_bar_part_styles_follow_the_warn_and_critical_tiers() {
        let bar = || ContextBarConfig {
            track_style: Some("dimmed".to_string()),
            warn_threshold: Some(50.0),
            warn_style: Some("yellow".to_string()),
            critical_threshold: Some(90.0),
            critical_style: Some("red".to_string()),
            ..Default::default()
        };
        let dimmed = |s: &str| apply_style(s, Some("dimmed"));
        let yellow = |s: &str| apply_style(s, Some("yellow"));
        let red = |s: &str| apply_style(s, Some("red"));
        assert_eq!(
            render_bar(40.0, bar()),
            format!("████{} 40%", dimmed("░░░░░░"))
        );
        assert_eq!(
            render_bar(60.0, bar()),
            format!("{}{} {}", yellow("██████"), dimmed("░░░░"), yellow("60%"))
        );
        assert_eq!(
            render_bar(95.0, bar()),
            format!("{}{} {}", red("█████████"), dimmed("░"), red("95%"))
        );
    }

    #[test]
    fn test_context_bar_style_written_in_format_ends_at_the_bar() {
        // Documented: with part styles set, a span's style stops where `$value` starts,
        // so literal text after `$value` in the same span renders unstyled.
        let bar = ContextBarConfig {
            filled_style: Some("red".to_string()),
            format: Some("[CTX $value tokens](bold)".to_string()),
            ..Default::default()
        };
        assert_eq!(bold_chars(&render_bar(50.0, bar)), "CTX ");
    }

    #[test]
    fn test_context_bar_filled_text_without_context_data_uses_empty_style() {
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                filled_text: Some("CTX".to_string()),
                full_at: Some(95.0),
                filled_style: Some("bold".to_string()),
                empty_style: Some("dimmed".to_string()),
                format: Some("[$value]($style)".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&Context::default(), &cfg).unwrap();
        assert_eq!(result, apply_style("░░░ 0%", Some("dimmed")));
    }
}
