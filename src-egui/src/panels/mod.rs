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
