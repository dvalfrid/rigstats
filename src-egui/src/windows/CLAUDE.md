# egui dialog design system

Every dialog is built from **`ui_kit.rs`** — one set of surfaces, type sizes and controls, modelled on Apple's grouped lists (Human Interface Guidelines): a page has a title, then groups — a small heading, a rounded card of rows separated by hairlines, an optional footnote — and each row is a title (and optional subtitle) on the left with its control on the right. **New UI uses `ui_kit`; don't hand-roll frames, toggles or tab buttons.** Change a value in `ui_kit` (and here), never per dialog.

## Where a setting lives (#305)

Every setting has exactly one home:

| A Control Center profile holds (`ProfileLook` + the service's profile) | Settings holds (applies to the whole app) |
|---|---|
| Power plan, fans, CPU/GPU limits, lighting | Start with Windows, model name, recordings |
| Accent colour, opacity, panels and their order, desktop background | Display profile, window layer, floating, fill screen, displayed GPU, PSU |
| Overlay shown, its metrics and layout | Overlay position, size, background, click-through |
| Alert thresholds | Notifications on/off, repeat interval, the microphone mute toast |
| — | Which microphone Ctrl+Alt+M mutes (#425) |
| — | A device's own settings (headset sidetone, noise reduction, #428) — kept in the device, edited on the Control Center's Headset page for every profile, sent at once |

Ask "does this change with what I'm doing (gaming, quiet, work)?" — yes → profile, no → Settings. Where a setting sits in the other window, add a row that links there (`OpenRequest`, `windows/mod.rs`), not a second editor. The same goes for actions: each has one home (the log folder is in Status, a recording's file in Session History), not a copy in every window that could have it. A link button is always named after where it goes — "Open Control Center", "Open Settings" — the same wherever it appears, and its row sits next to the settings it belongs with (in the page's first group, or first on the page), never at the bottom under a long list.

## Window lifecycle (mandatory for every dialog, #203)

A dialog's OS window must never be visible without rendered content, or it flashes white (badly on a slow/power-saving GPU). The call site in `src-egui/src/app/dialogs.rs` (`RigStatsApp::render_dialogs`) owns this, not the `windows/*.rs` file:

1. Register the dialog in the `dialog_reveal.track(id, open)` loop in `render_dialogs` (next to the other dialogs).
2. Before `show_viewport_immediate`: `let visible = self.dialog_reveal.visible("<id>");` and add `.with_visible(visible)` to the `ViewportBuilder` (right after `.with_title(...)`). Use the same `"<id>"` for `ViewportId::from_hash_of`.
3. After `show_viewport_immediate`: call `self.finish_dialog_frame(ui.ctx(), "<id>", found_hwnd, wants_focus, &focus, visible)`. It applies the dark title bar, disables DWM transitions, keeps repaints coming while hidden, and focuses the dialog once it is visible. Don't hand-roll any of that per dialog.
4. Closing needs nothing extra: when the `*_open` flag goes false, `track` queues the dialog for one hidden teardown frame automatically.

See "Dialog lifecycle" in `docs/architecture.md` for why each step is needed.

## Layout

Settings and the Control Center (the model for any multi-page dialog):

```rust
ui_kit::hero(ctx, dc, "xxx", "Title", |ui| { /* right side: status */ });
egui::TopBottomPanel::bottom("xxx_footer")            // Save/Cancel or Close
    .frame(ui_kit::dialog_frame(dc).inner_margin(Margin { left: 20, right: 20, top: 10, bottom: 12 }))
    .show(ctx, |ui| { /* right_to_left buttons */ });
egui::SidePanel::left("xxx_nav").exact_width(ui_kit::SIDEBAR_W)
    .frame(ui_kit::dialog_frame(dc).inner_margin(Margin::same(10)))
    .show(ctx, |ui| { ui_kit::nav_item(ui, dc, Icon::General, "General", selected); });
egui::CentralPanel::default().frame(ui_kit::dialog_frame(dc)).show(ctx, |ui| {
    // ScrollArea, inner margin 24/18, then:
    ui_kit::page_header(ui, dc, "General", "What this page is for.");
    ui_kit::group(ui, dc, Some("Heading"), Some("Footnote."), |g| {
        g.row("Title", Some("Subtitle"), |ui| { ui_kit::toggle(ui, dc, &mut on); });
    });
});
```

The Control Center adds a profile bar under the hero (`ui_kit::chip` per profile, `icon_button` + / ⋯). Single-page dialogs (About, Status, Updates) are `ui_kit::hero` / footer / central with groups of rows — no sidebar; state shows as `ui_kit::status` (coloured dot + text). Session History is master/detail: a sidebar of `ui_kit::list_item`s, the selected item's actions in a row under its title (destructive ones confirmed in place), summary cards and charts in `card_frame`s.

## Controls — pick by the choice

| Choice | Control |
|---|---|
| On/off | `toggle` (animated switch) |
| 2–5 short options | `segmented` |
| A longer list | `dropdown` (`CONTROL_W` wide, so rows line up), or `combo` for another width — never a raw `egui::ComboBox`, it comes out lower than the buttons beside it |
| A percentage | `slider_pct` (value shown beside it) |
| A device's own scale (sidetone 0–20) | `slider_steps` — same look; send on `drag_stopped` |
| A live level (the microphone) | `meter` — segments lit in green, smooth fall; `SLIDER_PCT_W` wide under a slider |
| A colour from a few | `swatches` |
| Show/hide + order of a list | `ordered_rows` (switch + arrows; a shown item returns to its natural place) |
| Warn/crit number | `threshold_field` |
| Navigate | `nav_item` with an `Icon` (tinted badge, one colour per kind of setting) |
| A summary that opens a page | `tile` (a failed domain: the problem's title, `dot` = `severity_color`) |
| A problem (something failed, with what to do) | `notice` — never a hand-made frame or coloured label |
| A picture (desktop background) | `Group::row_picture` — a 16:9 preview leading the row, cropped to fill; previews from `app_icons::PictureCache` |
| Content that scrolls (release notes, a log) | `fixed_card` — exact size, content clipped inside, so it can never widen the page (a `ScrollArea` inside a `group` puts its scrollbar beside the given width and pushes the card past the margin) |
| Main / other action | `theme::dialog_btn_primary` / `theme::dialog_btn_secondary` |

Buttons: `ui.with_layout(Layout::right_to_left(Align::Center), …)`, primary added first lands rightmost.

Every control in a row is **`CONTROL_H` (28 px) tall** — buttons, dropdowns, text fields (sized to it, `.vertical_align(Align::Center)`), segmented controls — so nothing sits lower than its neighbour.

## Problems (#370)

Every problem the user should know about — from the service or the app — is a `rigstats_backend::control::Problem` (id, severity, title, message, actions, detail) and is shown with **`ui_kit::notice`**, the same card everywhere: tinted by severity (`severity_color`: info = accent, attention = `C_ATTENTION`, error = `C_BAD` — no other reds or ambers), the title, what happened / what still works / what to do in plain words, the technical detail (selectable, for a bug report) and a button per action. Clicks go to `windows::run_problem_action`: `open_url` (https only — the text can come from the service) or `collect_diagnostics` (`OpenRequest::Diagnostics`: Status opens and collects the ZIP). Words: say what happened and that the rest keeps working before saying what to do; offer diagnostics whenever the user may not be able to fix it alone.

- **Levels:** a page or group → `notice`; inside a row (a lighting device, a footer result, Wallpaper Engine's last problem) → `ui_kit::row_problem(severity, text)`, the same colours compact; a state in a row's control → `ui_kit::status` with `C_GOOD` / `C_ATTENTION` / `C_BAD`.
- **One wording:** the app's own problems (service not answering, refused, version mismatch, sensor readings lost, a profile not or partly applied, the fan safety override, a service error, dry-run, update failed, Wallpaper Engine failed) and the connection's state words (`STATE_CONNECTED` … `STATE_MISMATCH`, `WAITING_FOR_SERVICE`) come from `src-egui/src/problems.rs` — never a second phrasing in a window.
- **Never hidden:** a page whose domain *failed* stays visible with its problem (`ControlState::domain_problem`) and its Overview tile shows the problem's title in its colour; only a domain this PC doesn't have is hidden. Refused or another version is a `notice` above every Control Center page.
- **One list:** Status's **Problems** group is `problems::active_problems` — everything wrong right now, most serious first; it also shows where the diagnostics ZIP went or why collecting failed.
- **Seen without a window:** a profile switch (hotkey, app rule, tray) that fails or only partly applies while the Control Center is closed raises one toast (`problems::apply_notification`).

## Saving

One rule, the same in every window:

- **Save / Revert (or Cancel)** where a change is previewed or tried first, or can be risky — Settings, the Control Center's Dashboard, Overlay, Alerts, Desktop, Fans, CPU, Graphics card and Lighting. Footer: "Unsaved changes to …" with Revert and Save.
- **Saved at once** where nothing can go wrong — the Power plan and App rules (and Settings' click-through). The page subtitle says "Changes are saved at once." and the footer shows a green **Saved** for a few seconds after each change (`ControlUi::saved_at`), so a change never passes silently. The same goes for one-off actions without a visible result: a Hue room switched ("Saved"), a profile reset ("Reset to its defaults"), a device's Dynamic Lighting switched ("Sent to the device"), and Settings' click-through ("Saved" beside Save/Cancel). Actions whose result shows by itself (rename, duplicate, delete, pairing) need none.

## Words

Write for someone who has never seen the app: say what a setting does ("How see-through the panels are"), not what it is called internally; sentence case; no jargon where a plain word exists ("Automatic (BIOS)" / "Custom curve", "Graphics card"). A row's subtitle explains, a group's footnote adds what applies to the whole group.

## Colour tokens

Every dialog function takes `dc: &DialogColors` (`theme.rs`), with a `dark()` and a `light()` — dialogs follow Windows' light/dark mode. Dark-mode values:

| Token | Value | Usage |
|---|---|---|
| `dc.bg` | `gray(38)` | Hero, footer, sidebar, page background |
| `dc.card` | `gray(27)` | Group cards, tiles, chips |
| `dc.card_border` | `gray(55)` | Card borders, row hairlines |
| `dc.inner` | `gray(33)` | Hover fill |
| `dc.inset` | `gray(22)` | Scroll areas, code blocks |
| `dc.title` | `rgb(210, 220, 235)` | Page titles, row titles |
| `dc.text` | `rgb(155, 180, 210)` | Values |
| `dc.label` | `gray(140)` | Group headings |
| `dc.muted` | `gray(115)` | Subtitles, footnotes |
| `dc.tab_active` | cyan | Selected nav item / chip, switch on |

## Mutex / action pattern

Avoid holding a `MutexGuard` across multiple `show()` closures (causes borrow-checker errors). Use `state.lock_safe()` (`lock_ext.rs`) rather than raw `.lock().unwrap()` — it recovers from a poisoned lock instead of panicking:

```rust
// 1. Lock once, extract view data into locals
let st = state.lock_safe().clone();
// 2. Render all panels (read-only via locals), recording actions
// 3. Apply actions (no guard held across the show() calls above)
```

## Seeing a page

Debug builds open a dialog at start-up with `RIGSTATS_OPEN=control:<page>`, `settings:<page>` (sidebar names in lower case, e.g. `control:alerts`), `about`, `status`, `updates` or `history` — see `/verifier-gui`.
