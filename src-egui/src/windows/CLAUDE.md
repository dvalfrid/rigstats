# egui dialog design system

All secondary windows (`settings.rs`, `about.rs`, `status.rs`, `updater.rs`) must follow this layout and colour contract. **Never deviate from these values without updating this file.**

## Layout skeleton

Every dialog uses three egui panels:

```rust
// 1. Hero — title + optional subtitle row
egui::TopBottomPanel::top("xxx_hero")
    .frame(dialog_hero_frame())
    .show_separator_line(true)
    .show(ctx, |ui| { /* title, installed/version row if relevant */ });

// 2. Footer — status message (optional) + buttons
egui::TopBottomPanel::bottom("xxx_footer")
    .frame(dialog_footer_frame())
    .show_separator_line(true)
    .show(ctx, |ui| {
        // optional status line (small, C_DATE), then add_space(6.0)
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // primary button on the right, secondary to its left
            theme::dialog_btn_primary(ui, "OK");
            ui.add_space(6.0);
            theme::dialog_btn_secondary(ui, "Cancel");
        });
    });

// 3. Central — fills all remaining space
egui::CentralPanel::default()
    .frame(dialog_central_frame())
    .show(ctx, |ui| { /* main content */ });
```

Frame constructors (all share the same background tone):

```rust
// fill = Color32::from_gray(38)  ← dialog surface colour
// hero   inner_margin: { left: 14, right: 14, top: 14, bottom: 12 }
// footer inner_margin: { left: 12, right: 12, top: 8,  bottom: 10 }
// central inner_margin: Margin::same(10)
```

## Colour tokens

| Token | Value | Usage |
|---|---|---|
| Dialog surface | `gray(38)` | Hero, footer, central background |
| Inset/scroll area | `gray(27)` | Scroll areas, code blocks — darker = inset |
| Section label | `gray(140)` | Small bold headings inside content |
| Muted text / dates | `gray(128)` | Secondary text, dates, status messages |
| Body text | `rgb(155, 180, 210)` | Primary content text |

## Button API (in `theme.rs`)

- `theme::dialog_btn_primary(ui, label)` — blue `#0078D4`, white text. Use for the main action (OK, Save, Install Now, Close, Check for Updates).
- `theme::dialog_btn_secondary(ui, label)` — gray `#343434` with border. Use for cancel/dismiss (Cancel, Later).
- `theme::dialog_btn_secondary_disabled(ui, label)` — same gray, non-interactive.

**Button layout rule:** `ui.with_layout(egui::Layout::right_to_left(Align::Center), ...)`. Primary added first (lands right), secondary after (lands left). Always `ui.separator()` immediately before the button row.

Hover/active states work via `ui.visuals_mut().widgets.{inactive,hovered,active}` inside a `ui.scope()` — the scope prevents leaking to surrounding UI.

## Section labels and scroll areas

- A **section label** is a free `ui.label()` — never inside a frame. Font: size 11.0, strong, `gray(140)`.
- A **scroll area with inset content**: `egui::Frame::new().fill(gray(27)).corner_radius(4)` wrapping a `ScrollArea`. No border stroke — fill difference alone provides the visual distinction.
- Use `egui::Frame::new()` (not the deprecated `Frame::none()`).

## Mutex / action pattern

Avoid holding a `MutexGuard` across multiple `show()` closures (causes borrow-checker errors):

```rust
// 1. Lock once, extract view data into locals
let st = state.lock().unwrap();
let heading = /* derive from st */;

// 2. Render all panels (read-only via locals)
egui::TopBottomPanel::top(...).show(ctx, |ui| { /* uses heading */ });
egui::TopBottomPanel::bottom(...).show(ctx, |ui| { action_close = ...; });
egui::CentralPanel::default().show(ctx, |ui| { /* read from st */ });

// 3. Drop guard, then apply actions
drop(st);
if action_close { open.store(false, ...); }
if action_check { state.lock().unwrap().status = ...; }
```
