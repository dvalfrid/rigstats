//! The running processes — name, CPU, memory, disk I/O — from one Windows
//! call per tick (#463).
//!
//! `NtQuerySystemInformation(SystemProcessInformation)` returns every
//! process in one buffer: its CPU times, working set and I/O byte counts
//! included. sysinfo asks the same call for the list, then opens every
//! process and asks five more calls each (start time, CPU times, system
//! times, I/O counters, memory) — ~1 300 calls a second with ~265
//! processes, 80 % of the poll loop's CPU. Here the one buffer is enough:
//! measured in the poll loop, ~13 M cycles a tick against sysinfo's ~27 M.
//! What is left is the call itself (~90 %) — sysinfo makes it too.
//! It also covers protected processes (System, Memory Compression, …) that
//! sysinfo can't open and counts as zero.
//!
//! Safety: only [`ProcessReader::read`] is `unsafe` — the call itself. The
//! buffer is then parsed in safe Rust by [`parse`], which checks every
//! offset and length against the buffer before using it: a malformed
//! buffer yields fewer entries or an error, never a read outside it. The
//! name's pointer is turned into an offset inside the buffer and dropped if
//! it points anywhere else.
//!
//! The layout is `SYSTEM_PROCESS_INFORMATION` as Windows has shipped it
//! since Vista (64-bit), the one Task Manager, Process Explorer and sysinfo
//! read. `reads_the_real_process_list` and `own_io_and_cpu_show_up_as_rates`
//! check every offset used against what this process knows about itself.

use std::collections::HashMap;
use std::time::Instant;

/// One process as Windows reports it: lifetime totals, not rates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawProcess {
    pub pid: u32,
    /// Image name, e.g. `chrome.exe`; empty for the idle process.
    pub name: String,
    /// When it started (FILETIME, 100 ns) — with the pid, tells a reused pid
    /// from the same process.
    pub create_time: i64,
    /// User + kernel time, 100 ns units.
    pub cpu_time: u64,
    pub working_set: u64,
    pub read_bytes: u64,
    pub write_bytes: u64,
}

/// One process this tick: rates over the time since the last tick.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessUsage {
    pub pid: u32,
    pub name: String,
    /// Share of the whole machine (all logical CPUs), 0–100.
    pub cpu_pct: f32,
    pub working_set: u64,
    /// Bytes read / written since the last tick; 0 on a process's first tick.
    pub read_bytes: u64,
    pub write_bytes: u64,
}

// SYSTEM_PROCESS_INFORMATION (64-bit): the offsets read and its size.
const NEXT_ENTRY_OFFSET: usize = 0;
const CREATE_TIME: usize = 32;
const USER_TIME: usize = 40;
const KERNEL_TIME: usize = 48;
const IMAGE_NAME_LENGTH: usize = 56;
const IMAGE_NAME_BUFFER: usize = 64;
const UNIQUE_PROCESS_ID: usize = 80;
const WORKING_SET_SIZE: usize = 144;
const READ_TRANSFER_COUNT: usize = 232;
const WRITE_TRANSFER_COUNT: usize = 240;
const ENTRY_SIZE: usize = 256;

/// More than any real system: a buffer this large means something is wrong.
const MAX_BUFFER: usize = 64 * 1024 * 1024;
/// Entries parsed at most — guards a buffer whose offsets loop.
const MAX_ENTRIES: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The buffer is shorter than one entry.
    TooShort,
    /// An entry's `NextEntryOffset` points back, onto itself, or past the end.
    BadOffset { at: usize },
}

fn u16_at(buf: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(buf.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(buf: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(buf.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(buf: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(buf.get(at..at + 8)?.try_into().ok()?))
}

/// Parses a `SystemProcessInformation` buffer. `base` is the address the
/// buffer sat at when Windows filled it: the names are pointers into it.
/// Entries before a bad `NextEntryOffset` are kept only if the walk could
/// still end cleanly; any inconsistency returns an error instead of a guess.
pub fn parse(buf: &[u8], base: usize) -> Result<Vec<RawProcess>, ParseError> {
    if buf.len() < ENTRY_SIZE {
        return Err(ParseError::TooShort);
    }
    let mut out = Vec::new();
    let mut at = 0usize;
    loop {
        // The whole fixed part must fit, or the entry can't be read.
        let entry = buf
            .get(at..at + ENTRY_SIZE)
            .ok_or(ParseError::BadOffset { at })?;
        let u64e = |off: usize| u64_at(entry, off).unwrap_or(0);
        let pid = u64e(UNIQUE_PROCESS_ID) as u32;
        out.push(RawProcess {
            pid,
            name: image_name(buf, base, entry),
            create_time: u64e(CREATE_TIME) as i64,
            cpu_time: u64e(USER_TIME).saturating_add(u64e(KERNEL_TIME)),
            working_set: u64e(WORKING_SET_SIZE),
            read_bytes: u64e(READ_TRANSFER_COUNT),
            write_bytes: u64e(WRITE_TRANSFER_COUNT),
        });
        let next = u32_at(entry, NEXT_ENTRY_OFFSET).unwrap_or(0) as usize;
        if next == 0 {
            return Ok(out);
        }
        // Forward by at least one entry, inside the buffer, a bounded number
        // of times.
        if next < ENTRY_SIZE || out.len() >= MAX_ENTRIES {
            return Err(ParseError::BadOffset { at });
        }
        at = at.checked_add(next).ok_or(ParseError::BadOffset { at })?;
        if at >= buf.len() {
            return Err(ParseError::BadOffset { at });
        }
    }
}

/// The entry's `ImageName`, if its pointer and length lie inside `buf`.
fn image_name(buf: &[u8], base: usize, entry: &[u8]) -> String {
    let len = u16_at(entry, IMAGE_NAME_LENGTH).unwrap_or(0) as usize;
    let ptr = u64_at(entry, IMAGE_NAME_BUFFER).unwrap_or(0) as usize;
    if len == 0 || ptr == 0 || !len.is_multiple_of(2) {
        return String::new();
    }
    let Some(start) = ptr.checked_sub(base) else {
        return String::new();
    };
    let Some(bytes) = start.checked_add(len).and_then(|end| buf.get(start..end)) else {
        return String::new();
    };
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&c| u16::from_le_bytes(c))
        .collect();
    String::from_utf16_lossy(&units)
}

/// Turns lifetime totals into per-tick rates. Keeps the last totals per
/// process; pure (the clock is passed in), so it is tested without Windows.
#[derive(Default)]
pub struct UsageTracker {
    last: HashMap<u32, Last>,
    at: Option<Instant>,
}

struct Last {
    create_time: i64,
    cpu_time: u64,
    read_bytes: u64,
    write_bytes: u64,
}

impl UsageTracker {
    /// Rates since the previous call. The idle process (pid 0) is left out —
    /// its "CPU" is the idle time. A process seen for the first time, or a
    /// pid now held by a new process, starts at zero; totals that went
    /// backwards count as zero.
    pub fn update(
        &mut self,
        now: Instant,
        logical_cpus: usize,
        procs: &[RawProcess],
    ) -> Vec<ProcessUsage> {
        // 100 ns units of CPU the whole machine had since the last call.
        let capacity = self
            .at
            .map(|t| now.saturating_duration_since(t).as_nanos() as f64 / 100.0)
            .unwrap_or(0.0)
            * logical_cpus.max(1) as f64;
        self.at = Some(now);
        let mut last = HashMap::with_capacity(procs.len());
        let usage = procs
            .iter()
            .filter(|p| p.pid != 0)
            .map(|p| {
                let before = self
                    .last
                    .get(&p.pid)
                    .filter(|l| l.create_time == p.create_time);
                let (cpu, read, write) = before.map_or((0, 0, 0), |l| {
                    (
                        p.cpu_time.saturating_sub(l.cpu_time),
                        p.read_bytes.saturating_sub(l.read_bytes),
                        p.write_bytes.saturating_sub(l.write_bytes),
                    )
                });
                last.insert(
                    p.pid,
                    Last {
                        create_time: p.create_time,
                        cpu_time: p.cpu_time,
                        read_bytes: p.read_bytes,
                        write_bytes: p.write_bytes,
                    },
                );
                let cpu_pct = if capacity > 0.0 {
                    ((cpu as f64 / capacity) * 100.0).clamp(0.0, 100.0) as f32
                } else {
                    0.0
                };
                ProcessUsage {
                    pid: p.pid,
                    name: p.name.clone(),
                    cpu_pct,
                    working_set: p.working_set,
                    read_bytes: read,
                    write_bytes: write,
                }
            })
            .collect();
        self.last = last;
        usage
    }
}

/// Reads the process list, reusing its buffer between ticks.
#[derive(Default)]
pub struct ProcessReader {
    /// `u64`s so the buffer is 8-byte aligned, as the call requires.
    buf: Vec<u64>,
}

impl ProcessReader {
    /// Every running process, or why there is no list.
    #[cfg(windows)]
    pub fn read(&mut self) -> Result<Vec<RawProcess>, String> {
        const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
        const SYSTEM_PROCESS_INFORMATION: u32 = 5;
        #[link(name = "ntdll", kind = "raw-dylib")]
        extern "system" {
            fn NtQuerySystemInformation(
                class: u32,
                info: *mut std::ffi::c_void,
                len: u32,
                returned: *mut u32,
            ) -> i32;
        }
        if self.buf.is_empty() {
            self.buf = vec![0; 512 * 1024 / 8];
        }
        // The list can grow between asking for the size and reading it, so
        // retry a few times with what Windows said plus room to spare.
        for _ in 0..4 {
            let bytes = self.buf.len() * 8;
            let mut returned = 0u32;
            #[allow(unsafe_code)]
            // SAFETY: Windows writes at most `bytes` bytes into our own
            // buffer and the length it used into `returned`.
            let status = unsafe {
                NtQuerySystemInformation(
                    SYSTEM_PROCESS_INFORMATION,
                    self.buf.as_mut_ptr().cast(),
                    bytes as u32,
                    &mut returned,
                )
            };
            if status == STATUS_INFO_LENGTH_MISMATCH {
                let want = (returned as usize + 64 * 1024).max(bytes * 2);
                if want > MAX_BUFFER {
                    return Err(format!("process list needs {want} bytes"));
                }
                self.buf = vec![0; want.div_ceil(8)];
                continue;
            }
            if status < 0 {
                return Err(format!("NtQuerySystemInformation status {status:#x}"));
            }
            let used = (returned as usize).min(bytes);
            let base = self.buf.as_ptr() as usize;
            #[allow(unsafe_code)]
            // SAFETY: a byte view of our own initialised `u64` buffer, no
            // longer than it (`used <= bytes`); `u8` has no alignment and
            // every bit pattern is valid. Borrowed only while parsing.
            let view = unsafe { std::slice::from_raw_parts(self.buf.as_ptr().cast::<u8>(), used) };
            return parse(view, base).map_err(|e| format!("process list: {e:?}"));
        }
        Err("process list kept growing".into())
    }

    #[cfg(not(windows))]
    pub fn read(&mut self) -> Result<Vec<RawProcess>, String> {
        Err("not supported".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Builds a buffer the way Windows lays it out: entries, each followed
    /// by its name. `base` = the buffer's address, for the name pointers.
    fn buffer(entries: &[(u32, &str, u64, u64, u64)]) -> (Vec<u8>, usize) {
        // Two passes: lay out, then write pointers once the address is known.
        let mut layout = Vec::new();
        let mut size = 0usize;
        for (_, name, ..) in entries {
            let name_len = name.encode_utf16().count() * 2;
            let stride = (ENTRY_SIZE + name_len).next_multiple_of(8);
            layout.push((size, stride));
            size += stride;
        }
        let mut buf = vec![0u8; size];
        let base = buf.as_ptr() as usize;
        for (i, ((pid, name, cpu, ws, io), (at, stride))) in entries.iter().zip(&layout).enumerate()
        {
            let put = |buf: &mut Vec<u8>, off: usize, v: &[u8]| {
                buf[at + off..at + off + v.len()].copy_from_slice(v)
            };
            let next = if i + 1 == entries.len() {
                0
            } else {
                *stride as u32
            };
            put(&mut buf, NEXT_ENTRY_OFFSET, &next.to_le_bytes());
            put(&mut buf, CREATE_TIME, &(1000 + *pid as i64).to_le_bytes());
            put(&mut buf, USER_TIME, &(cpu / 2).to_le_bytes());
            put(&mut buf, KERNEL_TIME, &(cpu - cpu / 2).to_le_bytes());
            put(&mut buf, UNIQUE_PROCESS_ID, &(*pid as u64).to_le_bytes());
            put(&mut buf, WORKING_SET_SIZE, &ws.to_le_bytes());
            put(&mut buf, READ_TRANSFER_COUNT, &io.to_le_bytes());
            put(&mut buf, WRITE_TRANSFER_COUNT, &(io * 2).to_le_bytes());
            let units: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
            if !units.is_empty() {
                put(
                    &mut buf,
                    IMAGE_NAME_LENGTH,
                    &(units.len() as u16).to_le_bytes(),
                );
                put(
                    &mut buf,
                    IMAGE_NAME_BUFFER,
                    &((base + at + ENTRY_SIZE) as u64).to_le_bytes(),
                );
                put(&mut buf, ENTRY_SIZE, &units);
            }
        }
        (buf, base)
    }

    fn raw(pid: u32, create_time: i64, cpu: u64, io: u64) -> RawProcess {
        RawProcess {
            pid,
            name: format!("p{pid}.exe"),
            create_time,
            cpu_time: cpu,
            working_set: 4096,
            read_bytes: io,
            write_bytes: io,
        }
    }

    #[test]
    fn parses_every_entry_and_its_name() {
        let (buf, base) = buffer(&[
            (0, "", 999, 8, 0),
            (4, "System", 50, 16, 7),
            (1234, "chrome.exe", 300, 4096, 10),
        ]);
        let list = parse(&buf, base).unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].name, "", "the idle process has no name");
        assert_eq!(list[1].name, "System");
        assert_eq!(
            list[2],
            RawProcess {
                pid: 1234,
                name: "chrome.exe".into(),
                create_time: 2234,
                cpu_time: 300,
                working_set: 4096,
                read_bytes: 10,
                write_bytes: 20,
            }
        );
    }

    #[test]
    fn a_name_with_non_ascii_letters_survives() {
        let (buf, base) = buffer(&[(8, "spel_åäö_🎮.exe", 1, 1, 1)]);
        assert_eq!(parse(&buf, base).unwrap()[0].name, "spel_åäö_🎮.exe");
    }

    #[test]
    fn rejects_a_buffer_shorter_than_one_entry() {
        assert_eq!(parse(&[0u8; 100], 0), Err(ParseError::TooShort));
        assert_eq!(parse(&[], 0), Err(ParseError::TooShort));
    }

    #[test]
    fn rejects_offsets_inside_the_entry_or_past_the_end() {
        let (mut buf, base) = buffer(&[(4, "a.exe", 1, 1, 1), (8, "b.exe", 1, 1, 1)]);
        // Into its own entry — the walk could revisit entries.
        for next in [1u32, 8, ENTRY_SIZE as u32 - 1] {
            buf[..4].copy_from_slice(&next.to_le_bytes());
            assert!(
                matches!(parse(&buf, base), Err(ParseError::BadOffset { .. })),
                "next {next}"
            );
        }
        // Past the end.
        let past = buf.len() as u32 + 8;
        buf[..4].copy_from_slice(&past.to_le_bytes());
        assert!(matches!(
            parse(&buf, base),
            Err(ParseError::BadOffset { .. })
        ));
    }

    #[test]
    fn rejects_a_last_entry_cut_short() {
        let (buf, base) = buffer(&[(4, "a.exe", 1, 1, 1), (8, "b.exe", 1, 1, 1)]);
        let cut = &buf[..buf.len() - 40];
        assert!(matches!(
            parse(cut, base),
            Err(ParseError::BadOffset { .. })
        ));
    }

    #[test]
    fn a_name_pointing_outside_the_buffer_is_dropped() {
        let (mut buf, base) = buffer(&[(4, "a.exe", 1, 1, 1)]);
        for ptr in [
            1u64,
            (base as u64).wrapping_sub(16),
            (base + buf.len()) as u64,
            u64::MAX,
        ] {
            buf[IMAGE_NAME_BUFFER..IMAGE_NAME_BUFFER + 8].copy_from_slice(&ptr.to_le_bytes());
            assert_eq!(parse(&buf, base).unwrap()[0].name, "", "pointer {ptr:#x}");
        }
    }

    #[test]
    fn a_name_longer_than_the_buffer_or_odd_is_dropped() {
        let (mut buf, base) = buffer(&[(4, "a.exe", 1, 1, 1)]);
        buf[IMAGE_NAME_LENGTH..IMAGE_NAME_LENGTH + 2].copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(parse(&buf, base).unwrap()[0].name, "");
        buf[IMAGE_NAME_LENGTH..IMAGE_NAME_LENGTH + 2].copy_from_slice(&3u16.to_le_bytes());
        assert_eq!(parse(&buf, base).unwrap()[0].name, "");
    }

    #[test]
    fn garbage_never_panics() {
        // A simple xorshift so the test is the same every run.
        let mut x = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..2_000 {
            let len = (x % 2048) as usize;
            let buf: Vec<u8> = (0..len)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    x as u8
                })
                .collect();
            let _ = parse(&buf, buf.as_ptr() as usize);
        }
    }

    #[test]
    fn a_valid_buffer_with_bytes_flipped_never_panics() {
        let (good, base) = buffer(&[
            (4, "System", 50, 16, 7),
            (1234, "chrome.exe", 300, 4096, 10),
            (99, "x.exe", 1, 1, 1),
        ]);
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..20_000 {
            let mut buf = good.clone();
            for _ in 0..(next() % 8 + 1) {
                let at = (next() % buf.len() as u64) as usize;
                buf[at] = next() as u8;
            }
            // `base` is the original's: names may now point anywhere.
            if let Ok(list) = parse(&buf, base) {
                assert!(list.len() <= 3);
            }
        }
    }

    #[test]
    fn first_tick_is_all_zero_then_rates_follow() {
        let mut t = UsageTracker::default();
        let t0 = Instant::now();
        let first = t.update(t0, 4, &[raw(10, 1, 1_000_000, 5_000)]);
        assert_eq!(first[0].cpu_pct, 0.0);
        assert_eq!((first[0].read_bytes, first[0].write_bytes), (0, 0));
        // One second on 4 CPUs = 40 000 000 units; 10 000 000 used = 25 %.
        let next = t.update(
            t0 + Duration::from_secs(1),
            4,
            &[raw(10, 1, 11_000_000, 8_000)],
        );
        assert!((next[0].cpu_pct - 25.0).abs() < 0.01, "{}", next[0].cpu_pct);
        assert_eq!((next[0].read_bytes, next[0].write_bytes), (3_000, 3_000));
    }

    #[test]
    fn the_idle_process_is_left_out() {
        let mut t = UsageTracker::default();
        let t0 = Instant::now();
        t.update(t0, 1, &[raw(0, 0, 0, 0), raw(4, 1, 0, 0)]);
        let next = t.update(
            t0 + Duration::from_secs(1),
            1,
            &[raw(0, 0, 9_000_000, 0), raw(4, 1, 0, 0)],
        );
        assert_eq!(next.iter().map(|p| p.pid).collect::<Vec<_>>(), vec![4]);
    }

    #[test]
    fn a_reused_pid_starts_again_from_zero() {
        let mut t = UsageTracker::default();
        let t0 = Instant::now();
        t.update(t0, 1, &[raw(10, 1, 5_000_000, 1_000_000)]);
        // Same pid, another process: its totals are not a delta of the old one's.
        let next = t.update(
            t0 + Duration::from_secs(1),
            1,
            &[raw(10, 2, 9_000_000, 9_000_000)],
        );
        assert_eq!(next[0].cpu_pct, 0.0);
        assert_eq!(next[0].read_bytes, 0);
    }

    #[test]
    fn totals_going_backwards_count_as_zero_and_cpu_is_capped() {
        let mut t = UsageTracker::default();
        let t0 = Instant::now();
        t.update(t0, 1, &[raw(10, 1, 5_000_000, 1_000)]);
        let back = t.update(t0 + Duration::from_secs(1), 1, &[raw(10, 1, 1_000, 10)]);
        assert_eq!((back[0].cpu_pct, back[0].read_bytes), (0.0, 0));
        // More CPU than the machine had (clock skew): capped at 100 %.
        let over = t.update(
            t0 + Duration::from_secs(2),
            1,
            &[raw(10, 1, 900_000_000, 10)],
        );
        assert_eq!(over[0].cpu_pct, 100.0);
    }

    #[test]
    fn same_instant_twice_gives_zero_not_nan() {
        let mut t = UsageTracker::default();
        let t0 = Instant::now();
        t.update(t0, 1, &[raw(10, 1, 0, 0)]);
        let again = t.update(t0, 1, &[raw(10, 1, 1_000, 0)]);
        assert_eq!(again[0].cpu_pct, 0.0);
    }

    #[test]
    fn a_process_gone_is_forgotten() {
        let mut t = UsageTracker::default();
        let t0 = Instant::now();
        t.update(t0, 1, &[raw(10, 1, 0, 0), raw(11, 1, 0, 0)]);
        t.update(t0 + Duration::from_secs(1), 1, &[raw(10, 1, 0, 0)]);
        assert_eq!(t.last.len(), 1);
    }

    /// The real call on this machine: the test's own process and System are
    /// there, with plausible numbers, and they agree with what Windows'
    /// other API says.
    #[cfg(windows)]
    #[test]
    fn reads_the_real_process_list() {
        let mut reader = ProcessReader::default();
        let list = reader.read().expect("process list");
        assert!(list.len() > 10, "{} processes", list.len());
        let me = list
            .iter()
            .find(|p| p.pid == std::process::id())
            .expect("own process listed");
        let exe = std::env::current_exe().unwrap();
        let exe_name = exe.file_name().unwrap().to_string_lossy();
        assert!(
            me.name.eq_ignore_ascii_case(&exe_name),
            "{} vs {exe_name}",
            me.name
        );
        assert!(
            me.working_set > 1024 * 1024,
            "working set {}",
            me.working_set
        );
        assert!(me.cpu_time > 0);
        // Started a moment ago: the create time is a FILETIME (100 ns since
        // 1601) within the last hour.
        let now_ft = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i64
            / 100
            + 116_444_736_000_000_000;
        let age = now_ft - me.create_time;
        assert!((0..36_000_000_000).contains(&age), "age {age}");
        // The working set agrees with sysinfo's own (separate API) reading.
        let mut sys = sysinfo::System::new();
        sys.refresh_processes_specifics(sysinfo::ProcessRefreshKind::new().with_memory());
        let theirs = sys
            .process(sysinfo::Pid::from_u32(std::process::id()))
            .unwrap()
            .memory() as f64;
        let ratio = me.working_set as f64 / theirs;
        assert!(
            (0.5..2.0).contains(&ratio),
            "{} vs {theirs}",
            me.working_set
        );
        assert!(list.iter().any(|p| p.pid == 4 && p.name == "System"));
        // A second read reuses the buffer and still works.
        assert!(reader.read().unwrap().len() > 10);
    }

    #[cfg(windows)]
    #[test]
    fn own_io_and_cpu_show_up_as_rates() {
        use std::io::{Read, Write};
        let mut reader = ProcessReader::default();
        let mut tracker = UsageTracker::default();
        tracker.update(Instant::now(), 1, &reader.read().unwrap());
        // Write and read back 4 MB, unbuffered by us.
        let path = std::env::temp_dir().join(format!("rigstats-io-{}.bin", std::process::id()));
        let chunk = vec![7u8; 4 * 1024 * 1024];
        std::fs::File::create(&path)
            .unwrap()
            .write_all(&chunk)
            .unwrap();
        let mut back = Vec::new();
        std::fs::File::open(&path)
            .unwrap()
            .read_to_end(&mut back)
            .unwrap();
        let _ = std::fs::remove_file(&path);
        // Burn ~200 ms of CPU on this thread.
        let start = Instant::now();
        let mut x = 0u64;
        while start.elapsed() < Duration::from_millis(200) {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
        }
        std::hint::black_box(x);
        let usage = tracker.update(Instant::now(), 1, &reader.read().unwrap());
        let me = usage.iter().find(|p| p.pid == std::process::id()).unwrap();
        assert!(
            me.write_bytes >= chunk.len() as u64,
            "written {}",
            me.write_bytes
        );
        assert!(
            me.read_bytes >= chunk.len() as u64,
            "read {}",
            me.read_bytes
        );
        // One busy thread on a "1-CPU" machine over ~200 ms: well above 30 %.
        assert!(me.cpu_pct > 30.0, "{}", me.cpu_pct);
    }
}
