//! Program icons and names for the app rules (#307), and the "Choose a
//! program" dialog. Windows only: the icon comes from `SHGetFileInfo`, the
//! name from the file's version information ("Counter-Strike 2" rather than
//! `cs2.exe`), the dialog is the common Open dialog on its own thread
//! (also used to choose a profile's desktop background picture, #306).
#![allow(unsafe_code)] // Win32 FFI below.
#![warn(clippy::undocumented_unsafe_blocks)]

use crate::lock_ext::LockSafe;
use std::collections::HashMap;

/// A program's name and icon, loaded once per path.
#[derive(Default)]
pub struct AppInfoCache {
    map: HashMap<String, (Option<String>, Option<egui::TextureHandle>)>,
}

impl AppInfoCache {
    /// The name and icon of the program at `path`, loading them the first
    /// time (a few milliseconds, on the UI thread).
    pub fn get(
        &mut self,
        ctx: &egui::Context,
        path: &str,
    ) -> (Option<String>, Option<egui::TextureHandle>) {
        self.map
            .entry(path.to_owned())
            .or_insert_with(|| {
                let name = file_description(path);
                let icon = icon_image(path).map(|img| {
                    ctx.load_texture(
                        format!("app-icon:{path}"),
                        img,
                        egui::TextureOptions::LINEAR,
                    )
                });
                (name, icon)
            })
            .clone()
    }
}

/// How big a preview is decoded: a screen's row (up to 384 × 216) or a
/// line in the wallpaper list (up to 128 × 72, a ninth of the memory).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewSize {
    Row,
    List,
}

impl PreviewSize {
    fn max(self) -> (u32, u32) {
        match self {
            PreviewSize::Row => (384, 216),
            PreviewSize::List => (128, 72),
        }
    }

    fn key(self, path: &str) -> String {
        match self {
            PreviewSize::Row => format!("row|{path}"),
            PreviewSize::List => format!("list|{path}"),
        }
    }
}

/// At most this many previews are kept; the least recently used goes first.
const KEEP: usize = 150;
/// At most this many waiting to be decoded; older requests are dropped
/// (their rows ask again when they come back into view).
const QUEUE: usize = 64;

/// The previews loaded or loading, with when each was last asked for.
#[derive(Debug)]
struct Slots<V> {
    map: HashMap<String, Slot<V>>,
    tick: u64,
    keep: usize,
}

#[derive(Debug)]
struct Slot<V> {
    /// `None` while loading, or when it couldn't be read (`loading` false).
    value: Option<V>,
    loading: bool,
    used: u64,
}

impl<V: Clone> Slots<V> {
    fn new(keep: usize) -> Self {
        Self {
            map: HashMap::new(),
            tick: 0,
            keep,
        }
    }

    /// `Some(value)` when known (loaded, or failed: `Some(None)`), marking
    /// it used; `None` when it was never asked for.
    fn get(&mut self, key: &str) -> Option<Option<V>> {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(key).map(|s| {
            s.used = tick;
            s.value.clone()
        })
    }

    fn start(&mut self, key: String) {
        self.tick += 1;
        self.map.insert(
            key,
            Slot {
                value: None,
                loading: true,
                used: self.tick,
            },
        );
    }

    /// Stores what was decoded, then drops the least recently used beyond
    /// the limit — never one still loading.
    fn finish(&mut self, key: &str, value: Option<V>) {
        if let Some(s) = self.map.get_mut(key) {
            s.value = value;
            s.loading = false;
        }
        while self.map.values().filter(|s| !s.loading).count() > self.keep {
            let Some(oldest) = self
                .map
                .iter()
                .filter(|(_, s)| !s.loading)
                .min_by_key(|(_, s)| s.used)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            self.map.remove(&oldest);
        }
    }

    fn forget(&mut self, key: &str) {
        self.map.remove(key);
    }
}

/// A preview to decode: its cache key, the file and the size.
type Job = (String, String, PreviewSize);

/// The decode queue: newest first, so the rows in view load first while
/// the list scrolls.
#[derive(Default)]
struct Pending {
    jobs: Vec<Job>,
    closed: bool,
    ctx: Option<egui::Context>,
}

impl Pending {
    /// Queues `job`; returns the keys of the oldest jobs dropped to stay
    /// within `limit`.
    fn push(&mut self, job: Job, limit: usize) -> Vec<String> {
        self.jobs.push(job);
        let over = self.jobs.len().saturating_sub(limit);
        self.jobs.drain(..over).map(|(key, _, _)| key).collect()
    }

    fn pop_newest(&mut self) -> Option<Job> {
        self.jobs.pop()
    }
}

/// Previews of pictures (#306), decoded one at a time on a thread of their
/// own — a 4K photo takes a moment, and a long Workshop list must not start
/// a thread per row — and kept as textures, at most [`KEEP`] of them.
pub struct PictureCache {
    slots: Slots<egui::TextureHandle>,
    queue: std::sync::Arc<(std::sync::Mutex<Pending>, std::sync::Condvar)>,
    rx: std::sync::mpsc::Receiver<(String, Option<egui::ColorImage>)>,
}

impl Default for PictureCache {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let queue = std::sync::Arc::new((
            std::sync::Mutex::new(Pending::default()),
            std::sync::Condvar::new(),
        ));
        let worker = queue.clone();
        let _ = std::thread::Builder::new()
            .name("picture-preview".into())
            .spawn(move || {
                let (lock, wake) = &*worker;
                loop {
                    let (job, ctx) = {
                        let mut p = lock.lock_safe();
                        loop {
                            if p.closed {
                                return;
                            }
                            if let Some(job) = p.pop_newest() {
                                break (job, p.ctx.clone());
                            }
                            p = wake.wait(p).unwrap_or_else(|e| e.into_inner());
                        }
                    };
                    let (key, path, size) = job;
                    // A broken file must not end the thread for every other.
                    let img = std::panic::catch_unwind(|| preview(&path, size.max()))
                        .ok()
                        .flatten();
                    if tx.send((key, img)).is_err() {
                        return;
                    }
                    if let Some(ctx) = ctx {
                        ctx.request_repaint();
                    }
                }
            });
        Self {
            slots: Slots::new(KEEP),
            queue,
            rx,
        }
    }
}

impl Drop for PictureCache {
    fn drop(&mut self) {
        let (lock, wake) = &*self.queue;
        lock.lock_safe().closed = true;
        wake.notify_all();
    }
}

impl std::fmt::Debug for PictureCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PictureCache")
            .field("pictures", &self.slots.map.len())
            .finish_non_exhaustive()
    }
}

impl PictureCache {
    /// The preview of the picture at `path` for a screen's row; `None`
    /// while it loads or when it can't be read.
    pub fn get(&mut self, ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
        self.get_sized(ctx, path, PreviewSize::Row)
    }

    /// The same, small, for a line in a list.
    pub fn get_small(&mut self, ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
        self.get_sized(ctx, path, PreviewSize::List)
    }

    fn get_sized(
        &mut self,
        ctx: &egui::Context,
        path: &str,
        size: PreviewSize,
    ) -> Option<egui::TextureHandle> {
        while let Ok((key, img)) = self.rx.try_recv() {
            let tex = img.map(|img| {
                ctx.load_texture(format!("picture:{key}"), img, egui::TextureOptions::LINEAR)
            });
            self.slots.finish(&key, tex);
        }
        if path.is_empty() {
            return None;
        }
        let key = size.key(path);
        if let Some(known) = self.slots.get(&key) {
            return known;
        }
        self.slots.start(key.clone());
        let (lock, wake) = &*self.queue;
        let dropped = {
            let mut p = lock.lock_safe();
            p.ctx.get_or_insert_with(|| ctx.clone());
            p.push((key, path.to_owned(), size), QUEUE)
        };
        for key in dropped {
            self.slots.forget(&key);
        }
        wake.notify_one();
        None
    }
}

/// The picture at `path`, scaled down to at most `max`. The format is read
/// from the file itself: Windows' own background can be its
/// `TranscodedWallpaper` copy, which has no extension.
fn preview(path: &str, max: (u32, u32)) -> Option<egui::ColorImage> {
    let img = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    // Only ever smaller: a small picture keeps its size.
    let img = if img.width() > max.0 || img.height() > max.1 {
        img.thumbnail(max.0, max.1)
    } else {
        img
    }
    .to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [img.width() as usize, img.height() as usize],
        img.as_raw(),
    ))
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The program's own name from its version information (`FileDescription`).
#[cfg(windows)]
pub fn file_description(path: &str) -> Option<String> {
    use winapi::um::winver::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    let wpath = wide(path);
    let mut handle = 0u32;
    // SAFETY: `wpath` is a NUL-terminated UTF-16 string that lives across the
    // call; `handle` is a valid out-param.
    let size = unsafe { GetFileVersionInfoSizeW(wpath.as_ptr(), &mut handle) };
    if size == 0 {
        return None;
    }
    let mut data = vec![0u8; size as usize];
    // SAFETY: `data` is exactly `size` bytes, the length passed in, so
    // Windows writes only inside it; `wpath` is NUL-terminated.
    if unsafe { GetFileVersionInfoW(wpath.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
        return None;
    }
    let query = |sub: &str| -> Option<(*mut winapi::ctypes::c_void, u32)> {
        let wsub = wide(sub);
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        // SAFETY: `data` holds the version block GetFileVersionInfoW filled
        // and outlives the call; `wsub` is NUL-terminated; `ptr`/`len` are
        // out-params. The pointer it returns points into `data`.
        let ok = unsafe { VerQueryValueW(data.as_ptr().cast(), wsub.as_ptr(), &mut ptr, &mut len) };
        (ok != 0 && !ptr.is_null() && len > 0).then_some((ptr, len))
    };
    // The first language/code page pair, else US English Unicode.
    let lang = query("\\VarFileInfo\\Translation")
        // Its length is in bytes; a malformed resource can be shorter than
        // the pair read below (#680).
        .filter(|&(_, len)| len >= 4)
        .map(|(ptr, _)| {
            // SAFETY: `ptr` points into `data`, alive for the whole function,
            // and the value is at least 4 bytes (checked above): two u16.
            let pair = unsafe { std::slice::from_raw_parts(ptr as *const u16, 2) };
            format!("{:04x}{:04x}", pair[0], pair[1])
        })
        .unwrap_or_else(|| "040904b0".to_owned());
    let (ptr, len) = query(&format!("\\StringFileInfo\\{lang}\\FileDescription"))?;
    // SAFETY: for a string value VerQueryValueW returns a pointer into `data`
    // (alive for the whole function) and its length in UTF-16 characters, so
    // `len` u16 are readable there.
    let chars = unsafe { std::slice::from_raw_parts(ptr as *const u16, len as usize) };
    let text = String::from_utf16_lossy(chars);
    let text = text.trim_end_matches('\0').trim();
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(not(windows))]
pub fn file_description(_path: &str) -> Option<String> {
    None
}

/// The program's large (32 px) icon as an image.
#[cfg(windows)]
fn icon_image(path: &str) -> Option<egui::ColorImage> {
    // SHGetFileInfo needs COM on this thread (Microsoft's docs). The UI
    // thread already has it; anywhere else, set it up for this call.
    // SAFETY: the reserved first argument must be null, which it is.
    let com = unsafe {
        winapi::um::combaseapi::CoInitializeEx(
            std::ptr::null_mut(),
            winapi::um::objbase::COINIT_APARTMENTTHREADED,
        )
    };
    let image = icon_image_com(path);
    // S_OK / S_FALSE: this call initialized (or re-counted) COM — undo it.
    if com >= 0 {
        // SAFETY: balances the successful CoInitializeEx above on this same
        // thread, after `icon_image_com` has freed everything it made.
        unsafe { winapi::um::combaseapi::CoUninitialize() };
    }
    image
}

/// An icon this code owns, destroyed when dropped — so an early return or a
/// panic can't leak it.
#[cfg(windows)]
struct OwnedIcon(winapi::shared::windef::HICON);

#[cfg(windows)]
impl Drop for OwnedIcon {
    fn drop(&mut self) {
        // SAFETY: the handle came from SHGetFileInfoW with SHGFI_ICON, which
        // hands it to the caller to destroy; this guard is its only owner, so
        // it is destroyed exactly once.
        unsafe { winapi::um::winuser::DestroyIcon(self.0) };
    }
}

/// A bitmap this code owns (one of the two GetIconInfo makes), deleted when
/// dropped. A null handle (an icon without a colour bitmap) is skipped.
#[cfg(windows)]
struct OwnedBitmap(winapi::shared::windef::HBITMAP);

#[cfg(windows)]
impl Drop for OwnedBitmap {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: GetIconInfo creates these bitmaps for the caller to
            // delete; this guard is the only owner and the bitmap is never
            // selected into a DC, so it is deleted exactly once, here.
            unsafe { winapi::um::wingdi::DeleteObject(self.0.cast()) };
        }
    }
}

#[cfg(windows)]
fn icon_image_com(path: &str) -> Option<egui::ColorImage> {
    use winapi::um::shellapi::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use winapi::um::wingdi::{
        GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use winapi::um::winuser::{GetDC, GetIconInfo, ReleaseDC, ICONINFO};

    let wpath = wide(path);
    // SAFETY: SHFILEINFOW is a plain C struct (integers, a handle, u16
    // arrays); all-zero is a valid value.
    let mut info: SHFILEINFOW = unsafe { std::mem::zeroed() };
    // SAFETY: `wpath` is NUL-terminated and outlives the call; `info` is a
    // live SHFILEINFOW and its exact size is passed.
    let got = unsafe {
        SHGetFileInfoW(
            wpath.as_ptr(),
            0,
            &mut info,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if got == 0 || info.hIcon.is_null() {
        return None;
    }
    // Declared before the bitmaps, so dropped after them: colour bitmap,
    // mask, then the icon — the order the hand-written cleanup used.
    let icon = OwnedIcon(info.hIcon);
    // SAFETY: ICONINFO is a plain C struct; all-zero is a valid value.
    let mut ii: ICONINFO = unsafe { std::mem::zeroed() };
    // SAFETY: `icon.0` is the live icon from SHGetFileInfoW; `ii` is a valid
    // out-param. On failure `ii` stays zeroed (no bitmaps to free).
    if unsafe { GetIconInfo(icon.0, &mut ii) } == 0 {
        return None;
    }
    let _mask = OwnedBitmap(ii.hbmMask);
    let color = OwnedBitmap(ii.hbmColor);
    if color.0.is_null() {
        return None;
    }
    // SAFETY: BITMAP is a plain C struct; all-zero is a valid value.
    let mut bm: BITMAP = unsafe { std::mem::zeroed() };
    // SAFETY: `color.0` is a live bitmap owned by `color`; `bm` is a BITMAP
    // and its exact size is passed, so no more than that is written.
    let read = unsafe {
        GetObjectW(
            color.0.cast(),
            std::mem::size_of::<BITMAP>() as i32,
            (&mut bm as *mut BITMAP).cast(),
        )
    };
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    if read == 0 || w <= 0 || h <= 0 || w > 256 || h > 256 {
        return None;
    }
    // SAFETY: BITMAPINFO is a plain C struct; all-zero is a valid value.
    let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
    bmi.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: -h, // top-down
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        // SAFETY: the remaining BITMAPINFOHEADER fields are integers; zero is
        // valid for each.
        ..unsafe { std::mem::zeroed() }
    };
    let mut bgra = vec![0u8; (w * h * 4) as usize];
    // SAFETY: a null HWND asks for the screen's DC, released right below.
    let hdc = unsafe { GetDC(std::ptr::null_mut()) };
    // SAFETY: `bgra` holds exactly w × h 32-bit pixels, what `bmi` describes
    // (32 bpp, `h` lines of width `w`, both at most 256); `color.0` is live
    // and not selected into a DC; a null `hdc` only makes the call fail.
    let lines = unsafe {
        GetDIBits(
            hdc,
            color.0,
            0,
            h as u32,
            bgra.as_mut_ptr().cast(),
            &mut bmi,
            DIB_RGB_COLORS,
        )
    };
    // SAFETY: `hdc` came from GetDC(null) above and is released once.
    unsafe { ReleaseDC(std::ptr::null_mut(), hdc) };
    if lines == 0 {
        return None;
    }
    // Old icons carry no alpha: treat them as opaque.
    let has_alpha = bgra.as_chunks::<4>().0.iter().any(|px| px[3] != 0);
    let rgba: Vec<u8> = bgra
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|px| [px[2], px[1], px[0], if has_alpha { px[3] } else { 255 }])
        .collect();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [w as usize, h as usize],
        &rgba,
    ))
}

#[cfg(not(windows))]
fn icon_image(_path: &str) -> Option<egui::ColorImage> {
    None
}

/// Opens the Open dialog for programs; see [`pick_file`].
pub fn pick_program(
    owner_title: &'static str,
    ctx: egui::Context,
) -> std::sync::mpsc::Receiver<Option<String>> {
    pick_file(
        owner_title,
        "Choose a program",
        "Programs (*.exe)\0*.exe\0\0",
        ctx,
    )
}

/// Opens the Open dialog for pictures Windows can show as the desktop
/// background (#306); see [`pick_file`].
pub fn pick_picture(
    owner_title: &'static str,
    ctx: egui::Context,
) -> std::sync::mpsc::Receiver<Option<String>> {
    pick_file(
        owner_title,
        "Choose a picture",
        "Pictures (*.jpg, *.png, *.bmp, *.webp)\0*.jpg;*.jpeg;*.png;*.bmp;*.webp\0\0",
        ctx,
    )
}

/// Opens the Open dialog on its own thread (it blocks until closed), owned
/// by the window titled `owner_title` so it stays on top of it. `filter` is
/// the dialog's NUL-separated filter list. The chosen path arrives on the
/// returned channel (`None` = cancelled).
fn pick_file(
    owner_title: &'static str,
    title: &'static str,
    filter: &'static str,
    ctx: egui::Context,
) -> std::sync::mpsc::Receiver<Option<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("pick-file".into())
        .spawn(move || {
            let _ = tx.send(open_dialog(owner_title, title, filter));
            ctx.request_repaint();
        });
    rx
}

#[cfg(windows)]
fn open_dialog(owner_title: &str, title: &str, filter: &str) -> Option<String> {
    use winapi::um::commdlg::{
        GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    let filter: Vec<u16> = filter.encode_utf16().collect();
    let title = wide(title);
    let mut file = vec![0u16; 2048];
    let owner = crate::win_opacity::find_hwnd(owner_title);
    // SAFETY: OPENFILENAMEW is a plain C struct (sizes, flags, pointers);
    // all-zero (null pointers) is a valid value.
    let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = owner as winapi::shared::windef::HWND;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = file.as_mut_ptr();
    ofn.nMaxFile = file.len() as u32;
    ofn.lpstrTitle = title.as_ptr();
    ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    // SAFETY: every pointer in `ofn` (`filter`, `file`, `title`) is a local
    // that outlives this blocking call; `filter` ends in a double NUL (the
    // callers' literals), `title` is NUL-terminated, `nMaxFile` is `file`'s
    // length; `owner` is 0 or one of this process's windows.
    if unsafe { GetOpenFileNameW(&mut ofn) } == 0 {
        return None;
    }
    let len = file.iter().position(|&c| c == 0).unwrap_or(file.len());
    Some(String::from_utf16_lossy(&file[..len]))
}

#[cfg(not(windows))]
fn open_dialog(_owner_title: &str, _title: &str, _filter: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod picture_tests {
    use super::*;

    #[test]
    fn a_preview_is_small_and_keeps_its_shape() {
        let path = std::env::temp_dir().join("rigstats-preview-test.png");
        image::RgbaImage::from_pixel(1920, 1080, image::Rgba([200, 30, 30, 255]))
            .save(&path)
            .unwrap();
        let img = preview(path.to_str().unwrap(), (384, 216)).expect("a png reads");
        let _ = std::fs::remove_file(&path);
        assert_eq!(img.size, [384, 216]);
        assert_eq!(img.pixels[0], egui::Color32::from_rgb(200, 30, 30));
    }

    #[test]
    fn a_picture_without_an_extension_still_has_a_preview() {
        // Like Windows' TranscodedWallpaper copy of its own background.
        let png = std::env::temp_dir().join("rigstats-preview-test-2.png");
        let bare = std::env::temp_dir().join("RigstatsTranscodedTest");
        image::RgbaImage::from_pixel(64, 36, image::Rgba([0, 90, 200, 255]))
            .save(&png)
            .unwrap();
        std::fs::rename(&png, &bare).unwrap();
        let img = preview(bare.to_str().unwrap(), (384, 216));
        let _ = std::fs::remove_file(&bare);
        assert_eq!(img.expect("read by its content").size, [64, 36]);
    }

    #[test]
    fn the_least_recently_used_preview_goes_first_but_never_one_loading() {
        let mut slots: Slots<u32> = Slots::new(2);
        for k in ["a", "b"] {
            slots.start(k.into());
            slots.finish(k, Some(1));
        }
        slots.start("loading".into());
        slots.get("a"); // a is used again: b is now the oldest
        slots.start("c".into());
        slots.finish("c", Some(3));
        assert!(slots.get("b").is_none(), "b went");
        assert!(slots.get("a").is_some());
        assert!(slots.get("c").is_some());
        assert!(slots.get("loading").is_some(), "still loading: kept");
    }

    #[test]
    fn a_picture_that_cant_be_read_is_remembered_as_such() {
        let mut slots: Slots<u32> = Slots::new(5);
        slots.start("bad".into());
        slots.finish("bad", None);
        assert_eq!(slots.get("bad"), Some(None), "not asked for again");
        slots.forget("bad");
        assert_eq!(slots.get("bad"), None);
    }

    #[test]
    fn the_queue_serves_the_newest_and_drops_the_oldest() {
        let job = |k: &str| (k.to_owned(), format!("{k}.jpg"), PreviewSize::List);
        let mut q = Pending::default();
        assert!(q.push(job("1"), 2).is_empty());
        assert!(q.push(job("2"), 2).is_empty());
        assert_eq!(q.push(job("3"), 2), ["1"], "the oldest is dropped");
        assert_eq!(q.pop_newest().map(|j| j.0), Some("3".into()));
        assert_eq!(q.pop_newest().map(|j| j.0), Some("2".into()));
        assert!(q.pop_newest().is_none());
    }

    #[test]
    fn a_list_preview_is_smaller_than_a_row_preview() {
        let path = std::env::temp_dir().join("rigstats-preview-test-3.png");
        image::RgbaImage::from_pixel(1920, 1080, image::Rgba([1, 2, 3, 255]))
            .save(&path)
            .unwrap();
        let small = preview(path.to_str().unwrap(), PreviewSize::List.max()).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(small.size, [128, 72]);
        assert_ne!(PreviewSize::List.key("x"), PreviewSize::Row.key("x"));
    }

    #[test]
    fn a_missing_picture_has_no_preview() {
        assert!(preview(r"C:\no\such\picture.jpg", (384, 216)).is_none());
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Notepad is on every Windows machine, the CI runner included.
    fn notepad() -> String {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        format!(r"{root}\System32\notepad.exe")
    }

    #[test]
    fn a_program_has_its_own_name_and_an_icon() {
        let name = file_description(&notepad());
        assert!(name.as_deref().is_some_and(|n| !n.is_empty()), "{name:?}");
        let icon = icon_image(&notepad()).expect("notepad has an icon");
        assert!(icon.size[0] >= 16 && icon.size[1] >= 16, "{:?}", icon.size);
        assert!(icon.pixels.iter().any(|p| p.a() > 0), "not blank");
    }

    #[test]
    fn a_missing_file_has_neither() {
        let gone = r"C:\no\such\program.exe";
        assert_eq!(file_description(gone), None);
    }
}
