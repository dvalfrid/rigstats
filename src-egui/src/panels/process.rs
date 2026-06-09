use egui::{pos2, Align2, Rect, RichText, Sense, Ui, Vec2};

use crate::theme;
use crate::PollStats;

// inner_margin: Margin::symmetric(12, 8) → 24 px total horizontal
const FRAME_H_MARGIN: f32 = 24.0;
const ROW_H: f32 = 14.0;
const CELL_PAD: f32 = 4.0; // inner padding so text doesn't sit flush at cell edges

fn fmt_ram(mb: u64) -> String {
    if mb >= 1024 {
        format!("{:.1}G", mb as f64 / 1024.0)
    } else {
        format!("{mb}M")
    }
}

/// Paint a table row at fixed absolute x positions — bypasses egui layout entirely
/// so column positions never shift regardless of text content.
/// Layout: NAME = 2/4, CPU = 1/4, RAM = 1/4 of inner_w.
/// Colors for [name, cpu, ram] columns.
type RowColors = [egui::Color32; 3];

fn paint_row(ui: &mut Ui, inner_w: f32, name: &str, cpu: &str, ram: &str, colors: RowColors) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(inner_w, ROW_H), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let font_id = egui::TextStyle::Small.resolve(ui.style());
    let cy = rect.center().y;
    let x0 = rect.min.x;

    let col = inner_w / 4.0;
    let name_w = col * 2.0;
    let cpu_w = col;
    // ram_w = col (implicitly from name_w + cpu_w to rect.max.x)

    // NAME — left-aligned with padding, clipped to name column
    let name_clip = Rect::from_min_size(rect.min, Vec2::new(name_w - CELL_PAD, ROW_H));
    ui.painter().with_clip_rect(name_clip).text(
        pos2(x0, cy),
        Align2::LEFT_CENTER,
        name,
        font_id.clone(),
        colors[0],
    );

    // CPU — right-aligned inside its quarter, with inner padding
    let cpu_right = x0 + name_w + cpu_w - CELL_PAD;
    ui.painter().text(
        pos2(cpu_right, cy),
        Align2::RIGHT_CENTER,
        cpu,
        font_id.clone(),
        colors[1],
    );

    // RAM — right-aligned flush to row right edge (with padding)
    ui.painter().text(
        pos2(rect.max.x - CELL_PAD, cy),
        Align2::RIGHT_CENTER,
        ram,
        font_id,
        colors[2],
    );
}

pub fn draw(ui: &mut Ui, stats: &PollStats, opacity: f32) {
    // Derive inner_w from the outer UI's max_rect — set once by window width, never changes.
    let inner_w = ui.max_rect().width() - FRAME_H_MARGIN;

    theme::panel_frame(ui, theme::C_PROC, opacity, |ui| {
        ui.set_min_height(theme::PANEL_DATA_H);
        ui.allocate_space(Vec2::new(inner_w, 0.0));

        ui.label(
            RichText::new("PROCESSES")
                .strong()
                .color(theme::C_PANEL_TITLE)
                .size(theme::FONT_PANEL_TITLE),
        );

        if stats.processes.is_empty() {
            ui.label(RichText::new("no data").small().color(theme::C_TEXT_MUTED));
            return;
        }

        ui.add_space(2.0);
        paint_row(ui, inner_w, "NAME", "CPU", "RAM", [theme::C_STAT_LABEL; 3]);
        ui.add_space(2.0);

        ui.spacing_mut().item_spacing.y = 3.0;
        for p in &stats.processes {
            let name = p.name.trim_end_matches(".exe");
            paint_row(
                ui,
                inner_w,
                name,
                &format!("{:.1}%", p.cpu),
                &fmt_ram(p.mem_mb),
                [theme::C_TEXT, theme::C_PROC, theme::C_TEXT_MUTED],
            );
        }
    });
}
