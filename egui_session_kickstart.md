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
- Header: brand-subtitle + hostname + modellnamn + ROG-logga + "REPUBLIC OF GAMERS" ✓
- `[LHM]` status-indikator i headern ✓
- Panel-ordning respekterar `visible_panels` från settings ✓
- CPU panel: ring + meta-grid + ScrollArea för 4 cores åt gången (thin bars) ✓
- GPU panel: ring centrerad + metadata 3×2 grid + thin bars ✓
- RAM panel: stort amber-tal + bar (korrekt bredd) + sparkline ✓
- Clock panel: stor tid + dag/datum + uptime ✓
- Drag-handtag ✓
- **Sparklines fixade:**
  - Push sker nu bara i `try_recv()`-loopen (1/s) — hover rusade inte längre ✓
  - Dynamisk skala: `max(data, 1.0)` — matchar JS-versionen ✓
  - Gradientfyll under linjen (matchar JS `${color}44` gradient) ✓
  - Pre-fylld med nollor — ingen stretch-effekt i början ✓
- **Logotyper:**
  - `brand.rs` — laddar PNG-texturer en gång vid start ✓
  - Header visar ROG-logga för asus-rog system ✓
  - CPU visar AMD-logga baserat på `cpu_model` ✓
  - GPU visar AMD/NVIDIA-logga baserat på `gpu_name` ✓
- `gpu_name: String` i PollStats (WMI-detekterad vid start) ✓
- `theme::thin_bar()` — 4px tunn bar-widget ✓

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

**Förväntad status:** Binären byggdes rent sist (4.48s) men har INTE körts visuellt sedan ändringarna. Verifiera visuellt direkt.

---

## Kvarstående problem / nästa uppgifter

### Högt prioritet (visuell paritet med Tauri)

1. **GPU modellnamn visas inte** — `gpu_name` finns nu i PollStats men GPU-panelens
   header visar bara "GPU LOAD". Lägg till rad under header:

   ```rust
   if !stats.gpu_name.is_empty() {
       ui.label(RichText::new(&stats.gpu_name).small().color(theme::C_TEXT_MUTED));
   }
   ```

2. **VRAM-bars visas inte för RX 9070 XT** — `gpu_vram_used_mb` / `gpu_vram_total_mb`
   är `None`. Undersök LHM-mappning i backend (`lhm.rs` → `vram_used`/`vram_total`).
   Sannolikt ett sensor-namnsproblem för denna AMD-modell.

3. **RAM metadata-grid saknas** — Tauri visar FREE/TEMP/SPEED/TYPE och
   PART/DIMMS/VENDOR. Egui visar bara `ram_spec`-strängen.
   Kräver att fler fält läggs till i PollStats:
   `ram_free: u64`, `ram_temp: Option<f64>` (från LHM), `ram_speed: Option<u32>`,
   `ram_type: String`, `ram_part: String`, `ram_dimms: Option<u8>`, `ram_vendor: String`.
   Populeras från `hardware::detect_ram_details()` (finns redan i backend).

4. **Net-panelen layout** — Tauri visar UP/DOWN med pilar (↑/↓) och stora tal.
   Egui-versionen matchar ganska väl men kontrollera visuellt.

### Medel prioritet

1. **`·` datum-separator** i `clock.rs` — `%Y·%m·%d` med U+00B7.
   Kontrollera om tecknet renderas korrekt. Byt till `-` om det inte gör det.

2. **Disk-panel** — kontrollera visuellt mot Tauri-versionen. Bör visa modellnamn,
   filsystem, använd/total, temperatur.

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
| `src-egui/src/main.rs` | App-entry, poll-loop, recv-loop (push sparklines), panel-dispatch |
| `src-egui/src/brand.rs` | Textures-struct, laddar PNG-logotyper |
| `src-egui/src/theme.rs` | Färgkonstanter + `panel_frame()` + `thin_bar()` |
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
- `egui::ColorImage::from_rgba_unmultiplied([w, h], data)` — använd denna, inte struct literal (saknar `source_size`-fält)
- `egui::load::SizedTexture::new(tex.id(), Vec2::new(w, h))` för att visa TextureHandle
- Sparklines: push sker i `recv`-loopen i `main.rs`, INTE i panel `draw()`-funktioner

---

## Jämförelse (mål = Tauri-version)

Tauri-versionen har:

- Header: "// GAMING RIG" → hostname (stor) → modell → ROG-logga + "REPUBLIC OF GAMERS"
- CPU: AMD-logga + ring + TEMP/FREQ/POWER + core-bars (scrolla för alla) + sparkline
- GPU: AMD-logga + ring (centrerad) + 3×2 metadata-grid + GPU-bar + VRAM-bar + sparkline
- RAM: Stort amber-tal + FREE/TEMP/SPEED/TYPE rad + PART/DIMMS/VENDOR rad + sparkline + MEM-bar
- NET: ↑ UP stort + ↓ DOWN stort + kombinerad sparkline + PING + IFACE
