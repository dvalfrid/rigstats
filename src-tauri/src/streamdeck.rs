//! Elgato Stream Deck XL integration.
//!
//! Renders live hardware telemetry on the 4 × 8 key grid of a Stream Deck XL.
//! Each key is a 96 × 96 px JPEG image produced from an in-memory RGB buffer.
//!
//! A dedicated `std::thread` connects to the first XL/XLV2 found via USB HID,
//! then reads the cached `AppState::last_stats` snapshot once per second and
//! pushes fresh key images.  Connection errors are handled transparently: the
//! thread sleeps 5 s and retries automatically.

use crate::debug::append_debug_log;
use crate::stats::{AppState, DiskDrive, StatsPayload};
use elgato_streamdeck::{info::Kind, list_devices, new_hidapi, StreamDeck};
use font8x8::UnicodeFonts;
use image::{DynamicImage, ImageBuffer, Rgb, RgbImage};
use std::time::Duration;
use tauri::{AppHandle, Manager};

// ── Key geometry ─────────────────────────────────────────────────────────────

const W: u32 = 96;
const H: u32 = 96;

// ── Colour palette (mirrors the main UI CSS variables) ────────────────────────
//
// --bg / --panel  →  BG
// --accent        →  C_CPU  (0, 200, 255)
// --amd           →  C_GPU  (255, 58, 31)
// --ram           →  C_RAM  (255, 179, 0)
// --grn           →  C_NET  (57, 255, 136)
// --pur           →  C_DISK (191, 127, 255)
// --text          →  TEXT
// --dim           →  DIM

const BG: [u8; 3] = [6, 7, 10];
const TEXT: [u8; 3] = [184, 204, 232];
const DIM: [u8; 3] = [46, 61, 90];
const WARN: [u8; 3] = [255, 179, 0];
const CRIT: [u8; 3] = [255, 58, 31];

const C_CPU: [u8; 3] = [0, 200, 255];
const C_GPU: [u8; 3] = [255, 58, 31];
const C_RAM: [u8; 3] = [255, 179, 0];
const C_NET: [u8; 3] = [57, 255, 136];
const C_DISK: [u8; 3] = [191, 127, 255];
const C_MB: [u8; 3] = [0, 180, 200];

// ── Entry point ───────────────────────────────────────────────────────────────

/// Spawns the Stream Deck XL background thread.  Safe to call when no device
/// is attached — the thread retries every 5 s until one appears.
pub fn spawn_streamdeck_loop(app: &AppHandle) {
  let app_clone = app.clone();
  let log_app = app.clone();
  if let Err(e) = std::thread::Builder::new()
    .name("streamdeck".into())
    .spawn(move || device_loop(&app_clone))
  {
    append_debug_log(&log_app, &format!("streamdeck: failed to spawn thread: {e}"));
  }
}

// ── Internal loop ─────────────────────────────────────────────────────────────

fn device_loop(app: &AppHandle) {
  loop {
    match run_session(app) {
      Ok(()) => append_debug_log(app, "streamdeck: session ended; retrying in 5 s"),
      Err(e) => append_debug_log(app, &format!("streamdeck: {e}; retrying in 5 s")),
    }
    std::thread::sleep(Duration::from_secs(5));
  }
}

fn run_session(app: &AppHandle) -> Result<(), String> {
  let hid = new_hidapi().map_err(|e| format!("hidapi init: {e}"))?;
  let devices = list_devices(&hid);

  let (kind, serial) = devices
    .iter()
    .find(|(k, _)| matches!(k, Kind::Xl | Kind::XlV2))
    .ok_or_else(|| "no Stream Deck XL detected".to_string())?;

  let deck = StreamDeck::connect(&hid, *kind, serial).map_err(|e| format!("connect: {e}"))?;

  append_debug_log(app, &format!("streamdeck: connected (serial {serial})"));
  deck.set_brightness(70).map_err(|e| format!("brightness: {e}"))?;

  let state = app.state::<AppState>();

  loop {
    let snap: Option<StatsPayload> = state.last_stats.lock().unwrap_or_else(|e| e.into_inner()).clone();

    if let Err(e) = push_frame(&deck, snap.as_ref()) {
      return Err(format!("render error: {e}"));
    }

    std::thread::sleep(Duration::from_secs(1));
  }
}

// ── Frame rendering ───────────────────────────────────────────────────────────

fn push_frame(deck: &StreamDeck, stats: Option<&StatsPayload>) -> Result<(), String> {
  for key in 0u8..32 {
    let img = render_key(key, stats);
    deck.set_button_image(key, img).map_err(|e| format!("key {key}: {e}"))?;
  }
  deck.flush().map_err(|e| format!("flush: {e}"))?;
  Ok(())
}

// ── Key layout ────────────────────────────────────────────────────────────────
//
//  Row 0 cols 0-3: CPU — header / load / temp / freq
//  Row 0 cols 4-7: GPU — header / load / temp / VRAM
//  Row 1 cols 0-3: CPU extras — power / core avg / blank / blank
//  Row 1 cols 4-7: GPU extras — power / fan / hotspot / blank
//  Row 2 cols 0-3: RAM — header / used% / GB used/total / temp
//  Row 2 cols 4-7: NET — header / upload / download / ping
//  Row 3 cols 0-4: DISK — header / read / write / drive 0 / drive 1
//  Row 3 cols 5-7: MB  — header / fan / temp

fn render_key(key: u8, s: Option<&StatsPayload>) -> DynamicImage {
  match key {
    // ── Row 0: CPU ────────────────────────────────────────────────────────
    0 => hdr("CPU", C_CPU),
    1 => pct_ring_opt("LOAD", s.map(|x| x.cpu.load), C_CPU),
    2 => temp_stat("TEMP", s.and_then(|x| x.cpu.temp), C_CPU),
    3 => stat("FREQ", &s.map(|x| fmt_ghz(x.cpu.freq)).unwrap_or_else(na), C_CPU, TEXT),

    // ── Row 0: GPU ────────────────────────────────────────────────────────
    4 => hdr("GPU", C_GPU),
    5 => pct_ring_opt("LOAD", s.and_then(|x| x.gpu.load.map(|l| l.round() as u8)), C_GPU),
    6 => temp_stat("TEMP", s.and_then(|x| x.gpu.temp), C_GPU),
    7 => stat(
      "VRAM",
      &s.map(|x| fmt_vram(x.gpu.vram_used, x.gpu.vram_total))
        .unwrap_or_else(na),
      C_GPU,
      TEXT,
    ),

    // ── Row 1: CPU extras ─────────────────────────────────────────────────
    8 => stat(
      "PWR",
      &s.and_then(|x| x.cpu.power.map(fmt_watts)).unwrap_or_else(na),
      C_CPU,
      TEXT,
    ),
    9 => pct_ring_opt("CORES", s.map(|x| core_avg_pct(&x.cpu.cores)), C_CPU),
    10 | 11 => blank(),

    // ── Row 1: GPU extras ─────────────────────────────────────────────────
    12 => stat(
      "PWR",
      &s.and_then(|x| x.gpu.power.map(fmt_watts)).unwrap_or_else(na),
      C_GPU,
      TEXT,
    ),
    13 => stat(
      "FAN",
      &s.and_then(|x| x.gpu.fan_speed.map(fmt_rpm)).unwrap_or_else(na),
      C_GPU,
      TEXT,
    ),
    14 => temp_stat("HTSPOT", s.and_then(|x| x.gpu.hotspot), C_GPU),
    15 => blank(),

    // ── Row 2: RAM ────────────────────────────────────────────────────────
    16 => hdr("RAM", C_RAM),
    17 => pct_ring_opt("USED", s.map(ram_pct), C_RAM),
    18 => stat(
      "GB",
      &s.map(|x| fmt_gb(x.ram.used, x.ram.total)).unwrap_or_else(na),
      C_RAM,
      TEXT,
    ),
    19 => temp_stat("TEMP", s.and_then(|x| x.ram.temp), C_RAM),

    // ── Row 2: NET ────────────────────────────────────────────────────────
    20 => hdr("NET", C_NET),
    21 => stat("UP", &s.map(|x| fmt_mbps(x.net.up)).unwrap_or_else(na), C_NET, TEXT),
    22 => stat("DOWN", &s.map(|x| fmt_mbps(x.net.down)).unwrap_or_else(na), C_NET, TEXT),
    23 => stat(
      "PING",
      &s.and_then(|x| x.net.ping_ms.map(fmt_ping)).unwrap_or_else(na),
      C_NET,
      TEXT,
    ),

    // ── Row 3: DISK ───────────────────────────────────────────────────────
    24 => hdr("DISK", C_DISK),
    25 => stat(
      "READ",
      &s.map(|x| fmt_mbps(x.disk.read)).unwrap_or_else(na),
      C_DISK,
      TEXT,
    ),
    26 => stat(
      "WRITE",
      &s.map(|x| fmt_mbps(x.disk.write)).unwrap_or_else(na),
      C_DISK,
      TEXT,
    ),
    27 => drive(s.and_then(|x| x.disk.drives.first())),
    28 => drive(s.and_then(|x| x.disk.drives.get(1))),

    // ── Row 3: MB ─────────────────────────────────────────────────────────
    29 => hdr("MB", C_MB),
    30 => stat(
      "FAN",
      &s.and_then(|x| x.motherboard.fans.first().map(|(_, r)| fmt_rpm(*r)))
        .unwrap_or_else(na),
      C_MB,
      TEXT,
    ),
    31 => temp_stat(
      "TEMP",
      s.and_then(|x| x.motherboard.temps.first().map(|(_, t)| *t)),
      C_MB,
    ),

    _ => blank(),
  }
}

fn na() -> String {
  "--".to_string()
}

// ── Key factories ─────────────────────────────────────────────────────────────

/// Header key: dark HUD background, large neon label with glow, corner brackets.
fn hdr(label: &str, accent: [u8; 3]) -> DynamicImage {
  let mut img = grid_bg(accent);
  draw_top_line(&mut img, accent, 2);
  draw_bottom_line(&mut img, dim(accent, 0.25));
  draw_corners(&mut img, accent);

  let nchars = label.chars().count() as u32;
  let scale = if nchars <= 4 { 3u32 } else { 2u32 };
  let tw = nchars * 8 * scale;
  let x = if tw < W { (W - tw) / 2 } else { 2 };
  let y = (H - 8 * scale) / 2;

  // Glow pass — offset copies at 20 % brightness
  let glow = dim(accent, 0.20);
  if x > 0 {
    draw_str(&mut img, label, x - 1, y, scale, glow);
  }
  if y > 0 {
    draw_str(&mut img, label, x, y - 1, scale, glow);
  }
  draw_str(&mut img, label, x + 1, y, scale, glow);
  draw_str(&mut img, label, x, y + 1, scale, glow);
  // Main text
  draw_str(&mut img, label, x, y, scale, accent);

  DynamicImage::ImageRgb8(img)
}

/// Stat key: dark HUD background, accent top line, dim label, bright value.
fn stat(label: &str, value: &str, accent: [u8; 3], val_col: [u8; 3]) -> DynamicImage {
  let mut img = grid_bg(accent);
  draw_top_line(&mut img, accent, 1);

  // Label — small, in a dim version of the accent colour
  let label_col = dim(accent, 0.55);
  draw_str(&mut img, label, 5, 9, 1, label_col);

  // Separator  (very faint, same accent)
  let sep = dim(accent, 0.18);
  for x in 3..93 {
    img.put_pixel(x, 22, Rgb(sep));
  }

  // Value — centred in the lower region
  let nv = value.chars().count() as u32;
  let scale = if nv <= 5 { 3u32 } else { 2u32 };
  let gh = 8 * scale;
  let tw = nv * 8 * scale;
  let vx = if tw < W { (W - tw) / 2 } else { 2 };
  // Centre vertically between separator and bottom
  let vy = 24 + (H - 24 - gh) / 2;
  draw_str(&mut img, value, vx, vy, scale, val_col);

  DynamicImage::ImageRgb8(img)
}

/// Temperature stat key — value colour tracks warn/crit thresholds.
fn temp_stat(label: &str, temp: Option<f64>, accent: [u8; 3]) -> DynamicImage {
  match temp {
    None => stat(label, "--", accent, DIM),
    Some(t) => {
      let col = if t >= 90.0 {
        CRIT
      } else if t >= 75.0 {
        WARN
      } else {
        TEXT
      };
      stat(label, &format!("{:.0}C", t), accent, col)
    }
  }
}

/// Drive key — shows mount label and usage percentage as a ring.
fn drive(d: Option<&DiskDrive>) -> DynamicImage {
  match d {
    None => blank(),
    Some(d) => {
      let lbl = d.fs.trim_end_matches(['\\', '/']);
      pct_ring(lbl, d.pct, C_DISK)
    }
  }
}

/// All-black placeholder for unused key slots.
fn blank() -> DynamicImage {
  DynamicImage::ImageRgb8(ImageBuffer::from_pixel(W, H, Rgb(BG)))
}

/// Percentage ring — arc with 60° bottom gap, gradient fill, centred "X%" label.
///
/// The 300° arc starts at 210° (bottom-left) and fills clockwise.
/// Colour interpolates blue → green → yellow → red based on the percentage value.
fn pct_ring(label: &str, pct: u8, accent: [u8; 3]) -> DynamicImage {
  let mut img = grid_bg(accent);

  let cx = (W / 2) as f32;
  let cy = (H / 2) as f32 + 4.0; // shift down slightly to leave room for label
  let r_outer: f32 = 36.0;
  let r_inner: f32 = 28.0;

  // 60° gap centred on 180° (bottom).  Arc spans 300°.
  let gap_start: f32 = 150.0;
  let gap_end: f32 = 210.0;
  let filled_deg = pct.min(100) as f32 * 3.0; // 100 % → 300 °

  let col_filled = pct_color(pct);
  let col_empty = dim(col_filled, 0.20);

  for py in 0..H {
    for px in 0..W {
      let dx = px as f32 - cx;
      let dy = py as f32 - cy;
      let dist = (dx * dx + dy * dy).sqrt();
      if dist < r_inner || dist > r_outer {
        continue;
      }
      // Angle: 0° = top (12 o'clock), increases clockwise.
      let mut angle = dx.atan2(-dy).to_degrees();
      if angle < 0.0 {
        angle += 360.0;
      }
      if angle >= gap_start && angle <= gap_end {
        continue;
      }
      // Arc position: 0° at gap_end (210°), increases clockwise to 300°.
      let arc_pos = (angle - gap_end + 360.0) % 360.0;
      img.put_pixel(px, py, Rgb(if arc_pos <= filled_deg { col_filled } else { col_empty }));
    }
  }

  // Small dim label above the ring.
  let label_col = dim(accent, 0.55);
  let llen = label.chars().count() as u32;
  let lx = if W > llen * 8 { (W - llen * 8) / 2 } else { 2 };
  draw_str(&mut img, label, lx, 6, 1, label_col);

  // Percentage value centred inside the ring (scale 2 = 16 px tall).
  let val = format!("{}%", pct);
  let tw = val.chars().count() as u32 * 16;
  let vx = if tw < W { (W - tw) / 2 } else { 2 };
  let vy = cy as u32 - 8; // 16 px text → offset 8 to centre on cy
  draw_str(&mut img, &val, vx, vy, 2, TEXT);

  DynamicImage::ImageRgb8(img)
}

/// Falls back to a plain `--` stat when percentage is unavailable.
fn pct_ring_opt(label: &str, pct: Option<u8>, accent: [u8; 3]) -> DynamicImage {
  match pct {
    Some(p) => pct_ring(label, p, accent),
    None => stat(label, "--", accent, DIM),
  }
}

/// Blue → green → yellow → red gradient keyed to 0–100 %.
fn pct_color(pct: u8) -> [u8; 3] {
  let t = pct.min(100) as f32 / 100.0;
  if t < 1.0 / 3.0 {
    lerp_color([0, 100, 255], [57, 255, 20], t * 3.0)
  } else if t < 2.0 / 3.0 {
    lerp_color([57, 255, 20], [255, 220, 0], (t - 1.0 / 3.0) * 3.0)
  } else {
    lerp_color([255, 220, 0], [255, 40, 0], (t - 2.0 / 3.0) * 3.0)
  }
}

fn lerp_color(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
  [
    (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).round() as u8,
    (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).round() as u8,
    (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).round() as u8,
  ]
}

/// Average of per-core load values as a percentage (0–100).
fn core_avg_pct(cores: &[u8]) -> u8 {
  if cores.is_empty() {
    return 0;
  }
  (cores.iter().map(|&c| c as u32).sum::<u32>() / cores.len() as u32) as u8
}

// ── Background ────────────────────────────────────────────────────────────────

/// Dark background with a very faint cyan dot grid (matches the dashboard
/// `::after` grid overlay).
fn grid_bg(accent: [u8; 3]) -> RgbImage {
  // Grid dot: BG shifted slightly toward the accent colour, barely visible.
  let dot = add_dim(BG, accent, 0.06);
  ImageBuffer::from_fn(W, H, |x, y| if x % 6 == 0 && y % 6 == 0 { Rgb(dot) } else { Rgb(BG) })
}

// ── Decorative elements ───────────────────────────────────────────────────────

/// Gradient horizontal line — brighter in the centre, fades toward the edges.
fn draw_top_line(img: &mut RgbImage, color: [u8; 3], thickness: u32) {
  for x in 0..W {
    let t = 1.0 - (x as f32 / W as f32 - 0.5).abs() * 1.6;
    let t = t.clamp(0.3, 1.0);
    let c = dim(color, t);
    for row in 0..thickness {
      img.put_pixel(x, row, Rgb(c));
    }
  }
}

fn draw_bottom_line(img: &mut RgbImage, color: [u8; 3]) {
  for x in 0..W {
    img.put_pixel(x, H - 1, Rgb(color));
  }
}

/// Small L-shaped corner brackets at all four corners.
fn draw_corners(img: &mut RgbImage, color: [u8; 3]) {
  let s = 5u32; // bracket arm length
  let p = 4u32; // inset from edge

  // top-left
  for i in 0..s {
    img.put_pixel(p + i, p, Rgb(color));
  }
  for i in 1..s {
    img.put_pixel(p, p + i, Rgb(color));
  }
  // top-right
  for i in 0..s {
    img.put_pixel(W - p - 1 - i, p, Rgb(color));
  }
  for i in 1..s {
    img.put_pixel(W - p - 1, p + i, Rgb(color));
  }
  // bottom-left
  for i in 0..s {
    img.put_pixel(p + i, H - p - 1, Rgb(color));
  }
  for i in 1..s {
    img.put_pixel(p, H - p - 1 - i, Rgb(color));
  }
  // bottom-right
  for i in 0..s {
    img.put_pixel(W - p - 1 - i, H - p - 1, Rgb(color));
  }
  for i in 1..s {
    img.put_pixel(W - p - 1, H - p - 1 - i, Rgb(color));
  }
}

// ── Colour helpers ────────────────────────────────────────────────────────────

/// Scale all channels of `c` by `factor` (clamp to [0, 255]).
fn dim(c: [u8; 3], factor: f32) -> [u8; 3] {
  [
    ((c[0] as f32 * factor) as u16).min(255) as u8,
    ((c[1] as f32 * factor) as u16).min(255) as u8,
    ((c[2] as f32 * factor) as u16).min(255) as u8,
  ]
}

/// Add a fraction of `accent` to `base` (for the subtle grid tint).
fn add_dim(base: [u8; 3], accent: [u8; 3], t: f32) -> [u8; 3] {
  [
    ((base[0] as f32 + accent[0] as f32 * t) as u16).min(255) as u8,
    ((base[1] as f32 + accent[1] as f32 * t) as u16).min(255) as u8,
    ((base[2] as f32 + accent[2] as f32 * t) as u16).min(255) as u8,
  ]
}

// ── Glyph rendering ───────────────────────────────────────────────────────────

fn draw_str(img: &mut RgbImage, text: &str, mut x: u32, y: u32, scale: u32, color: [u8; 3]) {
  for ch in text.chars() {
    if let Some(glyph) = font8x8::BASIC_FONTS.get(ch) {
      draw_glyph(img, &glyph, x, y, scale, color);
    }
    x += 8 * scale;
  }
}

fn draw_glyph(img: &mut RgbImage, glyph: &[u8; 8], x: u32, y: u32, scale: u32, color: [u8; 3]) {
  for (row, &bits) in glyph.iter().enumerate() {
    for col in 0..8u32 {
      if (bits >> col) & 1 != 0 {
        for dy in 0..scale {
          for dx in 0..scale {
            let px = x + col * scale + dx;
            let py = y + row as u32 * scale + dy;
            if px < W && py < H {
              img.put_pixel(px, py, Rgb(color));
            }
          }
        }
      }
    }
  }
}

// ── Value formatters ──────────────────────────────────────────────────────────

fn fmt_ghz(f: f64) -> String {
  format!("{:.1}G", f)
}

fn fmt_watts(w: f64) -> String {
  if w >= 1000.0 {
    format!("{:.1}kW", w / 1000.0)
  } else {
    format!("{:.0}W", w)
  }
}

fn fmt_rpm(r: f64) -> String {
  let rpm = r.round() as u64;
  if rpm >= 10_000 {
    format!("{}k", rpm / 1000)
  } else {
    format!("{}", rpm)
  }
}

fn fmt_mbps(mbps: f64) -> String {
  if mbps >= 1000.0 {
    format!("{:.0}G", mbps / 1000.0)
  } else {
    format!("{:.0}M", mbps)
  }
}

fn fmt_ping(ms: f64) -> String {
  format!("{:.0}ms", ms)
}

fn fmt_vram(used: Option<f64>, total: Option<f64>) -> String {
  match (used, total) {
    (Some(u), Some(t)) => format!("{}/{}G", (u / 1024.0).round() as u64, (t / 1024.0).round() as u64),
    _ => "--".to_string(),
  }
}

fn fmt_gb(used: u64, total: u64) -> String {
  let ug = used as f64 / 1_073_741_824.0;
  let tg = total as f64 / 1_073_741_824.0;
  format!("{:.0}/{:.0}", ug, tg)
}

fn ram_pct(s: &StatsPayload) -> u8 {
  if s.ram.total == 0 {
    return 0;
  }
  ((s.ram.used as f64 / s.ram.total as f64) * 100.0).round() as u8
}
