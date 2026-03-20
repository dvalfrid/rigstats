# Code Standards

## Overview

This document defines the coding, formatting, and architectural standards for this project. All contributors and AI assistants must follow these rules when writing, modifying, or reviewing code. The goal is to maintain a consistent codebase that is predictable, maintainable, and easy to reason about across Rust, JavaScript, CSS, and HTML.

## Contents

- [Tools and commands](#tools-and-commands)
- [Rust](#rust)
- [JavaScript](#javascript)
- [CSS](#css)
- [HTML](#html)
- [AI‑generated code](#aigenerated-code)

---

## Tools and commands

| Purpose | Command |
|---|---|
| Format Rust | `npm run fmt:rs` |
| Check Rust formatting (CI) | `npm run fmt:rs:check` |
| Lint Rust | `npm run clippy` |
| Lint JavaScript | `npm run lint` |
| Auto-fix JavaScript | `npm run lint:fix` |
| Run all tests | `npm test` |

Run `npm run lint` and `npm run fmt:rs:check` before every commit.

---

## Rust

Configured via [rustfmt.toml](src-tauri/rustfmt.toml) and `[lints]` in [Cargo.toml](src-tauri/Cargo.toml).

### Rust formatting

- **2-space indent** (project-specific; intentionally deviates from the Rust community default of 4)
- Max line width 120 characters
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

- Return `Result<T, String>` from Tauri commands (Tauri serialises the String error to the frontend)
- Prefer `unwrap_or_else` over `unwrap` for graceful fallback
- `expect()` is acceptable at startup for genuinely fatal conditions
- Log errors via `append_debug_log` — never `eprintln!` or `dbg!` in production code

### Unsafe and global mutable state

- `unsafe_code = "forbid"` applies to the entire crate
- Global `static` variables must use atomic types (`AtomicBool`, `AtomicI32`, `AtomicU64`) — never `static mut`
- Shared state in `AppState` is always protected by `Mutex`

### Module structure

Keep modules focused on a single responsibility — see [CLAUDE.md](CLAUDE.md) for the module overview.
`#[tauri::command]` functions belong in `commands.rs`, not in domain modules.

---

## JavaScript

Configured via [eslint.config.mjs](eslint.config.mjs). Run `npm run lint:fix` for auto-fix.

### JS formatting

- **2-space indent**
- **Single quotes** for strings: `'value'` (exception: strings that contain `'`)
- **Semicolons** at the end of statements
- **Trailing commas** in multiline arrays, objects, and parameter lists
- Max line length: keep lines under ~120 characters

### JS naming

| Kind | Convention | Example |
|---|---|---|
| Variables, functions | `camelCase` | `updateCpuPanel`, `lastValidStats` |
| Module-scope constants | `SCREAMING_SNAKE_CASE` | `PANEL_KEYS`, `BASE_PROFILE_HEIGHT` |
| DOM id references | match the HTML id | `getElementById('cpuLoad')` |

### Modules

- All files use ES modules (`import`/`export`) — no global state via `window.x` except where the Tauri bridge requires it
- One file = one responsibility
- Export only what is needed externally — internal helpers are not exported

### JS error handling

- Always attach `.catch()` or use `try/catch` on async calls to the backend
- Empty catch blocks are acceptable when the error is genuinely non-critical — add a short comment explaining why: `catch (_e) { /* non-critical, ignore */ }`
- Frontend errors are logged via `backend.invoke('log-frontend-error', { message })` so they appear in the Status dialog

### JS comments

- Comment *why*, not *what*
- Describe the module's responsibility in a comment at the top of the file (2–4 lines)
- Avoid JSDoc on internal helpers

---

## CSS

CSS is written inline in the HTML files — no separate CSS files.

### CSS variables

All visual values live in `:root` in `index.html` as CSS custom properties:

```css
:root {
  --accent: #00c8ff;
  --text: #b8cce8;
  --panel-pad-y: 22px;  /* overwritten by applyProfileMetrics() */
}
```

- Never hardcode pixel values for layout — use the custom properties
- Values that scale with the dashboard profile are set via `setPxVar()` in `app.js`, not directly in CSS

### Color variables

| Variable | Usage |
|---|---|
| `--accent` | CPU load, primary highlight |
| `--amd` | GPU load |
| `--ram` | RAM |
| `--grn` | Network |
| `--pur` | Disk |
| `--text` | Body text |
| `--dim` | Inactive / secondary |

### CSS selectors

- Classes for reusable components (`.panel`, `.bar-row`, `.ring`)
- IDs only for elements referenced from JavaScript (`#cpuLoad`, `#gpuTemp`)
- Avoid `!important`

---

## HTML

- **2-space indent** inside `<head>` and `<body>`
- `lang` attribute on `<html>`: `lang="sv"` for Swedish UI
- `charset="UTF-8"` meta in all files
- Panels are marked with `data-panel="cpu"` etc. for JavaScript-driven visibility control
- Inline `<script type="module">` at the end of `<body>` — `defer` is unnecessary on module scripts

---

## AI‑generated code

AI assistants such as Claude or GitHub Copilot may be used during development, but all generated code must follow the same standards as human‑written code. To ensure consistency and maintainability, AI‑generated code must adhere to the following rules:

- Keep solutions simple and concrete; avoid unnecessary abstractions, traits, generics, or lifetimes.
- Generated Rust code must compile without warnings under the project’s lint configuration.
- Follow all naming, formatting, and module‑structure rules defined in this document.
- Do not introduce new crates without explicit approval; prefer existing dependencies.
- Error handling must follow project conventions (e.g., Result<T, String> for Tauri commands, no unwrap() in production paths).
- Generated JavaScript must follow the ESLint configuration and formatting rules.
- All AI‑generated code must be reviewed with the same scrutiny as human‑written code.
- AI should not restructure modules, rename files, or change architecture unless explicitly instructed.

This ensures that AI assistance improves productivity without degrading code quality or introducing stylistic drift.
