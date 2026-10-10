use crate::lock_ext::LockSafe;
use crate::theme::{self, DialogColors};
use crate::update_check::{self, UpdateInfo, VerifiedInstaller, BUNDLED_CHANGELOG};
use crate::windows::ui_kit;
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
        installer: Arc<VerifiedInstaller>,
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
    /// A check/download is running (`update_flow`) — the button's and the
    /// background loop's never overlap.
    pub busy: bool,
}

impl Default for UpdaterState {
    fn default() -> Self {
        Self {
            status: UpdateStatus::Idle,
            notes_cache: None,
            busy: false,
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
    /// Conventional-commit scope (`**gpu:**` in the changelog), shown bold.
    scope: Option<String>,
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
                let (scope, text) = split_scope(&text);
                sec.items.push(NoteItem { scope, text, hash });
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

/// Splits release-please's leading `**scope:** ` off an item, so it renders
/// as a bold label instead of literal asterisks.
fn split_scope(text: &str) -> (Option<String>, String) {
    text.strip_prefix("**")
        .and_then(|rest| rest.split_once(":**"))
        .filter(|(scope, _)| !scope.is_empty() && !scope.contains("**"))
        .map_or_else(
            || (None, text.to_owned()),
            |(scope, rest)| (Some(scope.to_owned()), rest.trim_start().to_owned()),
        )
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
                    let text = tidy_item_text(&s[..open]);
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
                let text = tidy_item_text(&s[..open]);
                return (text, Some((hash.to_string(), String::new())));
            }
        }
    }

    (strip_md_links(s), None)
}

/// An item's text without its markdown links and issue references:
/// release-please writes `… mode ([#300](url))` before the commit link.
fn tidy_item_text(text: &str) -> String {
    let mut t = strip_md_links(text.trim());
    // "(#300)" left by the link: drop it, the item reads on its own.
    while let Some(open) = t.rfind(" (#") {
        let Some(close) = t[open..].find(')') else {
            break;
        };
        let inner = &t[open + 3..open + close];
        if inner.is_empty() || !inner.chars().all(|c| c.is_ascii_digit()) {
            break;
        }
        t.replace_range(open..open + close + 1, "");
    }
    t.trim().to_owned()
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

                if let Some(scope) = &item.scope {
                    ui.label(
                        egui::RichText::new(format!("{scope}:"))
                            .strong()
                            .color(dc.title)
                            .font(egui::FontId::proportional(12.0)),
                    );
                }
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

// ── Version group ─────────────────────────────────────────────────────────────

/// The installed version and where an update stands, as rows.
fn version_group(ui: &mut egui::Ui, dc: &DialogColors, status: &UpdateStatus) {
    let error_note = match status {
        UpdateStatus::Error(e) => Some(e.as_str()),
        _ => None,
    };
    ui_kit::group(ui, dc, None, error_note, |g| {
        let installed = match status {
            UpdateStatus::JustUpdated { version } => version.clone(),
            _ => VERSION.to_owned(),
        };
        g.row("Installed", None, |ui| {
            ui.label(
                egui::RichText::new(format!("v{installed}"))
                    .size(12.0)
                    .color(dc.text),
            );
        });
        match status {
            UpdateStatus::Ready { info, .. } => {
                g.row(
                    "New version",
                    Some("Downloaded and checked — ready to install"),
                    |ui| ui_kit::status(ui, ui_kit::C_GOOD, &format!("v{}", info.version)),
                );
            }
            UpdateStatus::UpToDate => {
                g.row("Latest version", None, |ui| {
                    ui_kit::status(ui, ui_kit::C_GOOD, "You're up to date");
                });
            }
            UpdateStatus::JustUpdated { version } => {
                g.row("Update", None, |ui| {
                    ui_kit::status(ui, ui_kit::C_GOOD, &format!("Updated to v{version}"));
                });
            }
            UpdateStatus::Checking => {
                g.row("Checking for updates…", None, |ui| {
                    ui.spinner();
                });
            }
            UpdateStatus::Downloading { downloaded, total } => {
                let mb = |b: u64| b as f32 / 1_048_576.0;
                let sub = if *total > 0 {
                    format!("{:.1} of {:.1} MB", mb(*downloaded), mb(*total))
                } else {
                    format!("{:.1} MB", mb(*downloaded))
                };
                g.row("Downloading", Some(&sub), |ui| {
                    let bar = if *total > 0 {
                        egui::ProgressBar::new(*downloaded as f32 / *total as f32)
                    } else {
                        egui::ProgressBar::new(0.0).animate(true)
                    };
                    ui.add_sized([ui_kit::CONTROL_W, 14.0], bar);
                });
            }
            UpdateStatus::Error(_) => {
                g.row("Update", None, |ui| {
                    ui_kit::status(ui, ui_kit::C_BAD, "Couldn't update");
                });
            }
            UpdateStatus::Idle => {
                g.row(
                    "Updates",
                    Some("Checked automatically a few seconds after start and every 6 hours"),
                    |_| {},
                );
            }
        }
    });
}

// ── Window ────────────────────────────────────────────────────────────────────

// The ctx-level panel API, as in every other dialog.
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
    let mut action_install: Option<Arc<VerifiedInstaller>> = None;

    let mut st = state.lock_safe();

    // Reset to Idle on close so "Check for Updates" appears next time the
    // dialog is opened. Keep Ready so a downloaded installer isn't lost.
    let reset_to_idle_on_close = matches!(
        st.status,
        UpdateStatus::UpToDate | UpdateStatus::JustUpdated { .. } | UpdateStatus::Error(_)
    );

    // Parse the changelog at most once per status change (not every frame).
    let has_notes = st.ensure_changelog_cache();

    ui_kit::hero(ctx, dc, "updater", "Updates", |_| {});

    // ── Footer ────────────────────────────────────────────────────────────────
    egui::TopBottomPanel::bottom("updater_footer")
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 10,
            bottom: 12,
        }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match &st.status {
                    UpdateStatus::Ready { installer, .. } => {
                        if theme::dialog_btn_primary(ui, "Install Now").clicked() {
                            action_install = Some(installer.clone());
                        }
                        ui.add_space(6.0);
                        if theme::dialog_btn_secondary(ui, "Later", dc).clicked() {
                            action_close = true;
                        }
                    }
                    UpdateStatus::Idle => {
                        if theme::dialog_btn_primary(ui, "Check for Updates").clicked() {
                            action_check = true;
                        }
                        ui.add_space(6.0);
                        if theme::dialog_btn_secondary(ui, "Close", dc).clicked() {
                            action_close = true;
                        }
                    }
                    UpdateStatus::Error(_) => {
                        if theme::dialog_btn_primary(ui, "Try Again").clicked() {
                            action_check = true;
                        }
                        ui.add_space(6.0);
                        if theme::dialog_btn_secondary(ui, "Close", dc).clicked() {
                            action_close = true;
                        }
                    }
                    _ => {
                        if theme::dialog_btn_primary(ui, "Close").clicked() {
                            action_close = true;
                        }
                    }
                },
            );
        });

    // ── The page ──────────────────────────────────────────────────────────────
    egui::CentralPanel::default()
        .frame(ui_kit::dialog_frame(dc).inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 16,
            bottom: 8,
        }))
        .show(ctx, |ui| {
            version_group(ui, dc, &st.status);
            if let (true, Some((_, entries))) = (has_notes, &st.notes_cache) {
                // Minus the group heading, the card's padding and the gap below it.
                let height = (ui.available_height() - 72.0).max(120.0);
                ui_kit::group(ui, dc, Some("What's new"), None, |g| {
                    g.block(|ui| {
                        // A fixed region the size of the card: long note
                        // lines wrap (or clip) inside it instead of widening
                        // the card past the page margin.
                        // The scrollbar sits beside the content, so leave room for it.
                        let bar = ui.spacing().scroll.allocated_width();
                        let size = egui::vec2(ui.available_width() - bar, height);
                        ui.allocate_ui_with_layout(
                            size,
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
                                egui::ScrollArea::vertical()
                                    .id_salt("updater_notes")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.set_max_width(ui.available_width());
                                        render_notes(ui, dc, entries);
                                    });
                            },
                        );
                    });
                });
            }
        });

    drop(st); // Release lock before applying actions.

    // ── Apply actions ─────────────────────────────────────────────────────────
    if action_check {
        state.lock_safe().status = UpdateStatus::Checking;
    }
    if let Some(installer) = action_install {
        if let Err(e) = update_check::launch_installer(&installer) {
            state.lock_safe().status = UpdateStatus::Error(e);
        }
    }
    if action_close || ctx.input(|i| i.viewport().close_requested()) {
        if reset_to_idle_on_close {
            state.lock_safe().status = UpdateStatus::Idle;
        }
        open.store(false, Ordering::Relaxed);
        main_ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_scope_takes_the_bold_scope_off_the_front() {
        assert_eq!(
            split_scope("**gpu:** select displayed GPU from tray menu"),
            (
                Some("gpu".to_owned()),
                "select displayed GPU from tray menu".to_owned()
            )
        );
        assert_eq!(split_scope("plain item"), (None, "plain item".to_owned()));
        assert_eq!(
            split_scope("**Breaking:** changed"),
            (Some("Breaking".to_owned()), "changed".to_owned())
        );
        // Bold text that isn't a scope prefix is left alone.
        assert_eq!(
            split_scope("**bold** text"),
            (None, "**bold** text".to_owned())
        );
    }

    #[test]
    fn issue_links_before_the_commit_link_are_dropped() {
        let notes = parse_notes(
            "## [1.45.1](https://x) (2026-10-09)

### Bug Fixes

* **wallpaper:** tray hover card keeps updating ([#300](https://github.com/x/issues/300)) ([f104ba6](https://github.com/x/commit/f104ba6))
",
        );
        let item = &notes[0].sections[0].items[0];
        assert_eq!(item.text, "tray hover card keeps updating");
        assert_eq!(item.hash.as_ref().map(|h| h.0.as_str()), Some("f104ba6"));
    }

    #[test]
    fn parsed_items_carry_scope_text_and_hash_separately() {
        let notes = parse_notes(
            "## [1.41.0](https://x) (2026-09-29)\n\n### Features\n\n* **gpu:** select displayed GPU ([417c8db](https://github.com/x/commit/417c8db))\n",
        );
        let item = &notes[0].sections[0].items[0];
        assert_eq!(item.scope.as_deref(), Some("gpu"));
        assert_eq!(item.text, "select displayed GPU");
        assert_eq!(item.hash.as_ref().map(|h| h.0.as_str()), Some("417c8db"));
    }
}
