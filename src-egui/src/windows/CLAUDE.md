# egui dialog design system

Every dialog is built from **`ui_kit.rs`** — one set of surfaces, type sizes and controls, modelled on Apple's grouped lists (Human Interface Guidelines): a page has a title, then groups — a small heading, a rounded card of rows separated by hairlines, an optional footnote — and each row is a title (and optional subtitle) on the left with its control on the right. **New UI uses `ui_kit`; don't hand-roll frames, toggles or tab buttons.** Change a value in `ui_kit` (and here), never per dialog.

## Where a setting lives (#305)

Every setting has exactly one home:

| A Control Center profile holds (`ProfileLook` + the service's profile) | Settings holds (applies to the whole app) |
|---|---|
| Power plan, fans, CPU/GPU limits, lighting | Start with Windows, model name, recordings |
| Accent colour, opacity, panels and their order | Display profile, window layer, floating, fill screen, displayed GPU, PSU |
| Overlay shown, its metrics and layout | Overlay position, size, background, click-through |
| Alert thresholds | Notifications on/off, repeat interval |

Ask "does this change with what I'm doing (gaming, quiet, work)?" — yes → profile, no → Settings. Where a setting sits in the other window, add a row that links there (`OpenRequest`, `windows/mod.rs`), not a second editor.

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

The Control Center adds a profile bar under the hero (`ui_kit::chip` per profile, `icon_button` + / ⋯). Single-page dialogs (About, Status, Updates) keep hero / central / footer with `dialog_frame` and a local `card_frame` using the same 10 px radius; `history.rs` adds a `SidePanel` (session list) for its master/detail layout.

## Controls — pick by the choice

| Choice | Control |
|---|---|
| On/off | `toggle` (animated switch) |
| 2–5 short options | `segmented` |
| A longer list | `dropdown` (`CONTROL_W` wide, so rows line up) |
| A percentage | `slider_pct` (value shown beside it) |
| A colour from a few | `swatches` |
| Show/hide + order of a list | `ordered_rows` (switch + arrows; a shown item returns to its natural place) |
| Warn/crit number | `threshold_field` |
| Navigate | `nav_item` with an `Icon` (tinted badge, one colour per kind of setting) |
| A summary that opens a page | `tile` |
| Main / other action | `theme::dialog_btn_primary` / `theme::dialog_btn_secondary` |

Buttons: `ui.with_layout(Layout::right_to_left(Align::Center), …)`, primary added first lands rightmost.

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

Debug builds open a dialog at start-up with `RIGSTATS_OPEN=control:<page>` or `settings:<page>` (sidebar names in lower case, e.g. `control:alerts`) — see `/verifier-gui`.
