use crate::lock_ext::LockSafe;
use crate::theme::{self, DialogColors};
use crate::update_check::{self, UpdateInfo, BUNDLED_CHANGELOG};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const VERSION: &str = env!("CARGO_PKG_VERSION");

// ── State machine ─────────────────────────────────────────────────────────────

#[derive(Default)]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    Downloading {
        downloaded: u64,
        total: u64,
    },
    /// A newer version was found and is ready to install.
    Ready {
        info: UpdateInfo,
        installer_path: PathBuf,
    },
    /// Already on the latest version.
    UpToDate,
    /// App was just updated; shown automatically on first launch after install.
    JustUpdated {
        version: String,
    },
    Error(String),
}

pub struct UpdaterState {
    pub status: UpdateStatus,
    /// Cached parsed changelog: (key, entries). Avoids re-parsing the whole
    /// CHANGELOG every frame (notably during scroll, which repaints at input
    /// rate). The key is derived from the current status so the cache is
    /// rebuilt only when the displayed notes actually change.
    notes_cache: Option<(u64, Vec<ReleaseEntry>)>,
}

impl Default for UpdaterState {
    fn default() -> Self {
        Self {
            status: UpdateStatus::Idle,
            notes_cache: None,
        }
    }
}

impl UpdaterState {
    /// Ensure `notes_cache` holds the parsed changelog for the current status.
    /// Returns `true` if the current status renders a changelog panel.
    fn ensure_changelog_cache(&mut self) -> bool {
        use std::hash::{Hash, Hasher};
        let key: u64 = match &self.status {
            UpdateStatus::Ready { info, .. } => {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                1u8.hash(&mut h);
                info.notes.hash(&mut h);
                h.finish()
            }
            UpdateStatus::UpToDate | UpdateStatus::Idle | UpdateStatus::JustUpdated { .. } => 2,
            // Checking / Downloading / Error: no changelog panel.
            _ => return false,
        };
        if self.notes_cache.as_ref().map(|(k, _)| *k) != Some(key) {
            let entries = match &self.status {
                UpdateStatus::Ready { info, .. } => {
                    let combined = if info.notes.is_empty() {
                        BUNDLED_CHANGELOG.to_string()
                    } else {
                        format!("{}\n\n{BUNDLED_CHANGELOG}", info.notes)
                    };
                    parse_notes(&combined)
                }
                _ => parse_notes(BUNDLED_CHANGELOG),
            };
            self.notes_cache = Some((key, entries));
        }
        true
    }
}

// ── Notes parser ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum SectionKind {
    Features,
    BugFixes,
    Other,
}

struct ReleaseEntry {
    version: String,
    version_url: Option<String>,
    date: String,
    sections: Vec<NoteSection>,
}

struct NoteSection {
    kind: SectionKind,
    title: String,
    items: Vec<NoteItem>,
}

struct NoteItem {
    text: String,
    /// Short commit hash and optional URL (for hyperlink rendering).
    hash: Option<(String, String)>,
}

fn parse_notes(markdown: &str) -> Vec<ReleaseEntry> {
    let mut entries: Vec<ReleaseEntry> = Vec::new();
    let mut cur_entry: Option<ReleaseEntry> = None;
    let mut cur_section: Option<NoteSection> = None;

    for raw_line in markdown.lines() {
        let line = raw_line.trim();

        if line.starts_with("## ") {
            flush_section(&mut cur_entry, &mut cur_section);
            if let Some(e) = cur_entry.take() {
                entries.push(e);
            }
            let heading = line.trim_start_matches('#').trim();
            let (version, version_url, date) = parse_version_date(heading);
            // Skip "Unreleased" section
            if version.to_lowercase().contains("unreleased") {
                continue;
            }
            cur_entry = Some(ReleaseEntry {
                version,
                version_url,
                date,
                sections: Vec::new(),
            });
        } else if line.starts_with("### ") {
            flush_section(&mut cur_entry, &mut cur_section);
            let title = line.trim_start_matches('#').trim().to_string();
            let kind = if title.eq_ignore_ascii_case("features") {
                SectionKind::Features
            } else if title.to_lowercase().contains("bug fix") {
                SectionKind::BugFixes
            } else {
                SectionKind::Other
            };
            // Skip "What's Changed" meta-section
            if !title.eq_ignore_ascii_case("what's changed") {
                cur_section = Some(NoteSection {
                    kind,
                    title,
                    items: Vec::new(),
                });
            }
        } else if line.starts_with("* ") || line.starts_with("- ") {
            let raw = &line[2..];
            if raw.starts_with("**Full Changelog**") {
                continue;
            }
            if let Some(sec) = cur_section.as_mut() {
                let (text, hash) = extract_hash(raw);
                sec.items.push(NoteItem { text, hash });
            }
        }
    }

    flush_section(&mut cur_entry, &mut cur_section);
    if let Some(e) = cur_entry.take() {
        entries.push(e);
    }
    entries
}

fn flush_section(entry: &mut Option<ReleaseEntry>, section: &mut Option<NoteSection>) {
    if let (Some(e), Some(sec)) = (entry.as_mut(), section.take()) {
        if !sec.items.is_empty() {
            e.sections.push(sec);
        }
    }
}

/// Parse `## v1.10.1 - 2026-03-26` or `## [1.25.0](url) (2026-03-26)` etc.
/// Returns `(version, version_url, date)`.
fn parse_version_date(s: &str) -> (String, Option<String>, String) {
    // Extract URL from `[text](url)` before stripping.
    let version_url = extract_md_link_url(s);

    let stripped = strip_md_links(s);
    let stripped = stripped.trim();

    // Try "vX.Y.Z - DATE" or "X.Y.Z - DATE"
    for sep in [" - ", " (", "("] {
        if let Some(idx) = stripped.find(sep) {
            let ver = stripped[..idx].trim().trim_start_matches('v');
            let date_raw = stripped[idx + sep.len()..].trim().trim_end_matches(')');
            return (format!("v{ver}"), version_url, date_raw.to_string());
        }
    }
    let ver = stripped.trim_start_matches('v');
    (format!("v{ver}"), version_url, String::new())
}

/// Extract the URL from the first `[text](url)` in `s`, if present.
fn extract_md_link_url(s: &str) -> Option<String> {
    let open = s.find('[')?;
    let after_open = &s[open + 1..];
    let close_bracket = after_open.find("](")?;
    let after_bracket = &after_open[close_bracket + 2..];
    let close_paren = after_bracket.find(')')?;
    let url = &after_bracket[..close_paren];
    if url.is_empty() {
        None
    } else {
        Some(url.to_string())
    }
}

/// Strip `[text](url)` → `text` throughout a string (UTF-8 safe).
fn strip_md_links(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        if let Some(close_bracket) = after_open.find("](") {
            let text = &after_open[..close_bracket];
            let after_bracket = &after_open[close_bracket + 2..];
            if let Some(close_paren) = after_bracket.find(')') {
                out.push_str(text);
                rest = &after_bracket[close_paren + 1..];
                continue;
            }
        }
        out.push('[');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

/// Strip trailing issue references like `, closes [#78](url)` generated by
/// conventional-commits tooling. These appear after the commit hash link and
/// prevent the `))` suffix check from matching.
fn strip_issue_refs(s: &str) -> String {
    for pat in [", closes [", ", fixes [", ", resolves ["] {
        if let Some(idx) = s.rfind(pat) {
            return s[..idx].trim().to_string();
        }
    }
    s.to_string()
}

/// Extract trailing commit hash from an item line.
/// Handles `text ([a6ca37e](url))` and `text (a6ca37e)`.
fn extract_hash(raw: &str) -> (String, Option<(String, String)>) {
    // Strip trailing issue refs (e.g. `, closes [#78](url)`) before hash extraction
    // so the commit link `([hash](url))` is left at the end where the checks expect it.
    let owned = strip_issue_refs(raw.trim());
    let s = owned.trim();

    // Markdown link form: text ([hash](url)) at end
    if s.ends_with("))") {
        if let Some(open) = s.rfind("([") {
            let inner = &s[open + 2..s.len() - 2];
            if let Some(bar) = inner.find("](") {
                let hash = &inner[..bar];
                let url = &inner[bar + 2..];
                if is_hex(hash) {
                    let text = s[..open].trim().to_string();
                    return (text, Some((hash.to_string(), url.to_string())));
                }
            }
        }
    }

    // Plain form: text (hash) at end
    if s.ends_with(')') {
        if let Some(open) = s.rfind('(') {
            let hash = s[open + 1..s.len() - 1].trim();
            if is_hex(hash) && (6..=10).contains(&hash.len()) {
                let text = s[..open].trim().to_string();
                return (text, Some((hash.to_string(), String::new())));
            }
        }
    }

    (strip_md_links(s), None)
}

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit())
}

// ── Notes renderer ────────────────────────────────────────────────────────────

// Semantic section colours — work on both dark and light backgrounds.
const C_FEATURES: egui::Color32 = egui::Color32::from_rgb(70, 162, 88);
const C_BUG_FIXES: egui::Color32 = egui::Color32::from_rgb(182, 152, 64);
const C_OTHER_SECTION: egui::Color32 = egui::Color32::from_gray(128);
const C_BAR_FEAT: egui::Color32 = egui::Color32::from_rgb(50, 136, 70);
const C_BAR_FIX: egui::Color32 = egui::Color32::from_rgb(162, 130, 48);
const C_BAR_OTHER: egui::Color32 = egui::Color32::from_rgb(36, 132, 158);

fn section_color(kind: SectionKind) -> egui::Color32 {
    match kind {
        SectionKind::Features => C_FEATURES,
        SectionKind::BugFixes => C_BUG_FIXES,
        SectionKind::Other => C_OTHER_SECTION,
    }
}

fn bar_color(kind: SectionKind) -> egui::Color32 {
    match kind {
        SectionKind::Features => C_BAR_FEAT,
        SectionKind::BugFixes => C_BAR_FIX,
        SectionKind::Other => C_BAR_OTHER,
    }
}

fn render_notes(ui: &mut egui::Ui, dc: &DialogColors, entries: &[ReleaseEntry]) {
    if entries.is_empty() {
        ui.label(
            egui::RichText::new("No release notes available.")
                .small()
                .color(dc.muted),
        );
        return;
    }

    for entry in entries {
        ui.horizontal(|ui| {
            match &entry.version_url {
                Some(url) => {
                    ui.style_mut().visuals.hyperlink_color = dc.text;
                    ui.hyperlink_to(egui::RichText::new(&entry.version).strong(), url);
                }
                None => {
                    ui.label(egui::RichText::new(&entry.version).strong().color(dc.text));
                }
            }
            if !entry.date.is_empty() {
                ui.label(egui::RichText::new(&entry.date).small().color(dc.muted));
            }
        });
        ui.add_space(2.0);

        for section in &entry.sections {
            ui.label(
                egui::RichText::new(&section.title)
                    .small()
                    .color(section_color(section.kind)),
            );

            for item in &section.items {
                render_item(ui, dc, item, bar_color(section.kind));
            }
            ui.add_space(2.0);
        }

        ui.add_space(6.0);
    }
}

fn render_item(ui: &mut egui::Ui, dc: &DialogColors, item: &NoteItem, bar: egui::Color32) {
    let bar_w = 3.0;
    let gap = 6.0;
    let text_w = (ui.available_width() - bar_w - gap - 4.0).max(10.0);

    ui.horizontal_top(|ui| {
        let ph_h = ui.text_style_height(&egui::TextStyle::Small);
        let (ph, _) = ui.allocate_exact_size(egui::vec2(bar_w, ph_h), egui::Sense::hover());
        ui.add_space(gap);

        let resp = ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                ui.set_max_width(text_w);

                ui.label(
                    egui::RichText::new(&item.text)
                        .color(dc.item)
                        .font(egui::FontId::proportional(12.0)),
                );

                if let Some((hash, url)) = &item.hash {
                    if url.is_empty() {
                        ui.label(
                            egui::RichText::new(format!("({hash})"))
                                .color(dc.item_hash)
                                .font(egui::FontId::monospace(11.0)),
                        );
                    } else {
                        ui.style_mut().visuals.hyperlink_color = dc.item_hash;
                        ui.hyperlink_to(
                            egui::RichText::new(format!("({hash})"))
                                .font(egui::FontId::monospace(11.0)),
                            url,
                        );
                    }
                }
            });
        });

        let bar_h = resp.response.rect.height().max(ph_h);
        ui.painter().rect_filled(
            egui::Rect::from_min_size(ph.min, egui::vec2(bar_w, bar_h)),
            0.0,
            bar,
        );
    });
}

// ── Changelog area ────────────────────────────────────────────────────────────

fn render_changelog_panel(
    ui: &mut egui::Ui,
    dc: &DialogColors,
    entries: &[ReleaseEntry],
    scroll_h: f32,
) {
    // "What's New" — free label, part of the dialog, not inside any frame.
    ui.label(
        egui::RichText::new("What's New")
            .size(11.0)
            .strong()
            .color(dc.label),
    );
    ui.add_space(5.0);

    // Scroll area with inset fill — no border stroke.
    egui::Frame::new()
        .fill(dc.inset)
        .corner_radius(egui::CornerRadius::same(4))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(scroll_h)
                .show(ui, |ui| {
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| render_notes(ui, dc, entries));
                });
        });
}

// ── Window ────────────────────────────────────────────────────────────────────

#[allow(deprecated)]
pub fn show(
    ctx: &egui::Context,
    main_ctx: &egui::Context,
    open: &Arc<AtomicBool>,
    needs_focus: &Arc<AtomicBool>,
    state: &Arc<Mutex<UpdaterState>>,
    dc: &DialogColors,
) {
    dc.apply_to_ctx(ctx);
    if needs_focus.swap(false, Ordering::Relaxed) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    // Collect action flags — avoids holding the MutexGuard across UI closures.
    let mut action_close = false;
    let mut action_check = false;
    let mut action_install: Option<PathBuf> = None;

    let mut st = state.lock_safe();

    // Reset to Idle on close so "Check for Updates" appears next time the
    // dialog is opened. Keep Ready so a downloaded installer isn't lost.
    let reset_to_idle_on_close = matches!(
        st.status,
        UpdateStatus::UpToDate | UpdateStatus::JustUpdated { .. } | UpdateStatus::Error(_)
    );

    // Parse the changelog at most once per status change (not every frame).
    st.ensure_changelog_cache();

    // Extract everything we need for display while the guard is held.
    let (heading, heading_color) = match &st.status {
        UpdateStatus::Ready { info, .. } => (
            format!("v{} Available", info.version),
            egui::Color32::from_rgb(88, 200, 88),
        ),
        UpdateStatus::UpToDate => ("Up to Date".to_string(), dc.text),
        UpdateStatus::JustUpdated { version } => (
            format!("Updated to v{version}"),
            egui::Color32::from_rgb(88, 200, 88),
        ),
        UpdateStatus::Error(_) => (
            "Update Error".to_string(),
            egui::Color32::from_rgb(220, 80, 80),
        ),
        _ => ("Updates".to_string(), dc.text),
    };

    let installed_row: Option<String> = match &st.status {
        UpdateStatus::Ready { .. } => Some(format!("v{VERSION}")),
        UpdateStatus::JustUpdated { version } => Some(format!("v{version}")),
        _ => None,
    };

    let status_below: Option<&str> = match &st.status {
        UpdateStatus::UpToDate => Some("You are running the latest version."),
        _ => None,
    };

    // ── Hero (top) ────────────────────────────────────────────────────────────
    egui::TopBottomPanel::top("updater_hero")
        .frame(
            egui::Frame::none()
                .fill(dc.bg)
                .inner_margin(egui::Margin {
                    left: 14,
                    right: 14,
                    top: 14,
                    bottom: 12,
                })
                .stroke(egui::Stroke::new(1.0_f32, dc.card_border)),
        )
        .show_separator_line(true)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(&heading)
                    .size(22.0)
                    .strong()
                    .color(heading_color),
            );
            if let Some(ver) = &installed_row {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Installed:").color(dc.muted));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(ver.as_str()).color(dc.text));
                });
            }
        });

    // ── Footer (bottom) ───────────────────────────────────────────────────────
    egui::TopBottomPanel::bottom("updater_footer")
        .frame(
            egui::Frame::none()
                .fill(dc.bg)
                .inner_margin(egui::Margin {
                    left: 12,
                    right: 12,
                    top: 8,
                    bottom: 10,
                })
                .stroke(egui::Stroke::new(1.0_f32, dc.card_border)),
        )
        .show_separator_line(true)
        .show(ctx, |ui| {
            if let Some(msg) = status_below {
                ui.label(egui::RichText::new(msg).small().color(dc.muted));
                ui.add_space(6.0);
            }
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match &st.status {
                    UpdateStatus::Ready { installer_path, .. } => {
                        let path = installer_path.clone();
                        if theme::dialog_btn_primary(ui, "Install Now").clicked() {
                            action_install = Some(path);
                        }
                        ui.add_space(6.0);
                        if theme::dialog_btn_secondary(ui, "Later", dc).clicked() {
                            action_close = true;
                        }
                    }
                    UpdateStatus::UpToDate | UpdateStatus::JustUpdated { .. } => {
                        if theme::dialog_btn_primary(ui, "Close").clicked() {
                            action_close = true;
                        }
                        ui.add_space(6.0);
                        theme::dialog_btn_secondary_disabled(ui, "Update Now", dc);
                    }
                    UpdateStatus::Idle => {
                        if theme::dialog_btn_primary(ui, "Check for Updates").clicked() {
                            action_check = true;
                        }
                    }
                    UpdateStatus::Error(_)
                        if theme::dialog_btn_primary(ui, "Try again").clicked() =>
                    {
                        action_check = true;
                    }
                    _ => {}
                },
            );
        });

    // ── Central content ───────────────────────────────────────────────────────
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(dc.bg)
                .inner_margin(egui::Margin::same(10)),
        )
        .show(ctx, |ui| match &st.status {
            UpdateStatus::Ready { .. }
            | UpdateStatus::UpToDate
            | UpdateStatus::JustUpdated { .. }
            | UpdateStatus::Idle => {
                if let Some((_, entries)) = &st.notes_cache {
                    render_changelog_panel(ui, dc, entries, ui.available_height());
                }
            }
            UpdateStatus::Checking => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Checking for updates…");
                });
            }
            UpdateStatus::Downloading { downloaded, total } => {
                ui.label("Downloading update…");
                ui.add_space(8.0);
                if *total > 0 {
                    let progress = *downloaded as f32 / *total as f32;
                    ui.add(egui::ProgressBar::new(progress).show_percentage());
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{:.1} / {:.1} MB",
                            *downloaded as f32 / 1_048_576.0,
                            *total as f32 / 1_048_576.0,
                        ))
                        .small()
                        .color(dc.muted),
                    );
                } else {
                    ui.add(egui::ProgressBar::new(0.0).animate(true));
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!(
                            "{:.1} MB downloaded",
                            *downloaded as f32 / 1_048_576.0,
                        ))
                        .small()
                        .color(dc.muted),
                    );
                }
            }
            UpdateStatus::Error(e) => {
                ui.label(
                    egui::RichText::new(format!("Error: {e}"))
                        .color(egui::Color32::from_rgb(220, 80, 80)),
                );
            }
        });

    drop(st); // Release lock before applying actions.

    // ── Apply actions ─────────────────────────────────────────────────────────
    if action_check {
        state.lock_safe().status = UpdateStatus::Checking;
    }
    if let Some(path) = action_install {
        if let Err(e) = update_check::launch_installer(&path) {
            state.lock_safe().status = UpdateStatus::Error(e);
        }
    }
    if action_close {
        if reset_to_idle_on_close {
            state.lock_safe().status = UpdateStatus::Idle;
        }
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    if ctx.input(|i| i.viewport().close_requested()) {
        if reset_to_idle_on_close {
            state.lock_safe().status = UpdateStatus::Idle;
        }
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}
