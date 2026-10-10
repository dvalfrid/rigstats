# Code Standards

## Overview

This document defines the coding, formatting, and architectural standards for this project. All contributors and AI assistants must follow these rules when writing, modifying, or reviewing code. RIGStats is a Windows-only native Rust/egui desktop app — there is no web frontend — so these standards cover Rust, the egui dialog design system (`ui_kit.rs`) for secondary windows, and C# for the sensor/control service (`sensor-sidecar/`).

## Contents

- [Tools and commands](#tools-and-commands)
- [Rust](#rust)
- [egui (secondary windows)](#egui-secondary-windows)
- [C# (sensor-sidecar)](#c-sensor-sidecar)
- [AI‑generated code](#aigenerated-code)

---

## Tools and commands

| Purpose | Command |
|---|---|
| Format Rust (modifies files) | `cargo xtask fmt` |
| Check Rust formatting (CI) | `cargo xtask fmt-check` |
| Lint Rust | `cargo xtask clippy` |
| Run all Rust tests | `cargo xtask test` |
| Full verification (sidecar + tests + clippy + fmt-check) | `cargo xtask verify` |

Run `cargo xtask verify` (or at minimum `cargo xtask fmt` + `cargo xtask clippy`) before every commit. See [CLAUDE.md](CLAUDE.md) for the full command reference, including sidecar and production build commands.

---

## Rust

Configured via `[lints]` in each crate's `Cargo.toml` (`src-egui/Cargo.toml`, `rigstats-backend/Cargo.toml`). `cargo xtask fmt`/`fmt-check`/`clippy` run against both crates.

### Rust formatting

- Standard `rustfmt` defaults (4-space indent, no project-specific `rustfmt.toml`)
- `cargo fmt` handles everything automatically — never format manually

### Rust naming

Follow Rust conventions without exception:

| Kind | Convention | Example |
|---|---|---|
| Functions, variables | `snake_case` | `fetch_lhm`, `gpu_load` |
| Types, traits, enums | `PascalCase` | `PollStats`, `LhmData` |
| Constants, statics | `SCREAMING_SNAKE_CASE` | `CREATE_NO_WINDOW` |
| Modules | `snake_case` | `lhm_process`, `hardware` |

### Documentation comments

- `//!` for module-level docs (top of file — describes the module's responsibility and design decisions)
- `///` for public functions and types
- Internal helpers do not need comments if the name is self-explanatory
- Explain *why*, not *what* — never restate the signature in prose

```rust
//! Module doc: describes responsibility and notable design decisions.

/// Detects the primary GPU name via WMI, falls back to PowerShell.
pub fn detect_gpu_name() -> Option<String> { ... }
```

### Rust error handling

- Return `Result<T, String>` from fallible functions (consistent with the rest of the codebase)
- Prefer `unwrap_or_else` over `unwrap` for graceful fallback
- `expect()` is acceptable at startup for genuinely fatal conditions
- Log errors via the `debug.rs` logging helpers (`log_debug`/`log_warn`/`log_error`) — never `eprintln!` or `dbg!` in production code

### Minimum Rust version

Both crates declare `rust-version = "1.77"`, and clippy enforces it: a standard-library API stabilised later fails `cargo xtask clippy` even though the pinned toolchain compiles it (e.g. `Option::is_none_or`, 1.82 — use `map_or(true, …)`). Raise `rust-version` deliberately, not to make one call compile.

### Unsafe and global mutable state

- `unsafe_code = "deny"` applies to both crates. Raw Win32/FFI code opts out with a narrowly-scoped, documented `#[allow(unsafe_code)]` — a module-level `#![allow(unsafe_code)]` for modules that are thin FFI shims (`win_opacity.rs`, `win32_*.rs`, `gpu_process.rs`, `hotkey.rs`, `single_instance.rs`), or an item-level `#[allow(unsafe_code)]` on the single block elsewhere (e.g. `geometry.rs`, `update_check.rs`, the tray thread in `main.rs`). Everything else stays safe Rust.
- Global `static` variables must use atomic types (`AtomicBool`, `AtomicI32`, `AtomicU64`) — never `static mut`
- State shared across threads (`RigStatsApp` ↔ poll loop/tray/background threads) is always behind `Mutex` (via `lock_safe()`) or an `Arc<Atomic*>`

### `#[allow(...)]` attributes

Every `#[allow(...)]` must have a clear reason documented in the code (a preceding or inline comment) — see [CLAUDE.md](CLAUDE.md). Do not add one to silence a lint without explaining why the lint doesn't apply here.

### Module structure

Keep modules focused on a single responsibility — see [CLAUDE.md](CLAUDE.md) for the module overview.
Keep domain logic in `rigstats-backend/` — `src-egui/` contains only UI and wiring, not business logic.

---

## egui (secondary windows)

Every dialog is built from the design system in `src-egui/src/windows/ui_kit.rs`, documented (with the reasoning) in [egui dialog design system](src-egui/src/windows/CLAUDE.md). It follows Apple's Human Interface Guidelines for grouped lists. Key rules:

- **One home per setting:** what changes with the activity (gaming, quiet, work) belongs to a Control Center profile; everything app-wide to Settings. Where a setting lives in the other window, add a link row (`windows::OpenRequest`), never a second editor.
- **Layout:** `ui_kit::hero` → footer → sidebar of `nav_item`s → page: `page_header`, then `group`s of `row`s (title and subtitle left, control right). Single-page dialogs keep hero / central / footer.
- **Controls by the choice:** on/off `toggle`, 2–5 short options `segmented`, longer lists `dropdown`, percentages `slider_pct`, show/hide + order `ordered_rows`. Don't hand-roll frames, toggles or tab buttons; change a value in `ui_kit`, never per dialog.
- **Words:** plain, sentence case, say what a setting does; a subtitle explains the row, a footnote the group.
- **Behaviour:** changes preview live; Save keeps, Revert / Cancel / Close undo. A control must never change a value just by being drawn.
- **Buttons:** `theme::dialog_btn_primary` / `theme::dialog_btn_secondary`; `right_to_left`, primary on the far right.
- **Verify by eye:** open the page in a debug build with `RIGSTATS_OPEN=control:<page>` / `settings:<page>` and screenshot it before handing over.
- **Frame API:** `egui::Frame::new()` — `Frame::none()` is deprecated in egui 0.34.
- **Mutex pattern:** extract all view data from the guard into local variables before any `show()` call; `drop(guard)` before applying mutations.

---

## C# (sensor-sidecar)

`sensor-sidecar/` is a .NET 10 Windows Service running as **LocalSystem** that writes to real hardware, so its rules are stricter than the app's. Hardware-write safety rules: [sensor-sidecar/Control/CLAUDE.md](sensor-sidecar/Control/CLAUDE.md).

### C# tools

| Purpose | Command |
|---|---|
| Build | `dotnet build sensor-sidecar/sensor-sidecar.csproj` |
| Test (xUnit + NSubstitute) | `dotnet test sensor-sidecar.Tests/sensor-sidecar.Tests.csproj` (also part of `cargo xtask verify`) |

Do not run `dotnet format` yet: `.editorconfig` has no `[*.cs]` section, so it would re-indent the 4-space C# code to the 2-space default.

### C# style

- File-scoped namespaces (`namespace SensorSidecar.Control;`), 4-space indent, primary constructors for services (`public sealed class ControlPipeWorker(ControlBroker broker, …)`), `sealed` by default.
- `<Nullable>enable</Nullable>` in both projects. A `!` needs a reason a reader can see.
- `///` comments in prose with Markdown backticks (not XML tags) — explain *why* and what the hardware does, as in the existing code.
- No new NuGet packages without approval. `LibreHardwareMonitorLib` is pinned to the same version in both csproj files — bump both together.

### C# error handling and logging

- A `BackgroundService` must never let an exception escape `ExecuteAsync` — the default `StopHost` behaviour takes telemetry and fan control down with it. Catch, log, back off (see `ControlPipeWorker.AcceptLoopAsync`).
- A broad `catch` needs a comment saying why (typically: best-effort release, one provider must not stop the rest).
- Log through `SidecarLog.Log("[area] …")`: the message for an explained failure, `e.ToString()` (with stack) for an unexpected one.
- P/Invoke: `SafeHandle`, `SetLastError = true`, and a comment on what the call is for. Keep FFI in its own class (`HidDevice`, `PawnIoModule`, `AdlxGpuPower`).

### C# security

- Pipe ACLs are part of the contract: `rigstats-sensors` is read-only for Users; `rigstats-control` is Interactive read/write and every connection goes through `PipeClientVerifier`. New control methods clamp their parameters and never take a file path.
- Files under `%ProgramData%\se.codeby.rigstats` are trusted only after `DataDirectory.EnsureSecure`. Secrets are DPAPI-encrypted.

### C# tests

- Hardware sits behind an interface (`IPawnIoModule`, `ILightingDevice`, `FakeFanHeader`) so providers are tested without it. Never call real hardware from a test.
- The test project compiles the sidecar's sources from an explicit list (the sidecar is a single-file exe a test project can't reference): **a new `.cs` file in `sensor-sidecar/` must be added to `sensor-sidecar.Tests.csproj`'s `<Compile Include=…>` list**, or the tests fail with "type not found" while the sidecar itself builds.
- Data shared with the Rust side goes in `sensor-sidecar.Tests/contract/`, checked by a test on each side.
- Real-hardware captures go in `sensor-sidecar.Tests/fixtures/` (auto-discovered by `FixtureTests`; see `/sensor-fixture`). Lighting models' `Verified` flags generate `docs/supported-devices.md`; `SupportedDevicesTests` fails on drift — regenerate with `RIGSTATS_UPDATE_SUPPORTED_DEVICES=1`.

---

## AI‑generated code

AI assistants such as Claude or GitHub Copilot may be used during development, but all generated code must follow the same standards as human‑written code. To ensure consistency and maintainability, AI‑generated code must adhere to the following rules:

- Keep solutions simple and concrete; avoid unnecessary abstractions, traits, generics, or lifetimes.
- Generated Rust code must compile without warnings under the project's lint configuration (`cargo xtask clippy` must pass for both crates).
- Follow all naming, formatting, and module‑structure rules defined in this document.
- Do not introduce new crates without explicit approval; prefer existing dependencies.
- Error handling must follow project conventions (e.g., `Result<T, String>` for fallible functions, no `unwrap()` in production paths).
- All AI‑generated code must be reviewed with the same scrutiny as human‑written code.
- AI should not restructure modules, rename files, or change architecture unless explicitly instructed.

This ensures that AI assistance improves productivity without degrading code quality or introducing stylistic drift.
