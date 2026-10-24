//! The global shortcuts' chords (#569): which keys each action uses, or
//! none, as stored in `Settings.shortcuts`. Registering them with Windows is
//! `src-egui/src/hotkey.rs`; this module is the pure part — names, parsing,
//! the rules a new chord must pass and the defaults (the chords before #569,
//! so a settings file without the field changes nothing).
//!
//! A chord is stored as its name (`"Ctrl+Alt+O"`), off as `null`; the
//! profile shortcuts are one modifier set for F1…F9 (`"Ctrl+Alt"`). A value
//! this version can't read falls back to that action's default rather than
//! failing the whole settings file.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// The modifier keys of a chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Mods {
    pub const CTRL_ALT: Mods = Mods {
        ctrl: true,
        alt: true,
        shift: false,
    };
    pub const CTRL_SHIFT: Mods = Mods {
        ctrl: true,
        alt: false,
        shift: true,
    };
    pub const ALT_SHIFT: Mods = Mods {
        ctrl: false,
        alt: true,
        shift: true,
    };
    pub const CTRL_ALT_SHIFT: Mods = Mods {
        ctrl: true,
        alt: true,
        shift: true,
    };

    fn count(self) -> usize {
        usize::from(self.ctrl) + usize::from(self.alt) + usize::from(self.shift)
    }

    /// Ctrl and Alt together are AltGr on many keyboard layouts.
    pub fn is_alt_gr(self) -> bool {
        self.ctrl && self.alt
    }

    /// Parses `"Ctrl+Alt"` (any order and case); `None` for anything else.
    pub fn parse(text: &str) -> Option<Mods> {
        let mut mods = Mods::default();
        for part in text.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" => mods.ctrl = true,
                "alt" => mods.alt = true,
                "shift" => mods.shift = true,
                _ => return None,
            }
        }
        (mods.count() > 0).then_some(mods)
    }
}

impl fmt::Display for Mods {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = [
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
        ];
        let text: Vec<&str> = names
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, name)| *name)
            .collect();
        f.write_str(&text.join("+"))
    }
}

/// Windows' virtual-key code of F1; F1…F24 follow in order.
pub const VK_F1: u32 = 0x70;
const VK_F24: u32 = 0x87;

/// How many profiles get a shortcut of their own (F1…F9).
pub const PROFILE_SHORTCUTS: usize = 9;

/// The keys a chord can end in besides letters, digits and F-keys: Windows'
/// virtual-key code and the name shown.
const OTHER_KEYS: &[(u32, &str)] = &[
    (0x20, "Space"),
    (0x21, "Page Up"),
    (0x22, "Page Down"),
    (0x23, "End"),
    (0x24, "Home"),
    (0x25, "Left"),
    (0x26, "Up"),
    (0x27, "Right"),
    (0x28, "Down"),
    (0x2D, "Insert"),
    (0x2E, "Delete"),
];

/// A key's name, or `None` for a key a chord can't end in.
pub fn key_name(vk: u32) -> Option<String> {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk).map(String::from),
        VK_F1..=VK_F24 => Some(format!("F{}", vk - VK_F1 + 1)),
        _ => OTHER_KEYS
            .iter()
            .find(|(code, _)| *code == vk)
            .map(|(_, name)| (*name).to_owned()),
    }
}

fn key_from_name(name: &str) -> Option<u32> {
    let compact: String = name.chars().filter(|c| !c.is_whitespace()).collect();
    let upper = compact.to_ascii_uppercase();
    let mut chars = upper.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_uppercase() || c.is_ascii_digit() {
            return Some(c as u32);
        }
    }
    if let Some(n) = upper.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(VK_F1 + n - 1);
        }
    }
    OTHER_KEYS
        .iter()
        .find(|(_, key)| key.replace(' ', "").eq_ignore_ascii_case(&compact))
        .map(|(code, _)| *code)
}

/// One chord: modifiers and a key (a Windows virtual-key code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub mods: Mods,
    pub vk: u32,
}

impl Shortcut {
    pub const fn new(mods: Mods, vk: u32) -> Self {
        Self { mods, vk }
    }

    /// Parses `"Ctrl+Alt+O"`: modifiers in any order and case, then a key
    /// [`key_name`] knows. `None` for anything else.
    pub fn parse(text: &str) -> Option<Shortcut> {
        let (mods, key) = text.trim().rsplit_once('+')?;
        Some(Shortcut {
            mods: Mods::parse(mods)?,
            vk: key_from_name(key)?,
        })
    }

    /// The profile number (1…9) this chord switches to when profiles use
    /// `mods` for F1…F9.
    fn profile_number(self, mods: Option<Mods>) -> Option<usize> {
        let index = self.vk.checked_sub(VK_F1)? as usize;
        (mods == Some(self.mods) && index < PROFILE_SHORTCUTS).then_some(index + 1)
    }
}

impl fmt::Display for Shortcut {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let key = key_name(self.vk).unwrap_or_else(|| format!("0x{:02X}", self.vk));
        write!(f, "{}+{key}", self.mods)
    }
}

impl Serialize for Shortcut {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Overlay,
    NextProfile,
    /// F1…F9: the profile at that place in the profile bar.
    Profiles,
    Microphone,
    /// The tray menu's actions, off unless chosen (#594).
    DeskLamp,
    ControlCenter,
    Settings,
    Status,
    Recording,
    OverlayLock,
    Floating,
}

/// Every action with one chord of its own (all but [`Action::Profiles`]).
pub const SINGLE_ACTIONS: [Action; 10] = [
    Action::Overlay,
    Action::NextProfile,
    Action::Microphone,
    Action::DeskLamp,
    Action::ControlCenter,
    Action::Settings,
    Action::Status,
    Action::Recording,
    Action::OverlayLock,
    Action::Floating,
];

impl Action {
    /// What it does, in the words Settings and Status use.
    pub fn purpose(self) -> &'static str {
        match self {
            Action::Overlay => "shows or hides the overlay",
            Action::NextProfile => "switches to the next profile",
            Action::Profiles => "switches to a profile",
            Action::Microphone => "mutes the microphone",
            Action::DeskLamp => "turns the desk lamp on or off",
            Action::ControlCenter => "opens the Control Center",
            Action::Settings => "opens Settings",
            Action::Status => "opens Status",
            Action::Recording => "starts or stops recording",
            Action::OverlayLock => "locks or unlocks the overlay",
            Action::Floating => "turns floating panels on or off",
        }
    }
}

/// Why a new chord can't be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    /// Fewer than two of Ctrl, Alt and Shift: it would take a key or a
    /// shortcut that programs use (Ctrl+C, Alt+F4) from every program.
    TooFewModifiers,
    /// Another action already uses it.
    InUse(Action),
}

/// Every global shortcut: a chord per action, `None` = off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shortcuts {
    pub overlay: Option<Shortcut>,
    pub next_profile: Option<Shortcut>,
    /// The modifiers for F1…F9.
    #[serde(serialize_with = "serialize_mods")]
    pub profiles: Option<Mods>,
    pub microphone: Option<Shortcut>,
    pub desk_lamp: Option<Shortcut>,
    pub control_center: Option<Shortcut>,
    pub settings: Option<Shortcut>,
    pub status: Option<Shortcut>,
    pub recording: Option<Shortcut>,
    pub overlay_lock: Option<Shortcut>,
    pub floating: Option<Shortcut>,
}

fn serialize_mods<S: Serializer>(mods: &Option<Mods>, serializer: S) -> Result<S::Ok, S::Error> {
    match mods {
        Some(mods) => serializer.serialize_str(&mods.to_string()),
        None => serializer.serialize_none(),
    }
}

impl Default for Shortcuts {
    /// The chords from before #569: Ctrl+Alt+O, P, F1…F9 and M. The tray
    /// actions added in #594 are off: a chord of ours could take one another
    /// program uses, and these are wanted by fewer people.
    fn default() -> Self {
        Self {
            overlay: Some(Shortcut::new(Mods::CTRL_ALT, 0x4F)),
            next_profile: Some(Shortcut::new(Mods::CTRL_ALT, 0x50)),
            profiles: Some(Mods::CTRL_ALT),
            microphone: Some(Shortcut::new(Mods::CTRL_ALT, 0x4D)),
            desk_lamp: None,
            control_center: None,
            settings: None,
            status: None,
            recording: None,
            overlay_lock: None,
            floating: None,
        }
    }
}

impl<'de> Deserialize<'de> for Shortcuts {
    /// Lenient: a missing or unreadable value is that action's default,
    /// `null` is off.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let defaults = Shortcuts::default();
        let field = |name: &str| value.get(name);
        let chord = |name: &str, default: Option<Shortcut>| match field(name) {
            None => default,
            Some(serde_json::Value::Null) => None,
            Some(v) => v.as_str().and_then(Shortcut::parse).or(default),
        };
        Ok(Shortcuts {
            overlay: chord("overlay", defaults.overlay),
            next_profile: chord("nextProfile", defaults.next_profile),
            profiles: match field("profiles") {
                None => defaults.profiles,
                Some(serde_json::Value::Null) => None,
                Some(v) => v.as_str().and_then(Mods::parse).or(defaults.profiles),
            },
            microphone: chord("microphone", defaults.microphone),
            desk_lamp: chord("deskLamp", defaults.desk_lamp),
            control_center: chord("controlCenter", defaults.control_center),
            settings: chord("settings", defaults.settings),
            status: chord("status", defaults.status),
            recording: chord("recording", defaults.recording),
            overlay_lock: chord("overlayLock", defaults.overlay_lock),
            floating: chord("floating", defaults.floating),
        })
    }
}

impl Shortcuts {
    /// The action's chord (the first profile's for [`Action::Profiles`]).
    pub fn get(&self, action: Action) -> Option<Shortcut> {
        match action {
            Action::Overlay => self.overlay,
            Action::NextProfile => self.next_profile,
            Action::Profiles => self.profiles.map(|mods| Shortcut::new(mods, VK_F1)),
            Action::Microphone => self.microphone,
            Action::DeskLamp => self.desk_lamp,
            Action::ControlCenter => self.control_center,
            Action::Settings => self.settings,
            Action::Status => self.status,
            Action::Recording => self.recording,
            Action::OverlayLock => self.overlay_lock,
            Action::Floating => self.floating,
        }
    }

    /// Sets a single chord's action; for [`Action::Profiles`] only the
    /// modifiers count.
    pub fn set(&mut self, action: Action, chord: Option<Shortcut>) {
        match action {
            Action::Overlay => self.overlay = chord,
            Action::NextProfile => self.next_profile = chord,
            Action::Profiles => self.profiles = chord.map(|c| c.mods),
            Action::Microphone => self.microphone = chord,
            Action::DeskLamp => self.desk_lamp = chord,
            Action::ControlCenter => self.control_center = chord,
            Action::Settings => self.settings = chord,
            Action::Status => self.status = chord,
            Action::Recording => self.recording = chord,
            Action::OverlayLock => self.overlay_lock = chord,
            Action::Floating => self.floating = chord,
        }
    }

    /// The chord that switches to the profile at `index` (0-based) in the
    /// profile bar; `None` past the ninth or when they are off.
    pub fn profile(&self, index: usize) -> Option<Shortcut> {
        let mods = self.profiles?;
        (index < PROFILE_SHORTCUTS).then(|| Shortcut::new(mods, VK_F1 + index as u32))
    }

    /// How the profile shortcuts read: `"Ctrl+Alt+F1…F9"`.
    pub fn profiles_label(mods: Mods) -> String {
        format!("{mods}+F1…F{PROFILE_SHORTCUTS}")
    }

    /// The action other than `except` that uses `chord`.
    fn user_of(&self, chord: Shortcut, except: Action) -> Option<Action> {
        SINGLE_ACTIONS
            .into_iter()
            .filter(|&a| a != except)
            .find(|&a| self.get(a) == Some(chord))
            .or_else(|| {
                (except != Action::Profiles && chord.profile_number(self.profiles).is_some())
                    .then_some(Action::Profiles)
            })
    }

    /// Why `chord` can't be `action`'s, or `None` when it can. Whether
    /// AltGr types a character with it depends on the keyboard layouts, so
    /// Windows answers that (`hotkey::alt_gr_char`).
    pub fn check(&self, action: Action, chord: Shortcut) -> Option<Rejected> {
        if chord.mods.count() < 2 {
            return Some(Rejected::TooFewModifiers);
        }
        self.user_of(chord, action).map(Rejected::InUse)
    }

    /// Why profiles can't use `mods` for F1…F9: another action uses one of
    /// those chords.
    pub fn check_profiles(&self, mods: Mods) -> Option<Rejected> {
        if mods.count() < 2 {
            return Some(Rejected::TooFewModifiers);
        }
        SINGLE_ACTIONS
            .into_iter()
            .find(|&a| {
                self.get(a)
                    .is_some_and(|c| c.profile_number(Some(mods)).is_some())
            })
            .map(Rejected::InUse)
    }
}

/// The modifier sets offered for the profile shortcuts. With F-keys even
/// Ctrl+Alt is safe: no keyboard layout types a character with AltGr+F1.
pub const PROFILE_MODS: [Mods; 4] = [
    Mods::CTRL_ALT,
    Mods::CTRL_SHIFT,
    Mods::ALT_SHIFT,
    Mods::CTRL_ALT_SHIFT,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chord_reads_back_as_it_was_written() {
        for text in [
            "Ctrl+Alt+O",
            "Ctrl+Shift+F12",
            "Alt+Shift+Page Up",
            "Ctrl+Alt+Shift+7",
            "Ctrl+Alt+Delete",
        ] {
            let chord = Shortcut::parse(text).unwrap_or_else(|| panic!("{text}"));
            assert_eq!(chord.to_string(), text);
        }
        // Order and case don't matter when reading; the name is canonical.
        assert_eq!(
            Shortcut::parse("shift+ctrl+o").unwrap().to_string(),
            "Ctrl+Shift+O"
        );
        assert_eq!(
            Shortcut::parse("Ctrl+Alt+pageup").unwrap().to_string(),
            "Ctrl+Alt+Page Up"
        );
        assert_eq!(Shortcut::parse("Ctrl+Alt+F1").unwrap().vk, VK_F1);
    }

    #[test]
    fn what_is_not_a_chord_is_refused() {
        for text in [
            "",
            "O",
            "Ctrl+",
            "Ctrl+Alt+F25",
            "Win+O",
            "Ctrl+Alt+Ö",
            "Ctrl+Alt+Esc",
        ] {
            assert_eq!(Shortcut::parse(text), None, "{text}");
        }
    }

    #[test]
    fn the_defaults_are_the_chords_from_before() {
        let s = Shortcuts::default();
        assert_eq!(s.overlay.unwrap().to_string(), "Ctrl+Alt+O");
        assert_eq!(s.next_profile.unwrap().to_string(), "Ctrl+Alt+P");
        assert_eq!(s.microphone.unwrap().to_string(), "Ctrl+Alt+M");
        assert_eq!(s.profile(0).unwrap().to_string(), "Ctrl+Alt+F1");
        assert_eq!(s.profile(8).unwrap().to_string(), "Ctrl+Alt+F9");
        assert_eq!(s.profile(9), None);
        assert_eq!(Shortcuts::profiles_label(Mods::CTRL_ALT), "Ctrl+Alt+F1…F9");
    }

    #[test]
    fn shortcuts_round_trip_through_json_and_null_is_off() {
        let mut s = Shortcuts::default();
        s.microphone = None;
        s.overlay = Shortcut::parse("Ctrl+Shift+F10");
        s.profiles = Some(Mods::ALT_SHIFT);
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(
            json,
            r#"{"overlay":"Ctrl+Shift+F10","nextProfile":"Ctrl+Alt+P","profiles":"Alt+Shift","microphone":null,"deskLamp":null,"controlCenter":null,"settings":null,"status":null,"recording":null,"overlayLock":null,"floating":null}"#
        );
        assert_eq!(serde_json::from_str::<Shortcuts>(&json).unwrap(), s);
    }

    #[test]
    fn a_missing_or_unreadable_value_is_that_actions_default() {
        let s: Shortcuts =
            serde_json::from_str(r#"{"overlay":"Ctrl+Alt+Nope","profiles":42,"microphone":null}"#)
                .unwrap();
        let d = Shortcuts::default();
        assert_eq!(s.overlay, d.overlay);
        assert_eq!(s.next_profile, d.next_profile);
        assert_eq!(s.profiles, d.profiles);
        assert_eq!(s.microphone, None);
    }

    #[test]
    fn a_chord_needs_two_modifiers_and_must_be_free() {
        let s = Shortcuts::default();
        let chord = |t| Shortcut::parse(t).unwrap();
        assert_eq!(
            s.check(Action::Overlay, chord("Ctrl+F5")),
            Some(Rejected::TooFewModifiers)
        );
        assert_eq!(
            s.check(Action::Overlay, chord("Ctrl+Alt+M")),
            Some(Rejected::InUse(Action::Microphone))
        );
        assert_eq!(
            s.check(Action::Microphone, chord("Ctrl+Alt+F3")),
            Some(Rejected::InUse(Action::Profiles))
        );
        // Ctrl+Alt+F10 is past the profiles' nine.
        assert_eq!(s.check(Action::Microphone, chord("Ctrl+Alt+F10")), None);
        // Its own chord is no clash.
        assert_eq!(s.check(Action::Overlay, chord("Ctrl+Alt+O")), None);
        assert_eq!(s.check(Action::Overlay, chord("Ctrl+Shift+O")), None);
    }

    /// The tray actions (#594) are off by default, and a settings file from
    /// #569 (without them) reads as off too — nothing new starts taking keys.
    #[test]
    fn the_tray_actions_are_off_until_chosen() {
        let d = Shortcuts::default();
        for action in [
            Action::DeskLamp,
            Action::ControlCenter,
            Action::Settings,
            Action::Status,
            Action::Recording,
            Action::OverlayLock,
            Action::Floating,
        ] {
            assert_eq!(d.get(action), None, "{action:?}");
        }
        let older: Shortcuts = serde_json::from_str(
            r#"{"overlay":"Ctrl+Alt+O","nextProfile":"Ctrl+Alt+P","profiles":"Ctrl+Alt","microphone":"Ctrl+Alt+M"}"#,
        )
        .unwrap();
        assert_eq!(older, d);
    }

    #[test]
    fn a_tray_action_is_stored_and_clashes_like_the_others() {
        let mut s = Shortcuts::default();
        let lamp = Shortcut::parse("Ctrl+Shift+L").unwrap();
        s.set(Action::DeskLamp, Some(lamp));
        assert_eq!(s.desk_lamp, Some(lamp));
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains(r#""deskLamp":"Ctrl+Shift+L""#), "{json}");
        assert_eq!(serde_json::from_str::<Shortcuts>(&json).unwrap(), s);
        // Taken by the lamp now, for any other action; free for the lamp itself.
        assert_eq!(
            s.check(Action::Status, lamp),
            Some(Rejected::InUse(Action::DeskLamp))
        );
        assert_eq!(s.check(Action::DeskLamp, lamp), None);
        // And a tray action can't take a profile's F-key.
        assert_eq!(
            s.check(Action::Recording, Shortcut::parse("Ctrl+Alt+F2").unwrap()),
            Some(Rejected::InUse(Action::Profiles))
        );
        // Nor can the profiles take a tray action's F-key.
        s.set(Action::Status, Shortcut::parse("Ctrl+Shift+F3"));
        assert_eq!(
            s.check_profiles(Mods::CTRL_SHIFT),
            Some(Rejected::InUse(Action::Status))
        );
    }

    #[test]
    fn profiles_cant_take_modifiers_whose_f_keys_another_action_uses() {
        let mut s = Shortcuts::default();
        assert_eq!(s.check_profiles(Mods::CTRL_SHIFT), None);
        s.overlay = Shortcut::parse("Ctrl+Shift+F2");
        assert_eq!(
            s.check_profiles(Mods::CTRL_SHIFT),
            Some(Rejected::InUse(Action::Overlay))
        );
        assert_eq!(s.check_profiles(Mods::CTRL_ALT), None);
    }
}
