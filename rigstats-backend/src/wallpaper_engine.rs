//! Wallpaper Engine per profile (#306, step 2): a profile can show chosen
//! Wallpaper Engine wallpapers — one per screen — or pause Wallpaper Engine
//! (to free the graphics card while gaming), or leave it alone.
//!
//! RIGStats doesn't draw live wallpapers itself; it drives Wallpaper Engine
//! through its documented command line
//! (<https://help.wallpaperengine.io/en/functionality/cli.html>):
//! `wallpaper64.exe -control openWallpaper -file <project.json> -monitor N`,
//! `-control pause` / `play`. Commands go to the running instance, so
//! nothing is sent while Wallpaper Engine isn't running.
//!
//! Screens: Wallpaper Engine numbers them itself; its `config.json` keeps a
//! `monitormap` from Windows' monitor id (the same id
//! `desktop_background::screens` reports, with `/` for `\`) to that number.
//!
//! Like the Windows background, Wallpaper Engine's own wallpapers are kept
//! the first time a profile changes them (`Settings::wallpaper_engine_before`)
//! and put back — and Wallpaper Engine resumed — in a profile that leaves it
//! alone.
//!
//! `#![allow(unsafe_code)]`: the process list (`CreateToolhelp32Snapshot`)
//! has no safe wrapper.
#![allow(unsafe_code)]

use crate::desktop_background::ScreenPicture;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What a profile does with Wallpaper Engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum WeChoice {
    /// Pause every wallpaper (frees the graphics card; the paused frame
    /// stays on the desktop).
    Pause,
    /// Stop its wallpapers: the Windows background (the profile's own
    /// picture, if it has one) shows, and Wallpaper Engine waits in the tray
    /// for a profile that uses it.
    Off,
    /// Show these wallpapers: `path` is a wallpaper's `project.json`, per
    /// Windows monitor id.
    Show {
        #[serde(default)]
        screens: Vec<ScreenPicture>,
    },
}

/// One of Wallpaper Engine's screens and the wallpaper on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeSlot {
    pub monitor: u32,
    pub file: String,
}

/// A command for Wallpaper Engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeCommand {
    Pause,
    Play,
    /// Stops every wallpaper: the Windows background shows, Wallpaper
    /// Engine keeps running in the tray, and `Play` brings them back.
    Stop,
    Open {
        monitor: u32,
        file: String,
    },
}

impl WeCommand {
    /// The arguments after `wallpaper64.exe`.
    pub fn args(&self) -> Vec<String> {
        let mut a = vec!["-control".to_owned()];
        match self {
            WeCommand::Pause => a.push("pause".into()),
            WeCommand::Play => a.push("play".into()),
            WeCommand::Stop => a.push("stop".into()),
            WeCommand::Open { monitor, file } => a.extend([
                "openWallpaper".into(),
                "-file".into(),
                file.clone(),
                "-monitor".into(),
                monitor.to_string(),
            ]),
        }
        a
    }
}

/// What to do when the active profile's choice is `wanted`.
#[derive(Debug, PartialEq, Eq)]
pub struct WeStep {
    pub commands: Vec<WeCommand>,
    /// Wallpaper Engine's own wallpapers from now on.
    pub before: Option<Vec<WeSlot>>,
}

/// A profile that pauses or shows wallpapers first keeps Wallpaper Engine's
/// own wallpapers (read through `current`) if they aren't kept already; a
/// profile that leaves it alone resumes it and puts its own back, if a
/// profile changed them. `monitor_of` maps a Windows monitor id to Wallpaper
/// Engine's number (screens it doesn't know are skipped).
pub fn step(
    wanted: Option<&WeChoice>,
    before: Option<&[WeSlot]>,
    current: impl FnOnce() -> Option<Vec<WeSlot>>,
    monitor_of: impl Fn(&str) -> Option<u32>,
) -> WeStep {
    let keep = |before: Option<&[WeSlot]>| before.map(<[WeSlot]>::to_vec).or_else(current);
    match wanted {
        Some(WeChoice::Pause) => WeStep {
            commands: vec![WeCommand::Pause],
            before: keep(before),
        },
        Some(WeChoice::Off) => WeStep {
            commands: vec![WeCommand::Stop],
            before: keep(before),
        },
        Some(WeChoice::Show { screens }) => {
            let mut commands = vec![WeCommand::Play];
            commands.extend(screens.iter().filter_map(|s| {
                Some(WeCommand::Open {
                    monitor: monitor_of(&s.screen)?,
                    file: s.path.clone(),
                })
            }));
            WeStep {
                commands,
                before: keep(before),
            }
        }
        None => match before {
            Some(own) => {
                let mut commands = vec![WeCommand::Play];
                commands.extend(own.iter().map(|s| WeCommand::Open {
                    monitor: s.monitor,
                    file: s.file.clone(),
                }));
                WeStep {
                    commands,
                    before: None,
                }
            }
            None => WeStep {
                commands: Vec::new(),
                before: None,
            },
        },
    }
}

/// Whether Wallpaper Engine's wallpapers play, are paused (frozen on the
/// desktop) or stopped (the Windows background shows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playback {
    Playing,
    Paused,
    Stopped,
}

impl Playback {
    /// What `command` leaves it in.
    pub fn after(self, command: &WeCommand) -> Self {
        match command {
            WeCommand::Play => Playback::Playing,
            WeCommand::Pause => Playback::Paused,
            WeCommand::Stop => Playback::Stopped,
            WeCommand::Open { .. } => self,
        }
    }
}

/// Fits the commands to what Wallpaper Engine does now, going through them
/// in order: drops what would change nothing, so a screen whose wallpaper
/// stays isn't reloaded (it would flash) — an `Open` for what `shown`
/// already has on that screen (Wallpaper Engine writes its `config.json` on
/// every change, so that is current) and a `Play`/`Pause`/`Stop` it is
/// already in — and plays stopped wallpapers before pausing them, as a
/// paused frame needs one to show. `state` is `None` when not known (just
/// after Wallpaper Engine started).
pub fn prune(
    commands: Vec<WeCommand>,
    shown: &[WeSlot],
    state: Option<Playback>,
) -> Vec<WeCommand> {
    let mut state = state;
    let mut out = Vec::new();
    for c in commands {
        let keep = match &c {
            WeCommand::Play => state != Some(Playback::Playing),
            WeCommand::Pause => {
                if state == Some(Playback::Stopped) {
                    out.push(WeCommand::Play);
                }
                state != Some(Playback::Paused)
            }
            WeCommand::Stop => state != Some(Playback::Stopped),
            WeCommand::Open { monitor, file } => !shown
                .iter()
                .any(|s| s.monitor == *monitor && s.file.eq_ignore_ascii_case(file)),
        };
        state = Some(state.unwrap_or(Playback::Playing).after(&c));
        if keep {
            out.push(c);
        }
    }
    out
}

/// A wallpaper in the user's Wallpaper Engine library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wallpaper {
    /// Its `project.json` — what `openWallpaper -file` takes.
    pub file: String,
    pub title: String,
    /// "scene", "video", "web", "application" (lower case; may be empty).
    pub kind: String,
    /// Its preview picture (jpg or gif), if it has one.
    pub preview: Option<String>,
}

#[derive(Deserialize)]
struct Project {
    #[serde(default)]
    title: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    preview: String,
}

/// A wallpaper from its folder and the text of its `project.json`.
pub fn parse_project(folder: &Path, json: &str) -> Option<Wallpaper> {
    let p: Project = serde_json::from_str(json).ok()?;
    let title = p.title.trim();
    let title = if title.is_empty() {
        folder.file_name()?.to_string_lossy().into_owned()
    } else {
        title.to_owned()
    };
    let preview = (!p.preview.is_empty())
        .then(|| folder.join(&p.preview))
        .filter(|f| f.is_file())
        .map(|f| f.to_string_lossy().into_owned());
    Some(Wallpaper {
        file: folder.join("project.json").to_string_lossy().into_owned(),
        title,
        kind: p.kind.to_ascii_lowercase(),
        preview,
    })
}

/// What RIGStats reads from Wallpaper Engine's `config.json`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Config {
    /// Windows monitor id (normalized, see [`normalize_id`]) → its number.
    pub monitors: Vec<(String, u32)>,
    /// The wallpapers shown now, per number.
    pub selected: Vec<WeSlot>,
}

impl Config {
    /// Wallpaper Engine's number for the screen with Windows monitor id `id`.
    pub fn monitor_of(&self, id: &str) -> Option<u32> {
        let id = normalize_id(id);
        self.monitors
            .iter()
            .find(|(m, _)| *m == id)
            .map(|(_, n)| *n)
    }

    /// The Windows monitor id with number `n`, as it was written in the map.
    pub fn screen_of(&self, n: u32) -> Option<&str> {
        self.monitors
            .iter()
            .find(|(_, m)| *m == n)
            .map(|(id, _)| id.as_str())
    }
}

/// Monitor ids compared without regard to `/` vs `\` or case:
/// Wallpaper Engine writes `//?/DISPLAY#…`, Windows reports `\\?\DISPLAY#…`.
pub fn normalize_id(id: &str) -> String {
    id.replace('\\', "/").to_ascii_lowercase()
}

/// Reads `config.json`'s text: the first user section with a monitor map.
/// The file holds one section per Windows user name, plus `?installdirectory`.
pub fn parse_config(json: &str) -> Config {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(json) else {
        return Config::default();
    };
    let Some(general) = root
        .as_object()
        .into_iter()
        .flat_map(|o| o.values())
        .filter_map(|user| user.get("general"))
        .find(|g| g.pointer("/user/monitormap").is_some())
    else {
        return Config::default();
    };
    let monitors = general
        .pointer("/user/monitormap")
        .and_then(|m| m.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(id, v)| Some((normalize_id(id), v.get("location")?.as_u64()? as u32)))
                .collect()
        })
        .unwrap_or_default();
    let selected = general
        .pointer("/wallpaperconfig/selectedwallpapers")
        .and_then(|s| s.as_object())
        .map(|s| {
            let mut v: Vec<WeSlot> = s
                .iter()
                .filter_map(|(key, w)| {
                    let monitor = key.strip_prefix("Monitor")?.parse().ok()?;
                    let file = w.get("file")?.as_str()?.replace('/', "\\");
                    Some(WeSlot {
                        monitor,
                        file: project_of(&file),
                    })
                })
                .collect();
            v.sort_by_key(|s| s.monitor);
            v
        })
        .unwrap_or_default();
    Config { monitors, selected }
}

/// `openWallpaper` takes a `project.json`; the config can name the scene
/// file (`scene.pkg`) instead, so use the project next to it when there is
/// one.
fn project_of(file: &str) -> String {
    let path = Path::new(file);
    match path.parent().map(|d| d.join("project.json")) {
        Some(p) if path.file_name().is_some_and(|n| n != "project.json") && p.is_file() => {
            p.to_string_lossy().into_owned()
        }
        _ => file.to_owned(),
    }
}

/// Where Wallpaper Engine is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    /// `wallpaper64.exe` (or `wallpaper32.exe`).
    pub exe: PathBuf,
}

impl Install {
    fn dir(&self) -> &Path {
        self.exe.parent().unwrap_or(Path::new("."))
    }

    /// Its settings: the screen map and the wallpapers shown now.
    pub fn config(&self) -> Option<Config> {
        let text = std::fs::read_to_string(self.dir().join("config.json")).ok()?;
        Some(parse_config(&text))
    }

    /// The user's wallpapers: their Steam Workshop subscriptions (in the
    /// Steam library Wallpaper Engine is installed in) and their own
    /// projects, sorted by title.
    pub fn library(&self) -> Vec<Wallpaper> {
        let dir = self.dir();
        // <library>\steamapps\common\wallpaper_engine → <library>\steamapps
        let workshop = dir
            .parent()
            .and_then(Path::parent)
            .map(|steamapps| steamapps.join(r"workshop\content\431960"));
        let roots = workshop
            .into_iter()
            .chain([dir.join(r"projects\myprojects")]);
        let mut list: Vec<Wallpaper> = roots
            .filter_map(|root| std::fs::read_dir(root).ok())
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let folder = entry.path();
                let json = std::fs::read_to_string(folder.join("project.json")).ok()?;
                parse_project(&folder, &json)
            })
            .collect();
        list.sort_by_key(|w| w.title.to_lowercase());
        list
    }

    /// Starts Wallpaper Engine (it opens in the tray with its own
    /// wallpapers). Doesn't wait for it.
    pub fn launch(&self) -> Result<(), String> {
        std::process::Command::new(&self.exe)
            .current_dir(self.dir())
            .spawn()
            .map(drop)
            .map_err(|e| e.to_string())
    }

    /// Sends `command` to the running Wallpaper Engine. Waits up to a few
    /// seconds for it to be handed over.
    pub fn send(&self, command: &WeCommand) -> Result<(), String> {
        let mut cmd = std::process::Command::new(&self.exe);
        cmd.args(command.args());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        for _ in 0..50 {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => return Err(format!("exited with {status}")),
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("no answer within 5 s".into())
    }
}

/// Where to get Wallpaper Engine: its page in the Steam client when Steam
/// is installed, else the store page in the browser.
pub fn store_link() -> &'static str {
    if steam_installed() {
        "steam://store/431960"
    } else {
        "https://store.steampowered.com/app/431960/Wallpaper_Engine/"
    }
}

#[cfg(windows)]
fn steam_installed() -> bool {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Valve\Steam")
        .and_then(|k| k.get_value::<String, _>("SteamExe"))
        .is_ok_and(|exe| Path::new(&exe).is_file())
}

#[cfg(not(windows))]
fn steam_installed() -> bool {
    false
}

/// What the diagnostics ZIP says about Wallpaper Engine: found or not,
/// running, its screen map and how many wallpapers it has — no titles.
pub fn diagnostics() -> String {
    let Some(install) = find() else {
        return "Wallpaper Engine: not installed (no HKCU\\Software\\WallpaperEngine\\installPath)\n"
            .to_owned();
    };
    let mut out = format!(
        "Wallpaper Engine: {}\nrunning: {}\n",
        install.exe.display(),
        is_running()
    );
    match install.config() {
        Some(c) => {
            out += &format!(
                "config.json: {} screens in its monitor map\n",
                c.monitors.len()
            );
            for (id, n) in &c.monitors {
                out += &format!("  {n} = {id}\n");
            }
            out += &format!("showing: {} screens\n", c.selected.len());
        }
        None => out += "config.json: couldn't be read\n",
    }
    out += &format!("library: {} wallpapers\n", install.library().len());
    out
}

/// Wallpaper Engine's install, from the path it registers for itself.
#[cfg(windows)]
pub fn find() -> Option<Install> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let path: String = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\WallpaperEngine")
        .ok()?
        .get_value("installPath")
        .ok()?;
    let exe = PathBuf::from(path);
    exe.is_file().then_some(Install { exe })
}

#[cfg(not(windows))]
pub fn find() -> Option<Install> {
    None
}

/// Whether Wallpaper Engine is running.
#[cfg(windows)]
pub fn is_running() -> bool {
    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
    use winapi::um::tlhelp32::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    // SAFETY: a process snapshot, walked with a correctly sized entry and
    // closed before returning.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut ok = Process32FirstW(snap, &mut entry) != 0;
        while ok && !found {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase();
            found = name == "wallpaper64.exe" || name == "wallpaper32.exe";
            ok = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
        found
    }
}

#[cfg(not(windows))]
pub fn is_running() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pic(screen: &str, path: &str) -> ScreenPicture {
        ScreenPicture {
            screen: screen.into(),
            path: path.into(),
        }
    }

    fn slot(monitor: u32, file: &str) -> WeSlot {
        WeSlot {
            monitor,
            file: file.into(),
        }
    }

    fn map(id: &str) -> Option<u32> {
        match id {
            "main" => Some(0),
            "left" => Some(1),
            _ => None,
        }
    }

    #[test]
    fn pausing_keeps_its_own_wallpapers_the_first_time() {
        let s = step(
            Some(&WeChoice::Pause),
            None,
            || Some(vec![slot(0, "own.json")]),
            map,
        );
        assert_eq!(s.commands, vec![WeCommand::Pause]);
        assert_eq!(s.before, Some(vec![slot(0, "own.json")]));
    }

    #[test]
    fn showing_plays_and_opens_each_known_screen() {
        let choice = WeChoice::Show {
            screens: vec![
                pic("main", "a.json"),
                pic("left", "b.json"),
                pic("gone", "c.json"),
            ],
        };
        let kept = [slot(0, "own.json")];
        let s = step(Some(&choice), Some(&kept), || panic!("kept already"), map);
        assert_eq!(
            s.commands,
            vec![
                WeCommand::Play,
                WeCommand::Open {
                    monitor: 0,
                    file: "a.json".into()
                },
                WeCommand::Open {
                    monitor: 1,
                    file: "b.json".into()
                },
            ]
        );
        assert_eq!(s.before, Some(kept.to_vec()));
    }

    #[test]
    fn leaving_it_alone_resumes_and_puts_its_own_back() {
        let kept = [slot(0, "own.json"), slot(2, "other.json")];
        let s = step(None, Some(&kept), || None, map);
        assert_eq!(
            s.commands,
            vec![
                WeCommand::Play,
                WeCommand::Open {
                    monitor: 0,
                    file: "own.json".into()
                },
                WeCommand::Open {
                    monitor: 2,
                    file: "other.json".into()
                },
            ]
        );
        assert_eq!(s.before, None);
    }

    #[test]
    fn leaving_it_alone_when_nothing_changed_sends_nothing() {
        let s = step(None, None, || panic!("not read"), map);
        assert!(s.commands.is_empty());
        assert_eq!(s.before, None);
    }

    #[test]
    fn commands_that_change_nothing_are_dropped() {
        let open = |m: u32, f: &str| WeCommand::Open {
            monitor: m,
            file: f.into(),
        };
        let shown = [slot(0, r"D:\A\project.json"), slot(1, "b.json")];
        let cmds = vec![
            WeCommand::Play,
            open(0, r"d:\a\PROJECT.json"),
            open(1, "c.json"),
        ];
        // Playing already, screen 0 shows A already: only screen 1 changes.
        assert_eq!(
            prune(cmds.clone(), &shown, Some(Playback::Playing)),
            vec![open(1, "c.json")]
        );
        // Not known whether it plays: Play is sent.
        assert_eq!(
            prune(cmds, &shown, None),
            vec![WeCommand::Play, open(1, "c.json")]
        );
        assert!(prune(vec![WeCommand::Pause], &[], Some(Playback::Paused)).is_empty());
        // Stopping wallpapers that play: sent.
        assert_eq!(
            prune(vec![WeCommand::Stop], &[], Some(Playback::Playing)),
            vec![WeCommand::Stop]
        );
        assert_eq!(
            prune(vec![WeCommand::Pause], &[], Some(Playback::Playing)),
            vec![WeCommand::Pause]
        );
        assert!(prune(vec![WeCommand::Stop], &[], Some(Playback::Stopped)).is_empty());
    }

    #[test]
    fn off_stops_and_keeps_its_own_wallpapers() {
        let s = step(
            Some(&WeChoice::Off),
            None,
            || Some(vec![slot(0, "own.json")]),
            map,
        );
        assert_eq!(s.commands, vec![WeCommand::Stop]);
        assert_eq!(s.before, Some(vec![slot(0, "own.json")]));
        let json = serde_json::to_string(&WeChoice::Off).unwrap();
        assert_eq!(json, r#"{"mode":"off"}"#);
    }

    #[test]
    fn from_off_to_paused_plays_first_so_a_frame_shows() {
        assert_eq!(
            prune(vec![WeCommand::Pause], &[], Some(Playback::Stopped)),
            vec![WeCommand::Play, WeCommand::Pause]
        );
    }

    #[test]
    fn the_state_follows_each_command_in_turn() {
        // Not known, then Stop, then Play: both sent; a second Play isn't.
        assert_eq!(
            prune(
                vec![WeCommand::Stop, WeCommand::Play, WeCommand::Play],
                &[],
                None
            ),
            vec![WeCommand::Stop, WeCommand::Play]
        );
        assert_eq!(Playback::Paused.after(&WeCommand::Stop), Playback::Stopped);
    }

    #[test]
    fn commands_are_the_documented_arguments() {
        assert_eq!(WeCommand::Pause.args(), ["-control", "pause"]);
        assert_eq!(WeCommand::Play.args(), ["-control", "play"]);
        assert_eq!(WeCommand::Stop.args(), ["-control", "stop"]);
        assert_eq!(
            WeCommand::Open {
                monitor: 2,
                file: r"D:\w\project.json".into()
            }
            .args(),
            [
                "-control",
                "openWallpaper",
                "-file",
                r"D:\w\project.json",
                "-monitor",
                "2"
            ]
        );
    }

    #[test]
    fn a_choice_is_stored_with_its_mode() {
        let show = WeChoice::Show {
            screens: vec![pic("main", "a.json")],
        };
        let json = serde_json::to_string(&show).unwrap();
        assert_eq!(
            json,
            r#"{"mode":"show","screens":[{"screen":"main","path":"a.json"}]}"#
        );
        assert_eq!(serde_json::from_str::<WeChoice>(&json).unwrap(), show);
        assert_eq!(
            serde_json::from_str::<WeChoice>(r#"{"mode":"pause"}"#).unwrap(),
            WeChoice::Pause
        );
    }

    /// The shape of the owner's config.json (other keys trimmed).
    const CONFIG: &str = r#"{
        "?installdirectory": "D:/SteamGames/steamapps/common/wallpaper_engine",
        "danie": {
            "general": {
                "user": {
                    "monitormap": {
                        "//?/DISPLAY#AUS27F2#7&1d43af03&0&UID256#{e6f07b5f}": { "location": 0 },
                        "//?/DISPLAY#AUS27F2#7&1d43af03&0&UID260#{e6f07b5f}": { "location": 1 },
                        "//?/DISPLAY#BOE0599#5&1ad3dd84&0&UID516#{e6f07b5f}": { "location": 2 }
                    }
                },
                "wallpaperconfig": {
                    "selectedwallpapers": {
                        "Monitor1": { "file": "D:/no/such/2229371846/scene.pkg" },
                        "Monitor0": { "file": "D:/no/such/2229371846/scene.pkg" },
                        "Monitor2": { "file": "D:/no/such/3102345696/project.json" }
                    }
                }
            }
        }
    }"#;

    #[test]
    fn the_config_maps_windows_monitor_ids_to_its_numbers() {
        let c = parse_config(CONFIG);
        assert_eq!(
            c.monitor_of(r"\\?\DISPLAY#AUS27F2#7&1d43af03&0&UID260#{e6f07b5f}"),
            Some(1),
            "Windows' backslashes match its forward slashes"
        );
        assert_eq!(
            c.monitor_of(r"\\?\DISPLAY#BOE0599#5&1ad3dd84&0&UID516#{e6f07b5f}"),
            Some(2)
        );
        assert_eq!(c.monitor_of(r"\\?\DISPLAY#OTHER"), None);
        assert!(c.screen_of(0).is_some_and(|id| id.contains("uid256")));
    }

    #[test]
    fn the_config_lists_the_wallpapers_shown_in_screen_order() {
        let c = parse_config(CONFIG);
        let monitors: Vec<u32> = c.selected.iter().map(|s| s.monitor).collect();
        assert_eq!(monitors, [0, 1, 2]);
        // No project.json next to the missing scene.pkg: kept as it is.
        assert_eq!(c.selected[0].file, r"D:\no\such\2229371846\scene.pkg");
        assert_eq!(c.selected[2].file, r"D:\no\such\3102345696\project.json");
    }

    #[test]
    fn the_store_link_is_steam_or_the_web() {
        let link = store_link();
        assert!(
            link == "steam://store/431960"
                || link.starts_with("https://store.steampowered.com/app/431960"),
            "{link}"
        );
    }

    #[test]
    fn diagnostics_always_say_something() {
        assert!(diagnostics().starts_with("Wallpaper Engine: "));
    }

    #[test]
    fn a_broken_config_gives_nothing() {
        assert_eq!(parse_config("not json"), Config::default());
        assert_eq!(parse_config(r#"{"x":{}}"#), Config::default());
    }

    #[test]
    fn a_project_gives_its_title_kind_and_preview() {
        let dir = std::env::temp_dir().join("rigstats-we-project-test");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("preview.gif"), b"GIF89a").unwrap();
        let w = parse_project(
            &dir,
            r#"{"title":" Space Storm [4K] ","type":"Scene","preview":"preview.gif","file":"scene.json"}"#,
        )
        .unwrap();
        assert_eq!(w.title, "Space Storm [4K]");
        assert_eq!(w.kind, "scene");
        assert!(w
            .preview
            .as_deref()
            .is_some_and(|p| p.ends_with("preview.gif")));
        assert!(w.file.ends_with("project.json"));
        // No title: the folder's name; a missing preview file: none.
        let w = parse_project(&dir, r#"{"preview":"missing.jpg"}"#).unwrap();
        assert_eq!(w.title, "rigstats-we-project-test");
        assert_eq!(w.preview, None);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(parse_project(&dir, "{"), None);
    }
}
