# egui-wgpu 0.37.0 — RIGStats patch

Unmodified copy of `egui-wgpu` 0.37.0 from crates.io (upstream tag `0.37.0`,
commit `7c0b4040`) with one change, wired in through `[patch.crates-io]` in the
workspace `Cargo.toml`.

**Change:** `RenderState::create` (`src/lib.rs`) no longer keeps
`available_adapters`; it is emptied after the render adapter is chosen.

**Why:** on DX12, wgpu creates an `ID3D12Device` for every adapter it
enumerates and the `wgpu::Adapter` keeps it alive. Holding the list kept a
device open on the NVIDIA dGPU of a hybrid laptop even while rendering on the
iGPU — so the dGPU couldn't power down, and tools that switch to Eco mode
(G-Helper, Armoury Crate) terminated RIGStats as a "GPU app". RIGStats never
reads `available_adapters`.

**Remove when** upstream egui no longer holds the enumerated adapters (upstream
issue: <https://github.com/emilk/egui/issues/8704>; status, planned upstream PR
and notes in `docs/upstream/egui-8704.md`). To drop the patch: delete `third_party/egui-wgpu` and the
`[patch.crates-io]` entry, then check with `nvidia-smi` on a hybrid laptop that
`rigstats.exe` isn't listed.

Licence: MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`), as upstream.
