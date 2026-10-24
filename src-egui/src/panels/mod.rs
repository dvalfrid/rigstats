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

/// The panels with a compact layout (#596), offered Full / Compact in the
/// Control Center's Dashboard page.
pub const COMPACT_PANELS: [&str; 4] = ["cpu", "gpu", "ram", "net"];

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

/// Caps the header's text column so a long model name is cut short (with …)
/// before it reaches the logo — matters on the narrower compact cards.
pub(crate) fn leave_room_for_logo(ui: &mut egui::Ui, logo: Option<egui::Vec2>, sc: f32) {
    if let Some(size) = logo {
        let room = ui.available_width() - size.x - 18.0 * sc;
        ui.set_max_width(room.max(0.0));
    }
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
                    ui.label(RichText::new(value).size(14.0 * sc).color(*value_color));
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
