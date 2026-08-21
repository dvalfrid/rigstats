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
use rigstats_backend::settings;
use std::sync::mpsc;

/// Per-component warn/crit thresholds (°C) used for temperature colour coding.
#[derive(Clone)]
pub struct PanelThresholds {
    pub cpu: (u8, u8),
    pub gpu: (u8, u8),
    pub gpu_hotspot: (u8, u8),
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
            gpu_hotspot: (90, 105),
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
            gpu_hotspot: def.gpu_hotspot, // not user-configurable
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
}

impl DashboardView<'_> {
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
                    self.thresholds.gpu_hotspot.0,
                    self.thresholds.gpu_hotspot.1,
                    sc,
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
                );
            }
            "disk" => {
                let _ = panels::disk::draw(
                    ui,
                    self.latest,
                    opacity,
                    self.thresholds.disk.0,
                    self.thresholds.disk.1,
                    self.app_theme,
                    sc,
                );
            }
            "motherboard" => {
                let _ = panels::motherboard::draw(
                    ui,
                    self.latest,
                    opacity,
                    self.thresholds.mb.0,
                    self.thresholds.mb.1,
                    self.app_theme,
                    sc,
                );
            }
            "process" => {
                let _ = panels::process::draw(ui, self.latest, opacity, self.app_theme, sc);
            }
            "power" => {
                let _ = panels::power::draw(
                    ui,
                    self.latest,
                    opacity,
                    self.app_theme,
                    sc,
                    self.psu_watts,
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
        let cell_h = if fill {
            let avail_h = ui.available_height().max(40.0);
            ((avail_h - gap * (n_rows as f32 - 1.0)) / n_rows as f32).max(40.0)
        } else {
            224.0 * sc
        };

        let origin = ui.cursor().min;
        let mut new_pref = None;
        for row in 0..n_rows {
            for col in 0..n_cols {
                let idx = row * n_cols + col;
                if idx >= n {
                    break;
                }
                let x = origin.x + col as f32 * (cell_w + gap);
                let y = origin.y + row as f32 * (cell_h + gap);
                // In fill mode the cell can be taller than the panel's natural
                // content (e.g. the scale clamp caps growth before the cell does) —
                // center the panel within it instead of leaving dead space below.
                let content_h = 224.0 * sc;
                let pad = if fill {
                    ((cell_h - content_h) * 0.5).max(0.0)
                } else {
                    0.0
                };
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
                if let Some(p) =
                    self.draw_one_panel(&mut child, &panels[idx], sc, opacity, update_ver)
                {
                    new_pref = Some(p);
                }
            }
        }
        // Advance the parent cursor past the whole grid.
        let total_h = n_rows as f32 * cell_h + (n_rows as f32 - 1.0) * gap;
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
        }
    }

    /// Drain pending [`PollStats`] from `rx` into the sparklines and `latest`.
    pub fn drain(&mut self, rx: &mpsc::Receiver<PollStats>) {
        while let Ok(stats) = rx.try_recv() {
            self.cpu_spark.push(stats.cpu_load as f32);
            self.gpu_spark.push(stats.gpu_load.unwrap_or(0.0) as f32);
            self.net_up_spark.push(stats.net_up_mbps as f32);
            self.net_dn_spark.push(stats.net_down_mbps as f32);
            self.latest = stats;
        }
    }

    /// Apply a new settings snapshot. Returns `true` if `visible_panels`
    /// changed (callers may need to invalidate cached panel-height state).
    pub fn apply_settings(&mut self, s: &settings::Settings) -> bool {
        self.app_theme = theme::AppTheme::from_key(&s.theme);
        self.thresholds = PanelThresholds::from_settings(s);
        self.psu_watts = s.psu_watts;
        let panels_changed = self.visible_panels != s.visible_panels;
        if panels_changed {
            self.visible_panels = s.visible_panels.clone();
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
        }
    }
}
