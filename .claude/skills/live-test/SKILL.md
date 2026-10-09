---
name: live-test
description: Prepare and run a live test of a sidecar/Control Center change on the owner's real hardware with tools/dev-sidecar.ps1 — build the right branch (or several unmerged PRs combined), hand over to the owner, read the log, record the result on the PR. Use when a change to sensor-sidecar/ (lighting, fans, CPU/GPU limits, discovery) needs confirming on hardware, or the owner says "bygg om sidecarn", "testa live", "starta debug-appen".
---

Claude never starts the dev sidecar or writes to hardware. The owner runs `tools\dev-sidecar.ps1 -Live` in an elevated window; Claude builds, starts the debug app when asked, and reads logs. Background and pitfalls: [reference.md](reference.md).

## 1. Build the code under test

- One PR: check out its branch.
- Several unmerged PRs together: a throwaway local branch, never pushed —
  `git switch main; git pull; git switch -C local/live-test; git merge --no-edit <branch-a> <branch-b>`.
- The dev sidecar locks `sensor-sidecar\bin\Debug`. If it runs (the project hook blocks Debug builds), ask the owner to press Ctrl+C first. Release builds and `cargo xtask verify` work meanwhile.
- `dotnet test sensor-sidecar.Tests -c Release`, then `dotnet build sensor-sidecar/sensor-sidecar.csproj`.
- Compare times: `rigstats-sensor.dll` must be newer than the last source edit (`stat -c '%y'`). An up-to-date build in 1–2 s can be stale output.
- App changes too: `cargo build --manifest-path src-egui/Cargo.toml` (fails while `target\debug\rigstats.exe` runs — stop it by PID first).

## 2. Hand over

Tell the owner: start `pwsh -File tools\dev-sidecar.ps1 -Live` (elevated), then — if the test needs the Control Center — quit the installed RIGStats; Claude starts `target\debug\rigstats.exe`. List the exact log lines that prove the change and what to do (e.g. change a colour in the Lighting tab).

## 3. Check

- Dev sidecar running = the `rigstats-sensor` service is **Stopped** while a `rigstats-sensor` process exists.
- Log: `%ProgramData%\se.codeby.rigstats\rigstats-sensor.log` (the dev sidecar writes there too). Filter by the start time of this run; older lines are from the previous build.
- After a sidecar restart, restart the debug app before trusting what the Control Center shows (it can keep the old device list).

## 4. Finish

- `gh pr comment <n>` with what was tested, the proving log lines, and before/after.
- The owner presses Ctrl+C (the installed service starts again) and reopens the installed RIGStats.
- `git switch main`; delete `local/live-test` (`git branch -D local/live-test`).
