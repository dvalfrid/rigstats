//! The sound playing on the PC as a spectrum (#719): the default output's
//! mix, caught with WASAPI loopback, turned into [`BANDS`] bar heights, and
//! (for the oscilloscope, #732) the newest few milliseconds of the wave
//! itself as [`WAVE_POINTS`] points.
//!
//! [`bars`] or [`wave`] starts a capture thread on first use; the thread
//! ends a second after the last call, so nothing runs while no panel draws a
//! spectrum. The audio stays in memory for one FFT window and is never
//! written anywhere: only the bar heights and a few hundred scaled points
//! leave this module.
//!
//! What it hears is everything on the output — a game, a call, a
//! notification — not only the music. An app in exclusive mode bypasses the
//! mix and isn't heard. Silence delivers no packets, which shows as falling
//! bars, not an error.
//!
//! `#![allow(unsafe_code)]`: the WASAPI calls are COM; the analysis is pure.

#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How many bars.
pub const BANDS: usize = 24;

/// Samples per FFT, a power of two: ~85 ms at 48 kHz, 11.7 Hz a bin — fine
pub const WINDOW: usize = 4096;

/// The lowest and highest frequency shown, in Hz.
const LOW_HZ: f32 = 60.0;
const HIGH_HZ: f32 = 16_000.0;

/// The level range a bar spans: quieter is an empty bar, louder a full one.
const FLOOR_DB: f32 = -70.0;
const CEIL_DB: f32 = -10.0;

/// How fast a bar falls, in bar heights per second; it rises at once.
const FALL_PER_SEC: f32 = 2.5;

/// How long the thread runs after the last [`bars`].
const IDLE: Duration = Duration::from_secs(1);

/// How often a new spectrum is computed (the panel draws at most 30 fps).
const STEP: Duration = Duration::from_millis(30);

/// How many samples the oscilloscope shows: ~21 ms at 48 kHz, a full cycle
/// of a 50 Hz bass note.
pub const WAVE_SPAN: usize = 1024;

/// How many points of it are handed out (one per ~1.5 px of the panel).
pub const WAVE_POINTS: usize = 256;

/// The peak at which the scope stops making a wave louder: a quieter one is
/// drawn smaller, so hiss or a fade-out doesn't fill the panel.
const WAVE_MIN_PEAK: f32 = 0.1;

/// The latest bars and wave, written by the capture thread.
struct Latest {
    bars: [f32; BANDS],
    wave: [f32; WAVE_POINTS],
}

impl Default for Latest {
    fn default() -> Self {
        Self {
            bars: [0.0; BANDS],
            wave: [0.0; WAVE_POINTS],
        }
    }
}

#[derive(Default)]
struct Shared {
    latest: Mutex<Latest>,
    last_read: Mutex<Option<Instant>>,
    running: AtomicBool,
}

fn shared() -> &'static Arc<Shared> {
    static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
    SHARED.get_or_init(Arc::default)
}

/// The latest bar heights, 0–1, low to high frequency. Starts the capture
/// thread when it isn't running; all zero until it has heard something.
pub fn bars() -> [f32; BANDS] {
    keep_listening()
        .latest
        .lock()
        .map(|l| l.bars)
        .unwrap_or([0.0; BANDS])
}

/// The latest wave, −1 to 1, oldest point first, starting where the sound
/// rises through zero so it stands still on screen (see [`waveform`]).
/// Starts the capture thread like [`bars`]; flat until it has heard something.
pub fn wave() -> [f32; WAVE_POINTS] {
    keep_listening()
        .latest
        .lock()
        .map(|l| l.wave)
        .unwrap_or([0.0; WAVE_POINTS])
}

/// Notes a read and starts the capture thread when it isn't running.
fn keep_listening() -> &'static Arc<Shared> {
    let shared = shared();
    if let Ok(mut t) = shared.last_read.lock() {
        *t = Some(Instant::now());
    }
    if !shared.running.swap(true, Ordering::AcqRel) {
        let s = Arc::clone(shared);
        let spawned = std::thread::Builder::new()
            .name("spectrum".into())
            .spawn(move || {
                let keep = || {
                    s.last_read
                        .lock()
                        .ok()
                        .and_then(|t| *t)
                        .is_some_and(|t| t.elapsed() < IDLE)
                };
                let publish = |bars: [f32; BANDS], wave: [f32; WAVE_POINTS]| {
                    if let Ok(mut l) = s.latest.lock() {
                        *l = Latest { bars, wave };
                    }
                };
                // A device that went away (headset unplugged) ends a run;
                // the next one opens the new default output.
                while keep() {
                    win::run(&keep, &publish);
                    if keep() {
                        std::thread::sleep(Duration::from_millis(500));
                    }
                }
                publish([0.0; BANDS], [0.0; WAVE_POINTS]);
                s.running.store(false, Ordering::Release);
            });
        if spawned.is_err() {
            shared.running.store(false, Ordering::Release);
        }
    }
    shared
}

/// The newest [`WINDOW`] mono samples, filled as audio arrives.
pub struct Ring {
    samples: Vec<f32>,
    next: usize,
}

impl Default for Ring {
    fn default() -> Self {
        Self {
            samples: vec![0.0; WINDOW],
            next: 0,
        }
    }
}

impl Ring {
    /// Adds interleaved frames of `channels` samples, mixed down to mono.
    pub fn push_frames(&mut self, interleaved: &[f32], channels: usize) {
        if channels == 0 {
            return;
        }
        for frame in interleaved.chunks_exact(channels) {
            self.push(frame.iter().sum::<f32>() / channels as f32);
        }
    }

    pub fn push(&mut self, sample: f32) {
        self.samples[self.next] = sample;
        self.next = (self.next + 1) % WINDOW;
    }

    /// The window, oldest sample first.
    pub fn window(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(WINDOW);
        out.extend_from_slice(&self.samples[self.next..]);
        out.extend_from_slice(&self.samples[..self.next]);
        out
    }
}

/// The oscilloscope's wave from a window (oldest sample first): the newest
/// [`WAVE_SPAN`] samples that start where the signal rises through zero —
/// searched in the [`WAVE_SPAN`] before them, so a steady tone is drawn at
/// the same phase every frame instead of running across the panel — then
/// cut to [`WAVE_POINTS`] points and scaled so its peak reaches 0.9 (a peak
/// below [`WAVE_MIN_PEAK`] gets that one's gain, and stays smaller).
pub fn waveform(window: &[f32]) -> [f32; WAVE_POINTS] {
    let mut out = [0.0; WAVE_POINTS];
    if window.len() < WAVE_SPAN {
        return out;
    }
    let latest = window.len() - WAVE_SPAN;
    let earliest = latest.saturating_sub(WAVE_SPAN).max(1);
    // The newest rising zero crossing; with none (silence, pure noise
    // floor), the newest samples as they are.
    let start = (earliest..=latest)
        .rev()
        .find(|&i| window[i - 1] < 0.0 && window[i] >= 0.0)
        .unwrap_or(latest);
    let span = &window[start..start + WAVE_SPAN];
    let peak = span.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let gain = 0.9 / peak.max(WAVE_MIN_PEAK);
    let step = WAVE_SPAN / WAVE_POINTS;
    for (p, o) in out.iter_mut().enumerate() {
        // The mean of each step's samples, so a high note between points
        // doesn't alias into a slow wobble.
        let chunk = &span[p * step..(p + 1) * step];
        *o = (chunk.iter().sum::<f32>() / step as f32 * gain).clamp(-1.0, 1.0);
    }
    out
}

/// Bar heights, 0–1, of a [`WINDOW`]-sample mono signal at `rate` Hz:
/// Hann window, FFT, the loudest bin of each logarithmic band, in dB
/// between [`FLOOR_DB`] and [`CEIL_DB`].
pub fn analyze(samples: &[f32], rate: u32) -> [f32; BANDS] {
    let n = samples.len();
    if n < 2 || !n.is_power_of_two() || rate == 0 {
        return [0.0; BANDS];
    }
    let mut re: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos();
            s * w
        })
        .collect();
    let mut im = vec![0.0f32; n];
    fft(&mut re, &mut im);
    // A full-scale sine under a Hann window peaks at n / 4.
    let scale = 4.0 / n as f32;
    let bin_hz = rate as f32 / n as f32;
    let mut out = [0.0f32; BANDS];
    for (b, bar) in out.iter_mut().enumerate() {
        let (lo, hi) = band_edges(b);
        let first = ((lo / bin_hz).floor() as usize).max(1);
        let last = ((hi / bin_hz).ceil() as usize).clamp(first + 1, n / 2);
        let peak = (first..last)
            .map(|k| (re[k] * re[k] + im[k] * im[k]).sqrt() * scale)
            .fold(0.0f32, f32::max);
        let db = 20.0 * peak.max(1e-9).log10();
        *bar = ((db - FLOOR_DB) / (CEIL_DB - FLOOR_DB)).clamp(0.0, 1.0);
    }
    out
}

/// Band `b`'s lower and upper frequency: equal steps on a log scale from
/// [`LOW_HZ`] to [`HIGH_HZ`].
pub fn band_edges(b: usize) -> (f32, f32) {
    let ratio = (HIGH_HZ / LOW_HZ).powf(1.0 / BANDS as f32);
    let lo = LOW_HZ * ratio.powi(b as i32);
    (lo, lo * ratio)
}

/// The bars shown after `dt`: a louder band jumps up, a quieter one falls
/// at [`FALL_PER_SEC`].
pub fn smooth(shown: [f32; BANDS], heard: [f32; BANDS], dt: Duration) -> [f32; BANDS] {
    let fall = FALL_PER_SEC * dt.as_secs_f32();
    let mut out = [0.0; BANDS];
    for b in 0..BANDS {
        out[b] = heard[b].max(shown[b] - fall).max(0.0);
    }
    out
}

/// In-place iterative radix-2 FFT; `re.len()` must be a power of two.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * std::f32::consts::PI / len as f32;
        let (w_im, w_re) = angle.sin_cos();
        for start in (0..n).step_by(len) {
            let (mut c_re, mut c_im) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let t_re = re[b] * c_re - im[b] * c_im;
                let t_im = re[b] * c_im + im[b] * c_re;
                re[b] = re[a] - t_re;
                im[b] = im[a] - t_im;
                re[a] += t_re;
                im[a] += t_im;
                let next = c_re * w_re - c_im * w_im;
                c_im = c_re * w_im + c_im * w_re;
                c_re = next;
            }
        }
        len <<= 1;
    }
}

/// Whether a mix format is 32-bit float, which a shared-mode mix is in
/// practice: `WAVE_FORMAT_IEEE_FLOAT`, or `WAVE_FORMAT_EXTENSIBLE` with the
/// float sub-format.
pub fn is_float32(tag: u16, bits: u16, subformat: Option<u128>) -> bool {
    const IEEE_FLOAT: u16 = 3;
    const EXTENSIBLE: u16 = 0xFFFE;
    // KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, 00000003-0000-0010-8000-00aa00389b71.
    const FLOAT_SUBTYPE: u128 = 0x00000003_0000_0010_8000_00aa00389b71;
    bits == 32 && (tag == IEEE_FLOAT || (tag == EXTENSIBLE && subformat == Some(FLOAT_SUBTYPE)))
}

#[cfg(windows)]
mod win {
    use super::{analyze, is_float32, smooth, waveform, Ring, BANDS, STEP, WAVE_POINTS};
    use std::time::Instant;
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
        MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_MULTITHREADED,
    };

    /// One capture run on the default output, until `keep()` ends or the
    /// device fails. Publishes new bars every [`STEP`].
    pub fn run(keep: &dyn Fn() -> bool, publish: &dyn Fn([f32; BANDS], [f32; WAVE_POINTS])) {
        // SAFETY: no reserved argument; balanced below on this thread.
        let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        capture(keep, publish);
        if init.is_ok() {
            // SAFETY: balances the successful CoInitializeEx above; every COM
            // interface made in `capture` has been dropped.
            unsafe { CoUninitialize() };
        }
    }

    fn capture(keep: &dyn Fn() -> bool, publish: &dyn Fn([f32; BANDS], [f32; WAVE_POINTS])) {
        // SAFETY: the CLSID and the interface match (MMDeviceEnumerator).
        let Ok(enumerator) = (unsafe {
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        }) else {
            return;
        };
        // SAFETY: `enumerator` is live.
        let Ok(device) = (unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole) }) else {
            return;
        };
        // SAFETY: `device` is live; the interface is the type asked for.
        let Ok(client) = (unsafe { device.Activate::<IAudioClient>(CLSCTX_ALL, None) }) else {
            return;
        };
        // SAFETY: `client` is live; on success the format is a CoTaskMem
        // WAVEFORMATEX(TENSIBLE) that is freed below, after the stream ends.
        let Ok(format) = (unsafe { client.GetMixFormat() }) else {
            return;
        };
        if format.is_null() {
            return;
        }
        // SAFETY: `format` is a live mix format; the struct is packed, so it
        // is read unaligned. A tag of EXTENSIBLE means the larger struct.
        let head = unsafe { format.read_unaligned() };
        let (tag, bits, channels, rate) = (
            head.wFormatTag,
            head.wBitsPerSample,
            usize::from(head.nChannels),
            head.nSamplesPerSec,
        );
        let subformat = (tag == 0xFFFE).then(|| {
            // SAFETY: an EXTENSIBLE tag says the format is a
            // WAVEFORMATEXTENSIBLE; read unaligned for the same reason.
            let ext = unsafe { format.cast::<WAVEFORMATEXTENSIBLE>().read_unaligned() };
            // Copied out first: a field of the packed struct can't be borrowed.
            let sub = ext.SubFormat;
            sub.to_u128()
        });
        if is_float32(tag, bits, subformat) && channels > 0 {
            stream(&client, format, channels, rate, keep, publish);
        }
        // SAFETY: from GetMixFormat (CoTaskMemAlloc), ours, freed once;
        // nothing reads it afterwards.
        unsafe { CoTaskMemFree(Some(format.cast())) };
    }

    fn stream(
        client: &IAudioClient,
        format: *const WAVEFORMATEX,
        channels: usize,
        rate: u32,
        keep: &dyn Fn() -> bool,
        publish: &dyn Fn([f32; BANDS], [f32; WAVE_POINTS]),
    ) {
        // SAFETY: `client` is live and not yet initialized; `format` is its
        // own mix format, live for the call. 200 ms buffer, in 100 ns units.
        if unsafe {
            client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                2_000_000,
                0,
                format,
                None,
            )
        }
        .is_err()
        {
            return;
        }
        // SAFETY: `client` is initialized; the service is the type asked for.
        let Ok(capture) = (unsafe { client.GetService::<IAudioCaptureClient>() }) else {
            return;
        };
        // SAFETY: `client` is initialized.
        if unsafe { client.Start() }.is_err() {
            return;
        }
        let mut ring = Ring::default();
        let mut shown = [0.0f32; BANDS];
        let mut last = Instant::now();
        let mut last_packet = Instant::now();
        'outer: while keep() {
            loop {
                // SAFETY: `capture` is live and its stream started.
                let Ok(frames) = (unsafe { capture.GetNextPacketSize() }) else {
                    break 'outer;
                };
                if frames == 0 {
                    break;
                }
                last_packet = Instant::now();
                let mut data: *mut u8 = std::ptr::null_mut();
                let (mut count, mut flags) = (0u32, 0u32);
                // SAFETY: the out pointers are valid locals; on success the
                // buffer holds `count` frames until ReleaseBuffer below.
                if unsafe { capture.GetBuffer(&mut data, &mut count, &mut flags, None, None) }
                    .is_err()
                {
                    break 'outer;
                }
                let samples = count as usize * channels;
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                    for _ in 0..count {
                        ring.push(0.0);
                    }
                } else {
                    // SAFETY: the buffer is `count` frames of `channels`
                    // 32-bit floats (the float mix format checked above),
                    // live until ReleaseBuffer; read before it, not kept.
                    let slice = unsafe { std::slice::from_raw_parts(data.cast::<f32>(), samples) };
                    ring.push_frames(slice, channels);
                }
                // SAFETY: releases exactly the frames GetBuffer gave.
                if unsafe { capture.ReleaseBuffer(count) }.is_err() {
                    break 'outer;
                }
            }
            let now = Instant::now();
            let window = ring.window();
            shown = smooth(shown, analyze(&window, rate), now - last);
            last = now;
            publish(shown, waveform(&window));
            std::thread::sleep(STEP);
            // No packets while nothing plays: let the window empty out so
            // the bars fall.
            if now - last_packet >= STEP * 4 {
                ring = Ring::default();
            }
        }
        // SAFETY: `client` is live and was started above.
        let _ = unsafe { client.Stop() };
    }
}

#[cfg(not(windows))]
mod win {
    use super::{BANDS, WAVE_POINTS};
    pub fn run(_keep: &dyn Fn() -> bool, _publish: &dyn Fn([f32; BANDS], [f32; WAVE_POINTS])) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn sine(hz: f32, amp: f32) -> Vec<f32> {
        (0..WINDOW)
            .map(|i| amp * (2.0 * std::f32::consts::PI * hz * i as f32 / RATE as f32).sin())
            .collect()
    }

    fn band_of(hz: f32) -> usize {
        (0..BANDS)
            .find(|&b| {
                let (lo, hi) = band_edges(b);
                hz >= lo && hz < hi
            })
            .unwrap()
    }

    #[test]
    fn silence_is_empty_bars() {
        assert_eq!(analyze(&vec![0.0; WINDOW], RATE), [0.0; BANDS]);
    }

    #[test]
    fn a_tone_fills_its_own_band_most() {
        for hz in [100.0, 1_000.0, 5_000.0] {
            let bars = analyze(&sine(hz, 0.05), RATE);
            let loudest = (0..BANDS)
                .max_by(|&a, &b| bars[a].total_cmp(&bars[b]))
                .unwrap();
            assert_eq!(loudest, band_of(hz), "{hz} Hz: {bars:?}");
            assert!(bars[loudest] > 0.5, "{hz} Hz: {bars:?}");
        }
    }

    #[test]
    fn a_quieter_tone_is_a_lower_bar() {
        let loud = analyze(&sine(1_000.0, 0.5), RATE)[band_of(1_000.0)];
        let quiet = analyze(&sine(1_000.0, 0.005), RATE)[band_of(1_000.0)];
        assert!(quiet < loud, "{quiet} vs {loud}");
        assert!(quiet > 0.0);
    }

    #[test]
    fn the_bands_cover_the_range_without_gaps() {
        assert!((band_edges(0).0 - LOW_HZ).abs() < 0.01);
        assert!((band_edges(BANDS - 1).1 - HIGH_HZ).abs() < 1.0);
        for b in 1..BANDS {
            assert!((band_edges(b).0 - band_edges(b - 1).1).abs() < 0.01);
        }
    }

    #[test]
    fn a_bar_jumps_up_and_falls_slowly() {
        let up = smooth([0.0; BANDS], [0.8; BANDS], Duration::from_millis(30));
        assert_eq!(up[0], 0.8);
        let down = smooth(up, [0.0; BANDS], Duration::from_millis(100));
        assert!(
            (down[0] - (0.8 - FALL_PER_SEC * 0.1)).abs() < 1e-4,
            "{down:?}"
        );
        let gone = smooth(down, [0.0; BANDS], Duration::from_secs(2));
        assert_eq!(gone[0], 0.0);
    }

    #[test]
    fn the_ring_mixes_down_and_keeps_order() {
        let mut ring = Ring::default();
        ring.push_frames(&[1.0, 3.0, 5.0, 7.0], 2);
        let w = ring.window();
        assert_eq!(&w[WINDOW - 2..], &[2.0, 6.0]);
        ring.push_frames(&[1.0], 0);
        assert_eq!(ring.window(), w);
    }

    /// A window holding `hz` with its phase at the window's end chosen so
    /// that the newest samples would start mid-cycle.
    fn tone_window(hz: f32, amp: f32, phase: f32) -> Vec<f32> {
        (0..WINDOW)
            .map(|i| amp * (2.0 * std::f32::consts::PI * hz * i as f32 / RATE as f32 + phase).sin())
            .collect()
    }

    #[test]
    fn the_wave_starts_rising_through_zero_at_any_phase() {
        for phase in [0.0, 1.0, 2.5, 4.0] {
            let w = waveform(&tone_window(200.0, 0.5, phase));
            assert!(w[0].abs() < 0.1, "phase {phase}: starts at {}", w[0]);
            assert!(w[1] > w[0], "phase {phase}: rises: {:?}", &w[..3]);
        }
    }

    #[test]
    fn the_wave_is_scaled_to_its_peak_but_hiss_stays_small() {
        let loud = waveform(&tone_window(200.0, 0.3, 0.0));
        let peak = loud.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.9).abs() < 0.05, "{peak}");
        let quiet = waveform(&tone_window(200.0, 0.01, 0.0));
        let peak = quiet.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.12, "{peak}");
        assert_eq!(waveform(&vec![0.0; WINDOW]), [0.0; WAVE_POINTS]);
        assert_eq!(waveform(&[0.5; 10]), [0.0; WAVE_POINTS], "too short");
    }

    #[test]
    fn only_32_bit_float_mixes_are_read() {
        let float_sub = 0x00000003_0000_0010_8000_00aa00389b71u128;
        let pcm_sub = 0x00000001_0000_0010_8000_00aa00389b71u128;
        assert!(is_float32(3, 32, None));
        assert!(is_float32(0xFFFE, 32, Some(float_sub)));
        assert!(!is_float32(0xFFFE, 32, Some(pcm_sub)));
        assert!(!is_float32(1, 16, None));
        assert!(!is_float32(3, 64, None));
    }
}
