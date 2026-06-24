//! Shared library for the RIGStats egui binaries.
//!
//! Both `rigstats` (the main tray app) and `rigstats-wallpaper` (the
//! WorkerW desktop-wallpaper host process) link against this crate so the
//! panel rendering core lives in exactly one place. See `docs/architecture.md`.

pub mod brand;
pub mod dashboard;
pub mod geometry;
pub mod lock_ext;
pub mod panels;
pub mod poll;
pub mod ring;
pub mod spark;
pub mod tempcolor;
pub mod theme;
pub mod tray;
pub mod update_check;
#[cfg(windows)]
pub mod win32_behind;
#[cfg(windows)]
pub mod win32_dark_mode;
#[cfg(windows)]
pub mod win32_wallpaper;
#[cfg(windows)]
pub mod win_opacity;
pub mod windows;

// Re-exported so panel modules (and the binaries) can refer to `crate::PollStats`.
pub use poll::PollStats;
