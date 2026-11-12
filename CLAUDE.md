# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

This repository contains the **Rust Control Station** in `rust/`, the successor of ArduPilot's
Mission Planner. The legacy C# code was removed from the tree; it lives in git history under the
tag `legacy-mission-planner` (`git show legacy-mission-planner:<path>`) and is the behavioral
reference. New feature work goes in Rust.

## Rust Control Station (`rust/`)

### Build & run

Requires Rust stable toolchain and Node 20+. Run from `rust/`:

```powershell
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Single test / single crate:

```powershell
cargo test -p <crate> <test_name>              # e.g. cargo test -p link --test mission round_trip
cargo test -p link --test loopback             # one integration-test file (crates/*/tests/*.rs)
```

Desktop app (Tauri 2 + React + Vite):

```powershell
cd rust/desktop/ui
npm ci
npm run build          # typecheck + vite build
npm run tauri          # launch desktop app (runs from ../src-tauri)
```

UI-only loop (no Tauri, mock backend): `npm run dev` in `rust/desktop/ui`, then open `/?mock=1`.
`npm run lint` runs ESLint 9 (flat config in `eslint.config.js`) on `src`. The UI has no unit-test script;
`npm run build` (typecheck) and `npm run lint` are its checks, and `?mock=1` is the manual loop.

The icon set in `rust/desktop/src-tauri/icons/` is tracked, so Linux builds need nothing extra; see HANDOFF.md
"Linux / Jetson development" for the run recipe and CDP caveats. Beta installers (Windows/Linux, x86_64/ARM64) are
built by `.github/workflows/beta-release.yml` on a `beta-*` tag; see HANDOFF.md "Beta installers" and `Beta/BETA.md`.

If the Tauri CLI can't find `tauri.conf.json`, run directly from `rust/desktop/src-tauri`:
```powershell
../ui/node_modules/.bin/tauri dev
```

### Workspace layout

```
rust/
  crates/
    mavlink/         — wraps the `mavlink` crate (ardupilotmega dialect): incremental v1/v2 Parser with CRC, encode()
    transport/       — open(Endpoint) -> Connection with split reader/writer: serial, TCP, UDP, UDPCl, WebSocket
    vehicle-state/   — VehicleState::apply(MavMessage), the named numeric FIELDS used by Quick View
    ardupilot-meta/  — VehicleType (from MAV_TYPE), flight-mode tables, pdef parameter metadata (parse + cached download),
                       mission command catalog (data/mavcmd.xml, copied from the repo root)
    params/          — ParamTable, .param file parse/format, natural sort (pure, no I/O)
    mission/         — MissionItem and the QGC WPL 110 `.waypoints` parser/formatter (pure)
    mavftp/          — MAVFTP wire format: frame encode/decode, NAK codes, directory listing parser, ArduPilot CRC32 (pure)
    firmware/        — firmware manifest, .apj images and the serial bootloader protocol (pure)
    dronecan/        — DroneCAN (UAVCAN v0): vendored DSDL embedded at build time (`dsdl/`), DSDL parser with 64-bit
                       signatures (checked against the legacy table in `tests/golden_signatures.txt`), data-driven bit
                       codec (`Value`), CAN transfer layer (29-bit id, tail byte, CRC16, multi-frame, reassembly) (pure)
    commands/        — builders for arm/mode/reboot/takeoff/actions (+ ACTIONS list)
    dataflash/       — reader for ArduPilot .BIN logs (FMT-driven, resyncs on garbage; offset index per message id,
                       single-column reads, FMTU/UNIT/MULT units and the `#` instance column)
    speedlog/        — SpeedLOG log analysis (port of github.com/Humerlok/SpeedLOG): field tree with instances
                       (`IMU[1]`), series with min/max reduction for a window, flight modes, PARM changes, MSG/ERR/EV
                       decoded, presets, embedded ArduPilot message/field/parameter docs in EN+PT
                       (`data/descriptions/*.json.gz`, provenance in `data/SOURCE.md`) (pure)
    link/            — LinkManager (one connection, vehicles, stats, ACK-checked commands, packet bus, AUTO scan, log Replay)
                       plus services built on its public API: ParamService (download/set/metadata), CalibrationService,
                       InspectorService (per-message stats, decoded fields, watched series), MissionService
                       (mission/fence/rally upload, download, clear), FtpService (MAVFTP list/read/write/remove/mkdir/
                       rename/CRC; also feeds ParamService's fast `param.pck` download), FirmwareService (manifest/.apj download,
                       bootloader identify and flash), MotorTestService (DO_MOTOR_TEST with the safety rules, ESC
                       telemetry), DroneCanService (CAN forward relay as node 127: node table, logs, node parameters,
                       save, restart, node firmware update through a file server, dynamic node id allocation, bus
                       inspector with the same snapshot format as the MAVLink one), LogService (onboard logs: lists
                       `/APM/LOGS` over MAVFTP when available, else LOG_REQUEST_LIST; downloads by FTP or LOG_DATA)
    map-cache/       — TileProvider, OfflineRegion, download policy validation
  desktop/
    src-tauri/       — thin Tauri shell: commands call LinkManager; telemetry/statustext/link events go to the UI
    ui/              — React + TypeScript + Vite; Nord theme; src/api.ts is the only place that calls invoke/listen
```

Domain crates must stay independent of Tauri and React. Tauri commands translate UI
requests into domain calls; React must not own protocol logic.

The JSON contract between `link` and the UI is camelCase and pinned by tests in
`crates/link/src/types.rs`; `ui/src/types.ts` and `ui/src/mockBackend.ts` mirror it. Tauri 2 denies
event listening unless `src-tauri/capabilities/default.json` grants it; adding a capability file
needs a rebuild that `tauri dev` does not trigger by itself (touch `src/main.rs`).

### Running without a drone

Connect type "Replay (log)" plays `exemplos/Teste 02 - tunning.BIN` (relative paths are also tried
from parent directories). The replay serves the log's PARM records as parameters (read, list, set
with echo), keeps an in-memory mission/fence/rally store (upload then download round-trips) and
answers every command with "unsupported". `exemplos/` is git-ignored: logs are large and are
telemetry.

To drive the real window without the OS foreground (screenshots, scripted clicks), launch with
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` and talk CDP to the page;
`window.__TAURI_INTERNALS__.invoke` works from `Runtime.evaluate`. `?mock=1` runs the UI alone.

### Current limitations

Phases 1-3 are done (connect, Flight Data, actions, reboot, parameters, calibration, Extended
Tuning, MAVLink Inspector, mission planning). Phase 4 is implemented: MAVFTP and firmware
flashing (see HANDOFF.md; the user flashed a Cube Orange with it). Tools > SpeedLOG (log
plotter, with "Vehicle logs" download) is implemented on branch `feature/speedlog`; its Hardware Report
and PID Review views are not ported yet. Phase 5 (Motor Test) and
Phase 6 (DroneCAN nodes, parameters, restart, node firmware update, node id allocation and the
DroneCAN Inspector) is implemented. DroneCAN has only been tested
against a fake CAN node behind a fake MAVLink vehicle, never on a real CAN bus.
Serial, AUTO scan, WebSocket and real-vehicle command/parameter/calibration/mission flows are
covered only by loopback and Replay tests, not by hardware.

The `mavlink` crate is built with `mav2-message-extensions` (needed for `mission_type` on
fence/rally); it adds extension fields to some messages (e.g. STATUSTEXT `id`/`chunk_seq`), so
struct literals of dialect messages need `..Default::default()`.

### CI

`.github/workflows/rust-control-station.yml` runs on push/PR when `rust/` or `AGENTS.md`
changes: `cargo fmt --check`, `cargo clippy`, `cargo test`, plus `npm install && npm run build`
for the UI, on both Windows and Ubuntu.

### Related docs

`docs/ARCHITECTURE.md` is the human contributor guide (data flow, where things live, how to add a feature end to end).
`AGENTS.md` (repo guidelines, same rules as below) and `HANDOFF.md` (per-phase state, what is verified
vs. needs a real drone, next phase) are the running project notes; read `HANDOFF.md` before starting
new phase work. `Beta/BETA.md` is the tester-facing beta guide (Portuguese). `Cargo.toml` raises opt-level for the dataflash crate even in dev builds because
parsing large `.BIN` logs is otherwise unusably slow.

### Style & rules

- Rust 2021 edition, `rustfmt`, Clippy-clean (`-D warnings`).
- `snake_case` for modules/functions, `PascalCase` for types.
- Explicit domain types — not UI-shaped JSON.
- Use a configured/self-hosted tile provider for offline maps. **Never prefetch from
  `tile.openstreetmap.org`**; show required map attribution in the UI.
- Tests go alongside modules; cross-crate tests under each crate's `tests/`. Use loopback
  UDP/TCP or fakes for transport tests; hardware/SITL tests must be opt-in.
- Commit messages: concise imperative subject with area prefix (`desktop: add connection status`).
