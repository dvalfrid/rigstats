use egui::{Color32, ProgressBar, RichText, Ui};

use crate::tempcolor::{color_accent, temp_color};
use crate::PollStats;

const LABEL_COLOR: Color32 = Color32::from_gray(130);
const PAGE_SIZE: usize = 3;
const TICKS_PER_PAGE: u32 = 5;

fn safe_bar_width(ui: &Ui) -> f32 {
  let w = ui.available_width();
  if w.is_finite() && w > 0.0 { w } else { ui.ctx().content_rect().width().max(1.0) }
}

fn fmt_gb(bytes: u64) -> String {
  format!("{:.1}", bytes as f64 / 1_073_741_824.0)
}

pub fn draw(ui: &mut Ui, stats: &PollStats, page: &mut usize, page_tick: &mut u32) {
  let drives = &stats.disk_drives;

  ui.horizontal(|ui| {
    ui.label(RichText::new("DISK").strong());
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
      if stats.lhm_connected {
        ui.label(
          RichText::new(format!("R {:.1}  W {:.1} MB/s", stats.disk_read_mbps, stats.disk_write_mbps))
            .small()
            .color(Color32::from_gray(160)),
        );
      }
    });
  });

  if drives.is_empty() {
    ui.label(RichText::new("no drives").small().color(LABEL_COLOR));
    return;
  }

  // Page cycling when more than PAGE_SIZE drives.
  let total_pages = drives.len().div_ceil(PAGE_SIZE);
  if total_pages > 1 {
    *page_tick += 1;
    if *page_tick >= TICKS_PER_PAGE {
      *page_tick = 0;
      *page = (*page + 1) % total_pages;
    }
  }

  let start = (*page * PAGE_SIZE).min(drives.len());
  let end = (start + PAGE_SIZE).min(drives.len());

  for drive in &drives[start..end] {
    let frac = drive.pct as f32 / 100.0;

    ui.horizontal(|ui| {
      ui.label(RichText::new(&drive.fs).small().color(Color32::from_gray(200)));
      ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if let Some(t) = drive.temp {
          ui.label(
            RichText::new(format!("{t:.0}°C"))
              .small()
              .color(temp_color(Some(t), 50, 60)),
          );
        }
        ui.label(
          RichText::new(format!("{}/{} GB  {}%", fmt_gb(drive.used), fmt_gb(drive.total), drive.pct))
            .small()
            .color(LABEL_COLOR),
        );
      });
    });

    let bar_color = if drive.pct >= 90 {
      egui::Color32::from_rgb(255, 58, 31)
    } else if drive.pct >= 75 {
      egui::Color32::from_rgb(255, 179, 0)
    } else {
      color_accent()
    };

    let bw = safe_bar_width(ui);
    ui.add(
      ProgressBar::new(frac)
        .desired_width(bw)
        .fill(bar_color),
    );
  }

  if total_pages > 1 {
    ui.label(
      RichText::new(format!("page {}/{}", *page + 1, total_pages))
        .small()
        .color(Color32::from_gray(100)),
    );
  }
}
