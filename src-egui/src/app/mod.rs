//! Parts of the main binary's `RigStatsApp`, split out of `main.rs` (#225).

pub(crate) mod background;
mod dialogs;
mod floating;
mod main_window;
mod overlay_window;
mod settings_reload;
pub(crate) mod startup;
pub(crate) mod state;
mod tray_actions;
pub(crate) mod tray_card;
