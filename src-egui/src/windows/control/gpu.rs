//! Control Center: the Graphics card page — power limit per adapter.

use super::{dry_run_note, ControlUi, GpuDraft};
use crate::theme::{self, DialogColors};
use crate::windows::ui_kit::{self};
use rigstats_backend::control::{ControlState, GpuAdapterCap, GpuCaps, Profile};
use std::collections::BTreeMap;

/// The active profile's GPU limits that are set (a missing adapter = original).
pub(super) fn saved_gpu_limits(profile: Option<&Profile>) -> BTreeMap<String, i32> {
    profile
        .and_then(|p| p.part.gpu.as_ref())
        .and_then(|g| g.adapters.as_ref())
        .map(|adapters| {
            adapters
                .iter()
                .filter_map(|(id, c)| Some((id.clone(), c.power_limit_pct?)))
                .collect()
        })
        .unwrap_or_default()
}

/// "−15 %", "+5 %", "0 %".
fn pct_text(value: i32) -> String {
    if value == 0 {
        "0 %".to_owned()
    } else {
        format!("{value:+} %")
    }
}

pub(super) fn gpu_tab(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    control: &ControlState,
    caps: &GpuCaps,
    saved: &BTreeMap<String, i32>,
    ui_state: &mut ControlUi,
) {
    let Some(profile_id) = control.active_profile.clone() else {
        return;
    };
    let mut limits = ui_state
        .gpu_draft
        .as_ref()
        .map_or_else(|| saved.clone(), |d| d.limits.clone());
    let before = limits.clone();

    ui_kit::group(
        ui,
        dc,
        Some("Power limit"),
        Some(
            "The same setting as the Power Limit in AMD Adrenalin or MSI Afterburner, relative \
             to the driver default. Lower runs cooler and quieter; higher allows more boost. \
             New limits are tried first and revert by themselves unless you keep them.",
        ),
        |g| {
            for adapter in &caps.adapters {
                let in_force = format!("In force now: {}", pct_text(adapter.current));
                g.row(&adapter.name, Some(&in_force), |ui| {
                    ui.add_enabled_ui(control.preview.is_none(), |ui| {
                        gpu_limit_slider(ui, dc, adapter, &mut limits);
                    });
                });
            }
        },
    );
    dry_run_note(ui, dc, control);

    if limits != before {
        ui_state.gpu_draft = Some(GpuDraft { profile_id, limits });
    }
}

/// One adapter's slider over the driver's range, its value and an
/// "Original" reset. A value equal to the original is stored as no value.
fn gpu_limit_slider(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    adapter: &GpuAdapterCap,
    limits: &mut BTreeMap<String, i32>,
) {
    let set = limits.get(&adapter.id).copied();
    if set.is_some() && theme::dialog_btn_secondary(ui, "Original", dc).clicked() {
        limits.remove(&adapter.id);
    }
    let text = match set {
        Some(v) => pct_text(v),
        None => format!("{} (original)", pct_text(adapter.original)),
    };
    ui.add_sized(
        [96.0, 20.0],
        egui::Label::new(egui::RichText::new(text).size(12.0).color(dc.text)),
    );
    ui.spacing_mut().slider_width = 170.0;
    let current = set.unwrap_or(adapter.original);
    let mut v = current;
    let step = adapter.step.max(1);
    let slider = egui::Slider::new(&mut v, adapter.min..=adapter.max)
        .show_value(false)
        .trailing_fill(true);
    if ui.add(slider).changed() && v != current {
        // Snap to the driver's step only on a real move.
        let snapped = adapter.min + ((v - adapter.min) as f32 / step as f32).round() as i32 * step;
        set_gpu_limit(limits, adapter, snapped.clamp(adapter.min, adapter.max));
    }
}

fn set_gpu_limit(limits: &mut BTreeMap<String, i32>, adapter: &GpuAdapterCap, value: i32) {
    if value == adapter.original {
        limits.remove(&adapter.id);
    } else {
        limits.insert(adapter.id.clone(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rx9070() -> GpuAdapterCap {
        GpuAdapterCap {
            id: "pci-9070".into(),
            name: "AMD Radeon RX 9070 XT".into(),
            min: -30,
            max: 10,
            step: 1,
            default: 0,
            original: 0,
            current: 0,
        }
    }

    #[test]
    fn gpu_percentages_read_naturally() {
        assert_eq!(pct_text(-15), "-15 %");
        assert_eq!(pct_text(5), "+5 %");
        assert_eq!(pct_text(0), "0 %");
    }

    #[test]
    fn a_gpu_limit_at_the_original_value_is_stored_as_none() {
        let mut limits = BTreeMap::new();
        set_gpu_limit(&mut limits, &rx9070(), -15);
        assert_eq!(limits.get("pci-9070"), Some(&-15));
        set_gpu_limit(&mut limits, &rx9070(), 0);
        assert!(limits.is_empty());
    }
}
