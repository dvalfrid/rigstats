//! The Control Center's Dashboard, Overlay and Alerts pages (#305): the part
//! of a profile that is the app's own look — accent colour, opacity, panels
//! and their order, the overlay's content and the alert thresholds
//! (`settings::ProfileLook`). Everything app-wide (window layer, display
//! profile, overlay placement, notifications, …) lives in Settings.
//!
//! The current settings are the active profile's look, so these pages edit
//! them directly: a change shows at once (and is written to the settings
//! file, so the wallpaper host follows), Save keeps it for the profile,
//! Revert or Close puts back what was there before.

use crate::app_icons::{pick_picture, PictureCache};
use crate::lock_ext::LockSafe;
use crate::overlay::ALL_OVERLAY_METRICS;
use crate::theme::{self, AppTheme, DialogColors};
use crate::we_link::WeAction;
use crate::windows::settings::{self as settings_win, ALL_PANELS};
use crate::windows::ui_kit::{self, Group, C_CRIT, C_WARN};
use crate::windows::{OpenRequest, OpenRequests};
use rigstats_backend::desktop_background::{self, DesktopBackground, Fit, Screen, ScreenPicture};
use rigstats_backend::settings::{self, ProfileLook, Settings};
use rigstats_backend::wallpaper_engine::{self, WeChoice};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The Control Center's window title — the Open dialog's owner.
const CONTROL_TITLE: &str = "RigStats — Control Center";

/// What the pages need to read and save the settings.
pub struct LookLink<'a> {
    pub settings: &'a Arc<Mutex<Settings>>,
    pub dir: &'a Path,
    pub reload: &'a Arc<AtomicBool>,
    pub open: &'a OpenRequests,
    pub battery_present: &'a Arc<AtomicBool>,
    /// The app rules watcher's shared state (#307).
    pub rules: &'a crate::app_rules::RulesHandle,
    /// Wallpaper Engine's state and the Start/Stop buttons' requests (#306).
    pub we: &'a crate::we_link::WeLink,
}

/// Theme keys with their colour and a readable name.
pub fn theme_options() -> Vec<(&'static str, egui::Color32, &'static str)> {
    theme::THEME_KEYS
        .iter()
        .map(|k| (*k, AppTheme::from_key(k).accent, theme_name(k)))
        .collect()
}

pub fn theme_name(key: &str) -> &'static str {
    match key {
        "amber" => "Amber",
        "green" => "Green",
        "purple" => "Purple",
        "slate" => "Slate",
        "red" => "Red",
        "blue" => "Blue",
        _ => "Cyan",
    }
}

/// Threshold rows: key, label, unit, what the value means.
const THRESHOLDS: &[(&str, &str, &str, Option<&str>)] = &[
    ("cpu", "CPU temperature", "°C", None),
    ("gpu", "GPU temperature", "°C", None),
    ("cpu_load", "CPU load", "%", None),
    ("gpu_load", "GPU load", "%", None),
    ("ram", "RAM temperature", "°C", None),
    ("ram_load", "RAM usage", "%", None),
    ("disk", "Disk temperature", "°C", None),
    ("disk_usage", "Disk usage", "%", None),
];
const BATTERY_THRESHOLDS: &[(&str, &str, &str, Option<&str>)] = &[
    (
        "battery",
        "Battery charge",
        "%",
        Some("The laptop and every wireless device — alerts when one drops below"),
    ),
    (
        "battery_power",
        "Battery power draw",
        "W",
        Some("The laptop only, while it runs on battery"),
    ),
];
/// Width of the unit after each row's fields.
const UNIT_W: f32 = 26.0;

/// The look being edited. Outside an edit it follows the settings (so a
/// change from the tray or the hotkey shows); during one it holds the
/// edit, and `before` what to put back.
#[derive(Debug, Default)]
pub struct LookEditor {
    profile_id: Option<String>,
    look: Option<ProfileLook>,
    before: Option<ProfileLook>,
    /// The desktop background group's state.
    background: BackgroundPicker,
}

/// The desktop background group's state (#306): the "Choose a picture"
/// dialog's answer to come (and for which screen — `None` = every
/// screen), previews, the connected screens and the "one per screen" choice.
#[derive(Debug, Default)]
struct BackgroundPicker {
    picking: Option<(Option<String>, Receiver<Option<String>>)>,
    pictures: PictureCache,
    screens: Vec<Screen>,
    screens_at: Option<Instant>,
    /// Windows' own background, for its preview.
    own: Option<DesktopBackground>,
    per_screen: bool,
    /// Wallpaper Engine, when it is installed.
    we: Option<WeView>,
    /// Looked for it and it isn't installed.
    we_missing: bool,
    /// The library being read on a thread of its own.
    we_loading: Option<Receiver<Vec<wallpaper_engine::Wallpaper>>>,
    /// When the Desktop page was last drawn: a gap means it was opened
    /// again, and the library is read again.
    we_page_seen: Option<Instant>,
    /// A wallpaper list was open last frame: opening one reads the library
    /// again, so a new Workshop subscription shows up.
    we_list_open: bool,
}

/// What the Wallpaper Engine group shows: the install, the user's
/// wallpapers and its screen map (whether it runs comes from `we_link`).
#[derive(Debug)]
struct WeView {
    install: wallpaper_engine::Install,
    library: Vec<wallpaper_engine::Wallpaper>,
    config: wallpaper_engine::Config,
}

impl WeView {
    fn wallpaper(&self, file: &str) -> Option<&wallpaper_engine::Wallpaper> {
        self.library
            .iter()
            .find(|w| w.file.eq_ignore_ascii_case(file))
    }
}

impl BackgroundPicker {
    /// The connected screens and Windows' own background, looked up again
    /// every few seconds. Windows' own is the one kept when a profile
    /// replaced it (`kept`), else what Windows shows now.
    fn refresh(&mut self, kept: Option<&DesktopBackground>) {
        if self
            .screens_at
            .is_none_or(|t| t.elapsed() > Duration::from_secs(3))
        {
            self.screens = desktop_background::screens();
            self.own = kept.cloned().or_else(desktop_background::current);
            self.screens_at = Some(Instant::now());
            self.refresh_wallpaper_engine();
        }
    }

    /// Wallpaper Engine's state with the screens; its library less often.
    fn refresh_wallpaper_engine(&mut self) {
        let Some(install) = wallpaper_engine::find() else {
            self.we = None;
            self.we_missing = true;
            return;
        };
        self.we_missing = false;
        // The library itself is read off the UI thread (`load_library`).
        let library = match self.we.take() {
            Some(old) if old.install == install => old.library,
            _ => Vec::new(),
        };
        self.we = Some(WeView {
            config: install.config().unwrap_or_default(),
            library,
            install,
        });
    }

    /// Reads the library on a thread of its own (a large Workshop library
    /// takes a while); `take_library` picks the answer up.
    fn load_library(&mut self, ctx: &egui::Context) {
        let Some(install) = self.we.as_ref().map(|we| we.install.clone()) else {
            return;
        };
        if self.we_loading.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let _ = std::thread::Builder::new()
            .name("we-library".into())
            .spawn(move || {
                // A broken library must not take the thread down unseen.
                let library = std::panic::catch_unwind(|| install.library()).unwrap_or_default();
                let _ = tx.send(library);
                ctx.request_repaint();
            });
        self.we_loading = Some(rx);
    }

    fn take_library(&mut self) {
        let Some(library) = self.we_loading.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        self.we_loading = None;
        if let Some(we) = self.we.as_mut() {
            we.library = library;
        }
    }

    /// The main screen's id, else the first one's.
    fn main_screen(&self) -> Option<&str> {
        self.screens
            .iter()
            .find(|s| s.is_main())
            .or(self.screens.first())
            .map(|s| s.id.as_str())
    }
}

/// Puts the chosen picture `path` on `screen` (`None` = every screen).
/// The first picture goes on every screen, whichever screen it was chosen
/// for: a profile can't name Windows' own picture for the others.
fn choose_picture(
    bg: &mut Option<DesktopBackground>,
    screen: Option<&str>,
    path: String,
    screens: &[String],
) {
    match (bg.as_mut(), screen) {
        (Some(b), Some(screen)) => b.set_screen(screen, path, screens),
        (Some(b), None) => {
            b.path = path;
            b.screens.clear();
        }
        (None, _) => {
            *bg = Some(DesktopBackground {
                path,
                fit: Fit::Fill,
                screens: Vec::new(),
            });
        }
    }
}

fn persist(link: &LookLink, s: &Settings) {
    if let Err(e) = settings::persist_settings(link.dir, s) {
        rigstats_backend::debug::log_error(link.dir, &format!("settings: persist failed — {e}"));
    }
    link.reload.store(true, Ordering::Relaxed);
}

impl LookEditor {
    /// Unsaved changes to the look.
    pub fn dirty(&self) -> bool {
        self.before.is_some() && self.before != self.look
    }

    /// Runs every frame, whichever page is shown. Outside an edit the look
    /// follows the settings. A profile switch ends an edit: the app has
    /// already stored the edited look in the outgoing profile and loaded
    /// the new one's, so the outgoing profile gets its look from before the
    /// edit back.
    pub fn sync(&mut self, active_id: Option<&str>, link: &LookLink) {
        if self.profile_id.as_deref() != active_id {
            if let (Some(before), Some(old)) = (self.before.take(), self.profile_id.take()) {
                let mut s = link.settings.lock_safe();
                s.profile_looks.insert(old, before);
                persist(link, &s);
            }
            self.profile_id = active_id.map(str::to_owned);
            self.look = None;
        }
        if self.before.is_none() {
            self.look = Some(ProfileLook::capture(&link.settings.lock_safe()));
        }
    }

    /// Shows the edit: writes it to the app when it differs from the app.
    fn preview(&mut self, link: &LookLink) {
        let Some(look) = &self.look else {
            return;
        };
        let mut s = link.settings.lock_safe();
        let current = ProfileLook::capture(&s);
        if current == *look {
            return;
        }
        self.before.get_or_insert(current);
        look.apply_to(&mut s);
        persist(link, &s);
    }

    /// Keeps the edit for the active profile.
    pub fn save(&mut self, link: &LookLink) {
        if self.before.take().is_some() {
            let mut s = link.settings.lock_safe();
            settings::store_active_look(&mut s);
            persist(link, &s);
        }
    }

    /// Ends the edit without keeping it (Revert, Close).
    pub fn discard(&mut self, link: &LookLink) {
        if let Some(before) = self.before.take() {
            let mut s = link.settings.lock_safe();
            before.apply_to(&mut s);
            persist(link, &s);
        }
        self.look = None;
    }

    /// The look as the overview shows it.
    pub fn current(&self) -> Option<&ProfileLook> {
        self.look.as_ref()
    }
}

fn open_settings_row(
    g: &mut Group<'_>,
    dc: &DialogColors,
    link: &LookLink,
    title: &str,
    subtitle: &str,
    page: settings_win::Page,
) {
    g.row(title, Some(subtitle), |ui| {
        if theme::dialog_btn_secondary(ui, "Open Settings", dc).clicked() {
            *link.open.lock_safe() = Some(OpenRequest::Settings(page));
        }
    });
}

pub fn dashboard_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    profile: &str,
    editor: &mut LookEditor,
    link: &LookLink,
) {
    ui_kit::page_header(
        ui,
        dc,
        "Dashboard",
        &format!("How the dashboard looks in {profile}."),
    );
    let Some(look) = editor.look.as_mut() else {
        return;
    };
    ui_kit::group(ui, dc, Some("Appearance"), None, |g| {
        g.row("Accent colour", Some(theme_name(&look.theme)), |ui| {
            ui_kit::swatches(ui, dc, &mut look.theme, &theme_options());
        });
        g.row("Opacity", Some("How see-through the panels are"), |ui| {
            ui_kit::slider_pct(ui, dc, &mut look.opacity, 0.1..=1.0);
        });
        open_settings_row(
            g,
            dc,
            link,
            "Window and screen",
            "Display profile, window layer and full screen apply to every profile",
            settings_win::Page::Display,
        );
    });
    let battery = link.battery_present.load(Ordering::Relaxed);
    let items: Vec<(&str, String)> = ALL_PANELS
        .iter()
        .map(|(k, l)| (*k, l.to_string()))
        .collect();
    ui_kit::group(
        ui,
        dc,
        Some("Panels"),
        Some("Shown panels appear top to bottom in this order. Use the arrows to move them."),
        |g| {
            ui_kit::ordered_rows(g, &mut look.visible_panels, &items, |k| {
                k == "battery" && !battery && cfg!(not(debug_assertions))
            });
        },
    );
    editor.preview(link);
}

/// The Desktop page (#306): the profile's desktop background.
pub fn desktop_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    profile: &str,
    editor: &mut LookEditor,
    link: &LookLink,
) {
    ui_kit::page_header(
        ui,
        dc,
        "Desktop",
        &format!("The desktop background in {profile}."),
    );
    let Some(look) = editor.look.as_mut() else {
        return;
    };
    let kept = link.settings.lock_safe().desktop_background_before.clone();
    editor.background.refresh(kept.as_ref());
    desktop_background_group(ui, dc, look, &mut editor.background);
    // The library: read when the page is opened (again).
    let picker = &mut editor.background;
    let reopened = picker
        .we_page_seen
        .is_none_or(|t| t.elapsed() > Duration::from_secs(1));
    picker.we_page_seen = Some(Instant::now());
    if reopened {
        picker.load_library(ui.ctx());
    }
    picker.take_library();
    if editor.background.we.is_some() {
        wallpaper_engine_group(ui, dc, look, &mut editor.background, link.we, link.open);
    } else if editor.background.we_missing {
        wallpaper_engine_missing(ui, dc);
    }
    editor.preview(link);
}

/// What a profile does with Wallpaper Engine, as the segmented control
/// offers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WeMode {
    Leave,
    Show,
    Pause,
    Off,
}

/// The Wallpaper Engine group (#306): leave it alone, show a wallpaper per
/// screen, or pause it.
/// Wallpaper Engine isn't installed: what it would add, and where to get it.
fn wallpaper_engine_missing(ui: &mut egui::Ui, dc: &DialogColors) {
    ui_kit::group(ui, dc, Some("Wallpaper Engine"), None, |g| {
        g.row(
            "Not installed",
            Some("Wallpaper Engine adds live wallpapers — video, animated scenes — that each profile can switch, or pause while you game"),
            |ui| {
                if theme::dialog_btn_secondary(ui, "Get it on Steam", dc).clicked() {
                    // explorer hands steam:// to Steam and https:// to the browser.
                    let _ = std::process::Command::new("explorer")
                        .arg(wallpaper_engine::store_link())
                        .spawn();
                }
            },
        );
    });
}

/// The state row's words, colour and button for what `we_link` reports.
fn we_state(
    state: &crate::we_link::WeShared,
) -> (
    &'static str,
    egui::Color32,
    Option<(&'static str, WeAction)>,
) {
    use crate::we_link::WeAction;
    match (state.running, state.stopped, state.starting) {
        (false, _, true) => ("Starting…", ui_kit::C_ATTENTION, None),
        (false, _, false) => (
            "Not running",
            ui_kit::C_ATTENTION,
            Some(("Start", WeAction::Start)),
        ),
        (true, true, _) => (
            "Stopped",
            ui_kit::C_ATTENTION,
            Some(("Start", WeAction::Resume)),
        ),
        (true, false, _) => ("Running", ui_kit::C_GOOD, Some(("Stop", WeAction::Stop))),
    }
}

fn wallpaper_engine_group(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    look: &mut ProfileLook,
    picker: &mut BackgroundPicker,
    we_link: &crate::we_link::WeLink,
    open: &OpenRequests,
) {
    let ctx = ui.ctx().clone();
    let state = {
        let s = we_link.lock_safe();
        (we_state(&s), s.running, s.last_error.clone())
    };
    let ((state_text, state_colour, button), running, last_error) = state;
    let Some(we) = picker.we.as_ref() else {
        return;
    };
    let mut mode = match &look.wallpaper_engine {
        None => WeMode::Leave,
        Some(WeChoice::Show { .. }) => WeMode::Show,
        Some(WeChoice::Pause) => WeMode::Pause,
        Some(WeChoice::Off) => WeMode::Off,
    };
    let screens = &picker.screens;
    let pictures = &mut picker.pictures;
    let mut list_open = false;
    let loading = picker.we_loading.is_some();
    ui_kit::group(
        ui,
        dc,
        Some("Wallpaper Engine"),
        Some("While it shows a wallpaper, Wallpaper Engine covers the Windows background. Paused freezes the wallpaper and frees the graphics card — handy for gaming. Off shows the Windows background in this profile; the others start it again."),
        |g| {
            let state_sub = match button {
                Some((_, WeAction::Stop)) => {
                    "Stop shows the Windows background; it stays in the tray, and Start brings the wallpapers back"
                }
                Some((_, WeAction::Resume)) => "The Windows background shows until you start it again",
                _ if running => "",
                _ => "Start it here or from the Start menu",
            };
            g.row("Wallpaper Engine", Some(state_sub), |ui| {
                if let Some((label, action)) = button {
                    if theme::dialog_btn_secondary(ui, label, dc).clicked() {
                        we_link.lock_safe().requests.push(action);
                    }
                    ui.add_space(8.0);
                }
                ui_kit::status(ui, state_colour, state_text);
            });
            // The last failed command, the same way as every problem (#373),
            // with the log a click away.
            if let Some(err) = &last_error {
                let problem = crate::problems::wallpaper_engine_failed(err);
                g.block(|ui| {
                    ui_kit::row_problem(ui, dc, problem.severity, &format!("{}: {err}", problem.title));
                    ui.horizontal(|ui| {
                        for action in &problem.actions {
                            if theme::dialog_btn_secondary(ui, &action.label, dc).clicked() {
                                crate::windows::run_problem_action(ui.ctx(), action, open);
                            }
                        }
                    });
                });
            }
            let sub = if running {
                "What Wallpaper Engine does while this profile is active"
            } else {
                "Applies as soon as Wallpaper Engine runs"
            };
            g.row("In this profile", Some(sub), |ui| {
                ui_kit::segmented(
                    ui,
                    dc,
                    &mut mode,
                    &[
                        (WeMode::Leave, "As it is"),
                        (WeMode::Show, "Wallpapers"),
                        (WeMode::Pause, "Paused"),
                        (WeMode::Off, "Off"),
                    ],
                );
            });
            // A new choice starts from what Wallpaper Engine shows now.
            look.wallpaper_engine = match (mode, look.wallpaper_engine.take()) {
                (WeMode::Leave, _) => None,
                (WeMode::Pause, _) => Some(WeChoice::Pause),
                (WeMode::Off, _) => Some(WeChoice::Off),
                (WeMode::Show, Some(show @ WeChoice::Show { .. })) => Some(show),
                (WeMode::Show, _) => Some(WeChoice::Show {
                    screens: screens
                        .iter()
                        .filter_map(|s| {
                            let n = we.config.monitor_of(&s.id)?;
                            let slot = we.config.selected.iter().find(|w| w.monitor == n)?;
                            Some(ScreenPicture {
                                screen: s.id.clone(),
                                path: slot.file.clone(),
                            })
                        })
                        .collect(),
                }),
            };
            let Some(WeChoice::Show { screens: chosen }) = look.wallpaper_engine.as_mut() else {
                return;
            };
            for (i, screen) in screens.iter().enumerate() {
                let known = we.config.monitor_of(&screen.id).is_some();
                let current = chosen
                    .iter()
                    .find(|c| c.screen == screen.id)
                    .map(|c| c.path.clone());
                let wallpaper = current.as_deref().and_then(|f| we.wallpaper(f));
                let preview = wallpaper
                    .and_then(|w| w.preview.as_deref())
                    .and_then(|p| pictures.get(&ctx, p));
                let title = if screen.is_main() {
                    format!("Screen {} (main)", i + 1)
                } else {
                    format!("Screen {}", i + 1)
                };
                let sub = match (known, wallpaper, &current) {
                    (false, _, _) => "Wallpaper Engine doesn't know this screen yet".to_owned(),
                    (true, Some(w), _) => w.title.clone(),
                    (true, None, Some(f)) if !std::path::Path::new(f).is_file() => {
                        "No longer in your library — choose another".to_owned()
                    }
                    (true, None, Some(f)) => file_name(f),
                    (true, None, None) => "Left as it is".to_owned(),
                };
                let mut pick: Option<String> = None;
                g.row_picture(preview.as_ref(), &title, Some(&sub), |ui| {
                    if !known {
                        return;
                    }
                    let shown = wallpaper.map_or("Choose…", |w| w.title.as_str());
                    ui_kit::combo(ui, ("we_screen", i), ui_kit::CONTROL_W, shown, |ui| {
                        list_open = true;
                        if we.library.is_empty() {
                            let text = if loading { "Reading your library…" } else { "No wallpapers found" };
                            ui.label(egui::RichText::new(text).color(dc.muted));
                        }
                        for w in &we.library {
                            let on = current.as_deref().is_some_and(|f| f.eq_ignore_ascii_case(&w.file));
                            let load = || w.preview.as_deref().and_then(|p| pictures.get_small(&ctx, p));
                            if ui_kit::picture_choice(ui, dc, on, &w.title, load) {
                                pick = Some(w.file.clone());
                                ui.close();
                            }
                        }
                    });
                });
                if let Some(file) = pick {
                    match chosen.iter_mut().find(|c| c.screen == screen.id) {
                        Some(c) => c.path = file,
                        None => chosen.push(ScreenPicture {
                            screen: screen.id.clone(),
                            path: file,
                        }),
                    }
                }
            }
        },
    );
    // Opening a list reads the library again (new subscriptions show up).
    if list_open && !picker.we_list_open {
        picker.load_library(&ctx);
    }
    picker.we_list_open = list_open;
}

/// What the Overview's Desktop tile says.
pub fn desktop_summary(bg: Option<&DesktopBackground>, we: Option<&WeChoice>) -> String {
    let picture = match bg {
        None => "Windows' own background".to_owned(),
        Some(b) if b.screens.is_empty() => file_name(&b.path),
        Some(b) => format!("One per screen · {} pictures", b.screens.len()),
    };
    match we {
        None => picture,
        Some(WeChoice::Pause) => format!("{picture} · Wallpaper Engine paused"),
        Some(WeChoice::Off) => format!("{picture} · Wallpaper Engine off"),
        Some(WeChoice::Show { .. }) => "Wallpaper Engine".to_owned(),
    }
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned())
}

/// The profile's desktop background (#306): a picture — the same on every
/// screen or one per screen — and how it fits, or Windows' own background.
fn desktop_background_group(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    look: &mut ProfileLook,
    picker: &mut BackgroundPicker,
) {
    let ctx = ui.ctx().clone();
    let ids: Vec<String> = picker.screens.iter().map(|s| s.id.clone()).collect();
    // A picture chosen in the Open dialog.
    let answer = picker
        .picking
        .as_ref()
        .and_then(|(_, rx)| rx.try_recv().ok());
    if let Some(answer) = answer {
        let screen = picker.picking.take().and_then(|(screen, _)| screen);
        if let Some(path) = answer {
            choose_picture(&mut look.desktop_background, screen.as_deref(), path, &ids);
        }
    }
    let several = picker.screens.len() > 1;
    let per_screen = several
        && look
            .desktop_background
            .as_ref()
            .is_some_and(|b| picker.per_screen || !b.screens.is_empty());
    let waiting = picker.picking.is_some();
    let choose = |ui: &mut egui::Ui| {
        ui.add_enabled_ui(!waiting, |ui| {
            let label = if waiting { "Choosing…" } else { "Choose…" };
            theme::dialog_btn_secondary(ui, label, dc).clicked()
        })
        .inner
    };
    // Which screen a Choose… was clicked for (`Some(None)` = every screen).
    let mut chosen_for: Option<Option<String>> = None;
    let mut own = false;
    let own_preview = picker.own.clone().map(|own| {
        let path = picker
            .main_screen()
            .map_or(own.path.as_str(), |m| own.path_for(m))
            .to_owned();
        let sub = if own.screens.is_empty() {
            file_name(&path)
        } else {
            "One picture per screen".to_owned()
        };
        (picker.pictures.get(&ctx, &path), sub)
    });
    let screens = &picker.screens;
    let pictures = &mut picker.pictures;
    let mut each = per_screen;
    ui_kit::group(
        ui,
        dc,
        Some("Background"),
        Some("Without a picture, the profile shows Windows' own background."),
        |g| {
            let Some(bg) = look.desktop_background.as_mut() else {
                // Windows' own, as it looks on the main screen.
                let own = own_preview.as_ref().map(|(_, sub)| sub.as_str());
                let tex = own_preview.as_ref().and_then(|(t, _)| t.as_ref());
                let sub = own.unwrap_or("Pick a picture to show in this profile");
                g.row_picture(tex, "Windows' own background", Some(sub), |ui| {
                    if choose(ui) {
                        chosen_for = Some(None);
                    }
                });
                return;
            };
            if several {
                g.row(
                    "Screens",
                    Some("Each screen can show a picture of its own"),
                    |ui| {
                        if ui_kit::segmented(
                            ui,
                            dc,
                            &mut each,
                            &[(false, "Same picture"), (true, "One per screen")],
                        ) && !each
                        {
                            // The main screen's picture goes everywhere.
                            let main = screens.iter().find(|s| s.is_main()).or(screens.first());
                            if let Some(main) = main {
                                bg.path = bg.path_for(&main.id).to_owned();
                            }
                            bg.screens.clear();
                        }
                    },
                );
            }
            if per_screen {
                for (i, screen) in screens.iter().enumerate() {
                    let path = bg.path_for(&screen.id).to_owned();
                    let preview = pictures.get(&ctx, &path);
                    let (w, h) = screen.size();
                    let title = if screen.is_main() {
                        format!("Screen {} (main)", i + 1)
                    } else {
                        format!("Screen {}", i + 1)
                    };
                    let sub = format!("{w} × {h} · {}", file_name(&path));
                    g.row_picture(preview.as_ref(), &title, Some(&sub), |ui| {
                        if choose(ui) {
                            chosen_for = Some(Some(screen.id.clone()));
                        }
                    });
                }
            } else {
                let preview = pictures.get(&ctx, &bg.path);
                let sub = file_name(&bg.path);
                g.row_picture(preview.as_ref(), "Picture", Some(&sub), |ui| {
                    if choose(ui) {
                        chosen_for = Some(None);
                    }
                });
            }
            let fit_sub = if several {
                "How the pictures fill the screens — Windows uses one setting for all"
            } else {
                "How the picture fills the screen"
            };
            g.row("Fit", Some(fit_sub), |ui| {
                ui_kit::dropdown(ui, "desktop_fit", bg.fit.label(), |ui| {
                    for fit in Fit::ALL {
                        ui.selectable_value(&mut bg.fit, fit, fit.label());
                    }
                });
            });
            g.row(
                "Windows' own background",
                Some("Show it in this profile instead"),
                |ui| {
                    own = theme::dialog_btn_secondary(ui, "Use Windows' own", dc).clicked();
                },
            );
        },
    );
    picker.per_screen = each;
    if own {
        look.desktop_background = None;
        picker.per_screen = false;
    }
    if let Some(screen) = chosen_for {
        picker.picking = Some((screen, pick_picture(CONTROL_TITLE, ctx.clone())));
    }
}
/// What an overlay metric is, in words — the overlay itself shows short
/// labels ("CPU", "HOTSPOT") with a unit.
fn metric_name(key: &str) -> Option<&'static str> {
    Some(match key {
        "cpu_load" => "CPU load",
        "cpu_temp" => "CPU temperature",
        "cpu_freq" => "CPU clock speed",
        "cpu_power" => "CPU power draw",
        "gpu_load" => "GPU load",
        "gpu_temp" => "GPU temperature",
        "gpu_hotspot" => "GPU hotspot temperature",
        "gpu_mem_temp" => "Video memory temperature",
        "gpu_core_clock" => "GPU clock speed",
        "gpu_mem_clock" => "Video memory clock speed",
        "gpu_vram_used" => "Video memory used",
        "gpu_vram_pct" => "Video memory used (%)",
        "gpu_power" => "GPU power draw",
        "gpu_fan" => "GPU fan speed",
        "ram_used" => "RAM used",
        "ram_pct" => "RAM used (%)",
        "net_up" => "Upload speed",
        "net_down" => "Download speed",
        "net_ping" => "Ping",
        "disk_read" => "Disk read speed",
        "disk_write" => "Disk write speed",
        "battery_pct" => "Battery charge",
        _ => return None,
    })
}

fn columns_label(n: u8) -> String {
    match n {
        0 => "All on one row".to_owned(),
        1 => "One column".to_owned(),
        n => format!("{n} columns"),
    }
}

pub fn overlay_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    profile: &str,
    editor: &mut LookEditor,
    link: &LookLink,
) {
    ui_kit::page_header(
        ui,
        dc,
        "Overlay",
        &format!("The metric strip shown over games in {profile}."),
    );
    let Some(look) = editor.look.as_mut() else {
        return;
    };
    ui_kit::group(ui, dc, None, None, |g| {
        g.row(
            "Show overlay",
            Some("Ctrl+Alt+O shows or hides it at any time"),
            |ui| {
                ui_kit::toggle(ui, dc, &mut look.overlay_enabled);
            },
        );
        g.row("Layout", None, |ui| {
            ui_kit::dropdown(
                ui,
                "look_overlay_columns",
                columns_label(look.overlay_columns),
                |ui| {
                    for n in 0..=6u8 {
                        ui.selectable_value(&mut look.overlay_columns, n, columns_label(n));
                    }
                },
            );
        });
        open_settings_row(
            g,
            dc,
            link,
            "Position and size",
            "Where the overlay sits, its size and click-through apply to every profile",
            settings_win::Page::Overlay,
        );
    });
    let battery = link.battery_present.load(Ordering::Relaxed);
    let items: Vec<(&str, String)> = ALL_OVERLAY_METRICS
        .iter()
        .map(|m| {
            let name = metric_name(m.key)
                .map_or_else(|| format!("{} ({})", m.label, m.unit.trim()), str::to_owned);
            (m.key, name)
        })
        .collect();
    ui_kit::group(
        ui,
        dc,
        Some("Metrics"),
        Some("Shown metrics appear in this order."),
        |g| {
            ui_kit::ordered_rows(g, &mut look.overlay_metrics, &items, |k| {
                k == "battery_pct" && !battery && cfg!(not(debug_assertions))
            });
        },
    );
    editor.preview(link);
}

pub fn alerts_page(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    profile: &str,
    editor: &mut LookEditor,
    link: &LookLink,
) {
    ui_kit::page_header(
        ui,
        dc,
        "Alerts",
        &format!("When RIGStats notifies you in {profile}."),
    );
    let Some(look) = editor.look.as_mut() else {
        return;
    };
    let defaults = settings::default_thresholds();
    let header = |g: &mut Group<'_>, title: &str| {
        g.row(title, None, |ui| {
            ui.add_space(UNIT_W);
            ui.add_sized(
                [48.0, 20.0],
                egui::Label::new(
                    egui::RichText::new("Critical")
                        .size(11.0)
                        .strong()
                        .color(C_CRIT),
                ),
            );
            ui.add_sized(
                [48.0, 20.0],
                egui::Label::new(
                    egui::RichText::new("Warning")
                        .size(11.0)
                        .strong()
                        .color(C_WARN),
                ),
            );
        });
    };
    let mut rows = |g: &mut Group<'_>, list: &[(&str, &str, &str, Option<&str>)]| {
        for &(key, label, unit, note) in list {
            let e = look
                .thresholds
                .entry(key.to_string())
                .or_insert_with(|| defaults.get(key).cloned().unwrap_or_default());
            g.row(label, note, |ui| {
                ui.add_sized(
                    [UNIT_W, 20.0],
                    egui::Label::new(egui::RichText::new(unit).size(12.0).color(dc.muted)),
                );
                ui_kit::threshold_field(ui, &mut e.crit);
                ui_kit::threshold_field(ui, &mut e.warn);
            });
        }
    };
    ui_kit::group(ui, dc, None, None, |g| {
        open_settings_row(
            g,
            dc,
            link,
            "Notifications",
            "Turning alerts on or off and how often they repeat apply to every profile",
            settings_win::Page::Notifications,
        );
    });
    ui_kit::group(
        ui,
        dc,
        Some("Thresholds"),
        Some("Leave a field empty for no alert at that level."),
        |g| {
            header(g, "Alert when above");
            rows(g, THRESHOLDS);
        },
    );
    ui_kit::group(
        ui,
        dc,
        Some("Battery"),
        Some("Each wireless device (headset, mouse, keyboard — see Peripherals) alerts on its own, and not while it is charging."),
        |g| {
            header(g, "Alert at");
            rows(g, BATTERY_THRESHOLDS);
        },
    );
    editor.preview(link);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rigstats_backend::settings::switch_profile_look;

    /// A settings handle and folder for one test, removed afterwards.
    struct Env {
        settings: Arc<Mutex<Settings>>,
        dir: std::path::PathBuf,
        reload: Arc<AtomicBool>,
        open: OpenRequests,
        battery: Arc<AtomicBool>,
        rules: crate::app_rules::RulesHandle,
        we: crate::we_link::WeLink,
    }

    impl Env {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("rigstats-look-test-{name}-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            let mut s = Settings::default();
            s.theme = "amber".into();
            switch_profile_look(&mut s, "balanced");
            Self {
                settings: Arc::new(Mutex::new(s)),
                dir,
                reload: Arc::new(AtomicBool::new(false)),
                open: OpenRequests::default(),
                battery: Arc::new(AtomicBool::new(false)),
                rules: crate::app_rules::RulesHandle::default(),
                we: crate::we_link::WeLink::default(),
            }
        }

        fn link(&self) -> LookLink<'_> {
            LookLink {
                settings: &self.settings,
                dir: &self.dir,
                reload: &self.reload,
                open: &self.open,
                battery_present: &self.battery,
                rules: &self.rules,
                we: &self.we,
            }
        }

        fn theme(&self) -> String {
            self.settings.lock_safe().theme.clone()
        }
    }

    impl Drop for Env {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn edit(editor: &mut LookEditor, theme: &str) {
        editor.look.as_mut().expect("synced").theme = theme.into();
    }

    #[test]
    fn an_edit_shows_at_once_and_revert_puts_it_back() {
        let env = Env::new("revert");
        let mut editor = LookEditor::default();
        editor.sync(Some("balanced"), &env.link());
        edit(&mut editor, "red");
        editor.preview(&env.link());
        assert_eq!(env.theme(), "red", "previewed live");
        assert!(env.reload.load(Ordering::Relaxed));
        assert!(editor.dirty());
        editor.discard(&env.link());
        assert_eq!(env.theme(), "amber");
        assert!(!editor.dirty());
    }

    #[test]
    fn save_keeps_the_edit_in_the_active_profile_and_on_disk() {
        let env = Env::new("save");
        let mut editor = LookEditor::default();
        editor.sync(Some("balanced"), &env.link());
        edit(&mut editor, "red");
        editor.preview(&env.link());
        editor.save(&env.link());
        assert!(!editor.dirty());
        assert_eq!(
            env.settings.lock_safe().profile_looks["balanced"].theme,
            "red"
        );
        let on_disk = settings::load_settings(&env.dir);
        assert_eq!(on_disk.theme, "red");
        assert_eq!(on_disk.profile_looks["balanced"].theme, "red");
    }

    #[test]
    fn outside_an_edit_the_page_follows_changes_made_elsewhere() {
        let env = Env::new("follow");
        let mut editor = LookEditor::default();
        editor.sync(Some("balanced"), &env.link());
        // The tray or Ctrl+Alt+O changes the overlay while the page is open.
        env.settings.lock_safe().overlay_enabled = true;
        editor.sync(Some("balanced"), &env.link());
        assert!(editor.current().unwrap().overlay_enabled);
        assert!(!editor.dirty());
    }

    #[test]
    fn a_profile_switch_mid_edit_gives_the_outgoing_profile_its_look_back() {
        let env = Env::new("switch");
        let mut editor = LookEditor::default();
        editor.sync(Some("balanced"), &env.link());
        edit(&mut editor, "red");
        editor.preview(&env.link());
        // The app switches profile: it stores the previewed look in Balanced.
        switch_profile_look(&mut env.settings.lock_safe(), "gaming");
        editor.sync(Some("gaming"), &env.link());
        // Balanced keeps what it had before the unsaved edit.
        assert_eq!(
            env.settings.lock_safe().profile_looks["balanced"].theme,
            "amber"
        );
        assert!(!editor.dirty());
    }

    #[test]
    fn the_state_row_offers_the_button_that_fits() {
        use crate::we_link::WeShared;
        let state = |running, stopped, starting| WeShared {
            running,
            stopped,
            starting,
            ..Default::default()
        };
        let button = |s: &WeShared| we_state(s).2.map(|(label, action)| (label, action));
        assert_eq!(
            button(&state(false, false, false)),
            Some(("Start", WeAction::Start))
        );
        assert_eq!(button(&state(false, false, true)), None, "starting: wait");
        assert_eq!(
            button(&state(true, false, false)),
            Some(("Stop", WeAction::Stop))
        );
        assert_eq!(
            button(&state(true, true, false)),
            Some(("Start", WeAction::Resume))
        );
        assert_eq!(we_state(&state(true, false, false)).0, "Running");
        assert_eq!(we_state(&state(true, true, false)).0, "Stopped");
    }

    #[test]
    fn the_desktop_tile_names_the_picture_or_counts_them() {
        assert_eq!(desktop_summary(None, None), "Windows' own background");
        assert_eq!(
            desktop_summary(None, Some(&WeChoice::Pause)),
            "Windows' own background · Wallpaper Engine paused"
        );
        assert_eq!(
            desktop_summary(None, Some(&WeChoice::Off)),
            "Windows' own background · Wallpaper Engine off"
        );
        assert_eq!(
            desktop_summary(
                None,
                Some(&WeChoice::Show {
                    screens: Vec::new()
                })
            ),
            "Wallpaper Engine"
        );
        let ids = vec!["left".to_owned(), "right".to_owned()];
        let mut bg = None;
        choose_picture(&mut bg, None, r"C:\pics\calm.jpg".into(), &ids);
        assert_eq!(desktop_summary(bg.as_ref(), None), "calm.jpg");
        choose_picture(&mut bg, Some("right"), "b.jpg".into(), &ids);
        assert_eq!(
            desktop_summary(bg.as_ref(), None),
            "One per screen · 2 pictures"
        );
    }

    #[test]
    fn the_first_picture_goes_on_every_screen_then_one_screen_gets_its_own() {
        let ids = vec!["left".to_owned(), "right".to_owned()];
        let mut bg = None;
        choose_picture(&mut bg, Some("right"), "a.jpg".into(), &ids);
        let b = bg.clone().unwrap();
        assert_eq!(
            (b.path_for("left"), b.path_for("right")),
            ("a.jpg", "a.jpg")
        );
        choose_picture(&mut bg, Some("right"), "b.jpg".into(), &ids);
        let b = bg.clone().unwrap();
        assert_eq!(
            (b.path_for("left"), b.path_for("right")),
            ("a.jpg", "b.jpg")
        );
        // A picture for every screen replaces the per-screen ones.
        choose_picture(&mut bg, None, "c.jpg".into(), &ids);
        let b = bg.unwrap();
        assert!(b.screens.is_empty());
        assert_eq!(b.path, "c.jpg");
    }
}
