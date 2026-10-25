pub mod battery;
pub mod clock;
pub mod cpu;
pub mod disk;
pub mod gpu;
pub mod gpu_processes;
pub mod header;
pub mod motherboard;
pub mod net;
pub mod peripherals;
pub mod power;
pub mod process;
pub mod ram;

/// The panels with a compact layout (#596, every panel since #604), offered
/// Full / Compact in the Control Center's Dashboard page.
pub const COMPACT_PANELS: [&str; 13] = [
    "header",
    "clock",
    "cpu",
    "gpu",
    "ram",
    "net",
    "disk",
    "motherboard",
    "process",
    "gpu_processes",
    "power",
    "battery",
    "peripherals",
];

/// How many rows a compact list panel shows (#604): Processes, GPU Apps.
pub const COMPACT_LIST_ROWS: usize = 3;

/// A compact list panel's rows (#604) are drawn this much bigger than the full
/// panel's 11 px, about 14 px, near the 17 px values of the other compact
/// panels; their header row keeps 11 px, like those panels' labels.
pub const COMPACT_LIST_SCALE: f32 = 1.3;

/// A compact panel's width as a floating card, before the panel scale (a full
/// one is 450). Narrower, so a compact card can be scaled up for reading
/// without growing as wide as a full one.
pub const COMPACT_FLOATING_W: f32 = 360.0;

/// A CPU/GPU brand logo's size in the panel header, its own aspect: 38 px
/// tall, 30 px in the compact layout — as tall as the title and model name, so
/// it doesn't outweigh the rings in the shorter panel.
pub(crate) fn logo_size(logo: &egui::TextureHandle, sc: f32, compact: bool) -> egui::Vec2 {
    let [lw, lh] = logo.size();
    let h = if compact { 30.0 } else { 38.0 } * sc;
    egui::Vec2::new(lw as f32 * (h / lh as f32), h)
}

/// One cell of a compact panel's row (#596).
pub(crate) enum CompactCell {
    /// A ring gauge: fill 0–1, colour, the text inside.
    Ring(f32, egui::Color32, String),
    /// A stat: label (and its colour), value (and its colour), and an optional
    /// muted note under it (a power limit).
    Stat {
        label: &'static str,
        label_color: egui::Color32,
        value: String,
        value_color: egui::Color32,
        note: Option<String>,
    },
}

/// A compact panel's row (#596): the cells in equal columns across the panel's
/// width, each centred in its column, so the values spread evenly at any width
/// and scale instead of crowding to the left.
pub(crate) fn compact_row(
    ui: &mut egui::Ui,
    cells: &[CompactCell],
    th: &crate::theme::AppTheme,
    sc: f32,
) {
    use crate::theme;
    use egui::RichText;
    ui.columns(cells.len(), |cols| {
        for (col, cell) in cols.iter_mut().zip(cells) {
            col.vertical_centered(|ui| match cell {
                CompactCell::Ring(frac, color, label) => {
                    crate::ring::show(ui, theme::RING_SIZE * sc, *frac, *color, label);
                }
                CompactCell::Stat {
                    label,
                    label_color,
                    value,
                    value_color,
                    note,
                } => {
                    ui.label(RichText::new(*label).size(11.0 * sc).color(*label_color));
                    // As big as the rings' numbers (64 px × 0.26 ≈ 17), so a row
                    // reads as one; full panels keep 14 px.
                    ui.label(RichText::new(value).size(17.0 * sc).color(*value_color));
                    if let Some(note) = note {
                        ui.label(RichText::new(note).size(10.0 * sc).color(th.text_muted));
                    }
                }
            });
        }
    });
}

/// The compact layout's temperature ring (#596): 0–100 °C, coloured like the
/// TEMP value (`color`, from the alert thresholds); `--°C` without a reading.
pub(crate) fn temp_cell(temp: Option<f64>, color: egui::Color32) -> CompactCell {
    let frac = temp.map_or(0.0, |t| (t as f32 / 100.0).clamp(0.0, 1.0));
    let label = temp.map_or("--°C".to_string(), |t| format!("{t:.0}°C"));
    CompactCell::Ring(frac, color, label)
}

#[cfg(test)]
mod tests {
    use crate::brand::Textures;
    use crate::geometry::floating_panel_w;
    use crate::spark::Sparkline;
    use crate::{theme, PollStats};

    /// A context whose fonts are loaded — a fresh one has none until its
    /// first pass (see `overlay.rs`'s `ctx_with_fonts`).
    fn ctx_with_fonts() -> egui::Context {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
        out.textures_delta.clear();
        ctx
    }

    /// Draws one panel in a window `width` wide, as a floating card is, and
    /// returns the panel's rect and what it painted. Two passes, so the layout
    /// has settled.
    fn draw_at(
        ctx: &egui::Context,
        width: f32,
        mut draw: impl FnMut(&mut egui::Ui) -> egui::Rect,
    ) -> (egui::Rect, Vec<egui::Shape>) {
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 4000.0),
            )),
            ..Default::default()
        };
        let mut rect = egui::Rect::NOTHING;
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let mut out = ctx.run_ui(input(), |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| rect = draw(ui));
            });
            out.textures_delta.clear();
            shapes = out.shapes.into_iter().map(|c| c.shape).collect();
        }
        (rect, shapes)
    }

    /// Every painted shape, nested ones included.
    fn flat(shapes: &[egui::Shape]) -> Vec<&egui::Shape> {
        let mut all = Vec::new();
        for shape in shapes {
            match shape {
                egui::Shape::Vec(inner) => all.extend(flat(inner)),
                other => all.push(other),
            }
        }
        all
    }

    /// Where the logo `texture` was painted.
    fn image_rect(shapes: &[egui::Shape], texture: egui::TextureId) -> Option<egui::Rect> {
        flat(shapes).into_iter().find_map(|s| match s {
            egui::Shape::Rect(r) if r.fill_texture_id() == texture => Some(r.rect),
            egui::Shape::Mesh(m) if m.texture_id == texture => Some(m.calc_bounds()),
            _ => None,
        })
    }

    /// Where the text starting with `start` was painted.
    fn text_rect(shapes: &[egui::Shape], start: &str) -> Option<egui::Rect> {
        flat(shapes).into_iter().find_map(|s| match s {
            egui::Shape::Text(t) if t.galley.text().starts_with(start) => {
                Some(t.galley.rect.translate(t.pos.to_vec2()))
            }
            _ => None,
        })
    }

    /// Long real model names and the widest logo (AMD's) for the CPU, NVIDIA's
    /// for the GPU; every value read.
    fn stats() -> PollStats {
        PollStats {
            cpu_model: "AMD Ryzen Threadripper PRO 7995WX 96-Cores".into(),
            cpu_load: 100,
            cpu_freq_mhz: 5_700.0,
            cpu_temp: Some(99.0),
            cpu_power: Some(250.0),
            gpu_name: "NVIDIA GeForce RTX 5090 Laptop GPU".into(),
            gpu_load: Some(100.0),
            gpu_temp: Some(88.0),
            gpu_freq_mhz: Some(2_850.0),
            gpu_power: Some(575.0),
            ram_used: 120 * 1024 * 1024 * 1024,
            ram_total: 128 * 1024 * 1024 * 1024,
            ram_temp: Some(55.0),
            lhm_connected: true,
            total_gpu_power: Some(575.0),
            hostname: "WORKSTATION-PRO-01".into(),
            model_name: "ROG Strix SCAR 18 G835LX".into(),
            system_brand: "rog".into(),
            disk_read_mbps: 6_543.0,
            disk_write_mbps: 1_234.5,
            disk_drives: ["C:\\", "D:\\", "E:\\"]
                .iter()
                .map(|fs| crate::poll::DriveInfo {
                    fs: fs.to_string(),
                    used: 1 << 40,
                    total: 2 << 40,
                    pct: 50,
                    temp: Some(71.0),
                    model: "Samsung SSD 990 PRO 4TB".into(),
                    kind: rigstats_backend::stats::DiskKind::NVMe,
                })
                .collect(),
            mb_board: Some("ASUS ROG CROSSHAIR X870E HERO".into()),
            mb_fans: (1..=7).map(|i| (format!("Fan #{i}"), 2_850.0)).collect(),
            mb_temps: (1..=6)
                .map(|i| (format!("Temperature #{i}"), 64.0))
                .collect(),
            mb_voltages: vec![("Vcore".into(), 1.35), ("AVCC".into(), 3.38)],
            processes: (0..8)
                .map(|i| crate::poll::ProcessInfo {
                    name: format!("MicrosoftEdgeWebView2Host{i}.exe"),
                    cpu: 99.9,
                    mem_mb: 16_000,
                })
                .collect(),
            gpu_processes: (0..8)
                .map(|i| crate::gpu_process::GpuProcessInfo {
                    pid: i,
                    name: format!("Cyberpunk2077-{i}.exe"),
                    util_pct: 100.0,
                    engines: vec!["3D".into(), "Codec".into()],
                    adapter: "NVIDIA GeForce RTX 5090".into(),
                })
                .collect(),
            peripherals: vec![rigstats_backend::lhm::Peripheral {
                id: "mouse".into(),
                name: "ROG Harpe Ace Aim Lab Edition Wireless".into(),
                kind: "mouse".into(),
                battery: 100,
                charging: true,
                connection: "2.4ghz".into(),
                connected: true,
                ..Default::default()
            }],
            battery_present: true,
            battery_charge_pct: Some(100),
            battery_charging: Some(false),
            battery_time_mins: Some(1_439),
            battery_power_w: Some(199.9),
            ..Default::default()
        }
    }

    /// Every compact panel at every scale: (key, panel rect) at the compact
    /// floating width, plus the same panel full at 450 × the scale.
    #[allow(clippy::type_complexity)] // a test table: key, rect, shapes, width
    fn draw_all(compact: bool, sc: f32) -> Vec<(&'static str, egui::Rect, Vec<egui::Shape>, f32)> {
        let ctx = ctx_with_fonts();
        let tex = Textures::load(&ctx);
        let th = theme::AppTheme::default();
        let spark = Sparkline::new(60);
        let s = stats();
        let w = floating_panel_w(compact, sc);
        let table = vec![
            (
                "cpu",
                draw_at(&ctx, w, |ui| {
                    super::cpu::draw(
                        ui,
                        &s,
                        &spark,
                        &tex,
                        1.0,
                        80,
                        90,
                        &th,
                        sc,
                        Some(250.0),
                        compact,
                    )
                }),
                w,
            ),
            (
                "gpu",
                draw_at(&ctx, w, |ui| {
                    super::gpu::draw(
                        ui,
                        &s,
                        &spark,
                        &tex,
                        1.0,
                        &th,
                        80,
                        90,
                        (90, 100),
                        (90, 100),
                        sc,
                        Some(-10),
                        compact,
                    )
                    .1
                }),
                w,
            ),
            (
                "ram",
                draw_at(&ctx, w, |ui| {
                    super::ram::draw(ui, &s, 1.0, 50, 60, &th, sc, compact)
                }),
                w,
            ),
            (
                "net",
                draw_at(&ctx, w, |ui| {
                    super::net::draw(ui, &s, &spark, &spark, 1.0, &th, sc, compact)
                }),
                w,
            ),
            (
                "header",
                draw_at(&ctx, w, |ui| {
                    let control = rigstats_backend::control::ControlState::default();
                    super::header::draw(ui, &s, &tex, 1.0, &th, sc, &control, compact)
                }),
                w,
            ),
            (
                "clock",
                draw_at(&ctx, w, |ui| {
                    super::clock::draw(ui, 3_599_999, 1.0, &th, Some("1.52.0-rc.10"), sc, compact)
                }),
                w,
            ),
            (
                "disk",
                draw_at(&ctx, w, |ui| {
                    super::disk::draw(ui, &s, 1.0, 60, 70, 80, 90, &th, sc, compact)
                }),
                w,
            ),
            (
                "motherboard",
                draw_at(&ctx, w, |ui| {
                    super::motherboard::draw(ui, &s, 1.0, (60, 70), &th, sc, true, compact)
                }),
                w,
            ),
            (
                "process",
                draw_at(&ctx, w, |ui| {
                    super::process::draw(ui, &s, 1.0, &th, sc, compact)
                }),
                w,
            ),
            (
                "gpu_processes",
                draw_at(&ctx, w, |ui| {
                    super::gpu_processes::draw(ui, &s, 1.0, &th, sc, compact)
                }),
                w,
            ),
            (
                "power",
                draw_at(&ctx, w, |ui| {
                    super::power::draw(ui, &s, 1.0, &th, sc, Some(1600), compact)
                }),
                w,
            ),
            (
                "battery",
                draw_at(&ctx, w, |ui| {
                    super::battery::draw(ui, &s, 1.0, &th, sc, 20, 10, 15, 25, compact)
                }),
                w,
            ),
            (
                "peripherals",
                draw_at(&ctx, w, |ui| {
                    super::peripherals::draw(ui, &s, 1.0, &th, sc, 20, 10, compact)
                }),
                w,
            ),
        ];
        table
            .into_iter()
            .map(|(key, (rect, shapes), w)| (key, rect, shapes, w))
            .collect()
    }

    #[test]
    fn compact_panels_fit_their_floating_card_at_every_scale() {
        for sc in [0.4, 1.0, 1.2, 1.7, 2.0] {
            for (key, rect, _, w) in draw_all(true, sc) {
                assert!(
                    rect.is_positive() && rect.right() <= w + 0.5,
                    "{key} at {sc}: {rect:?} is wider than its {w} px card"
                );
            }
        }
    }

    #[test]
    fn a_compact_panel_is_shorter_than_the_full_one() {
        let full = draw_all(false, 1.0);
        for ((key, compact, _, _), (_, full, _, _)) in draw_all(true, 1.0).into_iter().zip(full) {
            assert!(
                compact.height() < full.height(),
                "{key}: compact {} px, full {} px",
                compact.height(),
                full.height()
            );
        }
    }

    #[test]
    fn a_long_model_name_stops_before_the_logo_at_every_scale() {
        let ctx = ctx_with_fonts();
        let tex = Textures::load(&ctx);
        for compact in [true, false] {
            for sc in [0.4, 1.0, 1.2, 1.7, 2.0] {
                for (key, _, shapes, w) in draw_all(compact, sc) {
                    let (start, logo) = match key {
                        "cpu" => ("AMD Ryzen", tex.amd.id()),
                        "gpu" => ("NVIDIA GeForce", tex.nvidia.id()),
                        _ => continue,
                    };
                    let name = text_rect(&shapes, start).expect("the model name");
                    let logo = image_rect(&shapes, logo).expect("the logo");
                    assert!(
                        name.right() < logo.left() && logo.right() <= w + 0.5,
                        "{key} compact={compact} at {sc}: name {name:?}, logo {logo:?}, card {w}"
                    );
                }
            }
        }
    }

    #[test]
    fn no_text_in_a_compact_panel_runs_into_other_text() {
        for sc in [0.4, 1.0, 1.2, 1.7, 2.0] {
            for (key, _, shapes, _) in draw_all(true, sc) {
                // A value too wide for its column wraps rather than overlap:
                // that is as broken ("6.54" over "GB/s"), so one line each.
                for s in flat(&shapes) {
                    if let egui::Shape::Text(t) = s {
                        assert_eq!(
                            t.galley.rows.len(),
                            1,
                            "{key} at {sc}: {:?} wraps",
                            t.galley.text()
                        );
                    }
                }
                let texts: Vec<(String, egui::Rect)> = flat(&shapes)
                    .into_iter()
                    .filter_map(|s| match s {
                        egui::Shape::Text(t) if !t.galley.text().trim().is_empty() => Some((
                            t.galley.text().to_string(),
                            t.galley.rect.translate(t.pos.to_vec2()).shrink(0.5),
                        )),
                        _ => None,
                    })
                    .collect();
                for (i, (a, ra)) in texts.iter().enumerate() {
                    for (b, rb) in &texts[i + 1..] {
                        assert!(
                            !ra.intersects(*rb),
                            "{key} at {sc}: {a:?} {ra:?} runs into {b:?} {rb:?}"
                        );
                    }
                }
            }
        }
    }
}
