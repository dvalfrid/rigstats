# egui Migration — Session Kickstart

## Branch: `feat/egui-migration`

## Fas: Phase 6 (Visual Fidelity) — pågående

---

## Vad vi håller på med

Ersätter Tauri/WebView2-frontend med native egui för att eliminera 2–4 % idle CPU.
Mål för Phase 6: panelkort matchar Tauri-designen visuellt.

---

## Nuläge — vad som fungerar

- `panel_frame()` med gradient accent-linje och L-formade hörnbrackets ✓
- Ring-gauge (`ring.rs`) — CPU och GPU ✓
- Header: brand-subtitle + hostname + modellnamn + ROG-logga ✓
  - Ingen extra text under logotypen (borttagen) ✓
  - Ingen CPU-modell i headern ✓
  - Ingen LHM-indikator i headern ✓
  - Logotypen är höger-justerad och större (80px hög) ✓
- Panel-ordning respekterar `visible_panels` från settings ✓
- CPU panel: ring (80px, vänster) + meta-grid (TEMP/FREQ/POWER, höger) + AMD-logga (40px) + fullbredd core-bars + sparkline ✓
- GPU panel: ring (80px, vänster) + meta-grid 3×2 (höger) + AMD-logga (40px) + GPU-modellnamn + GPU-väljarprickar (multi-GPU) + bars + sparkline ✓
  - GPU-väljare: klickbara cirklar, ifylld röd = vald, skriver till settings ✓
  - `gpu_devices` i PollStats, populeras från LHM ✓
  - VRAM visas (RX 9070 XT visar 3.4/15.9 GB när Tauri-appen inte kör) ✓
- RAM panel: stort amber-tal + TEMP (om tillgänglig) + spec-rad + bar (ingen sparkline) ✓
  - `ram_temp` i PollStats, populeras från LHM ✓
- Clock panel: tid (40px), dag/datum, uptime ✓
- Drag-handtag med hover-indikator (3 prickar mitt i remsan) ✓
- **Sparklines:**
  - Push sker i `try_recv()`-loopen — hover rusas inte ✓
  - Dynamisk skala ✓
  - Gradientfyll under linjen ✓
  - Pre-fylld med nollor ✓
- **Logotyper:**
  - `brand.rs` — laddar PNG-texturer en gång vid start ✓
  - Header visar ROG-logga ✓
  - CPU visar AMD-logga baserat på `cpu_model` ✓
  - GPU visar AMD/NVIDIA-logga baserat på `gpu_name` ✓
- `theme::thin_bar()` — 4px tunn bar-widget ✓
- `PANEL_HEADER_H = 105` / `PANEL_DATA_H = 200` — `set_min_height` i alla paneler ✓
- `preferred_gpu` styrs via `Arc<Mutex<Option<String>>>` delad mellan UI och poll-loop ✓

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

**Förväntad status:** Binären byggdes rent (6.22s). Verifierad visuellt — allt ovan stämmer.

---

## Kvarstående problem / nästa uppgifter

### Högt prioritet (visuell paritet med Tauri)

1. **RAM metadata-grid saknas** — Tauri visar FREE/SPEED/TYPE och PART/DIMMS/VENDOR.
   Egui visar bara TEMP + `ram_spec`-strängen.
   Kräver fler fält i PollStats:
   `ram_free: u64`, `ram_speed: Option<u32>`, `ram_type: String`,
   `ram_part: String`, `ram_dimms: Option<u8>`, `ram_vendor: String`.
   Populeras från `hardware::detect_ram_details()` (finns redan i backend).

2. **Net-panelen layout** — Tauri visar UP/DOWN med pilar (↑/↓) och stora tal.
   Egui-versionen matchar ganska väl men kontrollera visuellt om det kan förbättras.

### Medel prioritet

1. **`·` datum-separator** i `clock.rs` — `%Y·%m·%d` med U+00B7.
   Kontrollera om tecknet renderas korrekt. Byt till `-` om det inte gör det.

2. **Disk-panel** — kontrollera visuellt mot Tauri-versionen.
   Bör visa modellnamn, filsystem, använd/total, temperatur.

3. **Motherboard-panel** — kontrollera visuellt.

4. **Process-panel** — kontrollera visuellt.

5. **Battery-panel** — kontrollera visuellt.

### Lågt prioritet (Phase 7/8)

1. **Opacity** — egui-fönstret saknar opacity-styrning (finns i Settings).
2. **`ViewportCommand::WindowLevel`** för live always-on-top — ej implementerat.
3. **Settings-dialogen** — opacity, profil, always-on-top ska kunna ändras live.

---

## Filer att känna till

| Fil | Roll |
|-----|------|
| `src-egui/src/main.rs` | App-entry, poll-loop, recv-loop, panel-dispatch, PollStats |
| `src-egui/src/brand.rs` | Textures-struct, laddar PNG-logotyper |
| `src-egui/src/theme.rs` | Färgkonstanter, `panel_frame()`, `thin_bar()`, PANEL_*_H |
| `src-egui/src/ring.rs` | Ring-gauge widget |
| `src-egui/src/spark.rs` | Sparkline: pre-fill, dynamisk skala, gradientfyll |
| `src-egui/src/panels/*.rs` | En fil per panel |
| `docs/egui-migration.md` | Roadmap för alla 8 faser |

---

## Viktiga egui 0.34.3-gotchas

- `CornerRadius::ZERO` (inte `Rounding::ZERO`)
- `Margin::symmetric(i8, i8)` (tar `i8`, inte `f32`)
- `ViewportCommand::StartDrag` (inte `StartWindowDrag`)
- `Color32::from_rgba_unmultiplied` är **inte** `const fn` — använd `from_rgba_premultiplied` i constants
- `ui.child_ui(...)` är deprecated — använd `ui.new_child(UiBuilder::new().max_rect(...).layout(...))`
- `ui.screen_rect()` är deprecated — använd `ui.ctx().content_rect()`
- `egui::ColorImage::from_rgba_unmultiplied([w, h], data)` — använd denna, inte struct literal
- `egui::load::SizedTexture::new(tex.id(), Vec2::new(w, h))` för att visa TextureHandle
- Sparklines: push sker i `recv`-loopen i `main.rs`, INTE i panel `draw()`-funktioner
- `gpu::draw()` returnerar `Option<String>` (ny vald GPU) — hantera i main.rs:s panel-dispatch
- `preferred_gpu` är `Arc<Mutex<Option<String>>>` delad mellan `RigStatsApp` och `poll_loop`

---

## Arkitektur-noteringar

### PollStats — nya fält sedan senaste session

- `ram_temp: Option<f64>` — RAM-temp från LHM
- `gpu_devices: Vec<(String, f64)>` — alla detekterade GPUer (namn, vram_mb)

### preferred_gpu-flödet

1. `main()` skapar `Arc<Mutex<Option<String>>>` från `settings.preferred_gpu`
2. En klon (`pref_poll`) skickas till `poll_loop` — läses varje tick från Arken
3. Originalet (`preferred_gpu_arc`) lagras i `RigStatsApp`
4. När användaren klickar ett GPU-dot → `gpu::draw()` returnerar `Some(name)`
5. `main.rs` uppdaterar Arken + skriver `settings::persist_settings`

### Panel-höjder

- `theme::PANEL_HEADER_H = 105.0` — header + clock
- `theme::PANEL_DATA_H = 200.0` — alla datapaneler
- `ui.set_min_height()` anropas i varje panel-closure
