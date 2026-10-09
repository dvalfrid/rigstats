# Live test — background and pitfalls

Full method: "Live testing on real hardware" in `docs/control-architecture.md`.

## Which app talks to which sidecar

| | Installed service (session 0, Program Files) | Dev sidecar (owner's console, repo `bin\Debug`) |
|---|---|---|
| **Installed release app** | ✅ | ❌ — `is_service_pipe` (`rigstats-backend/src/pipe_server.rs`) only accepts a server in session 0. The sidecar log then fills with `[rigstats-sensor] Client connected.` / `Client disconnected.` about once a second, and the Lighting tab never reaches the dev sidecar. Harmless, but the test is not running through the UI. |
| **Debug app** (`target\debug\rigstats.exe`) | ❌ — `PipeClientVerifier`: `Client rejected: client image path '…\target\debug\rigstats.exe' is not the installed rigstats.exe.` | ✅ — debug sidecars skip the verifier, debug apps skip the session check |

Single-instance guard: a second RIGStats only focuses the first, so quit the installed app before starting the debug one.

## Things that looked like bugs but weren't (2026-10-09)

- A colour change seen right after start came from the dev sidecar re-applying the active profile (`Re-applied active profile '…' at start-up`), not from the UI.
- The Control Center still showed an old device name after the sidecar restarted with a fix; restarting the debug app showed the new one.
- The test build lacked a fix that had just been merged to `main`: the PR branch predates it. Merge `origin/main` into the branch (or use `local/live-test`) before building.
- `cargo xtask verify` failed with `failed to remove file target\debug\rigstats.exe — Access is denied`: the debug app was running. C#-only changes can be checked with `dotnet test -c Release`; CI runs the full verify.

## Proving a fix

Pick log lines that only the new code can produce (a new log line, a new name, the absence of an error that used to appear on every change) and quote them on the PR. "It changed colour" alone can come from the start-up re-apply.
