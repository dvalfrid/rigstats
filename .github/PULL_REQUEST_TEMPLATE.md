## Summary

<!-- What does this change, and why? -->

Closes #

## Testing

<!-- How did you verify this in the running app? Golden path + edge cases. -->

## Checklist

- [ ] Commit subject follows [Conventional Commits](https://www.conventionalcommits.org/) (`type(scope): subject`) — see [CONTRIBUTING.md](../CONTRIBUTING.md#commit-convention)
- [ ] Ran the checks for what changed: `cargo xtask fmt` + `cargo xtask clippy` (Rust), `dotnet build sensor-sidecar/sensor-sidecar.csproj` (sidecar), `cargo xtask test` (logic changes) — or `cargo xtask verify` if unsure
- [ ] Verified the change in the running app, not just tests/clippy
- [ ] Updated docs if needed (`docs/architecture.md`, `website/index.html`, `ROADMAP.md`, `CLAUDE.md` — see [Documentation Requirements](../CONTRIBUTING.md#documentation-requirements))
