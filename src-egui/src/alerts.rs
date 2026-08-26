//! Threshold-breach alert detection — pure comparison logic, no I/O.
//!
//! [`pending_alerts`] only decides whether a component's current reading has
//! crossed its configured warn/crit threshold. It does not consult
//! `notify_on_warn`/`notify_on_crit`, does not track per-alert cooldowns, and
//! never sends a notification — all of that is the caller's job (`main.rs`),
//! kept separate so the comparison logic itself stays trivially unit-testable.

use crate::poll::PollStats;
use rigstats_backend::settings::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertLevel {
    Warn,
    Crit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingAlert {
    /// Threshold-map key + cooldown-tracking key prefix, e.g. `"cpu"`, `"disk"`, `"battery"`.
    pub component: String,
    pub level: AlertLevel,
    /// Human-readable label for the notification body, e.g. "CPU", "Disk (D:)".
    pub label: String,
    pub value: f64,
}

/// Checks a single "fires when reading **exceeds** the threshold" component
/// (temperatures, battery power draw) and returns the most severe breach, if
/// any. Crit takes precedence over warn.
fn check_above(
    settings: &Settings,
    component: &str,
    label: String,
    value: Option<f64>,
) -> Option<PendingAlert> {
    let v = value?;
    let t = settings.thresholds.get(component)?;
    if let Some(crit) = t.crit {
        if v >= crit as f64 {
            return Some(PendingAlert {
                component: component.to_string(),
                level: AlertLevel::Crit,
                label,
                value: v,
            });
        }
    }
    if let Some(warn) = t.warn {
        if v >= warn as f64 {
            return Some(PendingAlert {
                component: component.to_string(),
                level: AlertLevel::Warn,
                label,
                value: v,
            });
        }
    }
    None
}

/// Checks a single "fires when reading **drops below** the threshold"
/// component (battery charge %). Crit takes precedence over warn.
fn check_below(
    settings: &Settings,
    component: &str,
    label: String,
    value: Option<f64>,
) -> Option<PendingAlert> {
    let v = value?;
    let t = settings.thresholds.get(component)?;
    if let Some(crit) = t.crit {
        if v <= crit as f64 {
            return Some(PendingAlert {
                component: component.to_string(),
                level: AlertLevel::Crit,
                label,
                value: v,
            });
        }
    }
    if let Some(warn) = t.warn {
        if v <= warn as f64 {
            return Some(PendingAlert {
                component: component.to_string(),
                level: AlertLevel::Warn,
                label,
                value: v,
            });
        }
    }
    None
}

/// Returns every threshold currently breached by `stats`. Crit takes
/// precedence over warn for the same component (only one alert per
/// component per call, the more severe one).
pub fn pending_alerts(stats: &PollStats, settings: &Settings) -> Vec<PendingAlert> {
    let mut out = Vec::new();

    out.extend(check_above(
        settings,
        "cpu",
        "CPU".to_string(),
        stats.cpu_temp,
    ));
    out.extend(check_above(
        settings,
        "gpu",
        "GPU".to_string(),
        stats.gpu_temp,
    ));
    out.extend(check_above(
        settings,
        "ram",
        "RAM".to_string(),
        stats.ram_temp,
    ));
    for drive in &stats.disk_drives {
        out.extend(check_above(
            settings,
            "disk",
            format!("Disk ({})", drive.fs),
            drive.temp,
        ));
    }

    if stats.battery_present {
        // Battery charge: fires when % drops BELOW threshold (warn > crit).
        out.extend(check_below(
            settings,
            "battery",
            "Battery charge".to_string(),
            stats.battery_charge_pct.map(|p| p as f64),
        ));

        // Battery power draw: fires when watts exceed threshold, discharge only.
        if stats.battery_charging == Some(false) {
            out.extend(check_above(
                settings,
                "battery_power",
                "Battery power draw".to_string(),
                stats.battery_power_w,
            ));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rigstats_backend::settings::ComponentThresholds;
    use std::collections::HashMap;

    fn settings_with(thresholds: &[(&str, Option<u8>, Option<u8>)]) -> Settings {
        let mut s = Settings::default();
        let mut map = HashMap::new();
        for (k, warn, crit) in thresholds {
            map.insert(
                k.to_string(),
                ComponentThresholds {
                    warn: *warn,
                    crit: *crit,
                },
            );
        }
        s.thresholds = map;
        s
    }

    fn stats_with_cpu_temp(v: f64) -> PollStats {
        PollStats {
            cpu_temp: Some(v),
            ..Default::default()
        }
    }

    #[test]
    fn below_warn_fires_nothing() {
        let s = settings_with(&[("cpu", Some(80), Some(90))]);
        let stats = stats_with_cpu_temp(70.0);
        assert!(pending_alerts(&stats, &s).is_empty());
    }

    #[test]
    fn at_warn_fires_warn() {
        let s = settings_with(&[("cpu", Some(80), Some(90))]);
        let stats = stats_with_cpu_temp(80.0);
        let alerts = pending_alerts(&stats, &s);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].level, AlertLevel::Warn);
        assert_eq!(alerts[0].component, "cpu");
    }

    #[test]
    fn at_crit_fires_crit_not_warn() {
        let s = settings_with(&[("cpu", Some(80), Some(90))]);
        let stats = stats_with_cpu_temp(90.0);
        let alerts = pending_alerts(&stats, &s);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].level, AlertLevel::Crit);
    }

    #[test]
    fn missing_value_fires_nothing() {
        let s = settings_with(&[("cpu", Some(80), Some(90))]);
        let stats = PollStats::default();
        assert!(pending_alerts(&stats, &s).is_empty());
    }

    #[test]
    fn missing_threshold_entry_fires_nothing() {
        let s = settings_with(&[]);
        let stats = stats_with_cpu_temp(200.0);
        assert!(pending_alerts(&stats, &s).is_empty());
    }

    #[test]
    fn blank_threshold_field_disables_that_level() {
        // warn disabled, crit set — a value that would breach warn shouldn't
        // fire until it actually reaches crit.
        let s = settings_with(&[("cpu", None, Some(90))]);
        let stats = stats_with_cpu_temp(85.0);
        assert!(pending_alerts(&stats, &s).is_empty());
    }

    #[test]
    fn multiple_drives_each_checked_independently() {
        let s = settings_with(&[("disk", Some(50), Some(70))]);
        let stats = PollStats {
            disk_drives: vec![
                crate::poll::DriveInfo {
                    fs: "C:".to_string(),
                    temp: Some(40.0),
                    ..Default::default()
                },
                crate::poll::DriveInfo {
                    fs: "D:".to_string(),
                    temp: Some(75.0),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let alerts = pending_alerts(&stats, &s);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].label, "Disk (D:)");
        assert_eq!(alerts[0].level, AlertLevel::Crit);
    }

    #[test]
    fn battery_charge_fires_when_below_threshold() {
        let s = settings_with(&[("battery", Some(20), Some(10))]);
        let stats = PollStats {
            battery_present: true,
            battery_charge_pct: Some(15),
            ..Default::default()
        };
        let alerts = pending_alerts(&stats, &s);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].level, AlertLevel::Warn);
        assert_eq!(alerts[0].component, "battery");
    }

    #[test]
    fn battery_charge_above_warn_fires_nothing() {
        let s = settings_with(&[("battery", Some(20), Some(10))]);
        let stats = PollStats {
            battery_present: true,
            battery_charge_pct: Some(50),
            ..Default::default()
        };
        assert!(pending_alerts(&stats, &s).is_empty());
    }

    #[test]
    fn battery_charge_ignored_when_not_present() {
        let s = settings_with(&[("battery", Some(20), Some(10))]);
        let stats = PollStats {
            battery_present: false,
            battery_charge_pct: Some(5),
            ..Default::default()
        };
        assert!(pending_alerts(&stats, &s).is_empty());
    }

    #[test]
    fn battery_power_fires_only_while_discharging() {
        let s = settings_with(&[("battery_power", Some(15), Some(25))]);
        let charging = PollStats {
            battery_present: true,
            battery_charging: Some(true),
            battery_power_w: Some(30.0),
            ..Default::default()
        };
        assert!(pending_alerts(&charging, &s).is_empty());

        let discharging = PollStats {
            battery_present: true,
            battery_charging: Some(false),
            battery_power_w: Some(30.0),
            ..Default::default()
        };
        let alerts = pending_alerts(&discharging, &s);
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].level, AlertLevel::Crit);
    }
}
