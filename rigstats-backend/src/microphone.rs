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
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicU32, Ordering};
    use winapi::shared::mmreg::WAVEFORMATEX;
    use winapi::shared::winerror::S_OK;
    use winapi::shared::wtypes::VT_LPWSTR;
    use winapi::um::audioclient::IAudioClient;
    use winapi::um::audiosessiontypes::AUDCLNT_SHAREMODE_SHARED;
    use winapi::um::combaseapi::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    };
    use winapi::um::endpointvolume::{IAudioEndpointVolume, IAudioMeterInformation};
    use winapi::um::functiondiscoverykeys_devpkey::PKEY_Device_FriendlyName;
    use winapi::um::mmdeviceapi::{
        eCapture, eCommunications, CLSID_MMDeviceEnumerator, IMMDevice, IMMDeviceCollection,
        IMMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use winapi::um::objbase::COINIT_APARTMENTTHREADED;
    use winapi::um::propidl::PropVariantClear;
    use winapi::um::propsys::IPropertyStore;
    use winapi::Interface;

    const STGM_READ: u32 = 0;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A COM-allocated string, freed.
    unsafe fn take(p: *mut u16) -> String {
        if p.is_null() {
            return String::new();
        }
        let mut len = 0;
        while *p.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
        CoTaskMemFree(p.cast());
        s
    }

    /// Runs `f` with Windows' audio device enumerator, setting COM up on
    /// this thread for the call if it isn't already (the same shape as
    /// `desktop_background::win::with_wallpaper`).
    fn with_enumerator<T>(f: impl FnOnce(&IMMDeviceEnumerator) -> T) -> Result<T, String> {
        unsafe {
            let init = CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED);
            let mut raw: *mut IMMDeviceEnumerator = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_MMDeviceEnumerator,
                std::ptr::null_mut(),
                CLSCTX_ALL,
                &IMMDeviceEnumerator::uuidof(),
                (&mut raw as *mut *mut IMMDeviceEnumerator).cast(),
            );
            let out = match NonNull::new(raw) {
                Some(e) if hr >= 0 => {
                    // SAFETY: a live COM object; we hold the one reference
                    // CoCreateInstance gave us until the Release below.
                    let e = e.as_ref();
                    let r = f(e);
                    e.Release();
                    Ok(r)
                }
                _ => Err(format!("no audio device enumerator (0x{hr:08x})")),
            };
            // S_OK / S_FALSE: this call initialized (or re-counted) COM.
            if init >= 0 {
                CoUninitialize();
            }
            out
        }
    }

    /// Runs `f` with the capture endpoint `device` names, else (or when it
    /// isn't there) the default communications one.
    pub fn with_endpoint<T>(
        device: Option<&str>,
        f: impl FnOnce(&IMMDevice) -> T,
    ) -> Result<T, String> {
        with_enumerator(|enumerator| unsafe {
            let mut raw: *mut IMMDevice = std::ptr::null_mut();
            let mut hr = -1;
            if let Some(id) = device {
                hr = enumerator.GetDevice(wide(id).as_ptr(), &mut raw);
            }
            if raw.is_null() || hr < 0 {
                raw = std::ptr::null_mut();
                hr = enumerator.GetDefaultAudioEndpoint(eCapture, eCommunications, &mut raw);
            }
            match NonNull::new(raw) {
                Some(device) if hr >= 0 => {
                    let device = device.as_ref();
                    let r = f(device);
                    device.Release();
                    Ok(r)
                }
                _ => Err(format!("no microphone (0x{hr:08x})")),
            }
        })?
    }

    /// Every active capture endpoint.
    pub fn list() -> Result<Vec<Device>, String> {
        with_enumerator(|enumerator| unsafe {
            let mut raw: *mut IMMDeviceCollection = std::ptr::null_mut();
            let hr = enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE, &mut raw);
            let Some(collection) = NonNull::new(raw).filter(|_| hr >= 0) else {
                return Vec::new();
            };
            let collection = collection.as_ref();
            let mut count = 0u32;
            let mut out = Vec::new();
            if collection.GetCount(std::ptr::addr_of_mut!(count).cast_const()) == S_OK {
                for i in 0..count {
                    let mut raw: *mut IMMDevice = std::ptr::null_mut();
                    if collection.Item(i, &mut raw) != S_OK {
                        continue;
                    }
                    if let Some(device) = NonNull::new(raw) {
                        let device = device.as_ref();
                        out.push(Device {
                            id: id(device),
                            name: name(device),
                        });
                        device.Release();
                    }
                }
            }
            collection.Release();
            out
        })
    }

    /// Runs `f` with the endpoint's volume control.
    fn with_volume<T>(device: &IMMDevice, f: impl FnOnce(&IAudioEndpointVolume) -> T) -> Option<T> {
        unsafe {
            let mut raw: *mut IAudioEndpointVolume = std::ptr::null_mut();
            let hr = device.Activate(
                &IAudioEndpointVolume::uuidof(),
                CLSCTX_ALL,
                std::ptr::null_mut(),
                (&mut raw as *mut *mut IAudioEndpointVolume).cast(),
            );
            let volume = NonNull::new(raw).filter(|_| hr >= 0)?;
            let volume = volume.as_ref();
            let r = f(volume);
            volume.Release();
            Some(r)
        }
    }

    pub fn muted(device: &IMMDevice) -> Option<bool> {
        with_volume(device, |volume| unsafe {
            let mut muted = 0;
            (volume.GetMute(&mut muted) == S_OK).then_some(muted != 0)
        })
        .flatten()
    }

    pub fn set_muted(device: &IMMDevice, muted: bool) -> Result<(), String> {
        let hr = with_volume(device, |volume| unsafe {
            volume.SetMute(i32::from(muted), std::ptr::null())
        })
        .ok_or("no volume control on the microphone")?;
        if hr == S_OK {
            Ok(())
        } else {
            Err(format!("SetMute failed (0x{hr:08x})"))
        }
    }

    pub fn level(device: &IMMDevice) -> Option<f32> {
        with_volume(device, |volume| unsafe {
            let mut level = 0.0f32;
            (volume.GetMasterVolumeLevelScalar(&mut level) == S_OK).then_some(level)
        })
        .flatten()
    }

    pub fn set_level(device: &IMMDevice, level: f32) -> Result<(), String> {
        let hr = with_volume(device, |volume| unsafe {
            volume.SetMasterVolumeLevelScalar(level, std::ptr::null())
        })
        .ok_or("no volume control on the microphone")?;
        if hr == S_OK {
            Ok(())
        } else {
            Err(format!("SetMasterVolumeLevelScalar failed (0x{hr:08x})"))
        }
    }

    pub fn peak(device: &IMMDevice) -> Option<f32> {
        unsafe {
            let mut raw: *mut IAudioMeterInformation = std::ptr::null_mut();
            let hr = device.Activate(
                &IAudioMeterInformation::uuidof(),
                CLSCTX_ALL,
                std::ptr::null_mut(),
                (&mut raw as *mut *mut IAudioMeterInformation).cast(),
            );
            let meter = NonNull::new(raw).filter(|_| hr >= 0)?;
            let meter = meter.as_ref();
            let mut peak = 0.0f32;
            let ok = meter.GetPeakValue(&mut peak) == S_OK;
            meter.Release();
            ok.then_some(peak)
        }
    }

    /// `Meter`'s thread: opens a shared capture stream on the endpoint (never
    /// read — it only makes Windows meter the endpoint), then publishes the
    /// peak every 50 ms while `keep()` holds.
    pub fn run_meter(device: Option<&str>, level: &AtomicU32, keep: &dyn Fn() -> bool) {
        let _ = with_endpoint(device, |endpoint| unsafe {
            let mut raw: *mut IAudioClient = std::ptr::null_mut();
            let hr = endpoint.Activate(
                &IAudioClient::uuidof(),
                CLSCTX_ALL,
                std::ptr::null_mut(),
                (&mut raw as *mut *mut IAudioClient).cast(),
            );
            let Some(client) = NonNull::new(raw).filter(|_| hr >= 0) else {
                return;
            };
            let client = client.as_ref();
            let mut format: *mut WAVEFORMATEX = std::ptr::null_mut();
            let started = client.GetMixFormat(&mut format) == S_OK
                && client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    0,
                    1_000_000, // 100 ms buffer, in 100 ns units
                    0,
                    format,
                    std::ptr::null(),
                ) >= 0
                && client.Start() >= 0;
            if started {
                while keep() {
                    let peak = self::peak(endpoint).unwrap_or(0.0);
                    level.store(peak.to_bits(), Ordering::Relaxed);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                client.Stop();
            }
            if !format.is_null() {
                CoTaskMemFree(format.cast());
            }
            client.Release();
        });
        level.store(0, Ordering::Relaxed);
    }

    /// Windows' endpoint id, or "" when it can't be read.
    pub fn id(device: &IMMDevice) -> String {
        unsafe {
            let mut raw: *mut u16 = std::ptr::null_mut();
            if device.GetId(&mut raw) == S_OK {
                take(raw)
            } else {
                String::new()
            }
        }
    }

    /// Windows' friendly name for the endpoint, or "Microphone".
    pub fn name(device: &IMMDevice) -> String {
        unsafe {
            let mut raw: *mut IPropertyStore = std::ptr::null_mut();
            let hr = device.OpenPropertyStore(STGM_READ, &mut raw);
            let Some(store) = NonNull::new(raw).filter(|_| hr >= 0) else {
                return "Microphone".to_owned();
            };
            let store = store.as_ref();
            let mut value = std::mem::zeroed();
            let mut name = None;
            if store.GetValue(&PKEY_Device_FriendlyName, &mut value) == S_OK {
                if value.vt == VT_LPWSTR as u16 {
                    let p = *value.data.pwszVal();
                    if !p.is_null() {
                        let mut len = 0;
                        while *p.add(len) != 0 {
                            len += 1;
                        }
                        name = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)));
                    }
                }
                // The PROPVARIANT owns its string.
                PropVariantClear(&mut value);
            }
            store.Release();
            name.filter(|n| !n.is_empty())
                .unwrap_or_else(|| "Microphone".to_owned())
        }
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
