//! Shared dashboard render core.
//!
//! [`DashboardView`] is a lightweight borrow of the non-floating render state
//! (latest stats, sparklines, textures, theme, thresholds) plus the
//! panel-drawing methods shared by the main `rigstats` app and the
//! `rigstats-wallpaper` host. Both binaries own the underlying data and build a
//! `DashboardView` to render a frame, so the panel layout lives in exactly one
//! place. See `docs/architecture.md`.

use crate::spark::Sparkline;
use crate::{brand, panels, theme, PollStats};
use eframe::egui;
use rigstats_backend::settings;

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
    pub fn draw_one_panel(
        &self,
        ui: &mut egui::Ui,
        panel: &str,
        sc: f32,
        update_ver: Option<&str>,
    ) -> Option<String> {
        let mut new_pref = None;
        match panel {
            // Panels always render at full opacity; window-level transparency is
            // applied by SetLayeredWindowAttributes (win_opacity module).
            "header" => {
                let _ =
                    panels::header::draw(ui, self.latest, self.textures, 1.0, self.app_theme, sc);
            }
            "clock" => {
                let _ = panels::clock::draw(
                    ui,
                    self.latest.uptime_secs,
                    1.0,
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
                    1.0,
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
                    1.0,
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
                    1.0,
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
                    1.0,
                    self.app_theme,
                    sc,
                );
            }
            "disk" => {
                let _ = panels::disk::draw(
                    ui,
                    self.latest,
                    1.0,
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
                    1.0,
                    self.thresholds.mb.0,
                    self.thresholds.mb.1,
                    self.app_theme,
                    sc,
                );
            }
            "process" => {
                let _ = panels::process::draw(ui, self.latest, 1.0, self.app_theme, sc);
            }
            "power" => {
                let _ =
                    panels::power::draw(ui, self.latest, 1.0, self.app_theme, sc, self.psu_watts);
            }
            "battery" => {
                let _ = panels::battery::draw(
                    ui,
                    self.latest,
                    1.0,
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
    pub fn render_landscape_grid(
        &self,
        ui: &mut egui::Ui,
        panels: &[String],
        update_ver: Option<&str>,
    ) -> Option<String> {
        let n = panels.len();
        if n == 0 {
            return None;
        }
        let gap = 6.0_f32;
        let avail_w = ui.available_width().max(40.0);
        let avail_h = ui.available_height().max(40.0);

        // Choose the column count that maximises the per-cell content scale, so
        // panels are as large as possible for the given screen shape. Ties are
        // broken toward fewer rows (wider cells suit short landscape screens).
        let mut n_cols = 1usize;
        let mut best_s = f32::MIN;
        let mut best_rows = usize::MAX;
        for c in 1..=n {
            let rows = n.div_ceil(c);
            let cw = ((avail_w - gap * (c as f32 - 1.0)) / c as f32).max(40.0);
            let ch = ((avail_h - gap * (rows as f32 - 1.0)) / rows as f32).max(40.0);
            let s = (cw / 450.0).min(ch / 224.0).clamp(0.4, 1.6);
            if s > best_s + 0.02 || ((s - best_s).abs() <= 0.02 && rows < best_rows) {
                n_cols = c;
                best_s = s;
                best_rows = rows;
            }
        }
        let n_rows = n.div_ceil(n_cols);

        let cell_w = ((avail_w - gap * (n_cols as f32 - 1.0)) / n_cols as f32).max(40.0);
        let cell_h = ((avail_h - gap * (n_rows as f32 - 1.0)) / n_rows as f32).max(40.0);

        // Per-cell content scale. Reference card is 450 px wide × ~224 px tall
        // (PANEL_DATA_H + frame margins + a little slack). Use the smaller of the
        // width/height ratios so content never overflows the cell.
        let sc_w = cell_w / 450.0;
        let sc_h = cell_h / 224.0;
        let sc = sc_w.min(sc_h).clamp(0.4, 1.6);

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
                let cell_rect =
                    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell_w, cell_h));
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(cell_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                if let Some(p) = self.draw_one_panel(&mut child, &panels[idx], sc, update_ver) {
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
