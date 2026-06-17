# Contributing to RIGStats

Thanks for your interest in improving RIGStats! This guide covers everything you
need to set up the project, make a change, and get it merged.

RIGStats is a **Windows-only** project — both the egui app and the .NET sensor
sidecar depend on Windows APIs, WMI, and the PawnIO kernel driver. You will need
a Windows 10/11 x64 machine to build and test.

## Table of Contents

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

## Code of Conduct

Be respectful and constructive. Assume good faith, keep discussion focused on the
code, and help newcomers where you can.

## Prerequisites

- **Windows 10/11 x64** — the app and sensor sidecar are Windows-only
- **Rust stable** — install via [rustup.rs](https://rustup.rs)
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

> `cargo xtask verify` and `cargo xtask build` fail if the `rigstats-sensor`
> Windows Service is running, because the service holds the exe open. Stop it
> first (`sc.exe stop rigstats-sensor` in an elevated terminal), run the command,
> then restart the service.

## Development Workflow

Every bug fix and feature follows this sequence:

1. **Open a GitHub issue first** — describe the bug (incorrect behaviour, steps to
   reproduce, expected behaviour) or the feature (user-visible change and why it is
   needed) before starting work.
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
- **2-space indent** (project-specific — deviates from the Rust default of 4),
  max line width 120. `cargo fmt` handles it — never format manually.
- Standard Rust naming: `snake_case` functions, `PascalCase` types,
  `SCREAMING_SNAKE_CASE` constants.
- `unsafe_code = "forbid"` across the crate. Global `static` state uses atomics,
  never `static mut`; shared state in `AppState` is `Mutex`-protected.
- Fallible functions return `Result<T, String>`. Prefer `unwrap_or_else` over
  `unwrap`. Log via `append_debug_log` — never `eprintln!` or `dbg!` in production.
- Keep domain logic in `rigstats-backend/`; `src-egui/` is UI and wiring only.
- `//!` module docs at the top of each file; `///` on public items. Explain *why*,
  not *what*.

**egui secondary windows** must follow the dialog design system documented in
[CLAUDE.md — egui dialog design system](CLAUDE.md#egui-dialog-design-system)
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
4. Make sure the **Verify (Windows)** CI check passes — see below.

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
| `build/` | NSIS installer script + signed PawnIO kernel driver |

For a deeper architectural overview, read [docs/architecture.md](docs/architecture.md)
and [CLAUDE.md](CLAUDE.md).
