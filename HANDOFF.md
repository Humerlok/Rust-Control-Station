# Handoff: Rust Control Station

## Current state

This repository is the legacy C# Mission Planner plus a new Rust/Tauri foundation. The legacy build is unchanged. The Rust work is in `rust/` and the React UI is in `rust/desktop/ui/`.

The foundation is committed as `140b99445` (`rust: add control station foundation`). It contains crates for MAVLink framing, transport contracts, vehicle state, command/calibration models, map-provider policy, a Tauri shell, a React dashboard, and Windows/Linux CI. `AGENTS.md` documents contributor rules.

## Verified

- Rust stable/MSVC, `rustfmt`, Clippy, and Visual Studio 2022 Build Tools are installed.
- `cargo test --workspace` passes (5 tests).
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `npm run build` in `rust/desktop/ui` passes.
- The public `tile.openstreetmap.org` endpoint is intentionally rejected for offline prefetch.

Run Rust checks from `rust/` in a VS Developer Command Prompt, or initialize it with `VsDevCmd.bat -arch=x64` before invoking Cargo.

## Immediate next step

The last user request was to launch the app. Tauri initially failed because it was launched from `ui/` and could not discover `tauri.conf.json`. `rust/desktop/ui/package.json` now runs the CLI from `../src-tauri`; retry:

```powershell
cd rust/desktop/ui
npm run tauri
```

If the CLI still reports discovery issues, run `../ui/node_modules/.bin/tauri dev` from `rust/desktop/src-tauri` directly. The Tauri config uses `beforeDevCommand` to start Vite.

## Important limitations

The current backend uses an in-memory demo vehicle and parameter list. Serial/UDP/TCP are contracts only; real MAVLink I/O, heartbeat decoding, parameter protocol, calibration commands, map tile downloads, and SITL integration still need implementation. Do not describe the current UI as hardware-ready.

## Working rules

Keep the C# application as reference only. Implement protocol and safety behavior in Rust crates, not React. Use a licensed/self-hosted offline map provider; never add OSM public-tile prefetch. After changes, run Rust tests/Clippy and the UI build, then update this handoff with new blockers and commit IDs.
