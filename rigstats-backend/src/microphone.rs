//! The microphone's mute, through Windows (#425): a capture endpoint —
//! by default the one Windows uses for calls (Teams, Discord, games), or
//! one the user chose in Settings — read and switched with Core Audio's
//! `IAudioEndpointVolume`. Vendor-neutral and writes nothing to a device: a
//! headset's own HID protocol has no microphone command (ASUS GearLink mutes
//! through Windows too), so this is the one way that works for every headset
//! and microphone.
//!
//! `#![allow(unsafe_code)]`: a thin COM shim; the policy (what to toggle and
//! when) lives in the app.

#![allow(unsafe_code)]

/// One microphone Windows has right now, for the Settings dropdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// Windows' endpoint id — stable across restarts, what `Settings`
    /// stores (`microphone_device`).
    pub id: String,
    /// Windows' name for it ("Headset Microphone (ROG Delta II)").
    pub name: String,
}

/// The microphone in use and its mute, as Windows sees it right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Microphone {
    pub id: String,
    pub name: String,
    pub muted: bool,
}

/// Every active microphone, in Windows' order.
pub fn list() -> Vec<Device> {
    win::list().unwrap_or_default()
}

/// The microphone `device` names (an endpoint id from [`list`]), or — with
/// `None`, or when that one isn't connected — the one Windows uses for
/// calls. `None` when Windows has no microphone at all (or COM fails).
pub fn current(device: Option<&str>) -> Option<Microphone> {
    win::with_endpoint(device, |endpoint| {
        let muted = win::muted(endpoint)?;
        Some(Microphone {
            id: win::id(endpoint),
            name: win::name(endpoint),
            muted,
        })
    })
    .ok()
    .flatten()
}

/// Sets the microphone's mute and reads it back. The error names the step
/// that failed, for the log.
pub fn set_muted(device: Option<&str>, muted: bool) -> Result<Microphone, String> {
    win::with_endpoint(device, |endpoint| {
        win::set_muted(endpoint, muted)?;
        let now = win::muted(endpoint).ok_or("could not read the mute back")?;
        if now != muted {
            return Err(format!(
                "the microphone stayed {}",
                if now { "muted" } else { "on" }
            ));
        }
        Ok(Microphone {
            id: win::id(endpoint),
            name: win::name(endpoint),
            muted: now,
        })
    })?
}

/// Flips the mute: reads the current state and sets the opposite.
pub fn toggle(device: Option<&str>) -> Result<Microphone, String> {
    let now = current(device).ok_or("no microphone")?;
    set_muted(device, !now.muted)
}

/// The microphone's input level, 0–1 (Windows' "Input volume"), or `None`
/// when there is no microphone (#427).
pub fn level(device: Option<&str>) -> Option<f32> {
    win::with_endpoint(device, win::level).ok().flatten()
}

/// Sets the input level (clamped to 0–1) and reads it back.
pub fn set_level(device: Option<&str>, level: f32) -> Result<f32, String> {
    let level = level.clamp(0.0, 1.0);
    win::with_endpoint(device, |endpoint| {
        win::set_level(endpoint, level)?;
        win::level(endpoint).ok_or_else(|| "could not read the level back".to_owned())
    })?
}

/// What the microphone hears right now, 0–1 (the peak since the last
/// read), or `None` when there is no microphone (#427). Read-only. Windows
/// meters a capture endpoint only while some program records from it — use
/// a [`Meter`] to see it at any time.
pub fn peak(device: Option<&str>) -> Option<f32> {
    win::with_endpoint(device, win::peak).ok().flatten()
}

/// A live level meter for a microphone (#427). Windows meters a capture
/// endpoint only while something records from it (verified: the webcam read
/// 0 until a stream was open, then 0.04 silent / 0.5 talking), so reading the
/// meter opens a shared capture stream on a thread — its samples are never
/// read — and publishes the peak about 20 times a second.
///
/// While the stream is open Windows shows RIGStats as using the microphone,
/// so it runs only while someone looks: the first [`Meter::level`] starts the
/// thread, and it closes the stream by itself once `level` hasn't been
/// called for a second (a window closed, the tray card hidden). Nothing to
/// stop by hand; the next `level` starts it again.
pub struct Meter {
    device: Option<String>,
    shared: std::sync::Arc<MeterShared>,
}

#[derive(Default)]
struct MeterShared {
    level: std::sync::atomic::AtomicU32,
    running: std::sync::atomic::AtomicBool,
    last_read: std::sync::Mutex<Option<std::time::Instant>>,
}

/// How long the stream stays open after the last read.
const METER_IDLE: std::time::Duration = std::time::Duration::from_secs(1);

impl Meter {
    /// A meter for `device` (`None` = the one Windows uses for calls). No
    /// stream is opened until the first [`Meter::level`].
    pub fn new(device: Option<String>) -> Self {
        Self {
            device,
            shared: std::sync::Arc::default(),
        }
    }

    /// The latest peak, 0–1 — 0 for the first few frames after (re)start.
    pub fn level(&self) -> f32 {
        use std::sync::atomic::Ordering;
        if let Ok(mut t) = self.shared.last_read.lock() {
            *t = Some(std::time::Instant::now());
        }
        if !self.shared.running.swap(true, Ordering::AcqRel) {
            let (shared, device) = (std::sync::Arc::clone(&self.shared), self.device.clone());
            let spawned = std::thread::Builder::new()
                .name("mic-meter".into())
                .spawn(move || {
                    let keep = || {
                        shared
                            .last_read
                            .lock()
                            .ok()
                            .and_then(|t| *t)
                            .is_some_and(|t| t.elapsed() < METER_IDLE)
                    };
                    win::run_meter(device.as_deref(), &shared.level, &keep);
                    shared.level.store(0, Ordering::Relaxed);
                    shared.running.store(false, Ordering::Release);
                });
            if spawned.is_err() {
                self.shared.running.store(false, Ordering::Release);
            }
        }
        f32::from_bits(self.shared.level.load(Ordering::Relaxed))
    }

    /// The microphone this meter listens to (`None` = the one for calls).
    pub fn device(&self) -> Option<&str> {
        self.device.as_deref()
    }
}

#[cfg(windows)]
mod win {
    use super::Device;
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
    use windows::Win32::Media::Audio::{
        eCapture, eCommunications, IAudioClient, IMMDevice, IMMDeviceEnumerator,
        MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_APARTMENTTHREADED, STGM_READ,
    };
    use windows::Win32::System::Variant::VT_LPWSTR;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A COM-allocated string, freed.
    ///
    /// # Safety
    ///
    /// `p` must be null or a NUL-terminated UTF-16 string allocated with
    /// `CoTaskMemAlloc` that the caller owns; it is freed here and must not
    /// be used afterwards.
    unsafe fn take(p: PWSTR) -> String {
        if p.is_null() {
            return String::new();
        }
        // SAFETY: per this function's contract.
        let s = unsafe { p.to_string() }.unwrap_or_default();
        // SAFETY: the caller owns `p`, from CoTaskMemAlloc; freed once.
        unsafe { CoTaskMemFree(Some(p.0.cast())) };
        s
    }

    /// Runs `f` with Windows' audio device enumerator, setting COM up on
    /// this thread for the call if it isn't already (the same shape as
    /// `desktop_background::win::with_wallpaper`). COM interfaces from the
    /// windows crate release themselves when dropped; every one made inside
    /// is gone before CoUninitialize.
    fn with_enumerator<T>(f: impl FnOnce(&IMMDeviceEnumerator) -> T) -> Result<T, String> {
        // SAFETY: no reserved argument; balanced below on this thread.
        let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        // SAFETY: the CLSID and the interface match (MMDeviceEnumerator).
        let created = unsafe {
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        };
        let out = match created {
            Ok(enumerator) => {
                let r = f(&enumerator);
                // Released here, before CoUninitialize below.
                drop(enumerator);
                Ok(r)
            }
            Err(e) => Err(format!("no audio device enumerator (0x{:08x})", e.code().0)),
        };
        // S_OK / S_FALSE: this call initialized (or re-counted) COM.
        if init.is_ok() {
            // SAFETY: balances the successful CoInitializeEx above on this
            // thread; every COM interface made inside has been dropped.
            unsafe { CoUninitialize() };
        }
        out
    }

    /// Runs `f` with the capture endpoint `device` names, else (or when it
    /// isn't there) the default communications one.
    pub fn with_endpoint<T>(
        device: Option<&str>,
        f: impl FnOnce(&IMMDevice) -> T,
    ) -> Result<T, String> {
        with_enumerator(|enumerator| {
            let named = device.and_then(|id| {
                let id = wide(id);
                // SAFETY: `enumerator` is live for the closure; `id` is
                // NUL-terminated and outlives the call.
                unsafe { enumerator.GetDevice(PCWSTR(id.as_ptr())) }.ok()
            });
            let endpoint = match named {
                Some(endpoint) => Ok(endpoint),
                // SAFETY: `enumerator` is live for the closure.
                None => unsafe { enumerator.GetDefaultAudioEndpoint(eCapture, eCommunications) },
            };
            match endpoint {
                Ok(endpoint) => Ok(f(&endpoint)),
                Err(e) => Err(format!("no microphone (0x{:08x})", e.code().0)),
            }
        })?
    }

    /// Every active capture endpoint.
    pub fn list() -> Result<Vec<Device>, String> {
        with_enumerator(|enumerator| {
            // SAFETY: `enumerator` is live for the closure.
            let Ok(collection) =
                (unsafe { enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) })
            else {
                return Vec::new();
            };
            // SAFETY: `collection` is live.
            let count = unsafe { collection.GetCount() }.unwrap_or(0);
            (0..count)
                // SAFETY: `collection` is live and `i` < its count.
                .filter_map(|i| unsafe { collection.Item(i) }.ok())
                .map(|device| Device {
                    id: id(&device),
                    name: name(&device),
                })
                .collect()
        })
    }

    pub fn muted(device: &IMMDevice) -> Option<bool> {
        // SAFETY: `device` is a live endpoint; the interface is the type
        // asked for.
        let volume: IAudioEndpointVolume = unsafe { device.Activate(CLSCTX_ALL, None) }.ok()?;
        // SAFETY: `volume` is live.
        unsafe { volume.GetMute() }.ok().map(|m| m.as_bool())
    }

    pub fn set_muted(device: &IMMDevice, muted: bool) -> Result<(), String> {
        // SAFETY: `device` is a live endpoint.
        let volume: IAudioEndpointVolume = unsafe { device.Activate(CLSCTX_ALL, None) }
            .map_err(|_| "no volume control on the microphone".to_string())?;
        // SAFETY: `volume` is live; a null event-context GUID is allowed.
        unsafe { volume.SetMute(muted, std::ptr::null()) }
            .map_err(|e| format!("SetMute failed (0x{:08x})", e.code().0))
    }

    pub fn level(device: &IMMDevice) -> Option<f32> {
        // SAFETY: `device` is a live endpoint.
        let volume: IAudioEndpointVolume = unsafe { device.Activate(CLSCTX_ALL, None) }.ok()?;
        // SAFETY: `volume` is live.
        unsafe { volume.GetMasterVolumeLevelScalar() }.ok()
    }

    pub fn set_level(device: &IMMDevice, level: f32) -> Result<(), String> {
        // SAFETY: `device` is a live endpoint.
        let volume: IAudioEndpointVolume = unsafe { device.Activate(CLSCTX_ALL, None) }
            .map_err(|_| "no volume control on the microphone".to_string())?;
        // SAFETY: `volume` is live; `level` is clamped to 0–1 by the caller;
        // a null event-context GUID is allowed.
        unsafe { volume.SetMasterVolumeLevelScalar(level, std::ptr::null()) }
            .map_err(|e| format!("SetMasterVolumeLevelScalar failed (0x{:08x})", e.code().0))
    }

    pub fn peak(device: &IMMDevice) -> Option<f32> {
        // SAFETY: `device` is a live endpoint.
        let meter: IAudioMeterInformation = unsafe { device.Activate(CLSCTX_ALL, None) }.ok()?;
        // SAFETY: `meter` is live.
        unsafe { meter.GetPeakValue() }.ok()
    }

    /// `Meter`'s thread: opens a shared capture stream on the endpoint (never
    /// read — it only makes Windows meter the endpoint), then publishes the
    /// peak every 50 ms while `keep()` holds.
    pub fn run_meter(device: Option<&str>, level: &AtomicU32, keep: &dyn Fn() -> bool) {
        let _ = with_endpoint(device, |endpoint| {
            // SAFETY: `endpoint` is live for the closure. `client` is
            // released when it drops at the end of this closure — after
            // Stop and after the format is freed, the order used before.
            let Ok(client) = (unsafe { endpoint.Activate::<IAudioClient>(CLSCTX_ALL, None) })
            else {
                return;
            };
            // SAFETY: `client` is live; on success the format is a CoTaskMem
            // WAVEFORMATEX that Initialize reads and that is freed only
            // below, after the stream stopped.
            let format = unsafe { client.GetMixFormat() }.ok();
            let started = format.is_some_and(|format| {
                // SAFETY: as above; `format` is the live mix format.
                unsafe {
                    client
                        .Initialize(
                            AUDCLNT_SHAREMODE_SHARED,
                            0,
                            1_000_000, // 100 ms buffer, in 100 ns units
                            0,
                            format,
                            None,
                        )
                        .is_ok()
                        && client.Start().is_ok()
                }
            });
            if started {
                while keep() {
                    let peak = self::peak(endpoint).unwrap_or(0.0);
                    level.store(peak.to_bits(), Ordering::Relaxed);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                // SAFETY: `client` is live and was started above.
                let _ = unsafe { client.Stop() };
            }
            if let Some(format) = format.filter(|f| !f.is_null()) {
                // SAFETY: `format` came from GetMixFormat (CoTaskMemAlloc),
                // is owned by us and freed once; nothing reads it afterwards.
                unsafe { CoTaskMemFree(Some(format.cast())) };
            }
        });
        level.store(0, Ordering::Relaxed);
    }

    /// Windows' endpoint id, or "" when it can't be read.
    pub fn id(device: &IMMDevice) -> String {
        // SAFETY: `device` is a live endpoint; on success GetId returns a
        // NUL-terminated CoTaskMem string the caller owns — `take` frees it.
        match unsafe { device.GetId() } {
            // SAFETY: GetId's string is NUL-terminated, CoTaskMem, ours.
            Ok(raw) => unsafe { take(raw) },
            Err(_) => String::new(),
        }
    }

    /// Windows' friendly name for the endpoint, or "Microphone".
    pub fn name(device: &IMMDevice) -> String {
        // SAFETY: `device` is a live endpoint.
        let Ok(store) = (unsafe { device.OpenPropertyStore(STGM_READ) }) else {
            return "Microphone".to_owned();
        };
        // SAFETY: `store` is live; the key is the windows crate's constant.
        // The PROPVARIANT clears itself (PropVariantClear) when dropped,
        // before `store` is released at the end of the function.
        let name = match unsafe { store.GetValue(&PKEY_Device_FriendlyName) } {
            Ok(value) if value.vt() == VT_LPWSTR => Some(value.to_string()),
            _ => None,
        };
        name.filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Microphone".to_owned())
    }
}

#[cfg(not(windows))]
mod win {
    use super::Device;
    pub struct Endpoint;
    pub fn with_endpoint<T>(_: Option<&str>, _f: impl FnOnce(&Endpoint) -> T) -> Result<T, String> {
        Err("Windows only".to_owned())
    }
    pub fn list() -> Result<Vec<Device>, String> {
        Ok(Vec::new())
    }
    pub fn muted(_: &Endpoint) -> Option<bool> {
        None
    }
    pub fn set_muted(_: &Endpoint, _: bool) -> Result<(), String> {
        Err("Windows only".to_owned())
    }
    pub fn level(_: &Endpoint) -> Option<f32> {
        None
    }
    pub fn set_level(_: &Endpoint, _: f32) -> Result<(), String> {
        Err("Windows only".to_owned())
    }
    pub fn peak(_: &Endpoint) -> Option<f32> {
        None
    }
    pub fn run_meter(_: Option<&str>, _: &std::sync::atomic::AtomicU32, _: &dyn Fn() -> bool) {}
    pub fn id(_: &Endpoint) -> String {
        String::new()
    }
    pub fn name(_: &Endpoint) -> String {
        "Microphone".to_owned()
    }
}

/// #702: what Windows reports about its microphones, read only — nothing
/// here mutes, unmutes or changes a level. Checked against Windows' own
/// list in the registry, so they stay the same when the COM code moves off
/// winapi. On a PC without a microphone (a CI runner) the lists are empty.
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};

    /// The active capture endpoints in the registry, by the GUID that ends
    /// an endpoint id (`{0.0.1.00000000}.{guid}`); `DeviceState` 1 = active.
    fn active_in_registry() -> Vec<String> {
        let Ok(capture) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Capture")
        else {
            return Vec::new();
        };
        let mut guids: Vec<String> = capture
            .enum_keys()
            .flatten()
            .filter(|key| {
                capture
                    .open_subkey(key)
                    .and_then(|endpoint| endpoint.get_value::<u32, _>("DeviceState"))
                    .ok()
                    == Some(1)
            })
            .map(|key| key.to_ascii_lowercase())
            .collect();
        guids.sort();
        guids
    }

    #[test]
    fn the_list_is_the_active_microphones_windows_registers() {
        let mut listed: Vec<String> = list()
            .iter()
            .map(|d| {
                d.id.rsplit('.')
                    .next()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
            })
            .collect();
        listed.sort();
        assert_eq!(listed, active_in_registry());
    }

    #[test]
    fn each_listed_microphone_reads_back_by_its_id() {
        for device in list() {
            assert!(
                !device.id.is_empty() && !device.name.is_empty(),
                "{device:?}"
            );
            let mic = current(Some(&device.id)).expect("a listed microphone reads");
            assert_eq!(
                (mic.id.as_str(), mic.name.as_str()),
                (device.id.as_str(), device.name.as_str())
            );
            let level = level(Some(&device.id)).expect("its level reads");
            assert!((0.0..=1.0).contains(&level), "{level}");
            let peak = peak(Some(&device.id)).expect("its peak reads");
            assert!((0.0..=1.0).contains(&peak), "{peak}");
        }
    }

    #[test]
    fn the_one_for_calls_exists_exactly_when_there_is_a_microphone() {
        assert_eq!(current(None).is_some(), !list().is_empty());
    }

    #[test]
    fn an_unknown_id_falls_back_to_the_one_for_calls() {
        assert_eq!(
            current(Some("{0.0.1.00000000}.{no-such-microphone}")),
            current(None)
        );
    }

    #[test]
    fn a_meter_reads_a_level_and_lets_go() {
        let meter = Meter::new(None);
        let _ = meter.level();
        std::thread::sleep(std::time::Duration::from_millis(400));
        let level = meter.level();
        assert!((0.0..=1.0).contains(&level), "{level}");
        drop(meter);
    }
}
