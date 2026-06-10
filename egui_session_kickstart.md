# egui Migration — Session Kickstart

## Branch: `feat/egui-migration`

## Fas: Phase 7 (Floating Mode) — KLAR

---

## Nuläge — vad som fungerar

### Phase 6 (Visual Fidelity) — komplett

- `panel_frame()` med gradient accent-linje och L-formade hörnbrackets ✓
- Ring-gauge (`ring.rs`) — CPU och GPU ✓
- Header: brand-subtitle + hostname + modellnamn + ROG-logga ✓
- Panel-ordning respekterar `visible_panels` från settings ✓
- CPU, GPU, RAM, Clock, Net, Disk, Motherboard, Process, Battery panels ✓
- Sparklines med dynamisk skala + gradientfyll ✓
- `panel_frame()` med accent-färger per panel ✓
- Opacity via `SetLayeredWindowAttributes(LWA_ALPHA)` ✓
- Always-on-top live update via `ViewportCommand::WindowLevel` ✓

### Phase 7 (Floating Mode) — komplett

- `show_viewport_deferred()` per synlig panel ✓
- Main window dold (`ViewportCommand::Visible(false)`) när floating mode aktiv ✓
- Drag-handtag i varje panel-viewport (inaktiverat vid lock) ✓
- Positioner spårade och persisterade till `panel_layouts` i settings ✓
- Scale factor (`floating_panel_scale` 0.4–1.0) appliceras på fönsterbredd/-höjd ✓
- GPU-preferens-klick i floating GPU-panel propageras tillbaka till main app ✓
- Settings Dashboard-tab: lock-checkbox + scale-slider när floating mode aktivt ✓
- Auto-resize höjd per panel-viewport via `ui.min_rect().height()` ✓

---

## Första steget i ny session

```bash
# Bygg och verifiera att koden kompilerar
cargo check --manifest-path src-egui/Cargo.toml
cargo clippy --manifest-path src-egui/Cargo.toml -- -D warnings

# Bygg och kör
cargo build --manifest-path src-egui/Cargo.toml
.\target\debug\rigstats-egui.exe
```

**Förväntad status:** Binären byggdes rent. Phase 7 komplett.

---

## Nästa fas: Phase 8 — Autostart, logging, auto-update

- Autostart: återanvänd `autostart.rs` (redan importerat i settings.rs)
- Stats CSV logging: återanvänd `logging.rs`  
- Log pruning: befintlig prune-logik
- Auto-update: implementera med self_update crate eller direkt GitHub API
  (fetch `latest.json`, jämför versioner, ladda ner NSIS installer)

---

## Filer att känna till

| Fil | Roll |
|-----|------|
| `src-egui/src/main.rs` | App-entry, poll-loop, recv-loop, panel-dispatch, floating mode |
| `src-egui/src/brand.rs` | Textures-struct (Clone), laddar PNG-logotyper |
| `src-egui/src/theme.rs` | Färgkonstanter, `panel_frame()`, `thin_bar()`, PANEL_*_H |
| `src-egui/src/ring.rs` | Ring-gauge widget |
| `src-egui/src/spark.rs` | Sparkline (Clone): pre-fill, dynamisk skala, gradientfyll |
| `src-egui/src/panels/*.rs` | En fil per panel |
| `src-egui/src/windows/settings.rs` | Settings-dialog (4 tabbar) |
| `docs/egui-migration.md` | Roadmap för alla faser |

---

## Floating mode — arkitektur

- `floating_mode: bool` i `RigStatsApp` — speglar settings
- `render_floating_panels()` kallas från `ui()` när floating mode är på
- Varje panel: `show_viewport_deferred("float_{key}", ...)` — borderless, no decorations
- Positions: `Arc<Mutex<HashMap<String, [f32;2]>>>` + `positions_dirty: Arc<AtomicBool>`
- Persistering: sker i `ui()` när `positions_dirty` är satt (max 1x/tick)
- GPU-preferens: `float_new_pref_gpu: Arc<Mutex<Option<String>>>` — tas i `ui()`
- Main window: `ViewportCommand::Visible(false)` vid toggle-on, `Visible(true)` vid toggle-off

---

## Viktiga egui 0.34.3-gotchas

- `CornerRadius::ZERO` (inte `Rounding::ZERO`)
- `Margin::symmetric(i8, i8)` (tar `i8`, inte `f32`)
- `ViewportCommand::StartDrag` (inte `StartWindowDrag`)
- `Color32::from_rgba_unmultiplied` är **inte** `const fn` — använd `from_rgba_premultiplied` i constants
- `ui.child_ui(...)` är deprecated — använd `ui.new_child(UiBuilder::new().max_rect(...).layout(...))`
- `ui.screen_rect()` är deprecated — använd `ui.ctx().content_rect()`
- `#[allow(deprecated)]` krävs för `CentralPanel::default().show()` i viewport callbacks
- Sparklines: push sker i `recv`-loopen i `main.rs`, INTE i panel `draw()`-funktioner
- `gpu::draw()` returnerar `Option<String>` (ny vald GPU) — hantera i main.rs:s panel-dispatch
- `preferred_gpu` är `Arc<Mutex<Option<String>>>` delad mellan `RigStatsApp` och `poll_loop`

