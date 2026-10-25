//! Shared dashboard render core.
//!
//! [`DashboardRuntime`] owns the telemetry→renderer glue (sparklines, theme,
//! thresholds, textures) shared by both binaries. [`DashboardView`] is a
//! lightweight borrow over that state used for a single render frame. Both
//! types live here so the panel layout and runtime wiring are in one place.
//! See `docs/architecture.md`.

use crate::geometry::landscape_grid_layout;
use crate::spark::Sparkline;
use crate::{brand, panels, theme, PollStats};
use eframe::egui;
use rigstats_backend::control::{ControlEvent, ControlState};
use rigstats_backend::settings;
use std::sync::mpsc;

/// Per-component warn/crit thresholds used for colour coding. Most are °C;
/// `cpu_load`/`gpu_load`/`ram_load` are percentages (kept distinct from
/// `cpu`/`gpu`/`ram` since those are temperature thresholds — reusing them
/// for a usage % was an overlay-only inconsistency the main dashboard never
/// had: it colors RAM usage with a fixed color, and CPU/GPU load bars aren't
/// colored by threshold at all).
#[derive(Clone)]
pub struct PanelThresholds {
    pub cpu: (u8, u8),
    pub gpu: (u8, u8),
    pub cpu_load: (u8, u8),
    pub gpu_load: (u8, u8),
    pub ram_load: (u8, u8),
    pub disk_usage: (u8, u8),
    pub gpu_hotspot: (u8, u8),
    /// VRAM temperature; GDDR6X/GDDR7 throttle around 105-110 °C.
    pub gpu_mem: (u8, u8),
    pub ram: (u8, u8),
    pub disk: (u8, u8),
    pub mb: (u8, u8),
    pub battery: (u8, u8),
    pub battery_power: (u8, u8),
}

impl Default for PanelThresholds {
    fn default() -> Self {
        Self {
            cpu: (80, 90),
            gpu: (80, 90),
            cpu_load: (80, 95),
            gpu_load: (80, 95),
            ram_load: (80, 95),
            disk_usage: (75, 90),
            gpu_hotspot: (90, 105),
            gpu_mem: (90, 100),
            ram: (60, 70),
            disk: (50, 60),
            mb: (70, 90),
            battery: (20, 10),
            battery_power: (15, 25),
        }
    }
}

impl PanelThresholds {
    pub fn from_settings(s: &settings::Settings) -> Self {
        let get = |key: &str, default: (u8, u8)| -> (u8, u8) {
            s.thresholds.get(key).map_or(default, |t| {
                (t.warn.unwrap_or(default.0), t.crit.unwrap_or(default.1))
            })
        };
        let def = Self::default();
        Self {
            cpu: get("cpu", def.cpu),
            gpu: get("gpu", def.gpu),
            cpu_load: get("cpu_load", def.cpu_load),
            gpu_load: get("gpu_load", def.gpu_load),
            ram_load: get("ram_load", def.ram_load),
            disk_usage: get("disk_usage", def.disk_usage),
            gpu_hotspot: def.gpu_hotspot, // not user-configurable
            gpu_mem: def.gpu_mem,         // not user-configurable
            ram: get("ram", def.ram),
            disk: get("disk", def.disk),
            mb: def.mb, // not user-configurable
            battery: get("battery", def.battery),
            battery_power: get("battery_power", def.battery_power),
        }
    }
}

/// A borrowed view over the render state needed to draw the dashboard panels.
/// Constructed per frame by whichever binary owns the data.
pub struct DashboardView<'a> {
    pub latest: &'a PollStats,
    pub cpu_spark: &'a Sparkline,
    pub gpu_spark: &'a Sparkline,
    pub net_up_spark: &'a Sparkline,
    pub net_dn_spark: &'a Sparkline,
    pub textures: &'a brand::Textures,
    pub app_theme: &'a theme::AppTheme,
    pub thresholds: &'a PanelThresholds,
    pub psu_watts: Option<u16>,
    /// Control Center state (#187) — `ControlState::default()` (disconnected,
    /// empty) on the wallpaper host, which never spawns `control_task`; the
    /// header chip simply doesn't render an active profile there.
    pub control: &'a ControlState,
    /// Panels drawn in their compact layout (#596).
    pub compact_panels: &'a [String],
}

impl DashboardView<'_> {
    /// Whether `panel` is drawn in its compact layout (#596).
    fn compact(&self, panel: &str) -> bool {
        self.compact_panels.iter().any(|p| p == panel)
    }

    /// Draw a single panel by key into `ui` at content scale `sc`. Returns a new
    /// preferred-GPU key if the GPU panel's device selector was clicked. Shared by
    /// the portrait vertical stack and the landscape grid.
    ///
    /// The clock panel may set an `"open_updater"` temp flag on the egui context
    /// (badge click); the caller is responsible for consuming it — the host
    /// process has no updater dialog and simply ignores it.
    ///
    /// `opacity` (0.0-1.0) is baked into each panel's fills/borders/decorations
    /// via `theme::panel_frame`'s premultiply so panels blend correctly over a
    /// transparent window background (the wallpaper host, in Desktop Wallpaper
    /// mode). The main app always passes `1.0` here — its opacity is applied at
    /// the window level via `SetLayeredWindowAttributes` instead (see
    /// `win_opacity` module), which is unavailable once the window is
    /// reparented into WorkerW.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_one_panel(
        &self,
        ui: &mut egui::Ui,
        panel: &str,
        sc: f32,
        opacity: f32,
        update_ver: Option<&str>,
    ) -> Option<String> {
        let ctx = ui.ctx().clone();
        panels::with_compact_height(&ctx, panel, self.compact(panel), || {
            self.draw_panel(ui, panel, sc, opacity, update_ver)
        })
    }

    /// `draw_one_panel`'s body: the panel's own `draw`.
    fn draw_panel(
        &self,
        ui: &mut egui::Ui,
        panel: &str,
        sc: f32,
        opacity: f32,
        update_ver: Option<&str>,
    ) -> Option<String> {
        let mut new_pref = None;
        match panel {
            "header" => {
                let _ = panels::header::draw(
                    ui,
                    self.latest,
                    self.textures,
                    opacity,
                    self.app_theme,
                    sc,
                    self.control,
                    self.compact("header"),
                );
            }
            "clock" => {
                let _ = panels::clock::draw(
                    ui,
                    self.latest.uptime_secs,
                    opacity,
                    self.app_theme,
                    update_ver,
                    sc,
                    self.compact("clock"),
                );
            }
            "cpu" => {
                let _ = panels::cpu::draw(
                    ui,
                    self.latest,
                    self.cpu_spark,
                    self.textures,
                    opacity,
                    self.thresholds.cpu.0,
                    self.thresholds.cpu.1,
                    self.app_theme,
                    sc,
                    self.control.active_ppt_limit(),
                    self.compact("cpu"),
                );
            }
            "gpu" => {
                if let Some(p) = panels::gpu::draw(
                    ui,
                    self.latest,
                    self.gpu_spark,
                    self.textures,
                    opacity,
                    self.app_theme,
                    self.thresholds.gpu.0,
                    self.thresholds.gpu.1,
                    self.thresholds.gpu_hotspot,
                    self.thresholds.gpu_mem,
                    sc,
                    self.control.active_gpu_limit(&self.latest.gpu_name),
                    self.compact("gpu"),
                )
                .0
                {
                    new_pref = Some(p);
                }
            }
            "ram" => {
                let _ = panels::ram::draw(
                    ui,
                    self.latest,
                    opacity,
                    self.thresholds.ram.0,
                    self.thresholds.ram.1,
                    self.app_theme,
                    sc,
                    self.compact("ram"),
                );
            }
            "net" => {
                let _ = panels::net::draw(
                    ui,
                    self.latest,
                    self.net_up_spark,
                    self.net_dn_spark,
                    opacity,
                    self.app_theme,
                    sc,
                    self.compact("net"),
                );
            }
            "disk" => {
                let _ = panels::disk::draw(
                    ui,
                    self.latest,
                    opacity,
                    self.thresholds.disk.0,
                    self.thresholds.disk.1,
                    self.thresholds.disk_usage.0,
                    self.thresholds.disk_usage.1,
                    self.app_theme,
                    sc,
                    self.compact("disk"),
                );
            }
            "motherboard" => {
                let _ = panels::motherboard::draw(
                    ui,
                    self.latest,
                    opacity,
                    self.thresholds.mb,
                    self.app_theme,
                    sc,
                    self.control.has_fan_control(),
                    self.compact("motherboard"),
                );
            }
            "process" => {
                let _ = panels::process::draw(
                    ui,
                    self.latest,
                    opacity,
                    self.app_theme,
                    sc,
                    self.compact("process"),
                );
            }
            "gpu_processes" => {
                let _ = panels::gpu_processes::draw(
                    ui,
                    self.latest,
                    opacity,
                    self.app_theme,
                    sc,
                    self.compact("gpu_processes"),
                );
            }
            "power" => {
                let _ = panels::power::draw(
                    ui,
                    self.latest,
                    opacity,
                    self.app_theme,
                    sc,
                    self.psu_watts,
                    self.compact("power"),
                );
            }
            "peripherals" => {
                let _ = panels::peripherals::draw(
                    ui,
                    self.latest,
                    opacity,
                    self.app_theme,
                    sc,
                    self.thresholds.battery.0,
                    self.thresholds.battery.1,
                    self.compact("peripherals"),
                );
            }
            "battery" => {
                let _ = panels::battery::draw(
                    ui,
                    self.latest,
                    opacity,
                    self.app_theme,
                    sc,
                    self.thresholds.battery.0,
                    self.thresholds.battery.1,
                    self.thresholds.battery_power.0,
                    self.thresholds.battery_power.1,
                    self.compact("battery"),
                );
            }
            _ => {}
        }
        new_pref
    }

    /// Render the visible panels as an even, adaptive grid for landscape profiles.
    ///
    /// The column count is chosen so each cell is close to the portrait card width
    /// (~450 px), then the rows follow from the panel count. Every cell is the same
    /// size and the per-cell content scale `sc` is derived from the cell dimensions,
    /// so panels shrink/grow to fit any landscape resolution. Returns a new
    /// preferred-GPU key if the GPU device selector was clicked.
    ///
    /// `ref_h` is the profile's own declared height — used to choose the column
    /// count and content scale in *both* Fill Screen states, so a profile's
    /// natural panel scale stays stable and matches its transposed portrait
    /// counterpart (e.g. `landscape-qhd-top` vs `portrait-qhd-side`) regardless
    /// of Fill Screen or the live window height. Using the live window height
    /// instead would feed back on itself in content-fit mode (a shorter window
    /// implies fewer rows fit, which would shrink the window further) and drift
    /// from the profile's intended scale.
    ///
    /// `fill` selects between the two Fill Screen states: `true` (screen fill on,
    /// or wallpaper mode which always fills) stretches cells to use the whole
    /// *live* available height, exactly filling the fixed profile/monitor-sized
    /// window (panels keep `ref_h`'s natural scale and are centered within the
    /// taller cell). `false` (the default) keeps cells at their natural height,
    /// so the grid — and the window fit to it — shrinks or grows with the panel
    /// count instead of leaving a gap below a partial grid.
    #[allow(clippy::too_many_arguments)]
    pub fn render_landscape_grid(
        &self,
        ui: &mut egui::Ui,
        panels: &[String],
        update_ver: Option<&str>,
        fill: bool,
        ref_h: f32,
        opacity: f32,
    ) -> Option<String> {
        let n = panels.len();
        if n == 0 {
            return None;
        }
        let gap = 6.0_f32;
        let avail_w = ui.available_width().max(40.0);
        let (n_cols, n_rows, sc) = landscape_grid_layout(n, avail_w, ref_h);

        let cell_w = ((avail_w - gap * (n_cols as f32 - 1.0)) / n_cols as f32).max(40.0);
        let full_h = 224.0 * sc;
        let fill_cell_h = {
            let avail_h = ui.available_height().max(40.0);
            ((avail_h - gap * (n_rows as f32 - 1.0)) / n_rows as f32).max(40.0)
        };
        let ctx = ui.ctx().clone();
        let natural_id = |key: &str| egui::Id::unique(("landscape_compact_h", key));
        // A row of compact panels only (#596) is as tall as its tallest panel,
        // measured last frame, and every panel in it stretches to that, so the
        // frames end in line; any other row keeps the full panel's height.
        let row_content_h = |row: usize| -> (f32, bool) {
            let keys = &panels[row * n_cols..((row + 1) * n_cols).min(n)];
            if !keys.iter().all(|k| self.compact(k)) {
                return (full_h, false);
            }
            let tallest = keys
                .iter()
                .filter_map(|k| ctx.data(|d| d.get_temp::<f32>(natural_id(k))))
                .fold(None, |m: Option<f32>, h| Some(m.map_or(h, |m| m.max(h))));
            // At least the shared compact height (#604), which
            // `draw_one_panel` stretches every compact panel to.
            let all = match (tallest, panels::compact_height(&ctx)) {
                (Some(t), Some(s)) => Some(t.max(s)),
                (t, s) => t.or(s),
            };
            (all.unwrap_or(full_h), true)
        };

        let origin = ui.cursor().min;
        let mut new_pref = None;
        let mut y = origin.y;
        for row in 0..n_rows {
            let (content_h, compact_row) = row_content_h(row);
            let cell_h = if fill { fill_cell_h } else { content_h };
            // In fill mode the cell can be taller than the panel's natural
            // content (e.g. the scale clamp caps growth before the cell does) —
            // center the panel within it instead of leaving dead space below.
            let pad = if fill {
                ((cell_h - content_h) * 0.5).max(0.0)
            } else {
                0.0
            };
            for col in 0..n_cols {
                let idx = row * n_cols + col;
                if idx >= n {
                    break;
                }
                let x = origin.x + col as f32 * (cell_w + gap);
                let cell_rect = egui::Rect::from_min_size(
                    egui::pos2(x, y + pad),
                    egui::vec2(cell_w, cell_h - pad),
                );
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(cell_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                // `max_rect` alone is a layout hint, not a clip: a panel whose
                // content exceeds its cell budget would otherwise bleed into
                // neighbouring cells/rows instead of being contained.
                child.set_clip_rect(cell_rect.intersect(child.clip_rect()));
                if compact_row {
                    ctx.data_mut(|d| d.insert_temp(theme::panel_stretch_id(), content_h));
                }
                ctx.data_mut(|d| d.remove::<f32>(theme::panel_natural_id()));
                if let Some(p) =
                    self.draw_one_panel(&mut child, &panels[idx], sc, opacity, update_ver)
                {
                    new_pref = Some(p);
                }
                ctx.data_mut(|d| d.remove::<f32>(theme::panel_stretch_id()));
                // Remember the panel's own height for the next frame; a change
                // (a panel switched to compact, a new scale) settles in a frame.
                if let Some(h) = ctx.data(|d| d.get_temp::<f32>(theme::panel_natural_id())) {
                    let key = natural_id(&panels[idx]);
                    let old = ctx.data(|d| d.get_temp::<f32>(key));
                    if old.is_none_or(|o| (o - h).abs() > 0.5) {
                        ctx.data_mut(|d| d.insert_temp(key, h));
                        if self.compact(&panels[idx]) {
                            ctx.request_repaint();
                        }
                    }
                }
            }
            y += cell_h + gap;
        }
        // Advance the parent cursor past the whole grid.
        let total_h = (y - gap - origin.y).max(0.0);
        ui.allocate_space(egui::vec2(avail_w, total_h));
        new_pref
    }
}

/// Owned runtime state shared by both binaries: the telemetry→renderer glue
/// that wires sparklines, theme, thresholds, and textures to [`DashboardView`].
/// Both `RigStatsApp` and `WallpaperHost` embed one instance; each keeps only
/// its own shell concerns (dialogs/tray vs. WorkerW attach).
pub struct DashboardRuntime {
    pub latest: PollStats,
    pub cpu_spark: Sparkline,
    pub gpu_spark: Sparkline,
    pub net_up_spark: Sparkline,
    pub net_dn_spark: Sparkline,
    pub textures: brand::Textures,
    pub app_theme: theme::AppTheme,
    pub thresholds: PanelThresholds,
    pub psu_watts: Option<u16>,
    pub visible_panels: Vec<String>,
    /// Panels drawn compact (#596), from the profile's look.
    pub compact_panels: Vec<String>,
    /// Control Center state (#187) — see `DashboardView::control`'s doc.
    pub control: ControlState,
}

impl DashboardRuntime {
    pub fn new(ctx: &egui::Context, s: &settings::Settings) -> Self {
        Self {
            latest: PollStats::default(),
            cpu_spark: Sparkline::new(60),
            gpu_spark: Sparkline::new(60),
            net_up_spark: Sparkline::new(60),
            net_dn_spark: Sparkline::new(60),
            textures: brand::Textures::load(ctx),
            app_theme: theme::AppTheme::from_key(&s.theme),
            thresholds: PanelThresholds::from_settings(s),
            psu_watts: s.psu_watts,
            visible_panels: s.visible_panels.clone(),
            compact_panels: s.compact_panels.clone(),
            control: ControlState::default(),
        }
    }

    /// Drain pending [`PollStats`] from `rx` into the sparklines and `latest`.
    /// Returns `true` if at least one new sample arrived (`latest` changed).
    pub fn drain(&mut self, rx: &mpsc::Receiver<PollStats>) -> bool {
        let mut changed = false;
        while let Ok(stats) = rx.try_recv() {
            self.cpu_spark.push(stats.cpu_load as f32);
            self.gpu_spark.push(stats.gpu_load.unwrap_or(0.0) as f32);
            self.net_up_spark.push(stats.net_up_mbps as f32);
            self.net_dn_spark.push(stats.net_down_mbps as f32);
            self.latest = stats;
            changed = true;
        }
        changed
    }

    /// Drain pending [`ControlEvent`]s from `rx`, folding each into
    /// `control`. Same shape as `drain`, called alongside it — see
    /// `RigStatsApp::update`'s call site (main.rs). Never called by the
    /// wallpaper host, which has no control pipe connection.
    pub fn drain_control(&mut self, rx: &mpsc::Receiver<ControlEvent>) -> bool {
        let mut changed = false;
        while let Ok(event) = rx.try_recv() {
            changed |= self.control.apply(event);
        }
        changed
    }

    /// Whether `panel` is drawn in its compact layout (#596) — for the
    /// floating panels, which draw each panel themselves.
    pub fn is_compact(&self, panel: &str) -> bool {
        self.compact_panels.iter().any(|p| p == panel)
    }

    /// Apply a new settings snapshot. Returns `true` if `visible_panels` or
    /// `compact_panels` changed (callers may need to invalidate cached
    /// panel-height state: a compact panel is shorter).
    pub fn apply_settings(&mut self, s: &settings::Settings) -> bool {
        self.app_theme = theme::AppTheme::from_key(&s.theme);
        self.thresholds = PanelThresholds::from_settings(s);
        self.psu_watts = s.psu_watts;
        let panels_changed =
            self.visible_panels != s.visible_panels || self.compact_panels != s.compact_panels;
        if panels_changed {
            self.visible_panels = s.visible_panels.clone();
            self.compact_panels = s.compact_panels.clone();
        }
        panels_changed
    }

    /// Build a borrowed [`DashboardView`] for the current frame.
    pub fn view(&self) -> DashboardView<'_> {
        DashboardView {
            latest: &self.latest,
            cpu_spark: &self.cpu_spark,
            gpu_spark: &self.gpu_spark,
            net_up_spark: &self.net_up_spark,
            net_dn_spark: &self.net_dn_spark,
            textures: &self.textures,
            app_theme: &self.app_theme,
            thresholds: &self.thresholds,
            psu_watts: self.psu_watts,
            control: &self.control,
            compact_panels: &self.compact_panels,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panels' frames the grid drew (their fill is the panel fill), left
    /// to right, after a few passes so measured heights have settled.
    fn grid_frames(compact: &[&str], fill: bool) -> Vec<egui::Rect> {
        let ctx = egui::Context::default();
        let tex = brand::Textures::load(&ctx);
        let stats = PollStats {
            cpu_model: "AMD Ryzen 7 9800X3D 8-Core Processor".into(),
            gpu_name: "AMD Radeon RX 9070 XT".into(),
            ram_spec: "DDR5 6000 MT/s (2 DIMMs)".into(),
            ram_total: 32 << 30,
            ram_used: 16 << 30,
            ram_temp: Some(38.0),
            lhm_connected: true,
            ..Default::default()
        };
        let spark = Sparkline::new(60);
        let th = theme::AppTheme::default();
        let thresholds = PanelThresholds::default();
        let control = ControlState::default();
        let compact: Vec<String> = compact.iter().map(|k| k.to_string()).collect();
        let view = DashboardView {
            latest: &stats,
            cpu_spark: &spark,
            gpu_spark: &spark,
            net_up_spark: &spark,
            net_dn_spark: &spark,
            textures: &tex,
            app_theme: &th,
            thresholds: &thresholds,
            psu_watts: None,
            control: &control,
            compact_panels: &compact,
        };
        let panels: Vec<String> = ["cpu", "gpu", "ram", "net"].map(String::from).to_vec();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(2560.0, 450.0),
            )),
            ..Default::default()
        };
        let mut shapes = Vec::new();
        for _ in 0..4 {
            let mut out = ctx.run_ui(input(), |ui| {
                view.render_landscape_grid(ui, &panels, None, fill, 450.0, 1.0);
            });
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        let mut frames: Vec<egui::Rect> = shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Rect(r) if r.fill == theme::PANEL_FILL => Some(r.rect),
                _ => None,
            })
            .collect();
        frames.sort_by(|a, b| a.left().total_cmp(&b.left()));
        frames
    }

    #[test]
    fn a_row_of_compact_panels_ends_in_line_and_is_shorter() {
        let all = ["cpu", "gpu", "ram", "net"];
        let compact = grid_frames(&all, false);
        let full = grid_frames(&[], false);
        assert_eq!(compact.len(), 4, "{compact:?}");
        let h = compact[0].height();
        for f in &compact {
            assert!((f.height() - h).abs() < 0.5, "{compact:?}");
            assert!(
                (f.bottom() - compact[0].bottom()).abs() < 0.5,
                "{compact:?}"
            );
        }
        assert!(
            h < full[0].height() * 0.75,
            "compact {h}, full {}",
            full[0].height()
        );
    }

    #[test]
    fn a_row_with_a_full_panel_keeps_the_full_height() {
        let mixed = grid_frames(&["cpu", "gpu", "ram"], false);
        let full = grid_frames(&[], false);
        // Network is full: its frame, and the row, are as before.
        assert!((mixed[3].height() - full[3].height()).abs() < 0.5);
    }

    #[test]
    fn filling_the_screen_centres_a_compact_row() {
        let frames = grid_frames(&["cpu", "gpu", "ram", "net"], true);
        let f = frames[0];
        let above = f.top();
        let below = 450.0 - f.bottom();
        assert!(
            (above - below).abs() < 2.0,
            "{f:?}: {above} above, {below} below"
        );
    }

    /// All thirteen panels compact, stacked as in portrait, at `sc`: their
    /// frames, top to bottom, after a few passes.
    fn portrait_compact_frames(sc: f32) -> Vec<egui::Rect> {
        let ctx = egui::Context::default();
        let tex = brand::Textures::load(&ctx);
        let stats = PollStats {
            cpu_model: "AMD Ryzen 7 9800X3D 8-Core Processor".into(),
            gpu_name: "AMD Radeon RX 9070 XT".into(),
            ram_spec: "DDR5 6000 MT/s (2 DIMMs)".into(),
            ram_total: 32 << 30,
            ram_used: 16 << 30,
            hostname: "Unvork".into(),
            model_name: "ROG GM700TZ".into(),
            lhm_connected: true,
            ..Default::default()
        };
        let spark = Sparkline::new(60);
        let th = theme::AppTheme::default();
        let thresholds = PanelThresholds::default();
        let control = ControlState::default();
        let keys: Vec<String> = panels::COMPACT_PANELS.map(String::from).to_vec();
        let view = DashboardView {
            latest: &stats,
            cpu_spark: &spark,
            gpu_spark: &spark,
            net_up_spark: &spark,
            net_dn_spark: &spark,
            textures: &tex,
            app_theme: &th,
            thresholds: &thresholds,
            psu_watts: None,
            control: &control,
            compact_panels: &keys,
        };
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(450.0 * sc, 6000.0),
            )),
            ..Default::default()
        };
        let mut shapes = Vec::new();
        for _ in 0..4 {
            let mut out = ctx.run_ui(input(), |ui| {
                for key in &keys {
                    view.draw_one_panel(ui, key, sc, 1.0, None);
                }
            });
            out.textures_delta.clear();
            shapes = out.shapes;
        }
        let mut frames: Vec<egui::Rect> = shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Rect(r) if r.fill == theme::PANEL_FILL => Some(r.rect),
                _ => None,
            })
            .collect();
        frames.sort_by(|a, b| a.top().total_cmp(&b.top()));
        frames
    }

    #[test]
    fn every_compact_panel_is_one_height_at_every_scale() {
        for sc in [0.4, 0.7, 1.0, 1.2, 1.7, 2.0] {
            let frames = portrait_compact_frames(sc);
            assert_eq!(frames.len(), 13, "at {sc}: {frames:?}");
            let h = frames.iter().map(|f| f.height()).fold(0.0, f32::max);
            for f in &frames {
                assert!(
                    (f.height() - h).abs() < 0.5,
                    "at {sc}: a frame is {} px, the tallest {h}",
                    f.height()
                );
            }
        }
    }
}
