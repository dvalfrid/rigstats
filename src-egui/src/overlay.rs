//! Compact click-through overlay mode (issue #183): a metric registry plus a
//! small renderer for a corner-anchored or free-dragged strip of chosen
//! metrics, meant to sit over a game without stealing input.

use crate::dashboard::PanelThresholds;
use crate::tempcolor::temp_color;
use crate::theme;
use crate::PollStats;
use eframe::egui;
use egui::{Color32, CornerRadius, Frame, Margin, RichText, Stroke};

const GIB: f64 = 1_073_741_824.0;

/// Chip label/value font size at `scale == 1.0`.
const CHIP_FONT_SIZE: f32 = 13.0;
/// `draw_overlay`'s background frame margin at `scale == 1.0` — kept as
/// constants (not literals) so the frame's actual margin and
/// `estimate_window_size`'s prediction of it can never drift apart.
const FRAME_MARGIN_X: f32 = 10.0;
const FRAME_MARGIN_Y: f32 = 6.0;
/// Widest value likely to be displayed, used only to pre-measure a stable
/// chip width — never the actual rendered value. Wider than any realistic
/// metric (percentages top out at "100.0"; most others stay under 4 digits),
/// so real content is never wider than the estimate by more than a
/// character or two, while staying completely independent of live values.
const WIDTH_PROBE_VALUE: f64 = 9999.9;

/// One selectable overlay metric: how to label it, extract its value from a
/// [`PollStats`] snapshot, and (optionally) which [`PanelThresholds`] field
/// colours it warn/crit.
pub struct OverlayMetric {
    pub key: &'static str,
    pub label: &'static str,
    pub unit: &'static str,
    pub extract: fn(&PollStats) -> Option<f64>,
    pub threshold_key: Option<&'static str>,
}

fn ram_pct(s: &PollStats) -> Option<f64> {
    (s.ram_total > 0).then(|| s.ram_used as f64 / s.ram_total as f64 * 100.0)
}

fn ram_used_gb(s: &PollStats) -> Option<f64> {
    Some(s.ram_used as f64 / GIB)
}

fn gpu_vram_pct(s: &PollStats) -> Option<f64> {
    match (s.gpu_vram_used_mb, s.gpu_vram_total_mb) {
        (Some(used), Some(total)) if total > 0.0 => Some(used / total * 100.0),
        _ => None,
    }
}

fn cpu_freq_ghz(s: &PollStats) -> Option<f64> {
    (s.cpu_freq_mhz > 0.0).then_some(s.cpu_freq_mhz / 1000.0)
}

fn battery_pct(s: &PollStats) -> Option<f64> {
    s.battery_charge_pct.map(|p| p as f64)
}

/// All metrics selectable in the Overlay settings tab, in the order they're
/// offered there. `key` values are what's persisted in `Settings.overlay_metrics`.
pub const ALL_OVERLAY_METRICS: &[OverlayMetric] = &[
    OverlayMetric {
        key: "cpu_load",
        label: "CPU",
        unit: "%",
        extract: |s| Some(s.cpu_load as f64),
        threshold_key: None,
    },
    OverlayMetric {
        key: "cpu_temp",
        label: "CPU",
        unit: "°C",
        extract: |s| s.cpu_temp,
        threshold_key: Some("cpu"),
    },
    OverlayMetric {
        key: "cpu_freq",
        label: "CPU",
        unit: " GHz",
        extract: cpu_freq_ghz,
        threshold_key: None,
    },
    OverlayMetric {
        key: "cpu_power",
        label: "CPU",
        unit: " W",
        extract: |s| s.cpu_power,
        threshold_key: None,
    },
    OverlayMetric {
        key: "gpu_load",
        label: "GPU",
        unit: "%",
        extract: |s| s.gpu_load,
        threshold_key: Some("gpu"),
    },
    OverlayMetric {
        key: "gpu_temp",
        label: "GPU",
        unit: "°C",
        extract: |s| s.gpu_temp,
        threshold_key: Some("gpu"),
    },
    OverlayMetric {
        key: "gpu_hotspot",
        label: "HOTSPOT",
        unit: "°C",
        extract: |s| s.gpu_hotspot,
        threshold_key: Some("gpu_hotspot"),
    },
    OverlayMetric {
        key: "gpu_core_clock",
        label: "GPU CLK",
        unit: " MHz",
        extract: |s| s.gpu_freq_mhz,
        threshold_key: None,
    },
    OverlayMetric {
        key: "gpu_mem_clock",
        label: "GPU MEM",
        unit: " MHz",
        extract: |s| s.gpu_mem_freq_mhz,
        threshold_key: None,
    },
    OverlayMetric {
        key: "gpu_vram_used",
        label: "VRAM",
        unit: " MB",
        extract: |s| s.gpu_vram_used_mb,
        threshold_key: None,
    },
    OverlayMetric {
        key: "gpu_vram_pct",
        label: "VRAM",
        unit: "%",
        extract: gpu_vram_pct,
        threshold_key: None,
    },
    OverlayMetric {
        key: "gpu_power",
        label: "GPU",
        unit: " W",
        extract: |s| s.gpu_power,
        threshold_key: None,
    },
    OverlayMetric {
        key: "gpu_fan",
        label: "GPU FAN",
        unit: "%",
        extract: |s| s.gpu_fan,
        threshold_key: None,
    },
    OverlayMetric {
        key: "ram_used",
        label: "RAM",
        unit: " GB",
        extract: ram_used_gb,
        threshold_key: None,
    },
    OverlayMetric {
        key: "ram_pct",
        label: "RAM",
        unit: "%",
        extract: ram_pct,
        threshold_key: Some("ram"),
    },
    OverlayMetric {
        key: "net_up",
        label: "UP",
        unit: " Mbps",
        extract: |s| Some(s.net_up_mbps),
        threshold_key: None,
    },
    OverlayMetric {
        key: "net_down",
        label: "DOWN",
        unit: " Mbps",
        extract: |s| Some(s.net_down_mbps),
        threshold_key: None,
    },
    OverlayMetric {
        key: "net_ping",
        label: "PING",
        unit: " ms",
        extract: |s| s.net_ping_ms,
        threshold_key: None,
    },
    OverlayMetric {
        key: "disk_read",
        label: "READ",
        unit: " MB/s",
        extract: |s| Some(s.disk_read_mbps),
        threshold_key: None,
    },
    OverlayMetric {
        key: "disk_write",
        label: "WRITE",
        unit: " MB/s",
        extract: |s| Some(s.disk_write_mbps),
        threshold_key: None,
    },
    OverlayMetric {
        key: "battery_pct",
        label: "BATT",
        unit: "%",
        extract: battery_pct,
        threshold_key: None,
    },
];

fn find_metric(key: &str) -> Option<&'static OverlayMetric> {
    ALL_OVERLAY_METRICS.iter().find(|m| m.key == key)
}

fn threshold_color(thresholds: &PanelThresholds, key: &str, value: Option<f64>) -> Color32 {
    let (warn, crit) = match key {
        "cpu" => thresholds.cpu,
        "gpu" => thresholds.gpu,
        "gpu_hotspot" => thresholds.gpu_hotspot,
        "ram" => thresholds.ram,
        "disk" => thresholds.disk,
        _ => return theme::C_TEXT,
    };
    temp_color(value, warn, crit)
}

/// Formats a chip's label/value/unit text — shared by `draw_chip` (the real
/// value) and `estimate_window_size` (a fixed probe value), so the two can
/// never disagree about what text is actually being sized/rendered.
fn chip_text(m: &OverlayMetric, value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{}: {v:.1}{}", m.label, m.unit),
        None => format!("{}: --", m.label),
    }
}

fn draw_chip(
    ui: &mut egui::Ui,
    th: &theme::AppTheme,
    latest: &PollStats,
    thresholds: &PanelThresholds,
    m: &OverlayMetric,
    sc: f32,
) {
    let value = (m.extract)(latest);
    let color = match m.threshold_key {
        Some(key) => threshold_color(thresholds, key, value),
        None => th.stat_label,
    };
    let text = chip_text(m, value);
    ui.label(
        RichText::new(text)
            .size(CHIP_FONT_SIZE * sc)
            .strong()
            .color(color),
    );
}

/// The background card's own inner padding around content, in window-local
/// px at the given `scale` — `[0.0, 0.0]` when `background` is off. Shared
/// by [`estimate_window_size`] (adds this to the content size to get the
/// window size) and `overlay_anchor_position` (subtracts it back out of the
/// window's outer-edge placement) so that an anchored overlay's *content* —
/// not the window's outer edge — stays at a fixed on-screen distance from
/// the monitor edge regardless of whether Background is enabled. Without
/// this shared source of truth, toggling Background visibly shifted the
/// text inward by the card's padding instead of just adding a card behind
/// it in place.
pub fn content_inset(scale: f32, background: bool) -> egui::Vec2 {
    if background {
        let sc = scale.max(0.1);
        egui::vec2((FRAME_MARGIN_X * sc).round(), (FRAME_MARGIN_Y * sc).round())
    } else {
        egui::Vec2::ZERO
    }
}

/// Pre-measures the overlay's window size for a metric selection, layout,
/// and scale — using [`WIDTH_PROBE_VALUE`] instead of live values, so the
/// result is completely stable across value changes, but still tightly
/// fits each metric's actual label/unit text (unlike a flat per-chip width
/// estimate, which either wastes space for short labels like `"CPU: 24.0%"`
/// or clips long ones like `"GPU CLK: 2850.0 MHz"`). Call this whenever the
/// metric list, layout, scale, or background setting changes; it needs a
/// live [`egui::Context`] to measure real font metrics, so it can't be used
/// for the very first frame's `ViewportBuilder` (see `geometry::overlay_window_size`
/// for that startup-only rough estimate, self-corrected by this function on
/// the first active frame).
pub fn estimate_window_size(
    ctx: &egui::Context,
    metrics: &[String],
    layout: &str,
    scale: f32,
    background: bool,
) -> egui::Vec2 {
    let sc = scale.max(0.1);
    let selected: Vec<&OverlayMetric> = metrics.iter().filter_map(|k| find_metric(k)).collect();
    let content = if selected.is_empty() {
        egui::vec2(60.0 * sc, CHIP_FONT_SIZE * sc * 1.3)
    } else {
        let font_id = egui::FontId::proportional(CHIP_FONT_SIZE * sc);
        let spacing = ctx.global_style().spacing.item_spacing;
        let (gap_x, gap_y) = (spacing.x, spacing.y);
        let mut line_h = 0.0_f32;
        let widths: Vec<f32> = ctx.fonts_mut(|f| {
            selected
                .iter()
                .map(|m| {
                    let text = chip_text(m, Some(WIDTH_PROBE_VALUE));
                    let galley = f.layout_no_wrap(text, font_id.clone(), Color32::WHITE);
                    line_h = line_h.max(galley.size().y);
                    galley.size().x
                })
                .collect()
        });
        match layout {
            "vertical" => {
                let w = widths.iter().cloned().fold(0.0_f32, f32::max);
                let h = line_h * widths.len() as f32 + gap_y * (widths.len() as f32 - 1.0).max(0.0);
                egui::vec2(w, h)
            }
            "grid" => {
                let cols = (widths.len() as f32).sqrt().ceil().max(1.0) as usize;
                let mut max_row_w = 0.0_f32;
                let mut rows = 0.0_f32;
                for row in widths.chunks(cols) {
                    rows += 1.0;
                    let row_w = row.iter().sum::<f32>() + gap_x * (row.len() as f32 - 1.0).max(0.0);
                    max_row_w = max_row_w.max(row_w);
                }
                let h = line_h * rows + gap_y * (rows - 1.0).max(0.0);
                egui::vec2(max_row_w, h)
            }
            _ => {
                let w = widths.iter().sum::<f32>() + gap_x * (widths.len() as f32 - 1.0).max(0.0);
                egui::vec2(w, line_h)
            }
        }
    };
    let inset = content_inset(scale, background);
    content + 2.0 * inset
}

/// Result of drawing one overlay frame: the drawn rect and the drag
/// interaction over it (the caller decides what a drag/drag-stop means —
/// e.g. sending `ViewportCommand::StartDrag` and persisting a new position).
pub struct OverlayResponse {
    pub rect: egui::Rect,
    pub response: egui::Response,
}

/// Draw the compact overlay strip for the given metric keys (unknown keys are
/// silently skipped, e.g. a stale key left over from a registry change).
///
/// The background frame always fills the *entire* space given by `ui`
/// (the whole overlay window — `estimate_window_size` sizes that window from
/// a fixed probe value, never live values, so it never jitters), with the
/// chip content aligned to the corner matching `anchor`.
/// This is what keeps the visible box's size and on-screen corner fixed
/// regardless of how wide any individual value happens to render this tick —
/// without it, a content-hugging frame drawn top-left inside a stably-sized
/// but larger-than-content window would appear to float/resize independently
/// of the window's actual (fixed) position.
#[allow(clippy::too_many_arguments)]
pub fn draw_overlay(
    ui: &mut egui::Ui,
    th: &theme::AppTheme,
    latest: &PollStats,
    thresholds: &PanelThresholds,
    metrics: &[String],
    layout: &str,
    anchor: &str,
    scale: f32,
    opacity: f32,
    background: bool,
) -> OverlayResponse {
    let sc = scale.max(0.1);
    let selected: Vec<&OverlayMetric> = metrics.iter().filter_map(|k| find_metric(k)).collect();
    let avail = ui.available_size();

    let margin_x = (FRAME_MARGIN_X * sc).round();
    let margin_y = (FRAME_MARGIN_Y * sc).round();
    if background {
        // The rounded frame below doesn't paint its own four corners (that's
        // the point of a corner radius) — on a window backed by a per-pixel
        // transparent (DComp) surface, whatever's *behind* those corner
        // notches is the OS's own default swap-chain content, not the
        // desktop/game the window sits over, which showed up as a small but
        // persistent white triangle in each corner regardless of overlay
        // size. A plain, sharp-cornered fill at the same color, painted
        // first and covering the exact same rect the frame will occupy,
        // sits seamlessly behind the rounded card and eliminates it.
        ui.painter().rect_filled(
            ui.available_rect_before_wrap(),
            0.0,
            theme::premul(theme::PANEL_FILL, opacity),
        );
    }
    let frame = if background {
        Frame {
            inner_margin: Margin::symmetric(margin_x as i8, margin_y as i8),
            outer_margin: Margin::ZERO,
            corner_radius: CornerRadius::same((6.0 * sc).round() as u8),
            fill: theme::premul(theme::PANEL_FILL, opacity),
            stroke: Stroke::new(0.5_f32, theme::premul(theme::PANEL_BORDER, opacity)),
            ..Default::default()
        }
    } else {
        Frame::NONE
    };
    // Content area inside the frame's own margin (zero when there's no
    // visible frame, since `Frame::NONE` has no margin).
    let content_size = if background {
        egui::vec2(
            (avail.x - 2.0 * margin_x).max(0.0),
            (avail.y - 2.0 * margin_y).max(0.0),
        )
    } else {
        avail
    };

    let h_align = if anchor.contains("right") {
        egui::Align::Max
    } else {
        egui::Align::Min
    };
    let content_layout = if anchor.starts_with("bottom") {
        egui::Layout::bottom_up(h_align)
    } else {
        egui::Layout::top_down(h_align)
    };

    // `allocate_ui_with_layout` *pre-reserves* an exact-size rect before the
    // layout aligns anything inside it — the same pattern `theme::fixed_label_r`
    // uses to right-align a dynamically-sized label in a fixed column. Handing
    // the layout a loose/growing bound instead (e.g. `ui.with_layout` directly
    // on `ui`) does not reliably cross-align a child whose own size is only
    // known after its contents are added — confirmed empirically: content
    // rendered that way overflowed past the intended screen edge instead of
    // sitting flush against it. Pre-allocating here is also what makes the
    // frame's painted background expand to fill the whole (stably-sized, see
    // `geometry::overlay_window_size`) window instead of hugging content —
    // that's what keeps the visible box's size and corner fixed regardless of
    // how wide any individual value renders this tick.
    let inner = frame.show(ui, |ui| {
        ui.allocate_ui_with_layout(content_size, content_layout, |ui| match layout {
            "vertical" => {
                ui.vertical(|ui| {
                    for m in &selected {
                        draw_chip(ui, th, latest, thresholds, m, sc);
                    }
                });
            }
            "grid" => {
                let cols = (selected.len() as f32).sqrt().ceil().max(1.0) as usize;
                ui.vertical(|ui| {
                    for row in selected.chunks(cols) {
                        ui.horizontal(|ui| {
                            for m in row {
                                draw_chip(ui, th, latest, thresholds, m, sc);
                            }
                        });
                    }
                });
            }
            _ => {
                ui.horizontal(|ui| {
                    for m in &selected {
                        draw_chip(ui, th, latest, thresholds, m, sc);
                    }
                });
            }
        });
    });

    let rect = inner.response.rect;
    let drag_id = ui.id().with("overlay_drag_sense");
    let response = ui.interact(rect, drag_id, egui::Sense::drag());
    OverlayResponse { rect, response }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_metric_extractor_runs_against_default_stats() {
        let stats = PollStats::default();
        for m in ALL_OVERLAY_METRICS {
            // Must not panic; a default (mostly zeroed) snapshot is a valid input.
            let _ = (m.extract)(&stats);
        }
    }

    #[test]
    fn keys_are_unique() {
        let mut keys: Vec<&str> = ALL_OVERLAY_METRICS.iter().map(|m| m.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(
            keys.len(),
            ALL_OVERLAY_METRICS.len(),
            "duplicate metric key"
        );
    }

    #[test]
    fn cpu_load_extractor_reads_the_raw_percentage() {
        let mut stats = PollStats::default();
        stats.cpu_load = 42;
        let m = find_metric("cpu_load").unwrap();
        assert_eq!((m.extract)(&stats), Some(42.0));
    }

    #[test]
    fn cpu_freq_extractor_converts_mhz_to_ghz_and_hides_zero() {
        let mut stats = PollStats::default();
        assert_eq!(
            cpu_freq_ghz(&stats),
            None,
            "0 MHz means unavailable, not 0 GHz"
        );
        stats.cpu_freq_mhz = 3500.0;
        assert_eq!(cpu_freq_ghz(&stats), Some(3.5));
    }

    #[test]
    fn ram_pct_extractor_divides_used_by_total() {
        let mut stats = PollStats::default();
        assert_eq!(ram_pct(&stats), None, "zero total must not divide by zero");
        stats.ram_used = 50;
        stats.ram_total = 200;
        assert_eq!(ram_pct(&stats), Some(25.0));
    }

    #[test]
    fn gpu_vram_pct_extractor_requires_both_fields() {
        let mut stats = PollStats::default();
        assert_eq!(gpu_vram_pct(&stats), None);
        stats.gpu_vram_used_mb = Some(1024.0);
        assert_eq!(gpu_vram_pct(&stats), None, "total still missing");
        stats.gpu_vram_total_mb = Some(4096.0);
        assert_eq!(gpu_vram_pct(&stats), Some(25.0));
    }

    #[test]
    fn find_metric_returns_none_for_unknown_key() {
        assert!(find_metric("not_a_real_key").is_none());
    }

    #[test]
    fn estimate_window_size_zero_metrics_is_not_zero_sized() {
        // A user can save an empty metric list — the overlay must still be a
        // recoverable, visible (if empty) window, not a 0x0 one.
        let ctx = egui::Context::default();
        let size = estimate_window_size(&ctx, &[], "horizontal", 1.0, true);
        assert!(size.x > 0.0 && size.y > 0.0);
    }

    /// `Fonts` aren't loaded on a fresh `Context` until its first pass —
    /// `estimate_window_size` needs real font metrics, so tests that
    /// exercise the non-empty-metrics path must run one empty pass first.
    fn ctx_with_fonts() -> egui::Context {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        ctx
    }

    #[test]
    fn estimate_window_size_horizontal_widens_with_more_metrics() {
        let ctx = ctx_with_fonts();
        let one = estimate_window_size(&ctx, &["cpu_load".to_string()], "horizontal", 1.0, true);
        let four = estimate_window_size(
            &ctx,
            &[
                "cpu_load".to_string(),
                "cpu_temp".to_string(),
                "gpu_load".to_string(),
                "gpu_temp".to_string(),
            ],
            "horizontal",
            1.0,
            true,
        );
        assert_eq!(one.y, four.y, "horizontal layout keeps a single row height");
        assert!(four.x > one.x, "more metrics must widen a horizontal strip");
    }

    #[test]
    fn estimate_window_size_background_adds_margin() {
        let ctx = ctx_with_fonts();
        let metrics = vec!["cpu_load".to_string()];
        let with_bg = estimate_window_size(&ctx, &metrics, "horizontal", 1.0, true);
        let without_bg = estimate_window_size(&ctx, &metrics, "horizontal", 1.0, false);
        assert!(with_bg.x > without_bg.x);
        assert!(with_bg.y > without_bg.y);
    }
}
