use egui::{pos2, Align2, FontId, Rect, RichText, Sense, Ui, Vec2};

use crate::theme;
use crate::PollStats;

// inner_margin: Margin::symmetric(12, 8) → 24 px total horizontal
const FRAME_H_MARGIN: f32 = 24.0;
const ROW_H: f32 = 14.0;
const CELL_PAD: f32 = 4.0;

/// Colors for [name, mid, util] columns.
type RowColors = [egui::Color32; 3];

/// Paint a table row at fixed absolute x positions (same technique as the
/// PROCESSES panel — bypasses egui layout so columns never shift).
/// Layout: NAME 42% / MID 36% / UTIL 22% of inner_w — MID is wider than in
/// PROCESSES because full adapter names ("Radeon RX 9070 XT") live there while
/// the GPU-app process names are short.
fn paint_row(
    ui: &mut Ui,
    inner_w: f32,
    name: &str,
    mid: &str,
    util: &str,
    colors: RowColors,
    sc: f32,
) {
    let row_h = (ROW_H * sc).round();
    let cell_pad = CELL_PAD * sc;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(inner_w, row_h), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let font_id = FontId::proportional(11.0 * sc);
    let cy = rect.center().y;
    let x0 = rect.min.x;

    let name_w = inner_w * 0.42;
    let mid_w = inner_w * 0.36;

    // NAME — left-aligned, clipped to its column
    let name_clip = Rect::from_min_size(rect.min, Vec2::new(name_w - cell_pad, row_h));
    ui.painter().with_clip_rect(name_clip).text(
        pos2(x0, cy),
        Align2::LEFT_CENTER,
        name,
        font_id.clone(),
        colors[0],
    );

    // MID — right-aligned inside its quarter, clipped so long adapter names
    // don't bleed into NAME
    let mid_right = x0 + name_w + mid_w - cell_pad;
    let mid_clip = Rect::from_min_size(
        pos2(x0 + name_w, rect.min.y),
        Vec2::new(mid_w - cell_pad, row_h),
    );
    ui.painter().with_clip_rect(mid_clip).text(
        pos2(mid_right, cy),
        Align2::RIGHT_CENTER,
        mid,
        font_id.clone(),
        colors[1],
    );

    // UTIL — right-aligned flush to row right edge
    ui.painter().text(
        pos2(rect.max.x - cell_pad, cy),
        Align2::RIGHT_CENTER,
        util,
        font_id,
        colors[2],
    );
}

/// Drop the vendor prefix from an adapter name so the model stands out in the
/// narrow middle column ("NVIDIA GeForce RTX 4070" → "GeForce RTX 4070",
/// "AMD Radeon(TM) Graphics" → "Radeon Graphics"). The column is right-aligned
/// and clipped, so any remaining overflow keeps the most specific tail visible.
fn short_adapter(name: &str) -> String {
    let drop = ["nvidia", "amd", "intel", "corporation"];
    let kept: Vec<String> = name
        .split_whitespace()
        .map(|tok| {
            tok.replace("(TM)", "")
                .replace("(tm)", "")
                .replace("(R)", "")
                .replace("(r)", "")
        })
        .filter(|tok| !tok.is_empty() && !drop.contains(&tok.to_ascii_lowercase().as_str()))
        .collect();
    if kept.is_empty() {
        name.trim().to_string()
    } else {
        kept.join(" ")
    }
}

pub fn draw(
    ui: &mut Ui,
    stats: &PollStats,
    opacity: f32,
    th: &theme::AppTheme,
    sc: f32,
) -> egui::Rect {
    let inner_w = ui.max_rect().width() - FRAME_H_MARGIN * sc;

    // Show the physical-GPU column only when more than one adapter is actually in
    // play — on a single-GPU box it would just repeat the same name, so fall back
    // to the engine type there (Task-Manager style).
    let distinct_adapters: std::collections::BTreeSet<&str> = stats
        .gpu_processes
        .iter()
        .map(|p| p.adapter.as_str())
        .filter(|a| !a.is_empty())
        .collect();
    let show_adapter = distinct_adapters.len() > 1;
    let mid_header = if show_adapter { "GPU" } else { "ENG" };

    theme::panel_frame(ui, opacity, th, sc, |ui| {
        ui.set_min_height(theme::PANEL_DATA_H * sc);
        ui.allocate_space(Vec2::new(inner_w, 0.0));

        ui.label(
            RichText::new("GPU APPS")
                .strong()
                .color(theme::C_PANEL_TITLE)
                .size(theme::FONT_PANEL_TITLE * sc),
        );

        if stats.gpu_processes.is_empty() {
            ui.label(
                RichText::new("no GPU activity")
                    .size(11.0 * sc)
                    .color(th.text_muted),
            );
        } else {
            ui.add_space((2.0 * sc).round());
            paint_row(
                ui,
                inner_w,
                "NAME",
                mid_header,
                "GPU%",
                [th.stat_label; 3],
                sc,
            );
            ui.add_space((2.0 * sc).round());

            ui.spacing_mut().item_spacing.y = (3.0 * sc).round();
            for p in &stats.gpu_processes {
                let name = p.name.trim_end_matches(".exe");
                let mid = if show_adapter {
                    short_adapter(&p.adapter)
                } else {
                    let mut e = p.engines.clone();
                    e.truncate(2);
                    e.join("+")
                };
                paint_row(
                    ui,
                    inner_w,
                    name,
                    &mid,
                    &format!("{:.0}%", p.util_pct),
                    [theme::C_TEXT, th.text_muted, theme::C_PROC],
                    sc,
                );
            }
        }

        // Fill remaining space to reach PANEL_DATA_H (same pattern as PROCESSES).
        let cursor_y = ui.cursor().top();
        let filler = (theme::PANEL_DATA_H * sc - (cursor_y - ui.min_rect().top()))
            .max(0.0)
            .round();
        if filler > 0.0 {
            ui.add_space(filler);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::short_adapter;

    #[test]
    fn drops_nvidia_vendor_prefix() {
        assert_eq!(short_adapter("NVIDIA GeForce RTX 4070"), "GeForce RTX 4070");
    }

    #[test]
    fn drops_intel_vendor_and_tm_marker() {
        assert_eq!(
            short_adapter("Intel(R) UHD Graphics 770"),
            "UHD Graphics 770"
        );
        assert_eq!(short_adapter("AMD Radeon(TM) Graphics"), "Radeon Graphics");
    }

    #[test]
    fn keeps_amd_model() {
        assert_eq!(short_adapter("AMD Radeon RX 9070 XT"), "Radeon RX 9070 XT");
    }

    #[test]
    fn falls_back_when_only_vendor_present() {
        assert_eq!(short_adapter("AMD"), "AMD");
    }
}
