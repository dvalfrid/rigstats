//! Compact click-through overlay mode (issue #183): a metric registry plus a
//! small renderer for a corner-anchored or free-dragged strip of chosen
//! metrics, meant to sit over a game without stealing input.

use crate::dashboard::PanelThresholds;
use crate::panels::battery::charge_color;
use crate::tempcolor::{color_unknown, temp_color};
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

/// Horizontal space a grid column divider takes (line centred in it), at scale 1.
const GRID_DIVIDER_W: f32 = 17.0;

/// One selectable overlay metric: how to label it, extract its value from a
/// [`PollStats`] snapshot, and (optionally) how to colour it from the
/// current thresholds. `color` takes the full snapshot (not just the
/// extracted value) because some metrics need more than a scalar + a single
/// warn/crit pair — e.g. battery needs the `charging` flag too, to match
/// the main dashboard's battery panel.
pub struct OverlayMetric {
    pub key: &'static str,
    pub label: &'static str,
    pub unit: &'static str,
    pub extract: fn(&PollStats) -> Option<f64>,
    pub color: Option<fn(&PollStats, &PanelThresholds) -> Color32>,
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

fn cpu_load_color(s: &PollStats, t: &PanelThresholds) -> Color32 {
    temp_color(Some(s.cpu_load as f64), t.cpu_load.0, t.cpu_load.1)
}

fn cpu_temp_color(s: &PollStats, t: &PanelThresholds) -> Color32 {
    temp_color(s.cpu_temp, t.cpu.0, t.cpu.1)
}

fn gpu_load_color(s: &PollStats, t: &PanelThresholds) -> Color32 {
    temp_color(s.gpu_load, t.gpu_load.0, t.gpu_load.1)
}

fn gpu_temp_color(s: &PollStats, t: &PanelThresholds) -> Color32 {
    temp_color(s.gpu_temp, t.gpu.0, t.gpu.1)
}

fn gpu_hotspot_color(s: &PollStats, t: &PanelThresholds) -> Color32 {
    temp_color(s.gpu_hotspot, t.gpu_hotspot.0, t.gpu_hotspot.1)
}

fn ram_pct_color(s: &PollStats, t: &PanelThresholds) -> Color32 {
    // `t.ram` is RAM *temperature* (°C) — a usage percentage needs its own
    // threshold, same reasoning as cpu_load/gpu_load above. Reusing `t.ram`
    // here previously meant normal RAM usage (e.g. 70%) coincidentally read
    // as "critical" against a 70°C-designed cutoff.
    temp_color(ram_pct(s), t.ram_load.0, t.ram_load.1)
}

/// Low charge is bad (inverted vs. every other threshold here) and charging
/// overrides to accent regardless of level — same rule as the main
/// dashboard's battery panel, via the shared `charge_color`.
fn battery_color(s: &PollStats, t: &PanelThresholds) -> Color32 {
    match s.battery_charge_pct {
        Some(pct) => charge_color(
            pct,
            s.battery_charging.unwrap_or(false),
            t.battery.0,
            t.battery.1,
        ),
        None => color_unknown(),
    }
}

/// All metrics selectable in the Overlay settings tab, in the order they're
/// offered there. `key` values are what's persisted in `Settings.overlay_metrics`.
pub const ALL_OVERLAY_METRICS: &[OverlayMetric] = &[
    OverlayMetric {
        key: "cpu_load",
        label: "CPU",
        unit: "%",
        extract: |s| Some(s.cpu_load as f64),
        color: Some(cpu_load_color),
    },
    OverlayMetric {
        key: "cpu_temp",
        label: "CPU",
        unit: "°C",
        extract: |s| s.cpu_temp,
        color: Some(cpu_temp_color),
    },
    OverlayMetric {
        key: "cpu_freq",
        label: "CPU",
        unit: " GHz",
        extract: cpu_freq_ghz,
        color: None,
    },
    OverlayMetric {
        key: "cpu_power",
        label: "CPU",
        unit: " W",
        extract: |s| s.cpu_power,
        color: None,
    },
    OverlayMetric {
        key: "gpu_load",
        label: "GPU",
        unit: "%",
        extract: |s| s.gpu_load,
        color: Some(gpu_load_color),
    },
    OverlayMetric {
        key: "gpu_temp",
        label: "GPU",
        unit: "°C",
        extract: |s| s.gpu_temp,
        color: Some(gpu_temp_color),
    },
    OverlayMetric {
        key: "gpu_hotspot",
        label: "HOTSPOT",
        unit: "°C",
        extract: |s| s.gpu_hotspot,
        color: Some(gpu_hotspot_color),
    },
    OverlayMetric {
        key: "gpu_core_clock",
        label: "GPU CLK",
        unit: " MHz",
        extract: |s| s.gpu_freq_mhz,
        color: None,
    },
    OverlayMetric {
        key: "gpu_mem_clock",
        label: "GPU MEM",
        unit: " MHz",
        extract: |s| s.gpu_mem_freq_mhz,
        color: None,
    },
    OverlayMetric {
        key: "gpu_vram_used",
        label: "VRAM",
        unit: " MB",
        extract: |s| s.gpu_vram_used_mb,
        color: None,
    },
    OverlayMetric {
        key: "gpu_vram_pct",
        label: "VRAM",
        unit: "%",
        extract: gpu_vram_pct,
        color: None,
    },
    OverlayMetric {
        key: "gpu_power",
        label: "GPU",
        unit: " W",
        extract: |s| s.gpu_power,
        color: None,
    },
    OverlayMetric {
        key: "gpu_fan",
        label: "GPU FAN",
        unit: "%",
        extract: |s| s.gpu_fan,
        color: None,
    },
    OverlayMetric {
        key: "ram_used",
        label: "RAM",
        unit: " GB",
        extract: ram_used_gb,
        color: None,
    },
    OverlayMetric {
        key: "ram_pct",
        label: "RAM",
        unit: "%",
        extract: ram_pct,
        color: Some(ram_pct_color),
    },
    OverlayMetric {
        key: "net_up",
        label: "UP",
        unit: " Mbps",
        extract: |s| Some(s.net_up_mbps),
        color: None,
    },
    OverlayMetric {
        key: "net_down",
        label: "DOWN",
        unit: " Mbps",
        extract: |s| Some(s.net_down_mbps),
        color: None,
    },
    OverlayMetric {
        key: "net_ping",
        label: "PING",
        unit: " ms",
        extract: |s| s.net_ping_ms,
        color: None,
    },
    OverlayMetric {
        key: "disk_read",
        label: "READ",
        unit: " MB/s",
        extract: |s| Some(s.disk_read_mbps),
        color: None,
    },
    OverlayMetric {
        key: "disk_write",
        label: "WRITE",
        unit: " MB/s",
        extract: |s| Some(s.disk_write_mbps),
        color: None,
    },
    OverlayMetric {
        key: "battery_pct",
        label: "BATT",
        unit: "%",
        extract: battery_pct,
        color: Some(battery_color),
    },
];

fn find_metric(key: &str) -> Option<&'static OverlayMetric> {
    ALL_OVERLAY_METRICS.iter().find(|m| m.key == key)
}

/// Formats a chip's label/value/unit text — `draw_chip_row` draws the label
/// and value parts, `estimate_window_size` measures the joined `chip_text`
/// (a fixed probe value), so the two can never disagree about what text is
/// actually being sized/rendered.
fn chip_label(m: &OverlayMetric) -> String {
    format!("{}:", m.label)
}

fn chip_value(m: &OverlayMetric, value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{v:.1}{}", m.unit),
        None => "--".to_string(),
    }
}

fn chip_text(m: &OverlayMetric, value: Option<f64>) -> String {
    format!("{} {}", chip_label(m), chip_value(m, value))
}

/// Text width of each metric's chip at [`WIDTH_PROBE_VALUE`], plus the
/// tallest line height — the stable, value-independent sizes both the window
/// estimate and the drawn column widths are derived from.
fn probe_widths(
    ctx: &egui::Context,
    selected: &[&OverlayMetric],
    font_id: &egui::FontId,
) -> (Vec<f32>, f32) {
    let mut line_h = 0.0_f32;
    let widths = ctx.fonts_mut(|f| {
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
    (widths, line_h)
}

/// Fixed width of each column: the widest probe width among the metrics that
/// land in it (row-major fill, `cols` per row; `0` = all on one row).
fn grid_column_widths(widths: &[f32], cols: usize) -> Vec<f32> {
    let cols = if cols == 0 { widths.len() } else { cols };
    let cols = cols.clamp(1, widths.len().max(1));
    let mut col_w = vec![0.0_f32; cols];
    for (i, w) in widths.iter().enumerate() {
        col_w[i % cols] = col_w[i % cols].max(*w);
    }
    col_w
}

fn metric_color(
    th: &theme::AppTheme,
    latest: &PollStats,
    thresholds: &PanelThresholds,
    m: &OverlayMetric,
) -> Color32 {
    match m.color {
        Some(f) => f(latest, thresholds),
        None => th.stat_label,
    }
}

/// One cell: label flush left, value flush right within a fixed `width`, so
/// each column's left and right edges stay straight.
#[allow(clippy::too_many_arguments)] // per-metric render inputs plus the cell size
fn draw_chip_row(
    ui: &mut egui::Ui,
    th: &theme::AppTheme,
    latest: &PollStats,
    thresholds: &PanelThresholds,
    m: &OverlayMetric,
    sc: f32,
    width: f32,
    row_h: f32,
) {
    let color = metric_color(th, latest, thresholds, m);
    let text = |s: String| {
        RichText::new(s)
            .size(CHIP_FONT_SIZE * sc)
            .strong()
            .color(color)
    };
    ui.allocate_ui_with_layout(
        egui::vec2(width, row_h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(text(chip_label(m)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(text(chip_value(m, (m.extract)(latest))));
            });
        },
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
    columns: usize,
    scale: f32,
    background: bool,
) -> egui::Vec2 {
    let sc = scale.max(0.1);
    let selected: Vec<&OverlayMetric> = metrics.iter().filter_map(|k| find_metric(k)).collect();
    let content = if selected.is_empty() {
        egui::vec2(60.0 * sc, CHIP_FONT_SIZE * sc * 1.3)
    } else {
        let font_id = egui::FontId::proportional(CHIP_FONT_SIZE * sc);
        let gap_y = ctx.global_style().spacing.item_spacing.y;
        let (widths, line_h) = probe_widths(ctx, &selected, &font_id);
        let col_w = grid_column_widths(&widths, columns);
        let rows = widths.len().div_ceil(col_w.len()) as f32;
        let w =
            col_w.iter().sum::<f32>() + (GRID_DIVIDER_W * sc).round() * (col_w.len() as f32 - 1.0);
        let h = line_h * rows + gap_y * (rows - 1.0);
        egui::vec2(w, h)
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
    columns: usize,
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
        ui.allocate_ui_with_layout(content_size, content_layout, |ui| {
            // Each column gets a fixed width from the probe value (the same
            // widths the window was sized from), so values right-align against
            // a straight edge that doesn't move as they change. A thin divider
            // line separates columns.
            let font_id = egui::FontId::proportional(CHIP_FONT_SIZE * sc);
            let (widths, line_h) = probe_widths(ui.ctx(), &selected, &font_id);
            let col_w = grid_column_widths(&widths, columns);
            let div_w = (GRID_DIVIDER_W * sc).round();
            let grid = ui.vertical(|ui| {
                // `ui.horizontal` makes every row at least `interact_size.y`
                // tall, which is taller than one text line — the window is
                // sized from `line_h` per row, so the last row got clipped.
                ui.spacing_mut().interact_size.y = line_h;
                for row in selected.chunks(col_w.len()) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for (c, m) in row.iter().enumerate() {
                            if c > 0 {
                                ui.allocate_exact_size(
                                    egui::vec2(div_w, line_h),
                                    egui::Sense::hover(),
                                );
                            }
                            draw_chip_row(ui, th, latest, thresholds, m, sc, col_w[c], line_h);
                        }
                    });
                }
            });
            let r = grid.response.rect;
            let stroke = Stroke::new(1.0_f32, th.stat_label.gamma_multiply(0.4));
            let mut x = r.left();
            for w in &col_w[..col_w.len() - 1] {
                x += w + div_w;
                let line_x = (x - div_w / 2.0).round() + 0.5;
                ui.painter().vline(line_x, r.y_range(), stroke);
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
    use crate::tempcolor::{color_hot, color_ok, color_warn};

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
    fn chip_text_is_label_and_value_joined_by_a_space() {
        // The vertical layout draws label and value separately, but its column
        // width is measured from chip_text — they must stay in sync.
        let m = find_metric("cpu_load").unwrap();
        for v in [Some(4.0), None] {
            assert_eq!(
                chip_text(m, v),
                format!("{} {}", chip_label(m), chip_value(m, v))
            );
        }
        assert_eq!(
            chip_text(m, Some(4.0)),
            format!("{}: 4.0{}", m.label, m.unit)
        );
        assert_eq!(chip_text(m, None), format!("{}: --", m.label));
    }

    #[test]
    fn find_metric_returns_none_for_unknown_key() {
        assert!(find_metric("not_a_real_key").is_none());
    }

    #[test]
    fn cpu_load_color_uses_the_dedicated_load_threshold_not_the_temp_one() {
        // cpu_load is a percentage — it must not be colored by `thresholds.cpu`
        // (a °C threshold), only by the separate `thresholds.cpu_load`.
        let mut stats = PollStats::default();
        let mut t = PanelThresholds::default();
        t.cpu = (10, 20); // deliberately tiny, so a bug reusing it would show red
        t.cpu_load = (80, 95);
        stats.cpu_load = 50;
        assert_eq!(cpu_load_color(&stats, &t), color_ok());
        stats.cpu_load = 96;
        assert_eq!(cpu_load_color(&stats, &t), color_hot());
    }

    #[test]
    fn gpu_load_color_uses_the_dedicated_load_threshold_not_the_temp_one() {
        let mut stats = PollStats::default();
        let mut t = PanelThresholds::default();
        t.gpu = (10, 20);
        t.gpu_load = (80, 95);
        stats.gpu_load = Some(50.0);
        assert_eq!(gpu_load_color(&stats, &t), color_ok());
        stats.gpu_load = Some(96.0);
        assert_eq!(gpu_load_color(&stats, &t), color_hot());
    }

    #[test]
    fn ram_pct_color_uses_the_dedicated_usage_threshold_not_the_temp_one() {
        // ram_pct is usage % — must not be colored by `thresholds.ram` (RAM
        // temperature, °C), only by the separate `thresholds.ram_load`.
        let mut stats = PollStats::default();
        let mut t = PanelThresholds::default();
        t.ram = (10, 20);
        t.ram_load = (80, 95);
        stats.ram_used = 70;
        stats.ram_total = 100; // 70% usage — must not read as critical
        assert_eq!(ram_pct_color(&stats, &t), color_ok());
        stats.ram_used = 96;
        assert_eq!(ram_pct_color(&stats, &t), color_hot());
    }

    #[test]
    fn battery_color_is_inverted_low_charge_is_bad() {
        let mut stats = PollStats::default();
        let t = PanelThresholds::default(); // battery: warn=20, crit=10
        stats.battery_charge_pct = Some(50);
        stats.battery_charging = Some(false);
        assert_eq!(battery_color(&stats, &t), color_ok());
        stats.battery_charge_pct = Some(15);
        assert_eq!(battery_color(&stats, &t), color_warn());
        stats.battery_charge_pct = Some(5);
        assert_eq!(battery_color(&stats, &t), color_hot());
    }

    #[test]
    fn battery_color_charging_overrides_to_accent_regardless_of_level() {
        let mut stats = PollStats::default();
        let t = PanelThresholds::default();
        stats.battery_charge_pct = Some(5); // would be critical if discharging
        stats.battery_charging = Some(true);
        assert_eq!(battery_color(&stats, &t), theme::C_ACCENT);
    }

    #[test]
    fn battery_color_unknown_when_no_reading() {
        let stats = PollStats::default();
        let t = PanelThresholds::default();
        assert_eq!(battery_color(&stats, &t), color_unknown());
    }

    #[test]
    fn estimate_window_size_zero_metrics_is_not_zero_sized() {
        // A user can save an empty metric list — the overlay must still be a
        // recoverable, visible (if empty) window, not a 0x0 one.
        let ctx = egui::Context::default();
        let size = estimate_window_size(&ctx, &[], 0, 1.0, true);
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
    fn estimate_window_size_one_row_widens_with_more_metrics() {
        let ctx = ctx_with_fonts();
        let one = estimate_window_size(&ctx, &["cpu_load".to_string()], 0, 1.0, true);
        let four = estimate_window_size(
            &ctx,
            &[
                "cpu_load".to_string(),
                "cpu_temp".to_string(),
                "gpu_load".to_string(),
                "gpu_temp".to_string(),
            ],
            0,
            1.0,
            true,
        );
        assert_eq!(one.y, four.y, "one-row layout keeps a single row height");
        assert!(four.x > one.x, "more metrics must widen a one-row strip");
    }

    #[test]
    fn estimate_window_size_background_adds_margin() {
        let ctx = ctx_with_fonts();
        let metrics = vec!["cpu_load".to_string()];
        let with_bg = estimate_window_size(&ctx, &metrics, 0, 1.0, true);
        let without_bg = estimate_window_size(&ctx, &metrics, 0, 1.0, false);
        assert!(with_bg.x > without_bg.x);
        assert!(with_bg.y > without_bg.y);
    }

    #[test]
    fn grid_column_widths_take_the_widest_cell_per_column() {
        // Row-major: [a b] / [c d] / [e]
        let w = [10.0, 30.0, 25.0, 5.0, 12.0];
        assert_eq!(grid_column_widths(&w, 2), vec![25.0, 30.0]);
        assert_eq!(grid_column_widths(&w, 1), vec![30.0]);
        assert_eq!(grid_column_widths(&w, 0), w.to_vec(), "0 = one row");
        assert_eq!(
            grid_column_widths(&w[..2], 4),
            vec![10.0, 30.0],
            "never more columns than metrics"
        );
    }

    #[test]
    fn estimate_window_size_grid_column_count_trades_width_for_height() {
        let ctx = ctx_with_fonts();
        let metrics: Vec<String> = ["cpu_load", "cpu_temp", "gpu_load", "gpu_temp"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let two = estimate_window_size(&ctx, &metrics, 2, 1.0, true);
        let four = estimate_window_size(&ctx, &metrics, 4, 1.0, true);
        assert!(four.x > two.x);
        assert!(four.y < two.y);
    }
}
