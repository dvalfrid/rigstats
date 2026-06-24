//! Window geometry, profile dimensions, and monitor selection.
//!
//! Pure helpers (profile size/scale, monitor enumeration, pinned-position
//! resolution) extracted from `main.rs`. Most functions are unit-tested at the
//! bottom of this file without enumerating real monitors.

use crate::theme;

pub fn profile_to_size(profile: &str) -> [f32; 2] {
    match profile {
        "portrait-xl" => [450.0, 1920.0],
        "portrait-slim" => [480.0, 1920.0],
        "portrait-hd" => [720.0, 1280.0],
        "portrait-wxga" => [800.0, 1280.0],
        "portrait-fhd" => [1080.0, 1920.0],
        "portrait-wuxga" => [1200.0, 1920.0],
        "portrait-qhd" => [1440.0, 2560.0],
        "portrait-hdplus" => [768.0, 1366.0],
        "portrait-900x1600" => [900.0, 1600.0],
        "portrait-1050x1680" => [1050.0, 1680.0],
        "portrait-1600x2560" => [1600.0, 2560.0],
        "portrait-4k" => [2160.0, 3840.0],
        "portrait-fhd-side" => [253.0, 1080.0],
        "portrait-qhd-side" => [338.0, 1440.0],
        "portrait-4k-side" => [506.0, 2160.0],
        // Landscape profiles — the portrait dimensions transposed (w > h).
        "landscape-xl" => [1920.0, 450.0],
        "landscape-slim" => [1920.0, 480.0],
        "landscape-hd" => [1280.0, 720.0],
        "landscape-wxga" => [1280.0, 800.0],
        "landscape-fhd" => [1920.0, 1080.0],
        "landscape-wuxga" => [1920.0, 1200.0],
        "landscape-qhd" => [2560.0, 1440.0],
        "landscape-hdplus" => [1366.0, 768.0],
        "landscape-1600x900" => [1600.0, 900.0],
        "landscape-1680x1050" => [1680.0, 1050.0],
        "landscape-2560x1600" => [2560.0, 1600.0],
        "landscape-4k" => [3840.0, 2160.0],
        "landscape-fhd-top" => [1080.0, 253.0],
        "landscape-qhd-top" => [1440.0, 338.0],
        "landscape-4k-top" => [2160.0, 506.0],
        _ => [400.0, 780.0],
    }
}

/// True when the profile is a landscape (wide) layout. Landscape profiles use a
/// `landscape-` key prefix and render panels in a grid rather than a vertical stack.
pub fn profile_is_landscape(profile: &str) -> bool {
    profile.starts_with("landscape-")
}

/// Content scale for a profile. Uses portrait-xl (450 px) as the 1.0 reference.
/// Narrow profiles (side panels) scale down; wider profiles stay at 1.0.
pub fn profile_scale(profile: &str) -> f32 {
    const REF_W: f32 = 450.0;
    (profile_to_size(profile)[0] / REF_W).clamp(0.4, 1.0)
}

/// Estimated window height for the given visible panels at scale `sc`.
///
/// Values are calibrated estimates (content + frame inner margin 16 px).
/// Fine-tune by measuring actual rendered heights in the live app.
pub fn compute_window_height(visible_panels: &[String], sc: f32) -> f32 {
    // panel_frame inner_margin: Margin::symmetric(12, 8) → 8 top + 8 bottom = 16 px.
    const V_MARGIN: f32 = 16.0;
    let header_h = (theme::PANEL_HEADER_H + V_MARGIN) * sc;
    let data_h = (theme::PANEL_DATA_H + V_MARGIN) * sc;
    let n = visible_panels.len();
    let mut h = theme::DRAG_HANDLE_H;
    for key in visible_panels {
        h += match key.as_str() {
            "header" | "clock" => header_h,
            _ => data_h,
        };
    }
    // add_space(6.0 * sc) follows every panel in the render loop.
    h + (n as f32 * 6.0 * sc)
}

// ── Windows monitor enumeration ───────────────────────────────────────────────

#[cfg(windows)]
pub mod win_monitor {
    use winapi::shared::minwindef::LPARAM;
    use winapi::shared::windef::{HDC, HMONITOR, LPRECT};
    use winapi::um::shellscalingapi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use winapi::um::winuser::EnumDisplayMonitors;

    struct MonitorData {
        rects: Vec<(i32, i32, i32, i32)>,
    }

    /// Returns (left, top, right, bottom) for every connected display in **logical pixels**.
    /// Physical pixel coordinates from EnumDisplayMonitors are divided by the monitor's
    /// effective DPI scale so that egui window positions (which are in logical pixels) land
    /// on the correct screen position regardless of per-monitor DPI scaling.
    #[allow(unsafe_code)]
    pub fn list() -> Vec<(i32, i32, i32, i32)> {
        #[allow(unsafe_code)]
        unsafe extern "system" fn callback(hm: HMONITOR, _: HDC, lp: LPRECT, data: LPARAM) -> i32 {
            let d = &mut *(data as *mut MonitorData);
            let r = *lp;
            let mut dpi_x: u32 = 96;
            let mut dpi_y: u32 = 96;
            let _ = GetDpiForMonitor(hm, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
            let sx = dpi_x as f32 / 96.0;
            let sy = dpi_y as f32 / 96.0;
            d.rects.push((
                (r.left as f32 / sx) as i32,
                (r.top as f32 / sy) as i32,
                (r.right as f32 / sx) as i32,
                (r.bottom as f32 / sy) as i32,
            ));
            1
        }

        let mut data = MonitorData { rects: Vec::new() };
        #[allow(unsafe_code)]
        unsafe {
            EnumDisplayMonitors(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                Some(callback),
                &mut data as *mut _ as LPARAM,
            );
        }
        data.rects
    }
}

/// Returns the (x, y) top-left position to center a window of `w × h` on the
/// first landscape monitor (or first monitor if all are portrait).
pub fn dialog_center(w: f32, h: f32) -> [f32; 2] {
    #[cfg(windows)]
    {
        let monitors = win_monitor::list();
        let picked = monitors
            .iter()
            .find(|&&(l, t, r, b)| (r - l) >= (b - t)) // landscape first
            .or_else(|| monitors.first());
        if let Some(&(l, t, r, b)) = picked {
            let cx = (l + r) as f32 / 2.0;
            let cy = (t + b) as f32 / 2.0;
            return [cx - w / 2.0, cy - h / 2.0];
        }
    }
    [100.0, 100.0]
}

/// Returns `pos` if it is within any currently connected monitor (allowing 60 px
/// overhang so a panel that straddles a monitor edge is still considered visible),
/// otherwise returns `fallback`.  Call this before applying a saved floating-panel
/// position so panels that were left on a disconnected monitor snap back on-screen.
pub fn guard_panel_position(pos: [f32; 2], fallback: [f32; 2]) -> [f32; 2] {
    #[cfg(windows)]
    {
        if position_on_any_monitor(pos, &win_monitor::list()) {
            pos
        } else {
            fallback
        }
    }
    #[cfg(not(windows))]
    pos
}

/// True when `pos` lies on (or within the 60 px overhang margin of) any monitor
/// in `monitors`. Pure core of [`guard_panel_position`] and the pinned-position
/// check, split out so it can be unit-tested without enumerating real monitors.
fn position_on_any_monitor(pos: [f32; 2], monitors: &[(i32, i32, i32, i32)]) -> bool {
    let margin = 60.0_f32;
    monitors.iter().any(|&(l, t, r, b)| {
        pos[0] >= l as f32 - margin
            && pos[0] <= r as f32 + margin
            && pos[1] >= t as f32 - margin
            && pos[1] <= b as f32 + margin
    })
}

/// Returns the rect `[x, y, w, h]` of the monitor that contains `pos`, or `None`
/// when `pos` is off every monitor (or on non-Windows). Used by fill-screen mode
/// to fill the monitor the window currently sits on rather than the
/// profile-matching one.
pub fn monitor_rect_at(pos: [f32; 2]) -> Option<[f32; 4]> {
    #[cfg(windows)]
    {
        monitor_containing_point(pos, &win_monitor::list())
            .map(|(l, t, r, b)| [l as f32, t as f32, (r - l) as f32, (b - t) as f32])
    }
    #[cfg(not(windows))]
    {
        let _ = pos;
        None
    }
}

/// Pure: the monitor in `monitors` whose bounds contain `pos`, if any. Strict
/// containment (no overhang margin) so the window's own top-left corner resolves
/// to exactly the screen it sits on. Split out for unit testing.
fn monitor_containing_point(
    pos: [f32; 2],
    monitors: &[(i32, i32, i32, i32)],
) -> Option<(i32, i32, i32, i32)> {
    monitors.iter().copied().find(|&(l, t, r, b)| {
        pos[0] >= l as f32 && pos[0] < r as f32 && pos[1] >= t as f32 && pos[1] < b as f32
    })
}

/// Pure decision for where a pinned dashboard should be placed.
///
/// Returns the saved position only when the dashboard is `pinned`, a position is
/// `saved` for the current profile, and that position is still on a connected
/// monitor; otherwise `None`, so the caller auto-targets the matching monitor.
pub fn resolve_pinned_position(
    pinned: bool,
    saved: Option<[i32; 2]>,
    monitors: &[(i32, i32, i32, i32)],
) -> Option<[f32; 2]> {
    if !pinned {
        return None;
    }
    let p = saved?;
    let pos = [p[0] as f32, p[1] as f32];
    position_on_any_monitor(pos, monitors).then_some(pos)
}

/// Returns `[x, y, w, h]` for the best monitor to host `profile`.
///
/// A monitor is only chosen as a dedicated target when its resolution *matches*
/// the profile (both dimensions within ~10 %), so a small strip/secondary screen
/// is used only when its size actually fits the profile. When no monitor matches,
/// the window goes to the **primary** monitor (top-left at the virtual origin) so
/// e.g. a 1080×253 profile lands on the main screen rather than a 1920×450 strip.
/// Falls back to the first monitor, then `[0, 0, 0, 0]`.
pub fn pick_window_rect_for_profile(profile: &str) -> [f32; 4] {
    #[cfg(windows)]
    {
        let [pw, ph] = profile_to_size(profile);
        let monitors = win_monitor::list();
        if let Some(idx) = select_profile_monitor(&monitors, pw, ph) {
            let (l, t, r, b) = monitors[idx];
            return [l as f32, t as f32, (r - l) as f32, (b - t) as f32];
        }
    }
    #[cfg(not(windows))]
    let _ = profile;
    [0.0, 0.0, 0.0, 0.0]
}

/// Pure monitor-selection logic for [`pick_window_rect_for_profile`], split out so
/// it can be unit-tested without enumerating real monitors.
///
/// Returns the index into `monitors` of the screen that should host a profile of
/// size `pw`×`ph`:
/// 1. A monitor whose resolution *matches* the profile (both dims within ~10 %);
///    among matches, the closest fit. This keeps a small strip/secondary screen
///    reserved for the profile that actually fits it.
/// 2. Otherwise the **primary** monitor (top-left at the virtual origin `0,0`), so
///    e.g. a 1080×253 profile lands on the main screen rather than a 1920×450 strip.
/// 3. Otherwise the first monitor. `None` only when `monitors` is empty.
fn select_profile_monitor(monitors: &[(i32, i32, i32, i32)], pw: f32, ph: f32) -> Option<usize> {
    let dim = |&(l, t, r, b): &(i32, i32, i32, i32)| ((r - l) as f32, (b - t) as f32);

    // 1. A monitor whose resolution matches the profile, closest fit first.
    let tol_w = (pw * 0.10).max(8.0);
    let tol_h = (ph * 0.10).max(8.0);
    let matched = monitors
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            let (w, h) = dim(m);
            (w - pw).abs() <= tol_w && (h - ph).abs() <= tol_h
        })
        .min_by(|(_, a), (_, b)| {
            let (aw, ah) = dim(a);
            let (bw, bh) = dim(b);
            let da = (aw - pw).abs() + (ah - ph).abs();
            let db = (bw - pw).abs() + (bh - ph).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i);
    if matched.is_some() {
        return matched;
    }

    // 2. No dedicated match → the primary monitor (origin at 0,0).
    if let Some(i) = monitors.iter().position(|&(l, t, _, _)| l == 0 && t == 0) {
        return Some(i);
    }

    // 3. Fallback: first monitor.
    if monitors.is_empty() {
        None
    } else {
        Some(0)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        monitor_containing_point, position_on_any_monitor, profile_is_landscape, profile_to_size,
        resolve_pinned_position, select_profile_monitor,
    };

    #[test]
    fn landscape_profiles_detected_by_prefix() {
        assert!(profile_is_landscape("landscape-xl"));
        assert!(profile_is_landscape("landscape-4k-top"));
        assert!(!profile_is_landscape("portrait-xl"));
        assert!(!profile_is_landscape("portrait-4k-side"));
    }

    #[test]
    fn landscape_dims_are_wide() {
        for p in [
            "landscape-xl",
            "landscape-slim",
            "landscape-hd",
            "landscape-wxga",
            "landscape-fhd",
            "landscape-wuxga",
            "landscape-qhd",
            "landscape-hdplus",
            "landscape-1600x900",
            "landscape-1680x1050",
            "landscape-2560x1600",
            "landscape-4k",
            "landscape-fhd-top",
            "landscape-qhd-top",
            "landscape-4k-top",
        ] {
            let [w, h] = profile_to_size(p);
            assert!(w >= h, "{p} should be landscape (w >= h), got {w}x{h}");
        }
    }

    #[test]
    fn landscape_is_transpose_of_portrait() {
        // Each landscape profile is the matching portrait profile with axes swapped.
        let pairs = [
            ("portrait-xl", "landscape-xl"),
            ("portrait-fhd", "landscape-fhd"),
            ("portrait-qhd", "landscape-qhd"),
            ("portrait-4k", "landscape-4k"),
            ("portrait-fhd-side", "landscape-fhd-top"),
            ("portrait-qhd-side", "landscape-qhd-top"),
            ("portrait-4k-side", "landscape-4k-top"),
        ];
        for (portrait, landscape) in pairs {
            let [pw, ph] = profile_to_size(portrait);
            let [lw, lh] = profile_to_size(landscape);
            assert_eq!(
                [pw, ph],
                [lh, lw],
                "{landscape} should transpose {portrait}"
            );
        }
    }

    #[test]
    fn profile_monitor_prefers_matching_strip() {
        // Primary 2560x1440 at origin + a 1920x450 strip at (2560,0).
        let monitors = [(0, 0, 2560, 1440), (2560, 0, 4480, 450)];
        // landscape-xl is 1920x450 → must pick the strip (index 1).
        let [w, h] = profile_to_size("landscape-xl");
        assert_eq!(select_profile_monitor(&monitors, w, h), Some(1));
    }

    #[test]
    fn profile_monitor_falls_back_to_primary_when_no_match() {
        // Same layout; landscape-fhd-top (1080x253) matches neither screen, so it
        // must land on the primary monitor at the origin (index 0), NOT the strip.
        let monitors = [(0, 0, 2560, 1440), (2560, 0, 4480, 450)];
        let [w, h] = profile_to_size("landscape-fhd-top");
        assert_eq!(select_profile_monitor(&monitors, w, h), Some(0));
    }

    #[test]
    fn profile_monitor_empty_is_none() {
        let [w, h] = profile_to_size("landscape-xl");
        assert_eq!(select_profile_monitor(&[], w, h), None);
    }

    #[test]
    fn portrait_side_profile_prefers_matching_strip() {
        // Primary 2560x1440 at origin + a 338x1440 portrait side-strip at (2560,0).
        let monitors = [(0, 0, 2560, 1440), (2560, 0, 2898, 1440)];
        // portrait-qhd-side is 338x1440 → must pick the side-strip (index 1), so a
        // side profile lands on the strip that matches it, not the main screen.
        let [w, h] = profile_to_size("portrait-qhd-side");
        assert_eq!(select_profile_monitor(&monitors, w, h), Some(1));
    }

    #[test]
    fn portrait_profile_falls_back_to_primary_when_no_match() {
        // Same layout; a full portrait-fhd (1080x1920) matches neither screen, so it
        // must land on the primary monitor (index 0), NOT the small side-strip — the
        // old behaviour (any portrait monitor) wrongly sent it to the strip.
        let monitors = [(0, 0, 2560, 1440), (2560, 0, 2898, 1440)];
        let [w, h] = profile_to_size("portrait-fhd");
        assert_eq!(select_profile_monitor(&monitors, w, h), Some(0));
    }

    #[test]
    fn monitor_containing_point_strict_containment() {
        // Primary at origin + a small portrait strip to the left at negative x.
        let monitors = [(0, 0, 2560, 1440), (-450, 0, 0, 1920)];
        // A window on the small left strip resolves to that monitor (index 1).
        assert_eq!(
            monitor_containing_point([-200.0, 300.0], &monitors),
            Some((-450, 0, 0, 1920))
        );
        // A window on the primary resolves to the primary.
        assert_eq!(
            monitor_containing_point([100.0, 100.0], &monitors),
            Some((0, 0, 2560, 1440))
        );
        // Off every monitor → None (no overhang margin, unlike position_on_any_monitor).
        assert_eq!(monitor_containing_point([5000.0, 100.0], &monitors), None);
        // Empty monitor list → None.
        assert_eq!(monitor_containing_point([0.0, 0.0], &[]), None);
    }

    #[test]
    fn position_on_monitor_respects_overhang_margin() {
        let monitors = [(0, 0, 2560, 1440)];
        // Inside the monitor.
        assert!(position_on_any_monitor([100.0, 100.0], &monitors));
        // 40 px past the right edge — within the 60 px overhang margin.
        assert!(position_on_any_monitor([2600.0, 100.0], &monitors));
        // 200 px past the right edge — off-screen.
        assert!(!position_on_any_monitor([2760.0, 100.0], &monitors));
        // No monitors at all → never on-screen.
        assert!(!position_on_any_monitor([0.0, 0.0], &[]));
    }

    #[test]
    fn pinned_position_none_when_unpinned() {
        let monitors = [(0, 0, 2560, 1440)];
        // Even with a valid saved position, an unpinned dashboard auto-targets.
        assert_eq!(
            resolve_pinned_position(false, Some([100, 100]), &monitors),
            None
        );
    }

    #[test]
    fn pinned_position_none_when_no_saved() {
        let monitors = [(0, 0, 2560, 1440)];
        // Pinned but nothing saved for this profile → caller auto-targets.
        assert_eq!(resolve_pinned_position(true, None, &monitors), None);
    }

    #[test]
    fn pinned_position_restores_saved_when_on_screen() {
        let monitors = [(0, 0, 2560, 1440), (2560, 0, 4480, 450)];
        // Saved position sits on the strip → restored verbatim.
        assert_eq!(
            resolve_pinned_position(true, Some([2600, 10]), &monitors),
            Some([2600.0, 10.0])
        );
    }

    #[test]
    fn pinned_position_dropped_when_monitor_gone() {
        // The strip the position was saved on is now disconnected.
        let monitors = [(0, 0, 2560, 1440)];
        // Saved at (3000, 10) — well off the remaining monitor → auto-target.
        assert_eq!(
            resolve_pinned_position(true, Some([3000, 10]), &monitors),
            None
        );
    }
}
