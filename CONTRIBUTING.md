# Contributing to RIGStats

Thanks for your interest in improving RIGStats! This guide covers everything you
need to set up the project, make a change, and get it merged.

RIGStats is a **Windows-only** project — both the egui app and the .NET sensor
sidecar depend on Windows APIs, WMI, and the PawnIO kernel driver. You will need
a Windows 10/11 x64 machine to build and test.

**New here?** You don't need to read all of this to help. Testing RIGStats on
your hardware and reporting what works is just as valuable as code — see
[How You Can Help](README.md#how-you-can-help). For code, [Start in five
minutes](#start-in-five-minutes) is enough to get going; the rest of this guide is
reference.

## Table of Contents

- [Start in five minutes](#start-in-five-minutes)
- [Code of Conduct](#code-of-conduct)
- [Prerequisites](#prerequisites)
- [Getting Started](#getting-started)
- [Development Workflow](#development-workflow)
- [Commit Convention](#commit-convention)
- [Code Standards](#code-standards)
- [Testing Your Change](#testing-your-change)
- [Documentation Requirements](#documentation-requirements)
- [Opening a Pull Request](#opening-a-pull-request)
- [Continuous Integration](#continuous-integration)
- [Project Layout](#project-layout)

## Start in five minutes

What you need depends on what you change:

| You change… | You need |
| --- | --- |
| The app — dashboard, panels, dialogs, tray (`src-egui/`, `rigstats-backend/`) | Rust and the Visual Studio C++ Build Tools (below). That's it: the app runs against the installed RIGStats service. |
| The sensor service or Control Center hardware control (`sensor-sidecar/`) | Also the .NET 10 SDK, and the dev sidecar (`tools\dev-sidecar.ps1`, run elevated) — the installed service only accepts the signed release app, so a debug build talks to the dev sidecar instead. See "Live testing on real hardware" in [docs/control-architecture.md](docs/control-architecture.md). |
| Docs or the website | Nothing to build. |

Then: fork, clone, `cargo build --manifest-path src-egui/Cargo.toml`, run
`target\debug\rigstats.exe`. Small fixes can go straight to a pull request;
for anything bigger, open an issue first.

**AI tools** are welcome. If one wrote a substantial part of a change, say so in
the commit with a `Co-Authored-By:` line, as this project's own commits do. The
`CLAUDE.md` files are configuration for AI coding tools — a compact map of the
code and its rules; read them if they help, but nothing requires it.

## Code of Conduct

This project follows the [Code of Conduct](CODE_OF_CONDUCT.md). Be respectful
and constructive, assume good faith, and keep discussion focused on the code.

## Prerequisites

- **Windows 10/11 x64** — the app and sensor sidecar are Windows-only
- **Rust** — install [rustup.rs](https://rustup.rs); the toolchain version is pinned in [`rust-toolchain.toml`](rust-toolchain.toml), so `rustup` auto-installs the exact version CI uses the first time you run `cargo` in this repo
- **.NET 10 SDK** — `winget install Microsoft.DotNet.SDK.10` (required for the sensor sidecar)
- **Visual Studio 2022 Build Tools** with the "Desktop development with C++" workload (for linking)
- **NSIS** — `choco install nsis -y` (only needed for installer builds)

See [docs/setup.md](docs/setup.md) for the full local development setup, including
display profiles and installer builds.

## Getting Started

1. **Fork** the repository on GitHub and **clone your fork**:

   ```powershell
   git clone https://github.com/<your-username>/rigstats.git
   cd rigstats
   ```

2. **Install the git hooks** (run once after cloning):

   ```powershell
   cargo xtask setup
   ```

3. **Build and run** the debug binary:

   ```powershell
   cargo build --manifest-path src-egui/Cargo.toml
   .\target\debug\rigstats.exe
   ```

4. **Restart workflow** — Windows locks the exe while it runs, so kill the running
   process by PID before rebuilding:

   ```powershell
   Stop-Process -Id (Get-Process rigstats -ErrorAction Stop).Id -Force
   cargo build --manifest-path src-egui/Cargo.toml
   Start-Process .\target\debug\rigstats.exe
   ```

   Verify the exe timestamp changed before launching — if not, the process was
   still running and the build was skipped.

> The installed `rigstats-sensor` service runs from `C:\Program Files\RIGStats`
> and does not block builds. While `tools\dev-sidecar.ps1` runs, the debug
> sidecar exe in `sensor-sidecar\bin\Debug` is locked: stop it with Ctrl+C
> before building the sidecar.

## Development Workflow

Every bug fix and feature follows this sequence:

1. **Open a GitHub issue first** — describe the bug (incorrect behaviour, steps to
   reproduce, expected behaviour) or the feature (user-visible change and why it is
   needed) before starting work. Small fixes (a typo, a label, a broken link) can
   skip this and go straight to a pull request.
2. **Create a branch** from `main` in your fork.
3. **Implement** the fix or feature.
4. **Test in the running app** — see [Testing Your Change](#testing-your-change).
   Passing tests and a clean clippy are necessary but not sufficient; they verify
   code correctness, not behaviour.
5. **Run the checks** for whatever you changed (see below).
6. **Update documentation** — see [Documentation Requirements](#documentation-requirements).
7. **Commit** with a [Conventional Commits](#commit-convention) message that
   references the issue with `Closes #N`.
8. **Open a pull request** against `main`.

### Checks after making code changes

Always run the relevant checks before opening a PR — do not wait to be asked.

| Changed | Run |
| --- | --- |
| Any Rust file | `cargo xtask fmt` then `cargo xtask clippy` |
| Any `sensor-sidecar/*.cs` file | `dotnet build sensor-sidecar/sensor-sidecar.csproj` |
| Logic in Rust | `cargo xtask test` |
| Unsure | `cargo xtask verify` |

`cargo xtask clippy` runs with `-D warnings` — **zero warnings is the bar**. If
`cargo xtask fmt` modifies files, include those changes in the same commit. Do not
add `#[allow(...)]` to silence a lint without a clear reason documented in the code.

## Commit Convention

Commit subjects **must** follow [Conventional Commits](https://www.conventionalcommits.org/).
This is mandatory: [Release Please](https://github.com/googleapis/release-please)
parses commit subjects to generate `CHANGELOG.md` and bump the version. A commit
that does not follow this format is silently dropped from the changelog.

```
<type>(<scope>): <subject>

<optional body>

Closes #N
```

- **type** — one of: `feat` (new feature), `fix` (bug fix), `perf` (performance),
  `docs`, `refactor`, `test`, `build`, `chore`, `style`. Only `feat`, `fix`, and
  `perf` surface in the changelog; the rest are still required to be valid types.
- **scope** — the area changed, lower-case: e.g. `status`, `updater`, `cpu`, `gpu`,
  `settings`, `disk`, `readme`, `website`. Optional but expected.
- **subject** — imperative mood, no trailing period, lower-case start.
- A **breaking change** uses `feat!:` / `fix!:` or a `BREAKING CHANGE:` body footer.

Examples:

```
feat(gpu): add manual sensor override for multi-GPU systems
fix(updater): reset to Idle on close so Check for Updates reappears
docs(readme): update release instructions
```

## Code Standards

Full rules are in [STANDARDS.md](STANDARDS.md). The essentials:

**Rust**
- Standard `rustfmt` defaults (4-space indent, no `rustfmt.toml`). `cargo fmt`
  handles it — never format manually.
- Standard Rust naming: `snake_case` functions, `PascalCase` types,
  `SCREAMING_SNAKE_CASE` constants.
- `unsafe_code = "deny"` in both crates; Win32 FFI modules opt out with a
  scoped, documented `#[allow(unsafe_code)]` (see STANDARDS.md). Global
  `static` state uses atomics, never `static mut`; state shared across threads
  is behind `Mutex`/`Arc<Atomic*>`.
- Fallible functions return `Result<T, String>`. Prefer `unwrap_or_else` over
  `unwrap`. Log via `append_debug_log` — never `eprintln!` or `dbg!` in production.
- Keep domain logic in `rigstats-backend/`; `src-egui/` is UI and wiring only.
- `//!` module docs at the top of each file; `///` on public items. Explain *why*,
  not *what*.

**egui secondary windows** must follow the dialog design system documented in
[egui dialog design system](src-egui/src/windows/CLAUDE.md)
(three-panel layout, `gray(38)` surface, `theme::dialog_btn_*` buttons, the Mutex
extract-then-drop pattern).

**Keep it simple.** Prefer the simplest solution that solves the problem. Avoid
unnecessary abstractions, traits, generics, or lifetimes. Do not introduce new
crates without discussion — prefer existing dependencies. Do not restructure
modules, rename files, or change architecture unless the issue calls for it.

AI-assisted code is welcome but is held to the exact same standards and review
scrutiny as hand-written code.

## Testing Your Change

**Do not commit until the change is confirmed working in the running app.** Run
the app and verify the golden path **and** the edge cases for your change using the
kill → build → launch workflow above.

Run the Rust tests with `cargo xtask test`. Tests live in `#[cfg(test)]` modules at
the bottom of their respective files; most require Windows and the `wmi` feature.
Run a single test with:

```powershell
cargo test --manifest-path rigstats-backend/Cargo.toml classify_system_brand
```

## Documentation Requirements

A feature change must keep these consistent with the code — check all of them
before opening a PR:

| What changed | Where to update |
| --- | --- |
| New panel, data field, or backend module | `docs/architecture.md` — backend + renderer module sections |
| New panel or user-visible feature | `website/index.html` — panel count, panel/dialog card, hero copy |
| Feature complete or scope change | `ROADMAP.md` — mark done and add an implementation summary |
| New behaviour or architectural rule | `CLAUDE.md` — Architecture Overview section |

## Opening a Pull Request

1. Push your branch to your fork and open a PR against `dvalfrid/rigstats:main`.
2. Reference the issue in the PR description (and in a commit with `Closes #N`).
3. Describe the user-visible change and how you tested it in the running app.
4. Make sure the **Verify (Windows)** CI check passes — see below. The CodeQL
   jobs are not required to merge; Rust analysis takes ~20 minutes and only
   runs when Rust code changed.

A maintainer will review and merge. Release Please then folds your commit into the
next release automatically based on the commit type.

## Continuous Integration

`.github/workflows/verify.yml` runs on Windows for every push and pull request and
executes `cargo xtask verify`:

- Publishes the .NET sensor sidecar
- `cargo test` on `rigstats-backend` and `src-egui`
- `cargo clippy -- -D warnings`
- `cargo fmt --check`

Your PR must pass this check before it can be merged. See
[docs/release.md](docs/release.md) for the full CI and release pipeline.

## Project Layout

| Path | Contents |
| --- | --- |
| `src-egui/` | egui binary (`rigstats.exe`) — panels, tray, dialog windows |
| `rigstats-backend/` | Shared Rust lib — data sources, settings, hardware detection |
| `sensor-sidecar/` | .NET 10 C# Windows Service — LHM embedded, named-pipe server |
| `xtask/` | Cargo xtask — build, verify, fmt, clippy, test tasks |
| `docs/` | Architecture, setup, release, troubleshooting |
| `website/` | Product landing page source — not served at runtime |
| `build/` | NSIS installer script + official PawnIO setup |

For a deeper architectural overview, read [docs/architecture.md](docs/architecture.md)
and [CLAUDE.md](CLAUDE.md).
