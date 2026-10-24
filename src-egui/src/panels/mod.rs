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

/// A CPU/GPU brand logo's size in the panel header: 38 px tall, its own aspect.
pub(crate) fn logo_size(logo: &egui::TextureHandle, sc: f32) -> egui::Vec2 {
    let [lw, lh] = logo.size();
    let h = 38.0 * sc;
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
