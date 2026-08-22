//! Session History window — browse past recording sessions and chart them.

use crate::lock_ext::LockSafe;
use crate::theme::{self, DialogColors};
use eframe::egui;
use egui_plot::{Legend, Line, Plot, PlotPoints};
use rigstats_backend::logging::{self, SessionMeta, SessionRow, SessionSummary};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
pub struct HistoryState {
    pub sessions: Vec<SessionMeta>,
    pub selected: Option<String>,
    pub rows: Vec<SessionRow>,
    /// `(session id, in-progress name text)` while a rename is being edited.
    pub rename_draft: Option<(String, String)>,
}

impl HistoryState {
    pub fn placeholder() -> Self {
        Self::default()
    }
}

/// Loads the session list off the UI thread. No-op if a load is already in flight.
pub fn spawn_load_sessions(
    state: Arc<Mutex<HistoryState>>,
    refreshing: Arc<AtomicBool>,
    dir: PathBuf,
    ctx: egui::Context,
) {
    if refreshing.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        let sessions = logging::load_sessions(&dir);
        state.lock_safe().sessions = sessions;
        refreshing.store(false, Ordering::Relaxed);
        ctx.request_repaint();
    });
}

/// Loads one session's CSV rows (for charting) off the UI thread. No-op if a
/// load is already in flight.
pub fn spawn_load_rows(
    state: Arc<Mutex<HistoryState>>,
    loading: Arc<AtomicBool>,
    dir: PathBuf,
    id: String,
    ctx: egui::Context,
) {
    if loading.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(move || {
        let meta = {
            let st = state.lock_safe();
            st.sessions.iter().find(|s| s.id == id).cloned()
        };
        let rows = meta
            .map(|m| logging::read_session_rows(&dir, &m))
            .unwrap_or_default();
        {
            let mut st = state.lock_safe();
            st.selected = Some(id);
            st.rows = rows;
        }
        loading.store(false, Ordering::Relaxed);
        ctx.request_repaint();
    });
}

// ── Formatting helpers ───────────────────────────────────────────────────────

fn fmt_local(unix: u64) -> String {
    chrono::DateTime::from_timestamp(unix as i64, 0)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

fn fmt_duration(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

fn fmt_elapsed(secs: f64) -> String {
    let s = secs.max(0.0) as i64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m}:{sec:02}")
    }
}

fn session_duration_secs(meta: &SessionMeta) -> u64 {
    let end = meta.end_unix.unwrap_or_else(logging::unix_now_secs);
    end.saturating_sub(meta.start_unix)
}

// ── Widget helpers ────────────────────────────────────────────────────────────

fn card_frame(dc: &DialogColors) -> egui::Frame {
    egui::Frame::new()
        .fill(dc.card)
        .stroke(egui::Stroke::new(1.0_f32, dc.card_border))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(10, 8))
}

fn section_label(ui: &mut egui::Ui, dc: &DialogColors, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(11.0)
            .strong()
            .color(dc.label),
    );
    ui.add_space(4.0);
}

fn stat_tile(ui: &mut egui::Ui, dc: &DialogColors, label: &str, value: &str, color: egui::Color32) {
    egui::Frame::new()
        .fill(dc.inset)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(label).size(10.0).color(dc.muted));
                ui.label(egui::RichText::new(value).size(15.0).strong().color(color));
            });
        });
}

// ── Session list (left panel) ────────────────────────────────────────────────

/// Actions the list can request; applied after all panels have rendered.
#[derive(Default)]
struct ListActions {
    select: Option<String>,
    toggle_pin: Option<String>,
    delete: Option<String>,
    reveal: Option<String>,
    start_rename: Option<(String, String)>,
    commit_rename: Option<(String, String)>,
    cancel_rename: bool,
}

#[allow(clippy::too_many_arguments)]
fn render_session_row(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    meta: &SessionMeta,
    selected: bool,
    rename_draft: &mut Option<(String, String)>,
    actions: &mut ListActions,
) {
    let is_active = meta.is_active();
    let frame = egui::Frame::new()
        .fill(if selected {
            dc.inner
        } else {
            egui::Color32::TRANSPARENT
        })
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(8, 6));

    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());

        // Name row: inline edit if this session is being renamed.
        let editing = rename_draft.as_ref().is_some_and(|(id, _)| *id == meta.id);
        if editing {
            let (_, text) = rename_draft.as_mut().unwrap();
            let resp = ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
            resp.request_focus();
            if resp.lost_focus() {
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    actions.cancel_rename = true;
                } else {
                    actions.commit_rename = Some((meta.id.clone(), text.clone()));
                }
            }
        } else {
            // Every piece of the header (name, status/duration, avg stats) senses
            // its own click so the whole visual block acts as one target — not
            // just the name text — and shows a pointer cursor wherever hovered.
            let mut header_hovered = false;
            let mut header_clicked = false;
            let mut sense_click = |resp: egui::Response| {
                let resp = resp.interact(egui::Sense::click());
                header_hovered |= resp.hovered();
                header_clicked |= resp.clicked();
            };

            sense_click(
                ui.label(
                    egui::RichText::new(&meta.name)
                        .size(13.0)
                        .strong()
                        .color(if selected { dc.title } else { dc.text }),
                ),
            );

            ui.horizontal(|ui| {
                if is_active {
                    sense_click(
                        ui.label(
                            egui::RichText::new("● Recording")
                                .size(11.0)
                                .strong()
                                .color(theme::C_AMD),
                        ),
                    );
                } else {
                    sense_click(
                        ui.label(
                            egui::RichText::new(fmt_local(meta.start_unix))
                                .size(11.0)
                                .color(dc.muted),
                        ),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    sense_click(
                        ui.label(
                            egui::RichText::new(fmt_duration(session_duration_secs(meta)))
                                .size(11.0)
                                .color(dc.muted),
                        ),
                    );
                });
            });

            if !is_active {
                sense_click(
                    ui.label(
                        egui::RichText::new(format!(
                            "avg CPU {:.0}% · avg GPU {}",
                            meta.summary.avg_cpu_load,
                            meta.summary
                                .avg_gpu_load
                                .map(|v| format!("{v:.0}%"))
                                .unwrap_or_else(|| "—".to_string())
                        ))
                        .size(11.0)
                        .color(dc.muted),
                    ),
                );
            }

            if header_hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if header_clicked {
                actions.select = Some(meta.id.clone());
            }
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let pin_label = if meta.pinned { "Unpin" } else { "Pin" };
            if ui.small_button(pin_label).clicked() {
                actions.toggle_pin = Some(meta.id.clone());
            }
            if !editing && ui.small_button("Rename").clicked() {
                actions.start_rename = Some((meta.id.clone(), meta.name.clone()));
            }
            if ui.small_button("Reveal").clicked() {
                actions.reveal = Some(meta.id.clone());
            }
            if !is_active && ui.small_button("Delete").clicked() {
                actions.delete = Some(meta.id.clone());
            }
        });
    });
}

fn render_session_list(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    st: &HistoryState,
    rename_draft: &mut Option<(String, String)>,
) -> ListActions {
    let mut actions = ListActions::default();

    if st.sessions.is_empty() {
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(
                "No recording sessions yet.\nUse the tray's \"Start Recording\" to begin one.",
            )
            .size(12.0)
            .color(dc.muted),
        );
        return actions;
    }

    egui::ScrollArea::vertical()
        .id_salt("history_session_list")
        .show(ui, |ui| {
            for (i, meta) in st.sessions.iter().enumerate() {
                if i > 0 {
                    ui.add_space(2.0);
                }
                let selected = st.selected.as_deref() == Some(meta.id.as_str());
                render_session_row(ui, dc, meta, selected, rename_draft, &mut actions);
            }
        });

    actions
}

// ── Detail pane (charts) ─────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
/// One metric chart: title/unit for display, plus one `(name, color, points)`
/// series per line. `points` are `[elapsed_seconds, value]` pairs.
struct ChartSpec {
    id: &'static str,
    title: &'static str,
    unit: &'static str,
    series: Vec<(&'static str, egui::Color32, Vec<[f64; 2]>)>,
}

/// Renders one metric's plot. Returns the plot's screen rect and coordinate
/// transform (used afterwards to draw synced value readouts on every other
/// chart at whichever point the user is hovering), or `None` if it has no data.
fn plot_metric(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    chart: &ChartSpec,
    group_id: egui::Id,
) -> Option<(egui::Rect, egui_plot::PlotTransform)> {
    let any_data = chart.series.iter().any(|(_, _, pts)| !pts.is_empty());
    if !any_data {
        return None;
    }
    section_label(ui, dc, chart.title);
    let resp = Plot::new(chart.id)
        .height(140.0)
        .legend(Legend::default())
        .allow_scroll(false)
        // Same group_id across all metrics in a session: dragging/zooming any one
        // chart pans/zooms them all together (x only — each metric keeps its own
        // y-scale) and hovering any one chart shows the crosshair on all of them,
        // so CPU load, temp, RAM, etc. at a given moment line up visually.
        .link_axis(group_id, [true, false])
        .link_cursor(group_id, [true, false])
        .x_axis_formatter(|mark, _range| fmt_elapsed(mark.value))
        // Every chart renders its values the same way — via the synced readout
        // box drawn in `draw_synced_readout`, on this chart as much as any other
        // linked one — so the mouse-following tooltip here only ever shows the
        // time, never a name/value pair.
        .label_formatter(|_name, value| fmt_elapsed(value.x))
        .show(ui, |plot_ui| {
            for (name, color, pts) in &chart.series {
                if pts.is_empty() {
                    continue;
                }
                plot_ui.line(
                    Line::new((*name).to_string(), PlotPoints::from(pts.clone()))
                        .color(*color)
                        .width(1.6),
                );
            }
        });
    ui.add_space(10.0);
    Some((resp.response.rect, resp.transform))
}

/// Finds, per series, the point whose x is closest to `x` and draws a small
/// floating readout box for it near the top of the chart — the same values a
/// user would see by hovering this chart directly, kept in sync while they
/// hover a *different* linked chart instead.
/// For each series, finds the point closest to `x` and draws a small dot right
/// on the curve there plus a value label anchored just above it — so as the
/// hovered x moves, each label rides its own line up and down instead of
/// sitting in one fixed corner.
fn draw_synced_readout(
    ui: &egui::Ui,
    rect: egui::Rect,
    transform: &egui_plot::PlotTransform,
    x: f64,
    chart: &ChartSpec,
) {
    let painter = ui.painter().with_clip_rect(rect);
    let font = egui::FontId::proportional(11.0);
    let line_h = 14.0;

    for (name, color, pts) in &chart.series {
        let Some(p) = pts
            .iter()
            .min_by(|a, b| (a[0] - x).abs().total_cmp(&(b[0] - x).abs()))
        else {
            continue;
        };
        let point_pos = transform.position_from_point(&egui_plot::PlotPoint::new(p[0], p[1]));

        painter.circle_filled(point_pos, 3.0, *color);
        painter.circle_stroke(
            point_pos,
            3.0,
            egui::Stroke::new(1.0, egui::Color32::from_black_alpha(200)),
        );

        let text = format!("{name}: {:.1}{}", p[1], chart.unit);
        let box_w = text.chars().count() as f32 * 6.2 + 8.0;
        let box_x = (point_pos.x + 6.0).clamp(
            rect.left() + 2.0,
            (rect.right() - box_w - 2.0).max(rect.left() + 2.0),
        );
        let box_y = (point_pos.y - line_h - 4.0).max(rect.top() + 2.0);
        let box_rect =
            egui::Rect::from_min_size(egui::pos2(box_x, box_y), egui::vec2(box_w, line_h + 4.0));

        painter.rect_filled(box_rect, 3.0, egui::Color32::from_black_alpha(220));
        painter.text(
            box_rect.left_top() + egui::vec2(4.0, 2.0),
            egui::Align2::LEFT_TOP,
            text,
            font.clone(),
            *color,
        );
    }
}

fn render_detail(ui: &mut egui::Ui, dc: &DialogColors, meta: &SessionMeta, rows: &[SessionRow]) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(&meta.name)
                    .size(16.0)
                    .strong()
                    .color(dc.title),
            );
            let range = if let Some(end) = meta.end_unix {
                format!("{} \u{2192} {}", fmt_local(meta.start_unix), fmt_local(end))
            } else {
                format!("{} \u{2192} now", fmt_local(meta.start_unix))
            };
            ui.label(egui::RichText::new(range).size(11.0).color(dc.muted));
        });
    });
    ui.add_space(8.0);

    let summary: SessionSummary = logging::summarize_rows(rows);
    if summary.row_count == 0 {
        ui.label(
            egui::RichText::new("No data recorded yet for this session.")
                .size(12.0)
                .color(dc.muted),
        );
        return;
    }

    ui.horizontal_wrapped(|ui| {
        stat_tile(
            ui,
            dc,
            "AVG CPU",
            &format!("{:.0}%", summary.avg_cpu_load),
            theme::C_ACCENT,
        );
        stat_tile(
            ui,
            dc,
            "PEAK CPU",
            &format!("{:.0}%", summary.peak_cpu_load),
            theme::C_ACCENT,
        );
        if let Some(avg_gpu) = summary.avg_gpu_load {
            stat_tile(ui, dc, "AVG GPU", &format!("{avg_gpu:.0}%"), theme::C_AMD);
        }
        if let Some(peak_gpu) = summary.peak_gpu_load {
            stat_tile(ui, dc, "PEAK GPU", &format!("{peak_gpu:.0}%"), theme::C_AMD);
        }
        stat_tile(
            ui,
            dc,
            "AVG RAM",
            &format!("{:.1} GB", summary.avg_ram_gb),
            theme::C_RAM,
        );
        stat_tile(
            ui,
            dc,
            "PEAK RAM",
            &format!("{:.1} GB", summary.peak_ram_gb),
            theme::C_RAM,
        );
    });
    ui.add_space(10.0);

    let t0 = meta.start_unix as f64;
    let xs = |r: &SessionRow| r.timestamp_unix as f64 - t0;
    // Shared by every metric plot below so panning/zooming/hovering any one of
    // them stays in sync with the rest — unique per session so switching to a
    // different session doesn't inherit the previous one's zoom/pan state.
    let group_id = egui::Id::new(("history_link_group", &meta.id));

    let charts = [
        ChartSpec {
            id: "history_plot_load",
            title: "Load %",
            unit: "%",
            series: vec![
                (
                    "CPU",
                    theme::C_ACCENT,
                    rows.iter().map(|r| [xs(r), r.cpu_load]).collect(),
                ),
                (
                    "GPU",
                    theme::C_AMD,
                    rows.iter()
                        .filter_map(|r| r.gpu_load.map(|v| [xs(r), v]))
                        .collect(),
                ),
            ],
        },
        ChartSpec {
            id: "history_plot_temp",
            title: "Temperature °C",
            unit: "°C",
            series: vec![
                (
                    "CPU",
                    theme::C_ACCENT,
                    rows.iter()
                        .filter_map(|r| r.cpu_temp.map(|v| [xs(r), v]))
                        .collect(),
                ),
                (
                    "GPU",
                    theme::C_AMD,
                    rows.iter()
                        .filter_map(|r| r.gpu_temp.map(|v| [xs(r), v]))
                        .collect(),
                ),
            ],
        },
        ChartSpec {
            id: "history_plot_ram",
            title: "RAM Used (GB)",
            unit: " GB",
            series: vec![(
                "RAM",
                theme::C_RAM,
                rows.iter().map(|r| [xs(r), r.ram_used_gb]).collect(),
            )],
        },
        ChartSpec {
            id: "history_plot_net",
            title: "Network (Mbps)",
            unit: " Mbps",
            series: vec![
                (
                    "Up",
                    theme::C_GRN,
                    rows.iter().map(|r| [xs(r), r.net_up_mbps]).collect(),
                ),
                (
                    "Down",
                    theme::C_NET_DOWN,
                    rows.iter().map(|r| [xs(r), r.net_down_mbps]).collect(),
                ),
            ],
        },
        ChartSpec {
            id: "history_plot_disk",
            title: "Disk (MB/s)",
            unit: " MB/s",
            series: vec![
                (
                    "Read",
                    theme::C_PUR,
                    rows.iter().map(|r| [xs(r), r.disk_read_mbs]).collect(),
                ),
                (
                    "Write",
                    theme::C_PROC,
                    rows.iter().map(|r| [xs(r), r.disk_write_mbs]).collect(),
                ),
            ],
        },
        ChartSpec {
            id: "history_plot_ping",
            title: "Ping (ms)",
            unit: " ms",
            series: vec![(
                "Ping",
                theme::C_TEXT,
                rows.iter()
                    .filter_map(|r| r.ping_ms.map(|v| [xs(r), v]))
                    .collect(),
            )],
        },
    ];

    card_frame(dc).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let pointer_pos = ui.input(|i| i.pointer.hover_pos());
        egui::ScrollArea::vertical()
            .id_salt("history_detail_scroll")
            .show(ui, |ui| {
                let plot_geo: Vec<Option<(egui::Rect, egui_plot::PlotTransform)>> = charts
                    .iter()
                    .map(|chart| plot_metric(ui, dc, chart, group_id))
                    .collect();

                // Whichever chart the pointer is actually over defines the shared
                // hover x for this frame; every chart (including that one) then
                // draws its own values at that same x in the same style, so a
                // moment in time reads identically across all of them — the
                // mouse-following native tooltip is time-only (see
                // `label_formatter` above).
                let hover_x = pointer_pos.and_then(|pos| {
                    plot_geo.iter().find_map(|geo| {
                        let (rect, transform) = geo.as_ref()?;
                        rect.contains(pos)
                            .then(|| transform.value_from_position(pos).x)
                    })
                });
                if let Some(x) = hover_x {
                    for (chart, geo) in charts.iter().zip(plot_geo.iter()) {
                        if let Some((rect, transform)) = geo {
                            draw_synced_readout(ui, *rect, transform, x, chart);
                        }
                    }
                }
            });
    });
}

// ── Window ────────────────────────────────────────────────────────────────────

#[allow(deprecated)]
#[allow(clippy::too_many_arguments)]
pub fn show(
    ctx: &egui::Context,
    main_ctx: &egui::Context,
    open: &Arc<AtomicBool>,
    needs_focus: &Arc<AtomicBool>,
    state: &Arc<Mutex<HistoryState>>,
    refreshing: &Arc<AtomicBool>,
    loading_rows: &Arc<AtomicBool>,
    dir: &Arc<PathBuf>,
    dc: &DialogColors,
) {
    dc.apply_to_ctx(ctx);
    if needs_focus.swap(false, Ordering::Relaxed) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    let mut action_refresh = false;
    let mut action_close = false;
    let mut list_actions = ListActions::default();
    let mut rename_draft = state.lock_safe().rename_draft.clone();

    let st = state.lock_safe().clone();

    // ── Hero ──────────────────────────────────────────────────────────────────
    egui::TopBottomPanel::top("history_hero")
        .frame(egui::Frame::new().fill(dc.bg).inner_margin(egui::Margin {
            left: 14,
            right: 14,
            top: 14,
            bottom: 12,
        }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("Session History")
                    .size(22.0)
                    .strong()
                    .color(dc.text),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("Browse and chart past recording sessions.")
                    .size(12.0)
                    .color(dc.muted),
            );
        });

    // ── Footer ────────────────────────────────────────────────────────────────
    egui::TopBottomPanel::bottom("history_footer")
        .frame(egui::Frame::new().fill(dc.bg).inner_margin(egui::Margin {
            left: 12,
            right: 12,
            top: 8,
            bottom: 10,
        }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::dialog_btn_secondary(ui, "Close", dc).clicked() {
                        action_close = true;
                    }
                    ui.add_space(6.0);
                    if theme::dialog_btn_primary(ui, "Refresh").clicked() {
                        action_refresh = true;
                    }
                    if refreshing.load(Ordering::Relaxed) || loading_rows.load(Ordering::Relaxed) {
                        ui.add_space(6.0);
                        ui.spinner();
                    }
                });
            });
        });

    // ── Session list ──────────────────────────────────────────────────────────
    egui::SidePanel::left("history_sessions")
        .resizable(false)
        .exact_width(260.0)
        .frame(
            egui::Frame::new()
                .fill(dc.bg)
                .inner_margin(egui::Margin::same(10)),
        )
        .show(ctx, |ui| {
            section_label(ui, dc, "Sessions");
            list_actions = render_session_list(ui, dc, &st, &mut rename_draft);
        });

    // ── Detail ────────────────────────────────────────────────────────────────
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(dc.bg)
                .inner_margin(egui::Margin::same(10)),
        )
        .show(ctx, |ui| {
            let selected_meta = st
                .selected
                .as_ref()
                .and_then(|id| st.sessions.iter().find(|s| &s.id == id));
            match selected_meta {
                Some(meta) => render_detail(ui, dc, meta, &st.rows),
                None => {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("Select a session on the left to see its charts.")
                                .size(13.0)
                                .color(dc.muted),
                        );
                    });
                }
            }
        });

    // ── Apply actions ────────────────────────────────────────────────────────
    if let Some(id) = list_actions.select {
        spawn_load_rows(
            state.clone(),
            loading_rows.clone(),
            dir.as_ref().clone(),
            id,
            main_ctx.clone(),
        );
    }
    if let Some(id) = list_actions.toggle_pin {
        if let Some(meta) = st.sessions.iter().find(|s| s.id == id) {
            logging::set_session_pinned(dir, &id, !meta.pinned);
        }
        action_refresh = true;
    }
    if let Some(id) = list_actions.delete {
        logging::delete_session(dir, &id);
        if state.lock_safe().selected.as_deref() == Some(id.as_str()) {
            let mut s = state.lock_safe();
            s.selected = None;
            s.rows.clear();
        }
        action_refresh = true;
    }
    if let Some(id) = list_actions.reveal {
        if let Some(meta) = st.sessions.iter().find(|s| s.id == id) {
            let path = logging::session_file_path(dir, meta);
            let _ = Command::new("explorer")
                .args(["/select,", &path.display().to_string()])
                .spawn();
        }
    }
    if let Some(pair) = list_actions.start_rename {
        rename_draft = Some(pair);
    }
    if list_actions.cancel_rename {
        rename_draft = None;
    }
    if let Some((id, name)) = list_actions.commit_rename {
        let trimmed = name.trim();
        if !trimmed.is_empty() {
            logging::rename_session(dir, &id, trimmed.to_string());
        }
        rename_draft = None;
        action_refresh = true;
    }
    state.lock_safe().rename_draft = rename_draft;

    if action_refresh {
        spawn_load_sessions(
            state.clone(),
            refreshing.clone(),
            dir.as_ref().clone(),
            main_ctx.clone(),
        );
    }
    if action_close {
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
    if ctx.input(|i| i.viewport().close_requested()) {
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_duration_formats_hours_minutes_seconds() {
        assert_eq!(fmt_duration(45), "45s");
        assert_eq!(fmt_duration(125), "2m 5s");
        assert_eq!(fmt_duration(3725), "1h 2m");
    }

    #[test]
    fn fmt_elapsed_formats_short_and_long() {
        assert_eq!(fmt_elapsed(65.0), "1:05");
        assert_eq!(fmt_elapsed(3661.0), "1:01:01");
    }
}
