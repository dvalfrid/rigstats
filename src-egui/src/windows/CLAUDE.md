# egui dialog design system

All secondary windows (`settings.rs`, `about.rs`, `status.rs`, `updater.rs`) must follow this layout and colour contract. **Never deviate from these values without updating this file.**

`history.rs` follows the same hero/footer frames, colour tokens, and button API, but adds a `SidePanel` (session list) + `CentralPanel` (chart detail) between them instead of a single central content area — a deliberate exception for its master/detail layout, not a contract violation to fix.

## Layout skeleton

Every dialog uses three egui panels, all sharing one locally-defined
`dialog_frame(dc: &DialogColors) -> egui::Frame` (fill = `dc.bg`, zero inner
margin) that each call site overrides with its own `.inner_margin(...)` —
there is no shared `theme.rs` frame constructor, and exact margins vary a
little per dialog (check the file you're editing rather than copying numbers
from here):

```rust
// 1. Hero — title + optional subtitle row
egui::TopBottomPanel::top("xxx_hero")
    .frame(dialog_frame(dc).inner_margin(egui::Margin { left: 16, right: 16, top: 14, bottom: 12 }))
    .show_separator_line(true)
    .show(ctx, |ui| { /* title, installed/version row if relevant */ });

// 2. Footer — status message (optional) + buttons
egui::TopBottomPanel::bottom("xxx_footer")
    .frame(dialog_frame(dc).inner_margin(egui::Margin { left: 14, right: 14, top: 8, bottom: 12 }))
    .show_separator_line(true)
    .show(ctx, |ui| {
        // optional status line (small, muted), then add_space(6.0)
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // primary button on the right, secondary to its left
            theme::dialog_btn_primary(ui, "OK");
            ui.add_space(6.0);
            theme::dialog_btn_secondary(ui, "Cancel", dc);
        });
    });

// 3. Central — fills all remaining space
egui::CentralPanel::default()
    .frame(dialog_frame(dc))
    .show(ctx, |ui| { /* main content */ });
```

Also defined per-file next to `dialog_frame` (also not shared via `theme.rs`):

- `card_frame(dc)` — `dc.card` fill, 1px `dc.card_border` stroke, 6px corner radius. Sectioned content blocks.
- `inner_row(dc)` — `dc.inner` fill, 4px corner radius, no stroke. Nested rows inside a card (e.g. Settings toggle rows).

## Colour tokens

Every dialog function takes `dc: &DialogColors` (`theme.rs`), which has a
`DialogColors::dark()` and a `DialogColors::light()` — dialogs follow OS
light/dark mode, they are not hardcoded to one palette. Dark-mode values
(the common case during development):

| Token | Value | Usage |
|---|---|---|
| `dc.bg` | `gray(38)` | Hero, footer, central background |
| `dc.card` | `gray(27)` | Sectioned content blocks (`card_frame`) |
| `dc.inset` | `gray(22)` | Scroll areas, code blocks — darkest, deepest inset |
| `dc.label` | `gray(140)` | Small bold headings inside content |
| `dc.muted` | `gray(115)` | Secondary text, dates, status messages |
| `dc.text` | `rgb(155, 180, 210)` | Primary content text |

`DialogColors::light()` defines the same tokens with light-mode values — see
`theme.rs` for the full struct rather than assuming dark-mode numbers apply.

## Button API (in `theme.rs`)

- `theme::dialog_btn_primary(ui, label)` — blue `#0078D4`, white text. No `dc` argument (colour is fixed). Use for the main action (OK, Save, Install Now, Close, Check for Updates).
- `theme::dialog_btn_secondary(ui, label, dc)` — gray, colour sourced from `dc.btn_sec_*` (adapts to light/dark mode). Use for cancel/dismiss (Cancel, Later).
- `theme::dialog_btn_secondary_disabled(ui, label, dc)` — same gray, non-interactive.
- `theme::dialog_btn_secondary_compact(ui, label, dc, size: Vec2)` — smaller fixed-size variant of the secondary button, for inline row actions rather than a dialog footer (e.g. History's Pin/Rename/Reveal/Delete row buttons, all sharing one size per row).

**Button layout rule:** `ui.with_layout(egui::Layout::right_to_left(Align::Center), ...)`. Primary added first (lands right), secondary after (lands left). Always `ui.separator()` immediately before the button row.

Hover/active states work via `ui.visuals_mut().widgets.{inactive,hovered,active}` inside a `ui.scope()` — the scope prevents leaking to surrounding UI.

## Section labels and scroll areas

- A **section label** is a free `ui.label()` — never inside a frame. Font: size 11.0, strong, `dc.label`.
- A **scroll area with inset content**: `egui::Frame::new().fill(dc.inset).corner_radius(4)` wrapping a `ScrollArea`. No border stroke — fill difference alone provides the visual distinction.
- Use `egui::Frame::new()` (not the deprecated `Frame::none()`).

## Mutex / action pattern

Avoid holding a `MutexGuard` across multiple `show()` closures (causes borrow-checker errors). Use `state.lock_safe()` (`lock_ext.rs`) rather than raw `.lock().unwrap()` — it recovers from a poisoned lock instead of panicking, and every dialog in this directory uses it exclusively:

```rust
// 1. Lock once, extract view data into locals
let st = state.lock_safe().clone();
let heading = /* derive from st */;

// 2. Render all panels (read-only via locals)
egui::TopBottomPanel::top(...).show(ctx, |ui| { /* uses heading */ });
egui::TopBottomPanel::bottom(...).show(ctx, |ui| { action_close = ...; });
egui::CentralPanel::default().show(ctx, |ui| { /* read from st */ });

// 3. Apply actions (no guard held across the show() calls above)
if action_close { open.store(false, ...); }
if action_check { state.lock_safe().status = ...; }
```
