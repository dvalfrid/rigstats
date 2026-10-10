//! Session History window: recordings in a sidebar list; the selected one
//! with its actions (pin, rename, show file, delete — confirmed), summary
//! cards and synced charts. Built from `ui_kit`.

use crate::lock_ext::LockSafe;
use crate::theme::{self, DialogColors};
use crate::windows::ui_kit;
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
    /// The session whose Delete is waiting for confirmation.
    pub confirm_delete: Option<String>,
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

/// One resource's summary as a small card: label, average, peak.
fn stat_tile(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    label: &str,
    value: &str,
    peak: &str,
    color: egui::Color32,
) {
    ui_kit::card_frame(dc)
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(label).size(11.0).color(dc.muted));
            ui.label(egui::RichText::new(value).size(18.0).strong().color(color));
            ui.label(egui::RichText::new(peak).size(11.0).color(dc.muted));
        });
}

// ── Session list (sidebar) ───────────────────────────────────────────────────

/// Actions the window can request; applied after all panels have rendered.
#[derive(Default)]
struct ListActions {
    select: Option<String>,
    toggle_pin: Option<String>,
    delete: Option<String>,
    reveal: Option<String>,
    start_rename: Option<(String, String)>,
    commit_rename: Option<(String, String)>,
    cancel_rename: bool,
    ask_delete: Option<String>,
    cancel_delete: bool,
}

/// "Recording · 12m 5s" / "2026-10-10 14:02 · 1h 3m · Pinned".
fn session_subtitle(meta: &SessionMeta) -> String {
    let when = if meta.is_active() {
        "Recording".to_owned()
    } else {
        fmt_local(meta.start_unix)
    };
    let mut s = format!("{when} · {}", fmt_duration(session_duration_secs(meta)));
    if meta.pinned {
        s.push_str(" · Pinned");
    }
    s
}

/// Every session as a list item, newest first; a click selects it.
fn render_session_list(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    st: &HistoryState,
    actions: &mut ListActions,
) {
    if st.sessions.is_empty() {
        ui.add_space(8.0);
        ui_kit::footnote(
            ui,
            dc,
            "No recordings yet. Choose Start Recording in the tray menu to make one.",
        );
        return;
    }
    egui::ScrollArea::vertical()
        .id_salt("history_session_list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for meta in &st.sessions {
                let selected = st.selected.as_deref() == Some(meta.id.as_str());
                let dot = meta.is_active().then_some(ui_kit::C_BAD);
                if ui_kit::list_item(ui, dc, &meta.name, &session_subtitle(meta), selected, dot)
                    .clicked()
                    && !selected
                {
                    actions.select = Some(meta.id.clone());
                }
            }
        });
}

/// The selected session's name and time span, with its actions on the
/// right; renaming and deleting are confirmed in place.
fn detail_header(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    st: &HistoryState,
    meta: &SessionMeta,
    rename_draft: &mut Option<(String, String)>,
    actions: &mut ListActions,
) {
    let range = match meta.end_unix {
        Some(end) => format!("{} – {}", fmt_local(meta.start_unix), fmt_local(end)),
        None => format!("{} – now (recording)", fmt_local(meta.start_unix)),
    };
    let renaming = rename_draft.as_ref().is_some_and(|(id, _)| *id == meta.id);
    let deleting = st.confirm_delete.as_deref() == Some(meta.id.as_str());
    ui.horizontal(|ui| {
        if renaming {
            if let Some((_, text)) = rename_draft.as_mut() {
                let resp = ui.add(
                    egui::TextEdit::singleline(text)
                        .desired_width(260.0)
                        .font(egui::FontId::proportional(15.0)),
                );
                resp.request_focus();
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    actions.commit_rename = Some((meta.id.clone(), text.clone()));
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    actions.cancel_rename = true;
                }
                if theme::dialog_btn_primary(ui, "Rename").clicked() {
                    actions.commit_rename = Some((meta.id.clone(), text.clone()));
                }
                if theme::dialog_btn_secondary(ui, "Cancel", dc).clicked() {
                    actions.cancel_rename = true;
                }
            }
            return;
        }
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(&meta.name)
                    .size(18.0)
                    .strong()
                    .color(dc.title),
            );
            ui.label(egui::RichText::new(range).size(12.0).color(dc.muted));
        });
    });
    if renaming {
        ui.add_space(14.0);
        return;
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if deleting {
            ui.label(
                egui::RichText::new("Delete this recording?")
                    .size(12.0)
                    .color(dc.title),
            );
            ui.add_space(4.0);
            if theme::dialog_btn_primary(ui, "Delete").clicked() {
                actions.delete = Some(meta.id.clone());
            }
            if theme::dialog_btn_secondary(ui, "Cancel", dc).clicked() {
                actions.cancel_delete = true;
            }
            return;
        }
        let pin = if meta.pinned { "Unpin" } else { "Pin" };
        if theme::dialog_btn_secondary(ui, pin, dc)
            .on_hover_text("Pinned recordings are never deleted automatically")
            .clicked()
        {
            actions.toggle_pin = Some(meta.id.clone());
        }
        if theme::dialog_btn_secondary(ui, "Rename", dc).clicked() {
            actions.start_rename = Some((meta.id.clone(), meta.name.clone()));
        }
        if theme::dialog_btn_secondary(ui, "Show File", dc)
            .on_hover_text("Shows the recording's CSV file in Explorer")
            .clicked()
        {
            actions.reveal = Some(meta.id.clone());
        }
        if !meta.is_active() && theme::dialog_btn_secondary(ui, "Delete…", dc).clicked() {
            actions.ask_delete = Some(meta.id.clone());
        }
    });
    ui.add_space(14.0);
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
    ui.label(
        egui::RichText::new(chart.title)
            .size(12.0)
            .strong()
            .color(dc.label),
    );
    ui.add_space(4.0);
    // Keyed by group_id (which already includes the session id) so switching
    // sessions never inherits another session's zoom/pan state, and manual
    // drag/zoom is disabled so the y-axis always auto-fits the full data range
    // instead of a stale bounds getting "stuck" and clipping later values.
    let resp = Plot::new((group_id, chart.id))
        .height(140.0)
        .legend(Legend::default())
        .allow_scroll(false)
        .allow_drag(false)
        .allow_zoom(false)
        // Same group_id across all metrics in a session: hovering any one chart
        // shows the crosshair on all of them, so CPU load, temp, RAM, etc. at a
        // given moment line up visually.
        .link_axis(group_id, [true, false])
        .link_cursor(group_id, [true, false])
        // Default cursor color is additive gray, which reads as a stark white
        // line against the dark chart background — tone it down.
        .cursor_color(egui::Color32::from_white_alpha(40))
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
                        .width(1.6_f32),
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
            egui::Stroke::new(1.0_f32, egui::Color32::from_black_alpha(200)),
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
    let summary: SessionSummary = logging::summarize_rows(rows);
    if summary.row_count == 0 {
        ui.label(
            egui::RichText::new("No data recorded yet for this session.")
                .size(12.0)
                .color(dc.muted),
        );
        return;
    }

    // One card per resource: the average, and the peak beneath it.
    let mut tiles: Vec<(&str, String, String, egui::Color32)> = vec![(
        "CPU load",
        format!("{:.0} %", summary.avg_cpu_load),
        format!("Peak {:.0} %", summary.peak_cpu_load),
        theme::C_ACCENT,
    )];
    if let (Some(avg), Some(peak)) = (summary.avg_gpu_load, summary.peak_gpu_load) {
        tiles.push((
            "GPU load",
            format!("{avg:.0} %"),
            format!("Peak {peak:.0} %"),
            theme::C_AMD,
        ));
    }
    tiles.push((
        "Memory used",
        format!("{:.1} GB", summary.avg_ram_gb),
        format!("Peak {:.1} GB", summary.peak_ram_gb),
        theme::C_RAM,
    ));
    ui.columns(tiles.len(), |cols| {
        for (col, (label, value, peak, color)) in cols.iter_mut().zip(tiles.iter()) {
            stat_tile(col, dc, label, value, peak, *color);
        }
    });
    ui.add_space(12.0);

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

    ui_kit::card_frame(dc)
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
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

// The ctx-level panel API, as in every other dialog.
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
    let mut actions = ListActions::default();
    let mut rename_draft = state.lock_safe().rename_draft.clone();

    let st = state.lock_safe().clone();
    let busy = refreshing.load(Ordering::Relaxed) || loading_rows.load(Ordering::Relaxed);

    ui_kit::hero(ctx, dc, "history", "Session History", |ui| {
        if busy {
            ui.spinner();
        }
    });

    // ── Footer ────────────────────────────────────────────────────────────────
    egui::TopBottomPanel::bottom("history_footer")
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 10,
            bottom: 12,
        }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::dialog_btn_primary(ui, "Close").clicked() {
                    action_close = true;
                }
                ui.add_space(6.0);
                if theme::dialog_btn_secondary(ui, "Refresh", dc).clicked() {
                    action_refresh = true;
                }
            });
        });

    // ── Recordings (sidebar) ──────────────────────────────────────────────────
    egui::SidePanel::left("history_sessions")
        .resizable(false)
        .exact_width(260.0)
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin::same(10)))
        .show(ctx, |ui| {
            ui_kit::nav_header(ui, dc, "Recordings");
            ui.add_space(2.0);
            render_session_list(ui, dc, &st, &mut actions);
        });

    // ── The selected recording ────────────────────────────────────────────────
    egui::CentralPanel::default()
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin {
            left: 24,
            right: 24,
            top: 18,
            bottom: 8,
        }))
        .show(ctx, |ui| {
            let selected_meta = st
                .selected
                .as_ref()
                .and_then(|id| st.sessions.iter().find(|s| &s.id == id));
            match selected_meta {
                Some(meta) => {
                    detail_header(ui, dc, &st, meta, &mut rename_draft, &mut actions);
                    render_detail(ui, dc, meta, &st.rows);
                }
                None => {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("Choose a recording to see its charts.")
                                .size(13.0)
                                .color(dc.muted),
                        );
                    });
                }
            }
        });

    // ── Apply actions ────────────────────────────────────────────────────────
    if let Some(id) = actions.select {
        state.lock_safe().confirm_delete = None;
        spawn_load_rows(
            state.clone(),
            loading_rows.clone(),
            dir.as_ref().clone(),
            id,
            main_ctx.clone(),
        );
    }
    if let Some(id) = actions.toggle_pin {
        if let Some(meta) = st.sessions.iter().find(|s| s.id == id) {
            logging::set_session_pinned(dir, &id, !meta.pinned);
        }
        action_refresh = true;
    }
    if let Some(id) = actions.ask_delete {
        state.lock_safe().confirm_delete = Some(id);
    }
    if actions.cancel_delete {
        state.lock_safe().confirm_delete = None;
    }
    if let Some(id) = actions.delete {
        logging::delete_session(dir, &id);
        let mut s = state.lock_safe();
        s.confirm_delete = None;
        if s.selected.as_deref() == Some(id.as_str()) {
            s.selected = None;
            s.rows.clear();
        }
        drop(s);
        action_refresh = true;
    }
    if let Some(id) = actions.reveal {
        if let Some(meta) = st.sessions.iter().find(|s| s.id == id) {
            let path = logging::session_file_path(dir, meta);
            let _ = Command::new("explorer")
                .args(["/select,", &path.display().to_string()])
                .spawn();
        }
    }
    if let Some(pair) = actions.start_rename {
        rename_draft = Some(pair);
    }
    if actions.cancel_rename {
        rename_draft = None;
    }
    if let Some((id, name)) = actions.commit_rename {
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
    if action_close || ctx.input(|i| i.viewport().close_requested()) {
        state.lock_safe().confirm_delete = None;
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
