//! The design system every dialog is built from: one set of surfaces,
//! type sizes and controls, so Settings, the Control Center and the rest
//! look and behave the same. Modelled on Apple's grouped lists: a page has a
//! title, then groups — a small heading, a rounded card of rows separated by
//! hairlines, an optional footnote — and each row is a title (with an
//! optional subtitle) on the left and its control on the right.
//!
//! Navigation is a sidebar of [`nav_item`]s with coloured [`Icon`]s;
//! choices of two to five short options are a [`segmented`] control, longer
//! lists a [`dropdown`], on/off a [`toggle`]. Colours come from
//! [`DialogColors`], so everything follows Windows' light/dark mode.

use crate::theme::DialogColors;
use egui::text::{LayoutJob, TextFormat};
use egui::{
    pos2, vec2, Align, Color32, CornerRadius, FontId, Layout, Pos2, Rect, Response, RichText,
    Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};
use std::ops::RangeInclusive;

/// Row height without and with a subtitle.
pub const ROW_H: f32 = 40.0;
const ROW_H_SUB: f32 = 50.0;
/// Width of dropdowns and other right-hand controls, so they line up.
pub const CONTROL_W: f32 = 210.0;
pub const SIDEBAR_W: f32 = 200.0;
const CARD_RADIUS: u8 = 10;

// Semantic alert colours — independent of light/dark mode.
pub const C_WARN: Color32 = Color32::from_rgb(255, 180, 30);
pub const C_CRIT: Color32 = Color32::from_rgb(220, 60, 60);

// ── Surfaces ──────────────────────────────────────────────────────────────

/// The background of a dialog's panels; each call site sets its margin.
pub fn dialog_frame(dc: &DialogColors) -> egui::Frame {
    egui::Frame::new()
        .fill(dc.bg)
        .inner_margin(egui::Margin::same(0))
}

/// A rounded card: what a group's rows sit on.
pub fn card_frame(dc: &DialogColors) -> egui::Frame {
    egui::Frame::new()
        .fill(dc.card)
        .stroke(Stroke::new(1.0_f32, dc.card_border))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(egui::Margin::symmetric(14, 4))
}

// The ctx-level panel API, as in every dialog.
#[allow(deprecated)]
/// The dialog's title bar inside the window: big title left, `right`
/// (status, a button) on the right.
pub fn hero(
    ctx: &egui::Context,
    dc: &DialogColors,
    id: &str,
    title: &str,
    right: impl FnOnce(&mut Ui),
) {
    egui::TopBottomPanel::top(format!("{id}_hero"))
        .frame(dialog_frame(dc).inner_margin(egui::Margin {
            left: 20,
            right: 20,
            top: 14,
            bottom: 12,
        }))
        .show_separator_line(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).size(20.0).strong().color(dc.title));
                ui.with_layout(Layout::right_to_left(Align::Center), right);
            });
        });
}

/// A page's title and what it is about, above its groups.
pub fn page_header(ui: &mut Ui, dc: &DialogColors, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).size(18.0).strong().color(dc.title));
    if !subtitle.is_empty() {
        ui.add_space(2.0);
        ui.label(RichText::new(subtitle).size(12.0).color(dc.muted));
    }
    ui.add_space(14.0);
}

/// A small muted line of explanation (a group's footnote, a hint).
pub fn footnote(ui: &mut Ui, dc: &DialogColors, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(11.0).color(dc.muted)).wrap());
}

/// A group: optional heading, a card of rows, optional footnote.
pub fn group(
    ui: &mut Ui,
    dc: &DialogColors,
    header: Option<&str>,
    footer: Option<&str>,
    add: impl FnOnce(&mut Group<'_>),
) {
    if let Some(header) = header {
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new(header).size(12.0).strong().color(dc.label));
        });
        ui.add_space(4.0);
    }
    card_frame(dc).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut g = Group { ui, dc, rows: 0 };
        add(&mut g);
    });
    if let Some(footer) = footer {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            footnote(ui, dc, footer);
        });
    }
    ui.add_space(16.0);
}

/// A card of exactly `size`, for content that scrolls (release notes, a
/// log): the card is allocated and painted first and the content placed
/// inside it, clipped, so nothing in it can make the card wider than the
/// page or taller than `size`.
pub fn fixed_card(ui: &mut Ui, dc: &DialogColors, size: Vec2, add: impl FnOnce(&mut Ui)) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(CARD_RADIUS), dc.card);
    p.rect_stroke(
        rect,
        CornerRadius::same(CARD_RADIUS),
        Stroke::new(1.0_f32, dc.card_border),
        StrokeKind::Inside,
    );
    let inner = rect.shrink2(vec2(14.0, 10.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_clip_rect(inner.intersect(ui.clip_rect()));
    add(&mut child);
}

/// The rows of one [`group`]; each one after the first gets a hairline.
pub struct Group<'a> {
    ui: &'a mut Ui,
    dc: &'a DialogColors,
    rows: usize,
}

impl Group<'_> {
    fn divider(&mut self) {
        if self.rows > 0 {
            let x = self.ui.available_rect_before_wrap().x_range();
            let y = self.ui.cursor().top();
            self.ui
                .painter()
                .hline(x, y, Stroke::new(1.0_f32, self.dc.card_border));
        }
        self.rows += 1;
    }

    /// A row: `title` (and `subtitle`) left, `control` right-aligned.
    pub fn row(&mut self, title: &str, subtitle: Option<&str>, control: impl FnOnce(&mut Ui)) {
        self.divider();
        let dc = self.dc;
        let w = self.ui.available_width();
        // The text is laid out first, at the width left of a typical
        // control, so a wrapping subtitle makes the row taller instead of
        // running into the next one.
        let texts = |ui: &Ui, wrap: f32| {
            let title =
                ui.painter()
                    .layout(title.to_owned(), FontId::proportional(13.0), dc.title, wrap);
            let sub = subtitle.map(|s| {
                ui.painter()
                    .layout(s.to_owned(), FontId::proportional(11.0), dc.muted, wrap)
            });
            (title, sub)
        };
        let text_h = |t: &(
            std::sync::Arc<egui::Galley>,
            Option<std::sync::Arc<egui::Galley>>,
        )| { t.0.size().y + t.1.as_ref().map_or(0.0, |g| g.size().y + 2.0) };
        let estimate = texts(self.ui, (w - CONTROL_W - 16.0).max(80.0));
        let base = if subtitle.is_some() { ROW_H_SUB } else { ROW_H };
        let h = base.max(text_h(&estimate) + 16.0);
        self.ui
            .allocate_ui_with_layout(vec2(w, h), Layout::right_to_left(Align::Center), |ui| {
                ui.set_min_height(h);
                ui.spacing_mut().item_spacing.x = 6.0;
                let row = ui.max_rect();
                control(ui);
                // What the control left, minus a gap.
                let wrap = (ui.available_width() - 12.0).max(60.0);
                let laid = texts(ui, wrap);
                let mut y = row.center().y - text_h(&laid) / 2.0;
                let (title, sub) = laid;
                let title_h = title.size().y;
                ui.painter().galley(pos2(row.left(), y), title, dc.title);
                if let Some(sub) = sub {
                    y += title_h + 2.0;
                    ui.painter().galley(pos2(row.left(), y), sub, dc.muted);
                }
            });
    }

    /// Free-form content as one row (a table, a note).
    pub fn block(&mut self, add: impl FnOnce(&mut Ui)) {
        self.divider();
        self.ui.add_space(10.0);
        add(self.ui);
        self.ui.add_space(10.0);
    }

    pub fn ui(&mut self) -> &mut Ui {
        self.ui
    }
}

// ── Controls ──────────────────────────────────────────────────────────────

/// An on/off switch, animated.
pub fn toggle(ui: &mut Ui, dc: &DialogColors, on: &mut bool) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(40.0, 24.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    if resp.hovered() && ui.is_enabled() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.12);
        let dim = if ui.is_enabled() { 1.0 } else { 0.4 };
        let bg = dc
            .toggle_off
            .lerp_to_gamma(dc.toggle_on, t)
            .gamma_multiply(dim);
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(12), bg);
        let r = rect.height() / 2.0 - 2.5;
        let x = egui::lerp(rect.left() + 12.0..=rect.right() - 12.0, t);
        p.circle_filled(
            pos2(x, rect.center().y + 1.0),
            r,
            Color32::from_black_alpha(50),
        );
        p.circle_filled(
            pos2(x, rect.center().y),
            r,
            Color32::WHITE.gamma_multiply(dim),
        );
    }
    resp
}

/// A segmented control: one of a few short options, all visible at once.
/// Returns whether the value changed.
pub fn segmented<T: PartialEq + Clone>(
    ui: &mut Ui,
    dc: &DialogColors,
    value: &mut T,
    options: &[(T, &str)],
) -> bool {
    let font = FontId::proportional(12.0);
    let seg_w = options
        .iter()
        .map(|(_, l)| {
            ui.painter()
                .layout_no_wrap(l.to_string(), font.clone(), dc.text)
                .size()
                .x
        })
        .fold(0.0_f32, f32::max)
        + 24.0;
    let size = vec2(seg_w * options.len() as f32 + 4.0, 28.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter().clone();
    let track = if dc.is_dark {
        Color32::from_gray(22)
    } else {
        Color32::from_gray(222)
    };
    let raised = if dc.is_dark {
        Color32::from_gray(70)
    } else {
        Color32::WHITE
    };
    p.rect_filled(rect, CornerRadius::same(8), track);
    let mut changed = false;
    for (i, (v, label)) in options.iter().enumerate() {
        let r = Rect::from_min_size(
            pos2(rect.left() + 2.0 + i as f32 * seg_w, rect.top() + 2.0),
            vec2(seg_w, rect.height() - 4.0),
        );
        let id = egui::Id::new(("segmented", rect.min.x as i32, rect.min.y as i32, i));
        let resp = ui.interact(r, id, Sense::click());
        let selected = *value == *v;
        if selected {
            p.rect_filled(r, CornerRadius::same(6), raised);
        } else if resp.hovered() {
            p.rect_filled(r, CornerRadius::same(6), raised.gamma_multiply(0.35));
        }
        if resp.hovered() && ui.is_enabled() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let color = if selected { dc.title } else { dc.muted };
        p.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            font.clone(),
            color,
        );
        if resp.clicked() && !selected {
            *value = v.clone();
            changed = true;
        }
    }
    changed
}

/// A drop-down list of [`CONTROL_W`] width.
pub fn dropdown(
    ui: &mut Ui,
    id: &str,
    selected: impl Into<egui::WidgetText>,
    add: impl FnOnce(&mut Ui),
) -> Response {
    egui::ComboBox::from_id_salt(id)
        .width(CONTROL_W)
        .selected_text(selected)
        .show_ui(ui, add)
        .response
}

/// A slider with its value shown to the right of it, as a percentage.
/// For a row's control (right-to-left layout). Returns whether it changed.
pub fn slider_pct(
    ui: &mut Ui,
    dc: &DialogColors,
    value: &mut f64,
    range: RangeInclusive<f64>,
) -> bool {
    ui.add_sized(
        [40.0, 20.0],
        egui::Label::new(
            RichText::new(format!("{:.0} %", *value * 100.0))
                .size(12.0)
                .color(dc.text),
        ),
    );
    ui.spacing_mut().slider_width = 150.0;
    // Whole percents, and only an actual drag writes the value: a stepped
    // float slider snaps a stored 0.87 to 0.85 the moment it is drawn,
    // which reads as an edit (an "Unsaved changes" nobody made).
    let pct = |v: f64| (v * 100.0).round() as i32;
    let mut p = pct(*value);
    let changed = ui
        .add(
            egui::Slider::new(&mut p, pct(*range.start())..=pct(*range.end()))
                .show_value(false)
                .trailing_fill(true),
        )
        .changed()
        && p != pct(*value);
    if changed {
        *value = f64::from(p) / 100.0;
    }
    changed
}

/// Text field for an optional threshold value; hint "—" when None.
pub fn threshold_field(ui: &mut Ui, value: &mut Option<u8>) {
    let mut text = value.map(|v| v.to_string()).unwrap_or_default();
    let resp = ui.add_sized(
        [48.0, 24.0],
        egui::TextEdit::singleline(&mut text)
            .hint_text("—")
            .horizontal_align(Align::Center)
            .font(egui::TextStyle::Monospace),
    );
    if resp.changed() {
        *value = text.trim().parse::<u8>().ok();
    }
}

/// Small clickable reorder button with a painted triangle arrow.
pub fn arrow_btn(ui: &mut Ui, dc: &DialogColors, down: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
    if ui.is_rect_visible(rect) {
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(5), dc.inner);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let color = if resp.hovered() { dc.title } else { dc.muted };
        let c = rect.center();
        let tri = if down {
            vec![
                pos2(c.x - 5.0, c.y - 3.0),
                pos2(c.x + 5.0, c.y - 3.0),
                pos2(c.x, c.y + 4.0),
            ]
        } else {
            vec![
                pos2(c.x, c.y - 4.0),
                pos2(c.x + 5.0, c.y + 3.0),
                pos2(c.x - 5.0, c.y + 3.0),
            ]
        };
        ui.painter()
            .add(Shape::convex_polygon(tri, color, Stroke::NONE));
    }
    resp
}

/// Shows or hides `key` in `list`; a shown key goes back to its place in
/// `order` (the registry's natural order), not to the end.
pub fn toggle_in_order(list: &mut Vec<String>, key: &str, on: bool, order: &[&str]) {
    let present = list.iter().any(|k| k == key);
    if on && !present {
        let rank = |k: &str| order.iter().position(|o| *o == k).unwrap_or(usize::MAX);
        let at = list
            .iter()
            .position(|k| rank(k) > rank(key))
            .unwrap_or(list.len());
        list.insert(at, key.to_string());
    } else if !on {
        list.retain(|k| k != key);
    }
}

/// Moves `key` one place up (`-1`) or down (`1`) in `list`.
pub fn move_by(list: &mut [String], key: &str, delta: isize) {
    if let Some(i) = list.iter().position(|k| k == key) {
        let j = i as isize + delta;
        if (0..list.len() as isize).contains(&j) {
            list.swap(i, j as usize);
        }
    }
}

/// One row per `(key, label)` in a group: a switch to show it and, while
/// shown, arrows to move it. Shown items come first, in their order; the
/// rest follow in the registry's order. `unavailable` keys are greyed out.
pub fn ordered_rows(
    g: &mut Group<'_>,
    list: &mut Vec<String>,
    items: &[(&str, String)],
    unavailable: impl Fn(&str) -> bool,
) {
    let dc = g.dc;
    let order: Vec<&str> = items.iter().map(|(k, _)| *k).collect();
    let mut rows: Vec<&(&str, String)> = list
        .iter()
        .filter_map(|k| items.iter().find(|(i, _)| i == k))
        .collect();
    rows.extend(items.iter().filter(|(k, _)| !list.iter().any(|l| l == k)));
    let shown = list.len();
    let mut toggle_to: Option<(&str, bool)> = None;
    let mut shift: Option<(&str, isize)> = None;
    for (key, label) in rows {
        let pos = list.iter().position(|k| k == key);
        let off = unavailable(key);
        g.row(label, off.then_some("Not available on this PC"), |ui| {
            ui.add_enabled_ui(!off, |ui| {
                let mut on = pos.is_some();
                if toggle(ui, dc, &mut on).changed() {
                    toggle_to = Some((key, on));
                }
                ui.add_space(8.0);
                if let Some(i) = pos {
                    if i + 1 < shown {
                        if arrow_btn(ui, dc, true).on_hover_text("Move down").clicked() {
                            shift = Some((key, 1));
                        }
                    } else {
                        ui.add_space(22.0);
                    }
                    if i > 0 {
                        if arrow_btn(ui, dc, false).on_hover_text("Move up").clicked() {
                            shift = Some((key, -1));
                        }
                    } else {
                        ui.add_space(22.0);
                    }
                }
            });
        });
    }
    if let Some((key, on)) = toggle_to {
        toggle_in_order(list, key, on, &order);
    }
    if let Some((key, delta)) = shift {
        move_by(list, key, delta);
    }
}

/// Colour circles to pick from, the chosen one ringed. Returns whether the
/// choice changed.
pub fn swatches(
    ui: &mut Ui,
    dc: &DialogColors,
    value: &mut String,
    options: &[(&str, Color32, &str)],
) -> bool {
    let mut changed = false;
    ui.spacing_mut().item_spacing.x = 8.0;
    // Right-to-left row: add in reverse so they read left to right.
    for (key, color, name) in options.iter().rev() {
        let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
        let selected = value == key;
        let p = ui.painter();
        if selected {
            p.circle_stroke(rect.center(), 10.5, Stroke::new(2.0_f32, dc.title));
        }
        p.circle_filled(rect.center(), if selected { 7.0 } else { 9.0 }, *color);
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.on_hover_text(*name).clicked() && !selected {
            *value = key.to_string();
            changed = true;
        }
    }
    changed
}

// ── Navigation ────────────────────────────────────────────────────────────

/// A small heading between groups of sidebar items.
pub fn nav_header(ui: &mut Ui, dc: &DialogColors, text: &str) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(RichText::new(text).size(11.0).strong().color(dc.muted));
    });
    ui.add_space(2.0);
}

/// A sidebar item: coloured icon and label, filled while selected.
pub fn nav_item(
    ui: &mut Ui,
    dc: &DialogColors,
    icon: Icon,
    label: &str,
    selected: bool,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if selected {
            p.rect_filled(rect, CornerRadius::same(7), dc.tab_active);
        } else if resp.hovered() {
            p.rect_filled(rect, CornerRadius::same(7), dc.inner);
        }
        let badge =
            Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(22.0, 22.0));
        icon_badge(p, badge, icon);
        let color = if selected {
            dc.tab_active_text
        } else {
            dc.title
        };
        p.text(
            pos2(rect.left() + 40.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::proportional(13.0),
            color,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.add_space(2.0);
    resp
}

/// Good / attention / bad, for [`status`].
pub const C_GOOD: Color32 = Color32::from_rgb(48, 209, 88);
pub const C_ATTENTION: Color32 = Color32::from_rgb(255, 179, 71);
pub const C_BAD: Color32 = Color32::from_rgb(255, 99, 88);

/// A state as a coloured dot and text ("● Running"). For a row's control.
pub fn status(ui: &mut Ui, color: Color32, text: &str) {
    // Right-to-left row: the text first, so the dot ends up before it.
    ui.label(RichText::new(text).size(12.0).color(color));
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 12.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

/// A two-line item in a sidebar list (a recording session): title, a muted
/// subtitle and an optional coloured dot before the subtitle. Filled while
/// selected.
pub fn list_item(
    ui: &mut Ui,
    dc: &DialogColors,
    title: &str,
    subtitle: &str,
    selected: bool,
    dot: Option<Color32>,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if selected {
            p.rect_filled(rect, CornerRadius::same(7), dc.tab_active);
        } else if resp.hovered() {
            p.rect_filled(rect, CornerRadius::same(7), dc.inner);
        }
        let (title_c, sub_c) = if selected {
            (dc.tab_active_text, dc.tab_active_text.gamma_multiply(0.75))
        } else {
            (dc.title, dc.muted)
        };
        let clip = rect.shrink2(vec2(10.0, 0.0));
        let p = p.with_clip_rect(clip);
        p.text(
            pos2(clip.left(), rect.top() + 14.0),
            egui::Align2::LEFT_CENTER,
            title,
            FontId::proportional(13.0),
            title_c,
        );
        let mut x = clip.left();
        if let Some(c) = dot {
            p.circle_filled(pos2(x + 4.0, rect.top() + 32.0), 3.5, c);
            x += 12.0;
        }
        p.text(
            pos2(x, rect.top() + 32.0),
            egui::Align2::LEFT_CENTER,
            subtitle,
            FontId::proportional(11.0),
            sub_c,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    ui.add_space(2.0);
    resp
}

/// A pill-shaped choice (a profile). Filled while selected.
pub fn chip(ui: &mut Ui, dc: &DialogColors, label: &str, selected: bool) -> Response {
    let font = FontId::proportional(13.0);
    let w = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), dc.text)
        .size()
        .x
        + 30.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w.max(64.0), 30.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let (fill, text) = if selected {
            (dc.tab_active, dc.tab_active_text)
        } else if resp.hovered() {
            (dc.inner, dc.title)
        } else {
            (dc.card, dc.title)
        };
        p.rect_filled(rect, CornerRadius::same(15), fill);
        if !selected {
            p.rect_stroke(
                rect,
                CornerRadius::same(15),
                Stroke::new(1.0_f32, dc.card_border),
                StrokeKind::Inside,
            );
        }
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            font,
            text,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// A round icon-only button (more, add).
pub fn icon_button(ui: &mut Ui, dc: &DialogColors, icon: Icon, tooltip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_filled(
            rect,
            CornerRadius::same(15),
            if resp.hovered() { dc.inner } else { dc.card },
        );
        p.rect_stroke(
            rect,
            CornerRadius::same(15),
            Stroke::new(1.0_f32, dc.card_border),
            StrokeKind::Inside,
        );
        paint_glyph(
            p,
            Rect::from_center_size(rect.center(), vec2(16.0, 16.0)),
            icon,
            dc.title,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(tooltip)
}

/// A clickable summary tile (the Control Center's overview): icon, title,
/// value (up to two lines) and an optional colour dot.
pub fn tile(
    ui: &mut Ui,
    dc: &DialogColors,
    size: Vec2,
    icon: Icon,
    title: &str,
    value: &str,
    dot: Option<Color32>,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let fill = if resp.hovered() { dc.inner } else { dc.card };
        p.rect_filled(rect, CornerRadius::same(CARD_RADIUS), fill);
        let border = if resp.hovered() {
            dc.tab_active.gamma_multiply(0.7)
        } else {
            dc.card_border
        };
        p.rect_stroke(
            rect,
            CornerRadius::same(CARD_RADIUS),
            Stroke::new(1.0_f32, border),
            StrokeKind::Inside,
        );
        let pad = 12.0;
        let badge = Rect::from_min_size(rect.min + vec2(pad, pad), vec2(26.0, 26.0));
        icon_badge(p, badge, icon);
        paint_glyph(
            p,
            Rect::from_center_size(
                pos2(rect.right() - pad - 4.0, badge.center().y),
                vec2(12.0, 12.0),
            ),
            Icon::Chevron,
            dc.muted,
        );
        p.text(
            pos2(badge.right() + 10.0, badge.center().y),
            egui::Align2::LEFT_CENTER,
            title,
            FontId::proportional(12.0),
            dc.muted,
        );
        let mut x = rect.left() + pad;
        let y = badge.bottom() + 10.0;
        if let Some(c) = dot {
            p.circle_filled(pos2(x + 6.0, y + 9.0), 6.0, c);
            x += 18.0;
        }
        let mut job = LayoutJob::single_section(
            value.to_owned(),
            TextFormat::simple(FontId::proportional(14.0), dc.title),
        );
        job.wrap.max_width = rect.right() - pad - x;
        job.wrap.max_rows = 2;
        job.wrap.overflow_character = Some('…');
        let galley = p.layout_job(job);
        p.galley(pos2(x, y), galley, dc.title);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

// ── Icons ─────────────────────────────────────────────────────────────────

/// Line icons painted with shapes (the embedded font has no symbols).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Overview,
    Dashboard,
    Overlay,
    Alerts,
    Power,
    Fans,
    Cpu,
    Gpu,
    Lighting,
    General,
    Display,
    More,
    Plus,
    Chevron,
}

impl Icon {
    /// The badge colour, one per kind of setting (as in Apple's System Settings).
    pub fn tint(self) -> Color32 {
        match self {
            Icon::Overview => Color32::from_rgb(99, 110, 130),
            Icon::Dashboard => Color32::from_rgb(10, 132, 255),
            Icon::Overlay => Color32::from_rgb(175, 82, 222),
            Icon::Alerts => Color32::from_rgb(255, 69, 58),
            Icon::Power => Color32::from_rgb(48, 176, 80),
            Icon::Fans => Color32::from_rgb(50, 173, 200),
            Icon::Cpu => Color32::from_rgb(255, 149, 0),
            Icon::Gpu => Color32::from_rgb(94, 92, 230),
            Icon::Lighting => Color32::from_rgb(255, 55, 95),
            Icon::General => Color32::from_rgb(142, 142, 147),
            Icon::Display => Color32::from_rgb(0, 122, 255),
            Icon::More | Icon::Plus | Icon::Chevron => Color32::from_rgb(142, 142, 147),
        }
    }
}

/// The icon in white on its tinted rounded square.
pub fn icon_badge(p: &egui::Painter, rect: Rect, icon: Icon) {
    p.rect_filled(
        rect,
        CornerRadius::same((rect.width() * 0.26) as u8),
        icon.tint(),
    );
    paint_glyph(p, rect.shrink(rect.width() * 0.2), icon, Color32::WHITE);
}

/// Paints `icon` into the square `rect`.
pub fn paint_glyph(p: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width();
    let c = rect.center();
    let at = |x: f32, y: f32| -> Pos2 { c + vec2(x * s, y * s) };
    let stroke = Stroke::new((s * 0.11).max(1.2), color);
    let fill = |pts: Vec<Pos2>| Shape::convex_polygon(pts, color, Stroke::NONE);
    match icon {
        Icon::Overview => {
            let q = s * 0.36;
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                let r = Rect::from_center_size(at(dx * 0.22, dy * 0.22), vec2(q, q));
                p.rect_filled(r, CornerRadius::same(2), color);
            }
        }
        Icon::Dashboard => {
            let r = Rect::from_center_size(c, vec2(s * 0.6, s * 0.9));
            p.rect_stroke(r, CornerRadius::same(2), stroke, StrokeKind::Inside);
            for dy in [-0.22, 0.0, 0.22] {
                let bar = Rect::from_center_size(at(0.0, dy), vec2(s * 0.3, s * 0.1));
                p.rect_filled(bar, CornerRadius::same(1), color);
            }
        }
        Icon::Overlay => {
            let back = Rect::from_center_size(at(-0.12, -0.12), vec2(s * 0.6, s * 0.6));
            p.rect_stroke(back, CornerRadius::same(2), stroke, StrokeKind::Inside);
            let front = Rect::from_center_size(at(0.14, 0.14), vec2(s * 0.6, s * 0.6));
            p.rect_filled(front, CornerRadius::same(2), color);
        }
        Icon::Alerts => {
            p.add(fill(vec![
                at(-0.08, -0.4),
                at(0.08, -0.4),
                at(0.27, -0.2),
                at(0.32, 0.24),
                at(-0.32, 0.24),
                at(-0.27, -0.2),
            ]));
            p.circle_filled(at(0.0, 0.36), s * 0.08, color);
        }
        Icon::Power => {
            p.add(fill(vec![at(0.1, -0.48), at(-0.26, 0.08), at(0.04, 0.08)]));
            p.add(fill(vec![
                at(-0.04, -0.08),
                at(0.26, -0.08),
                at(-0.1, 0.48),
            ]));
        }
        Icon::Fans => {
            for k in 0..3 {
                let a = std::f32::consts::TAU * k as f32 / 3.0;
                let rot = |x: f32, y: f32| at(x * a.cos() - y * a.sin(), x * a.sin() + y * a.cos());
                p.add(fill(vec![
                    rot(0.0, 0.0),
                    rot(0.16, -0.12),
                    rot(0.1, -0.46),
                    rot(-0.08, -0.42),
                ]));
            }
            p.circle_filled(c, s * 0.1, color);
        }
        Icon::Cpu => {
            let r = Rect::from_center_size(c, vec2(s * 0.56, s * 0.56));
            p.rect_stroke(r, CornerRadius::same(2), stroke, StrokeKind::Inside);
            p.rect_filled(
                Rect::from_center_size(c, vec2(s * 0.22, s * 0.22)),
                CornerRadius::same(1),
                color,
            );
            let thin = Stroke::new(stroke.width * 0.8, color);
            for o in [-0.14, 0.0, 0.14] {
                p.line_segment([at(o, -0.28), at(o, -0.44)], thin);
                p.line_segment([at(o, 0.28), at(o, 0.44)], thin);
                p.line_segment([at(-0.28, o), at(-0.44, o)], thin);
                p.line_segment([at(0.28, o), at(0.44, o)], thin);
            }
        }
        Icon::Gpu => {
            let r = Rect::from_center_size(at(0.04, -0.04), vec2(s * 0.84, s * 0.56));
            p.rect_stroke(r, CornerRadius::same(2), stroke, StrokeKind::Inside);
            p.circle_stroke(at(0.14, -0.04), s * 0.15, stroke);
            p.line_segment([at(-0.38, 0.24), at(-0.38, 0.44)], stroke);
        }
        Icon::Lighting => {
            p.circle_filled(c, s * 0.18, color);
            for k in 0..8 {
                let a = std::f32::consts::TAU * k as f32 / 8.0;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * s * 0.3, c + d * s * 0.46], stroke);
            }
        }
        Icon::General => {
            p.circle_stroke(c, s * 0.2, Stroke::new(s * 0.13, color));
            let teeth = Stroke::new(s * 0.15, color);
            for k in 0..8 {
                let a = std::f32::consts::TAU * k as f32 / 8.0;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * s * 0.27, c + d * s * 0.42], teeth);
            }
        }
        Icon::Display => {
            let r = Rect::from_center_size(at(0.0, -0.1), vec2(s * 0.84, s * 0.56));
            p.rect_stroke(r, CornerRadius::same(2), stroke, StrokeKind::Inside);
            p.line_segment([at(0.0, 0.18), at(0.0, 0.34)], stroke);
            p.line_segment([at(-0.2, 0.36), at(0.2, 0.36)], stroke);
        }
        Icon::More => {
            for dx in [-0.3, 0.0, 0.3] {
                p.circle_filled(at(dx, 0.0), s * 0.08, color);
            }
        }
        Icon::Plus => {
            p.line_segment([at(-0.32, 0.0), at(0.32, 0.0)], stroke);
            p.line_segment([at(0.0, -0.32), at(0.0, 0.32)], stroke);
        }
        Icon::Chevron => {
            p.add(Shape::line(
                vec![at(-0.15, -0.3), at(0.15, 0.0), at(-0.15, 0.3)],
                stroke,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDER: &[&str] = &["header", "clock", "cpu", "gpu", "ram"];

    fn list(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|k| k.to_string()).collect()
    }

    #[test]
    fn a_shown_item_goes_back_to_its_natural_place() {
        let mut l = list(&["header", "gpu"]);
        toggle_in_order(&mut l, "cpu", true, ORDER);
        assert_eq!(l, list(&["header", "cpu", "gpu"]));
        toggle_in_order(&mut l, "ram", true, ORDER);
        assert_eq!(l, list(&["header", "cpu", "gpu", "ram"]));
    }

    #[test]
    fn a_user_order_is_kept_and_hiding_removes() {
        // The user had GPU before CPU; showing the clock must not reorder them.
        let mut l = list(&["gpu", "cpu"]);
        toggle_in_order(&mut l, "clock", true, ORDER);
        assert_eq!(l, list(&["clock", "gpu", "cpu"]));
        toggle_in_order(&mut l, "gpu", false, ORDER);
        assert_eq!(l, list(&["clock", "cpu"]));
        toggle_in_order(&mut l, "cpu", true, ORDER);
        assert_eq!(l, list(&["clock", "cpu"]), "already shown: no duplicate");
    }

    #[test]
    fn move_by_swaps_with_the_neighbour_and_stops_at_the_ends() {
        let mut l = list(&["a", "b", "c"]);
        move_by(&mut l, "c", -1);
        assert_eq!(l, list(&["a", "c", "b"]));
        move_by(&mut l, "a", -1);
        assert_eq!(l, list(&["a", "c", "b"]), "first can't move up");
        move_by(&mut l, "b", 1);
        assert_eq!(l, list(&["a", "c", "b"]), "last can't move down");
    }
}
