# Handoff: Rust Control Station

## Current state (2026-11-04)

Legacy C# Mission Planner plus the new Rust/Tauri app in `rust/`. Everything below is **uncommitted** (last commit `7d6b25a3d`). The approved plan is in `C:\Users\renan\.claude\plans\floofy-sauteeing-toast.md` (4 phases, stop after each).

**Phase 1 is implemented and verified**: connection types, Flight Data, Actions, Quick View, messages, reboot, Nord theme.

- `mavlink`: real v1/v2 parser with CRC (wraps the `mavlink` 0.21 crate, features `dialect-ardupilotmega`, no built-in transports).
- `transport`: Serial (`tokio-serial`), TCP, UDP server, UDPCl, WebSocket, each with split reader/writer.
- `link`: `LinkManager` (heartbeat timeout 5 s, GCS heartbeat 1 Hz, data-stream requests, loss/quality stats, COMMAND_ACK waits with resend, AUTO serial scan, DataFlash replay).
- `vehicle-state`, `ardupilot-meta`, `commands`, `dataflash`: see CLAUDE.md.
- Tauri shell: thin commands + `telemetry` (10 Hz when dirty), `statustext` and `link` events. `capabilities/default.json` grants `core:event:default` (without it the UI never receives events).
- UI (`rust/desktop/ui/src`): `ConnectBar`, `RebootButton`, `FlightMap` (Esri satellite default, OSM streets, live tiles only), `Hud`, `QuickView`/`QuickPicker`, `MessagesPanel`, `ActionsPanel`. `?mock=1` runs it in a plain browser against `mockBackend.ts`.

## Verified

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (all pass), `npm run build`.
- The real Tauri window was driven over CDP (`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`, then a WebSocket to the page): Replay of the sample log connected through the UI, showed Copter/Loiter at -20.5475,-47.4203, 49.6 V, real STATUSTEXT, Esri tiles loaded; Arm returned "denied: unsupported" and opened the Force dialog.
- The Phase 2 limitations of the demo (fake parameters, always-"Connected" UI) are gone: commands now fail with "no vehicle connected; nothing was sent" without a live vehicle.

## Not verified (needs the real drone)

Real serial (CubeOrange+ over USB/telemetry), AUTO scan, WebSocket against a real server, and every command against a live ArduCopter (arm, mode, takeoff, fly-to, set home, reboot, actions). Test with props off and the drone on the bench. `Terminate_Flight`, `Format_SD_Card` and the reboot actions are in the Actions list behind confirm dialogs.

## Known rough edges

- Replay load parses the 79 MB log once per message type (~8 s before the link opens); a single pass would be faster.
- Takeoff on a Copter sets Guided first; if that is rejected, the takeoff is not sent.
- The Force dialog appears after any failed arm/disarm, including "no ACK" and "unsupported".
- `transport::natural_key`, Replay speeds above 8x and `Loiter_Unlim`/`Do_Parachute`/engine actions from the legacy list were left out on purpose.

## Next: Phase 2 (stop after it)

Parameters by category (download, tree by prefix, pdef metadata, write with echo check, .param files), Calibration screen, Extended Tuning. See the plan file.

## Working rules

Keep the C# application as reference only. Protocol and safety behavior live in Rust crates, never in React. Use a licensed/self-hosted offline map provider; never add OSM public-tile prefetch (live viewing with attribution is fine). After changes run `cargo fmt`, Clippy with `-D warnings`, `cargo test --workspace` and `npm run build`, then update this file with blockers and commit IDs. Launch with `cd rust/desktop/ui && npm run tauri`; if port 5173 is busy an orphaned Vite from an earlier run is holding it.
