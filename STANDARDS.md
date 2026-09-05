# Code Standards

## Overview

This document defines the coding, formatting, and architectural standards for this project. All contributors and AI assistants must follow these rules when writing, modifying, or reviewing code. RIGStats is a Windows-only native Rust/egui desktop app — there is no web frontend — so these standards cover Rust exclusively, plus the egui-specific dialog design system for secondary windows.

## Contents

- [Tools and commands](#tools-and-commands)
- [Rust](#rust)
- [egui (secondary windows)](#egui-secondary-windows)
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
| Types, traits, enums | `PascalCase` | `AppState`, `LhmData` |
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

### Unsafe and global mutable state

- `unsafe_code = "deny"` applies to both crates (`src-egui`'s wallpaper binary is the one place that legitimately needs a documented, narrowly-scoped `#[allow(unsafe_code)]` — see `win_opacity.rs`)
- Global `static` variables must use atomic types (`AtomicBool`, `AtomicI32`, `AtomicU64`) — never `static mut`
- Shared state (`AppState`, `RigStatsApp` fields) is always protected by `Mutex`

### `#[allow(...)]` attributes

Every `#[allow(...)]` must have a clear reason documented in the code (a preceding or inline comment) — see [CLAUDE.md](CLAUDE.md). Do not add one to silence a lint without explaining why the lint doesn't apply here.

### Module structure

Keep modules focused on a single responsibility — see [CLAUDE.md](CLAUDE.md) for the module overview.
Keep domain logic in `rigstats-backend/` — `src-egui/` contains only UI and wiring, not business logic.

---

## egui (secondary windows)

All secondary windows must follow the dialog design system documented in [CLAUDE.md — egui dialog design system](CLAUDE.md#egui-dialog-design-system). Key rules:

- **Three-panel layout:** `TopBottomPanel::top` (hero) → `TopBottomPanel::bottom` (footer) → `CentralPanel` (content).
- **Surface colour:** `Color32::from_gray(38)` for all three panels — uniform dialog background.
- **Inset colour:** `Color32::from_gray(27)` for scroll areas and content wells — no border stroke, fill difference is the only visual cue.
- **No visible frame borders** inside dialogs — use fill tone differences, not strokes, to separate regions.
- **Section labels** (`"What's New"`, tab headings, etc.) are free `ui.label()` calls — never wrapped in a frame.
- **Buttons:** always use `theme::dialog_btn_primary` / `theme::dialog_btn_secondary` / `theme::dialog_btn_secondary_disabled`. Layout with `right_to_left` — primary action on the far right.
- **Frame API:** use `egui::Frame::new()` — `Frame::none()` is deprecated in egui 0.34.
- **Mutex pattern:** extract all view data from the guard into local variables before any `show()` call; `drop(guard)` before applying mutations.

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
