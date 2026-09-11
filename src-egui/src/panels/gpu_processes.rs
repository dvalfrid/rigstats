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
/// Layout: NAME 36% / MID 42% / UTIL 22% of inner_w — MID is wider than in
/// PROCESSES because it can carry both the adapter name and the engine type
/// ("Radeon RX 9070 XT · 3D+Decode") while the GPU-app process names are short.
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

    let name_w = inner_w * 0.36;
    let mid_w = inner_w * 0.42;

    // NAME — left-aligned, clipped to its column
    let name_clip = Rect::from_min_size(rect.min, Vec2::new(name_w - cell_pad, row_h));
    ui.painter().with_clip_rect(name_clip).text(
        pos2(x0, cy),
        Align2::LEFT_CENTER,
        name,
        font_id.clone(),
        colors[0],
    );

    // MID — left-aligned, anchored right after NAME. Was right-aligned against
    // UTIL, which let its start position drift row to row as content length
    // varied ("Radeon Graphics · Decode" vs "RX 9070 XT · 3D") — since rows
    // re-sort by % every tick, that made the column look like it jumped around
    // continuously. Left-aligned to a fixed x, it only ever grows rightward;
    // clipped so overflow doesn't bleed into UTIL.
    let mid_x0 = x0 + name_w;
    let mid_clip =
        Rect::from_min_size(pos2(mid_x0, rect.min.y), Vec2::new(mid_w - cell_pad, row_h));
    ui.painter().with_clip_rect(mid_clip).text(
        pos2(mid_x0, cy),
        Align2::LEFT_CENTER,
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
/// "AMD Radeon(TM) Graphics" → "Radeon Graphics"). The column is left-aligned
/// and clipped, so this also keeps the visible head of an overlong name
/// meaningful rather than starting mid-word.
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

/// Format the panel's middle column: engine type(s) always, plus the physical
/// adapter name when more than one GPU is active. Left-aligned and clipped
/// (see `paint_row`) at a fixed x, so its start position stays put as rows
/// re-sort by utilisation every tick — right-aligned against UTIL, it used to
/// visibly jump around as different-length text landed in different rows.
/// On overflow (a long adapter name in a narrow window) the engine suffix is
/// what gets clipped; the column is sized generously enough in practice that
/// this is rare.
fn mid_cell(adapter: &str, engines: &[String], show_adapter: bool) -> String {
    let mut e = engines.to_vec();
    e.truncate(2);
    let engine_str = e.join("+");
    if !show_adapter {
        return engine_str;
    }
    let adapter = short_adapter(adapter);
    match (adapter.is_empty(), engine_str.is_empty()) {
        (false, false) => format!("{adapter} · {engine_str}"),
        (false, true) => adapter,
        (true, _) => engine_str,
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
                let mid = mid_cell(&p.adapter, &p.engines, show_adapter);
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
    use super::{mid_cell, short_adapter};

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

    #[test]
    fn mid_cell_single_gpu_shows_engine_only() {
        let engines = vec!["3D".to_string(), "Decode".to_string()];
        assert_eq!(
            mid_cell("AMD Radeon RX 9070 XT", &engines, false),
            "3D+Decode"
        );
    }

    #[test]
    fn mid_cell_multi_gpu_combines_adapter_and_engine() {
        let engines = vec!["3D".to_string()];
        assert_eq!(
            mid_cell("AMD Radeon RX 9070 XT", &engines, true),
            "Radeon RX 9070 XT · 3D"
        );
    }

    #[test]
    fn mid_cell_multi_gpu_falls_back_when_engine_unknown() {
        assert_eq!(
            mid_cell("AMD Radeon RX 9070 XT", &[], true),
            "Radeon RX 9070 XT"
        );
    }

    #[test]
    fn mid_cell_multi_gpu_falls_back_when_adapter_unknown() {
        let engines = vec!["Copy".to_string()];
        assert_eq!(mid_cell("", &engines, true), "Copy");
    }
}
