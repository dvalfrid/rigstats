#![allow(unsafe_code)]
//! Per-process GPU engine utilisation.
//!
//! Reads the Windows **`\GPU Engine(*)\Utilization Percentage`** PDH counter set —
//! the exact data source Task Manager's per-process "GPU" column uses — and maps
//! each sample's adapter LUID to a physical GPU name via DXGI. Vendor-neutral
//! (NVIDIA / AMD / Intel) and readable without elevation, so it needs neither the
//! sensor sidecar nor admin rights.
//!
//! `#![allow(unsafe_code)]`: the module is a thin FFI shim over `pdh.dll` and
//! `dxgi.dll`. Every `unsafe` block is a single Win32 call whose contract is
//! noted inline; no raw pointer escapes the function that creates it.
//!
//! Deviation from issue #182: the issue proposed `rigstats-backend/src/gpu_process.rs`,
//! but `rigstats-backend` carries no raw Win32 FFI (it goes through the `wmi`
//! crate) and denies `unsafe`. All hand-rolled Win32 in this workspace already
//! lives in `src-egui` next to `poll.rs`, so the module lands here instead.

use std::collections::{BTreeSet, HashMap};

/// `(HighPart, LowPart)` of an adapter LUID — the identity carried in a PDH
/// `\GPU Engine` instance name and matched against DXGI's `AdapterLuid`.
pub type Luid = (i32, u32);

/// PID → process display name (from `sysinfo`).
pub type ProcNames = HashMap<u32, String>;

/// LUID → physical adapter display name (from DXGI).
pub type AdapterMap = HashMap<Luid, String>;

/// One process's aggregate GPU usage on a single physical adapter.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuProcessInfo {
    pub pid: u32,
    pub name: String,
    /// Max utilisation across this process's engines on this adapter, in percent
    /// (Task Manager's "GPU %" is the max, not the sum — summing double-counts).
    pub util_pct: f64,
    /// Distinct engine types touched, compact labels ("3D", "Decode", ...).
    pub engines: Vec<String>,
    /// Physical GPU display name, or `""` when the LUID had no DXGI match.
    pub adapter: String,
}

// ── Instance-name parsing (pure — unit-tested) ────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
struct ParsedInstance {
    pid: u32,
    luid: Luid,
    /// Compact engine label.
    engine: String,
}

/// Parse a `\GPU Engine(...)` PDH instance name.
///
/// Shape: `pid_<PID>_luid_0x<HighPart>_0x<LowPart>_phys_<N>_eng_<N>_engtype_<TYPE>`
/// `<TYPE>` is open-ended (`3D`, `Copy`, `VideoDecode`, `Compute_0`, `Graphics_1`,
/// `Security`, ...); unknown suffixes are tolerated and a trailing `_<digits>`
/// ordinal is stripped. Returns `None` for any shape that doesn't fit.
fn parse_instance(name: &str) -> Option<ParsedInstance> {
    let rest = name.strip_prefix("pid_")?;
    let (pid_s, rest) = rest.split_once("_luid_")?;
    let pid: u32 = pid_s.parse().ok()?;
    let (high_s, rest) = rest.split_once('_')?;
    let (low_s, rest) = rest.split_once("_phys_")?;
    let high = parse_hex_u32(high_s)? as i32;
    let low = parse_hex_u32(low_s)?;
    let engine_raw = rest.split_once("_engtype_").map(|(_, t)| t)?;
    if engine_raw.is_empty() {
        return None;
    }
    Some(ParsedInstance {
        pid,
        luid: (high, low),
        engine: compact_engine(engine_raw),
    })
}

fn parse_hex_u32(s: &str) -> Option<u32> {
    let hex = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))?;
    u32::from_str_radix(hex, 16).ok()
}

/// Shorten a raw `engtype_` token to something that fits a narrow panel column.
fn compact_engine(raw: &str) -> String {
    // Drop a trailing "_<digits>" ordinal ("Compute_0" -> "Compute").
    let base = match raw.rsplit_once('_') {
        Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => raw,
    };
    match base {
        "3D" => "3D",
        "VideoDecode" => "Decode",
        "VideoEncode" => "Encode",
        "VideoProcessing" => "VideoProc",
        "Compute" => "Compute",
        "Copy" => "Copy",
        "Security" => "Security",
        "GraphicsInternal" | "Graphics" => "Graphics",
        other => other,
    }
    .to_string()
}

// ── Aggregation (pure — unit-tested) ──────────────────────────────────────────

/// Collapse raw `(instance-name, value)` samples into one row per
/// `(pid, adapter)`, taking the max utilisation across engines and collecting the
/// distinct engine set. `proc_names` maps PID -> process name (from `sysinfo`);
/// `luid_map` maps LUID -> adapter name (from DXGI). Sorted by utilisation desc.
fn aggregate(
    samples: &[(String, f64)],
    proc_names: &ProcNames,
    luid_map: &AdapterMap,
) -> Vec<GpuProcessInfo> {
    // (pid, luid) -> (max util, engine set)
    let mut acc: HashMap<(u32, Luid), (f64, BTreeSet<String>)> = HashMap::new();
    for (instance, value) in samples {
        let Some(p) = parse_instance(instance) else {
            continue;
        };
        let value = if value.is_finite() && *value > 0.0 {
            value.min(100.0)
        } else {
            0.0
        };
        let entry = acc.entry((p.pid, p.luid)).or_insert((0.0, BTreeSet::new()));
        entry.0 = entry.0.max(value);
        entry.1.insert(p.engine);
    }

    let mut rows: Vec<GpuProcessInfo> = acc
        .into_iter()
        .map(|((pid, luid), (util_pct, engines))| GpuProcessInfo {
            pid,
            name: proc_names
                .get(&pid)
                .cloned()
                .unwrap_or_else(|| format!("PID {pid}")),
            util_pct,
            engines: engines.into_iter().collect(),
            adapter: luid_map.get(&luid).cloned().unwrap_or_default(),
        })
        .collect();
    rows.sort_by(|a, b| {
        b.util_pct
            .partial_cmp(&a.util_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    rows
}

// ── PDH query ────────────────────────────────────────────────────────────────

#[cfg(windows)]
mod win {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::{null, null_mut};
    use winapi::shared::winerror::{ERROR_SUCCESS, SUCCEEDED};
    use winapi::um::pdh::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
    };

    // pdhmsg.h — not re-exported by the `winapi` crate.
    const PDH_MORE_DATA: i32 = 0x800007D2u32 as i32;

    /// Live handle to the wildcard `\GPU Engine(*)\Utilization Percentage` query.
    ///
    /// PDH utilisation counters need two `PdhCollectQueryData` calls spaced in
    /// time (the first yields 0), so the query is created once and kept across
    /// poll ticks. `Send`: PDH handles carry no thread affinity and this handle
    /// is only ever touched from the single `poll_loop` task.
    pub struct GpuEngineQuery {
        query: PDH_HQUERY,
        counter: PDH_HCOUNTER,
    }

    unsafe impl Send for GpuEngineQuery {}

    impl GpuEngineQuery {
        pub fn new() -> Option<Self> {
            let mut query: PDH_HQUERY = null_mut();
            // SAFETY: out-param written on success; checked before use.
            let rc = unsafe { PdhOpenQueryW(null(), 0, &mut query) };
            if rc != ERROR_SUCCESS as i32 {
                return None;
            }
            let path: Vec<u16> = std::ffi::OsStr::new(r"\GPU Engine(*)\Utilization Percentage")
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let mut counter: PDH_HCOUNTER = null_mut();
            // SAFETY: `query` is a valid handle from PdhOpenQueryW; `path` is
            // NUL-terminated. English counter name -> works on localised Windows.
            let rc = unsafe { PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) };
            if rc != ERROR_SUCCESS as i32 {
                // SAFETY: `query` is a valid handle we own.
                unsafe { PdhCloseQuery(query) };
                return None;
            }
            // Prime the query so the next collect produces real deltas.
            // SAFETY: valid handle.
            unsafe { PdhCollectQueryData(query) };
            Some(Self { query, counter })
        }

        /// Collect one sample and fold it into per-process rows. Returns an empty
        /// vec on any PDH error or when no GPU engine is currently active.
        pub fn sample(&self, proc_names: &ProcNames, luid_map: &AdapterMap) -> Vec<GpuProcessInfo> {
            // SAFETY: valid handle.
            if unsafe { PdhCollectQueryData(self.query) } != ERROR_SUCCESS as i32 {
                return Vec::new();
            }

            let mut buf_size: u32 = 0;
            let mut item_count: u32 = 0;
            // SAFETY: size-probe call — null buffer, PDH writes the two out sizes.
            let rc = unsafe {
                PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_DOUBLE,
                    &mut buf_size,
                    &mut item_count,
                    null_mut(),
                )
            };
            if rc != PDH_MORE_DATA || buf_size == 0 || item_count == 0 {
                return Vec::new();
            }

            // PDH packs the item array plus its trailing UTF-16 string data into
            // one buffer sized `buf_size` *bytes*. Allocate it as a `Vec` of the
            // item type (not `Vec<u8>`) so the block is correctly aligned for the
            // pointer/union fields; the string bytes land in the slack capacity.
            let item_sz = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let slots = (buf_size as usize)
                .div_ceil(item_sz)
                .max(item_count as usize);
            let mut buf: Vec<PDH_FMT_COUNTERVALUE_ITEM_W> = Vec::with_capacity(slots);
            // SAFETY: `buf` has room for `buf_size` bytes at a correctly aligned
            // address; PDH fills it. The `Vec` len stays 0, so only PDH's data —
            // never uninitialised `Vec` slots — is ever read (`item_count` bound).
            let rc = unsafe {
                PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_DOUBLE,
                    &mut buf_size,
                    &mut item_count,
                    buf.as_mut_ptr(),
                )
            };
            if rc != ERROR_SUCCESS as i32 {
                return Vec::new();
            }

            // SAFETY: PDH initialised `item_count` items at the buffer head.
            let items = unsafe { std::slice::from_raw_parts(buf.as_ptr(), item_count as usize) };
            let mut samples: Vec<(String, f64)> = Vec::with_capacity(items.len());
            for item in items {
                if item.szName.is_null() {
                    continue;
                }
                // SAFETY: szName points into `buf`'s trailing string region and
                // is NUL-terminated by PDH.
                let name = unsafe { wide_to_string(item.szName) };
                // SAFETY: union read — we requested PDH_FMT_DOUBLE, so the
                // `doubleValue` arm is the populated one.
                let value = unsafe { *item.FmtValue.u.doubleValue() };
                samples.push((name, value));
            }
            aggregate(&samples, proc_names, luid_map)
        }
    }

    impl Drop for GpuEngineQuery {
        fn drop(&mut self) {
            // SAFETY: `query` is a valid handle we own; closing also frees its
            // counters.
            unsafe { PdhCloseQuery(self.query) };
        }
    }

    /// Walk a NUL-terminated UTF-16 string into a `String`.
    ///
    /// # Safety
    /// `ptr` must be non-null and point to a NUL-terminated UTF-16 sequence.
    unsafe fn wide_to_string(ptr: *const u16) -> String {
        let mut len = 0usize;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
    }

    /// Build a `LUID -> adapter display name` map by enumerating DXGI adapters.
    ///
    /// Software adapters (WARP / "Microsoft Basic Render Driver") are skipped.
    /// Returns an empty map on any DXGI failure — callers then leave the
    /// `adapter` field blank rather than failing the whole sample.
    pub fn adapter_luid_map() -> AdapterMap {
        use winapi::shared::dxgi::{
            CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, IID_IDXGIFactory1,
            DXGI_ADAPTER_DESC1, DXGI_ADAPTER_FLAG_SOFTWARE,
        };

        let mut map = HashMap::new();
        let mut factory: *mut IDXGIFactory1 = null_mut();
        // SAFETY: standard DXGI factory creation; out-param checked via HRESULT.
        let hr = unsafe {
            CreateDXGIFactory1(
                &IID_IDXGIFactory1,
                &mut factory as *mut *mut IDXGIFactory1 as *mut *mut winapi::ctypes::c_void,
            )
        };
        if !SUCCEEDED(hr) || factory.is_null() {
            return map;
        }

        let mut i = 0u32;
        loop {
            let mut adapter: *mut IDXGIAdapter1 = null_mut();
            // SAFETY: `factory` is a live COM pointer; `adapter` out-param.
            let hr = unsafe { (*factory).EnumAdapters1(i, &mut adapter) };
            if !SUCCEEDED(hr) || adapter.is_null() {
                break;
            }
            let mut desc: DXGI_ADAPTER_DESC1 = unsafe { std::mem::zeroed() };
            // SAFETY: `adapter` is a live COM pointer; `desc` is adapter-owned out.
            let hr = unsafe { (*adapter).GetDesc1(&mut desc) };
            if SUCCEEDED(hr) && desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE == 0 {
                let end = desc
                    .Description
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(desc.Description.len());
                let name = String::from_utf16_lossy(&desc.Description[..end]);
                map.insert(
                    (desc.AdapterLuid.HighPart, desc.AdapterLuid.LowPart),
                    name.trim().to_string(),
                );
            }
            // SAFETY: release our reference from EnumAdapters1.
            unsafe { (*adapter).Release() };
            i += 1;
        }
        // SAFETY: release our factory reference from CreateDXGIFactory1.
        unsafe { (*factory).Release() };
        map
    }
}

#[cfg(windows)]
pub use win::{adapter_luid_map, GpuEngineQuery};

// Non-Windows stub so the crate still type-checks off Windows (tests, CI lint).
#[cfg(not(windows))]
pub struct GpuEngineQuery;

#[cfg(not(windows))]
impl GpuEngineQuery {
    pub fn new() -> Option<Self> {
        None
    }
    pub fn sample(&self, _proc_names: &ProcNames, _luid_map: &AdapterMap) -> Vec<GpuProcessInfo> {
        Vec::new()
    }
}

#[cfg(not(windows))]
pub fn adapter_luid_map() -> AdapterMap {
    HashMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_instance_name() {
        let p = parse_instance("pid_1234_luid_0x00000000_0x0000A1B2_phys_0_eng_3_engtype_3D")
            .expect("should parse");
        assert_eq!(p.pid, 1234);
        assert_eq!(p.luid, (0, 0xA1B2));
        assert_eq!(p.engine, "3D");
    }

    #[test]
    fn parses_engtype_with_ordinal_suffix() {
        let p = parse_instance("pid_9_luid_0x00000000_0x00001111_phys_0_eng_0_engtype_Compute_0")
            .unwrap();
        assert_eq!(p.engine, "Compute");
        let p = parse_instance("pid_9_luid_0x00000000_0x00001111_phys_1_eng_2_engtype_Graphics_1")
            .unwrap();
        assert_eq!(p.engine, "Graphics");
    }

    #[test]
    fn tolerates_unknown_engtype() {
        let p = parse_instance("pid_5_luid_0x00000000_0x00002222_phys_0_eng_9_engtype_Wizardry")
            .unwrap();
        assert_eq!(p.engine, "Wizardry");
    }

    #[test]
    fn maps_video_engines_to_short_labels() {
        assert_eq!(compact_engine("VideoDecode"), "Decode");
        assert_eq!(compact_engine("VideoEncode"), "Encode");
        assert_eq!(compact_engine("VideoProcessing"), "VideoProc");
    }

    #[test]
    fn negative_high_part_luid_round_trips() {
        let p = parse_instance("pid_1_luid_0xFFFFFFFF_0x0000ABCD_phys_0_eng_0_engtype_3D").unwrap();
        assert_eq!(p.luid, (-1, 0xABCD));
    }

    #[test]
    fn rejects_malformed_names() {
        assert!(parse_instance("").is_none());
        assert!(parse_instance("pid_abc_luid_0x0_0x0_phys_0_eng_0_engtype_3D").is_none());
        assert!(parse_instance("pid_1_luid_0x0_0x0_phys_0_eng_0").is_none());
        assert!(parse_instance("GPU Engine total").is_none());
        assert!(parse_instance("pid_1_luid_0xZZ_0x0_phys_0_eng_0_engtype_3D").is_none());
    }

    #[test]
    fn aggregate_takes_engine_max_and_collects_set() {
        let samples = vec![
            (
                "pid_100_luid_0x00000000_0x00000001_phys_0_eng_0_engtype_3D".to_string(),
                20.0,
            ),
            (
                "pid_100_luid_0x00000000_0x00000001_phys_0_eng_1_engtype_VideoDecode".to_string(),
                75.0,
            ),
            (
                "pid_200_luid_0x00000000_0x00000001_phys_0_eng_0_engtype_3D".to_string(),
                5.0,
            ),
        ];
        let mut names = HashMap::new();
        names.insert(100u32, "game.exe".to_string());
        let mut luids = HashMap::new();
        luids.insert((0i32, 1u32), "NVIDIA GeForce RTX 4070".to_string());

        let rows = aggregate(&samples, &names, &luids);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "game.exe");
        assert_eq!(rows[0].util_pct, 75.0);
        assert_eq!(
            rows[0].engines,
            vec!["3D".to_string(), "Decode".to_string()]
        );
        assert_eq!(rows[0].adapter, "NVIDIA GeForce RTX 4070");
        // Unknown PID falls back to a synthetic label; unknown LUID -> blank.
        assert_eq!(rows[1].name, "PID 200");
    }

    #[test]
    fn aggregate_clamps_and_drops_garbage_values() {
        let samples = vec![
            (
                "pid_1_luid_0x00000000_0x00000001_phys_0_eng_0_engtype_3D".to_string(),
                150.0,
            ),
            (
                "pid_2_luid_0x00000000_0x00000001_phys_0_eng_0_engtype_3D".to_string(),
                f64::NAN,
            ),
        ];
        let rows = aggregate(&samples, &HashMap::new(), &HashMap::new());
        assert_eq!(rows[0].util_pct, 100.0);
        assert_eq!(rows[1].util_pct, 0.0);
    }
}
