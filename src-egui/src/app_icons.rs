//! Program icons and names for the app rules (#307), and the "Choose a
//! program" dialog. Windows only: the icon comes from `SHGetFileInfo`, the
//! name from the file's version information ("Counter-Strike 2" rather than
//! `cs2.exe`), the dialog is the common Open dialog on its own thread
//! (also used to choose a profile's desktop background picture, #306).
#![allow(unsafe_code)] // Win32 FFI below.

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

/// Previews of pictures (#306), decoded on a thread of their own — a 4K
/// photo takes a moment — and kept as textures.
pub struct PictureCache {
    map: HashMap<String, Option<egui::TextureHandle>>,
    tx: std::sync::mpsc::Sender<(String, Option<egui::ColorImage>)>,
    rx: std::sync::mpsc::Receiver<(String, Option<egui::ColorImage>)>,
}

impl Default for PictureCache {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            map: HashMap::new(),
            tx,
            rx,
        }
    }
}

impl std::fmt::Debug for PictureCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PictureCache")
            .field("pictures", &self.map.len())
            .finish_non_exhaustive()
    }
}

impl PictureCache {
    /// The preview of the picture at `path`; `None` while it loads or when
    /// it can't be read.
    pub fn get(&mut self, ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
        while let Ok((p, img)) = self.rx.try_recv() {
            let tex = img.map(|img| {
                ctx.load_texture(format!("picture:{p}"), img, egui::TextureOptions::LINEAR)
            });
            self.map.insert(p, tex);
        }
        if path.is_empty() {
            return None;
        }
        if let Some(tex) = self.map.get(path) {
            return tex.clone();
        }
        self.map.insert(path.to_owned(), None);
        let (tx, ctx, path) = (self.tx.clone(), ctx.clone(), path.to_owned());
        let _ = std::thread::Builder::new()
            .name("picture-preview".into())
            .spawn(move || {
                let img = preview(&path);
                let _ = tx.send((path, img));
                ctx.request_repaint();
            });
        None
    }
}

/// The picture at `path`, scaled down to at most 384 × 216.
fn preview(path: &str) -> Option<egui::ColorImage> {
    let img = image::open(path).ok()?.thumbnail(384, 216).to_rgba8();
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
    let size = unsafe { GetFileVersionInfoSizeW(wpath.as_ptr(), &mut handle) };
    if size == 0 {
        return None;
    }
    let mut data = vec![0u8; size as usize];
    if unsafe { GetFileVersionInfoW(wpath.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
        return None;
    }
    let query = |sub: &str| -> Option<(*mut winapi::ctypes::c_void, u32)> {
        let wsub = wide(sub);
        let mut ptr = std::ptr::null_mut();
        let mut len = 0u32;
        let ok = unsafe { VerQueryValueW(data.as_ptr().cast(), wsub.as_ptr(), &mut ptr, &mut len) };
        (ok != 0 && !ptr.is_null() && len > 0).then_some((ptr, len))
    };
    // The first language/code page pair, else US English Unicode.
    let lang = query("\\VarFileInfo\\Translation")
        .map(|(ptr, _)| {
            let pair = unsafe { std::slice::from_raw_parts(ptr as *const u16, 2) };
            format!("{:04x}{:04x}", pair[0], pair[1])
        })
        .unwrap_or_else(|| "040904b0".to_owned());
    let (ptr, len) = query(&format!("\\StringFileInfo\\{lang}\\FileDescription"))?;
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
    let com = unsafe {
        winapi::um::combaseapi::CoInitializeEx(
            std::ptr::null_mut(),
            winapi::um::objbase::COINIT_APARTMENTTHREADED,
        )
    };
    let image = icon_image_com(path);
    // S_OK / S_FALSE: this call initialized (or re-counted) COM — undo it.
    if com >= 0 {
        unsafe { winapi::um::combaseapi::CoUninitialize() };
    }
    image
}

#[cfg(windows)]
fn icon_image_com(path: &str) -> Option<egui::ColorImage> {
    use winapi::um::shellapi::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use winapi::um::wingdi::{
        DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS,
    };
    use winapi::um::winuser::{DestroyIcon, GetDC, GetIconInfo, ReleaseDC, ICONINFO};

    let wpath = wide(path);
    let mut info: SHFILEINFOW = unsafe { std::mem::zeroed() };
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
    let hicon = info.hIcon;
    let mut ii: ICONINFO = unsafe { std::mem::zeroed() };
    let image = (|| {
        if unsafe { GetIconInfo(hicon, &mut ii) } == 0 || ii.hbmColor.is_null() {
            return None;
        }
        let mut bm: BITMAP = unsafe { std::mem::zeroed() };
        let read = unsafe {
            GetObjectW(
                ii.hbmColor.cast(),
                std::mem::size_of::<BITMAP>() as i32,
                (&mut bm as *mut BITMAP).cast(),
            )
        };
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        if read == 0 || w <= 0 || h <= 0 || w > 256 || h > 256 {
            return None;
        }
        let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..unsafe { std::mem::zeroed() }
        };
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        let hdc = unsafe { GetDC(std::ptr::null_mut()) };
        let lines = unsafe {
            GetDIBits(
                hdc,
                ii.hbmColor,
                0,
                h as u32,
                bgra.as_mut_ptr().cast(),
                &mut bmi,
                DIB_RGB_COLORS,
            )
        };
        unsafe { ReleaseDC(std::ptr::null_mut(), hdc) };
        if lines == 0 {
            return None;
        }
        // Old icons carry no alpha: treat them as opaque.
        let has_alpha = bgra.chunks_exact(4).any(|px| px[3] != 0);
        let rgba: Vec<u8> = bgra
            .chunks_exact(4)
            .flat_map(|px| [px[2], px[1], px[0], if has_alpha { px[3] } else { 255 }])
            .collect();
        Some(egui::ColorImage::from_rgba_unmultiplied(
            [w as usize, h as usize],
            &rgba,
        ))
    })();
    unsafe {
        if !ii.hbmColor.is_null() {
            DeleteObject(ii.hbmColor.cast());
        }
        if !ii.hbmMask.is_null() {
            DeleteObject(ii.hbmMask.cast());
        }
        DestroyIcon(hicon);
    }
    image
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
    let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = owner as winapi::shared::windef::HWND;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.lpstrFile = file.as_mut_ptr();
    ofn.nMaxFile = file.len() as u32;
    ofn.lpstrTitle = title.as_ptr();
    ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
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
        let img = preview(path.to_str().unwrap()).expect("a png reads");
        let _ = std::fs::remove_file(&path);
        assert_eq!(img.size, [384, 216]);
        assert_eq!(img.pixels[0], egui::Color32::from_rgb(200, 30, 30));
    }

    #[test]
    fn a_missing_picture_has_no_preview() {
        assert!(preview(r"C:\no\such\picture.jpg").is_none());
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
