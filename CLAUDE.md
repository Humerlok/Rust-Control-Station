# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

This repository contains two codebases: the **legacy Mission Planner** (ArduPilot's C#/.NET
Framework WinForms GCS) and a **new Rust Control Station** being built in `rust/` to replace it.
The legacy app is the behavioral reference; new feature work goes in Rust.

Mission Planner configures, flashes, plans missions for, and monitors ArduPilot/PX4 vehicles
over MAVLink. The same C# source also targets Android (Xamarin) and macOS/iOS via a
cross-platform library build.

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
`npm run lint` runs ESLint 9 (flat config in `eslint.config.js`) on `src`.

On Linux the Tauri build needs an untracked `rust/desktop/src-tauri/icons/icon.png` (the config only lists
an `.ico`); see HANDOFF.md "Linux / Jetson development" for the run recipe and CDP caveats.

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
    commands/        — builders for arm/mode/reboot/takeoff/actions (+ ACTIONS list)
    dataflash/       — reader for ArduPilot .BIN logs (FMT-driven, resyncs on garbage)
    link/            — LinkManager (one connection, vehicles, stats, ACK-checked commands, packet bus, AUTO scan, log Replay)
                       plus services built on its public API: ParamService (download/set/metadata), CalibrationService,
                       InspectorService (per-message stats, decoded fields, watched series), MissionService
                       (mission/fence/rally upload, download, clear), FtpService (MAVFTP list/read/write/remove/mkdir/
                       rename/CRC; also feeds ParamService's fast `param.pck` download)
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
Tuning, MAVLink Inspector, mission planning). Phase 4 is half done: MAVFTP is implemented, firmware
flashing is still a placeholder (see HANDOFF.md).
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

`AGENTS.md` (repo guidelines, same rules as below) and `HANDOFF.md` (per-phase state, what is verified
vs. needs a real drone, next phase) are the running project notes; read `HANDOFF.md` before starting
new phase work. `Cargo.toml` raises opt-level for the dataflash crate even in dev builds because
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

## Legacy C# Mission Planner

### Build & run

Windows + Visual Studio 2022 + MSBuild only. No Linux/macOS build support.

```bat
git submodule update --init
.nuget\nuget.exe restore MissionPlanner.sln
msbuild -v:m -restore -t:Build -p:Configuration=Release MissionPlanner.sln
```

- Output: `bin\Release\net461\` (the `net461` folder name is intentional, not a separate target).
- `build.bat` / `build - debug.bat` / `build - Clean.bat` are maintainer scripts (packaging/signing).
- Import VS workloads via `vs2022.vsconfig`.

### Cross-platform library build (Android / Mac)

`MissionPlannerLib.csproj` (`netstandard2.0`) recompiles with `DefineConstants=...;LIB`.
Android packaging via `build - Lib.ps1` and `ExtLibs/Xamarin/Xamarin.Android`.

### Tests

`MissionPlannerTests/` — MSTest project (`net472`). CI runs MAVLink regression tests
(`Sysid32Tests`) but not the full suite.

```bat
vstest.console.exe bin\Release\net461\MissionPlannerTests.dll
vstest.console.exe MissionPlannerTests.dll /Tests:DetectBoardTest   :: single test
```

### CI

`.github/workflows/main.yml` ("DotNet Build") plus `android.yml`, `mac.yml`. Builds on
Windows with MSBuild; tagging `beta` publishes a release.

## Architecture (legacy C# — behavioral reference)

Dependency flow: **transport → protocol → vehicle state → UI**.

- **`Program.cs`** — entry point and crash handling.
- **`MainV2.cs`** — main Form, central hub. Owns `MainV2.comPort` (`MAVLinkInterface`), hosts
  menu, swaps `GCSViews` screens. ~190 KB — navigate by symbol.
- **`GCSViews/`** — top-level screens: `FlightData`, `FlightPlanner`, `InitialSetup`,
  `ConfigurationView`, `SITL`, `Help`.

### Connection & telemetry stack (`ExtLibs/`)

- **`ExtLibs/Comms/`** — transport layer (`ICommsSerial`): serial, TCP, UDP, BLE, NTRIP.
- **`ExtLibs/Mavlink/`** — MAVLink protocol: generated message structs, `MavlinkParse`, CRC.
- **`ExtLibs/ArduPilot/Mavlink/`** — live link: `MAVLinkInterface` (packet I/O, params,
  missions), `MAVState` (per-vehicle state + `CurrentState`), `MAVList` (multi-vehicle).
- **`ExtLibs/ArduPilot/CurrentState.cs`** — decoded telemetry model. UI binds to
  `MainV2.comPort.MAV.cs`.

The solution has ~93 projects; `ExtLibs/` is the bulk of functionality. Notable: `Controls/`,
`Core/` + `Interfaces/`, `Maps/` + `GMap.NET.*`, `GDAL`/`ProjNet`/`SharpKml`, `DroneCAN`.

### Plugins & scripting

- `Plugin/` + `Plugins/` — runtime plugin system (`Plugin` base class, `PluginLoader`).
- `Script.cs` + `Scripts/` — IronPython scripting host.

## Conventions & gotchas (legacy C#)

- **`LIB` conditional compilation is the #1 trap.** Code compiles as both desktop `Exe` and
  `netstandard2.0` library. Desktop-only APIs (`System.Drawing`, WinForms, P/Invoke) must be
  guarded with `#if !LIB` or the Android/Mac build breaks. `extern alias Drawing;` is guarded
  by `#if !LIB`; `ZZZLibShims.cs` provides cross-platform shims.
- **Localization.** English `.resx` is source of truth. **Do not hand-edit non-English `.resx`** —
  translations flow back via Crowdin. App strings use `Strings.resx` / `L10N` class.
- **WinForms file triplets.** `Foo.cs` (logic) + `Foo.Designer.cs` (generated layout) + `Foo.resx`.
- **`createproj.bat`** regenerates `<Compile>`/`<EmbeddedResource>` item lists when files change.
- **`ServiceLocator`** (`ExtLibs/Interfaces/ServiceLocator.cs`) — lightweight DI, resolve with
  `ServiceLocator.Get<T>()`.
- **`.editorconfig`** downgrades many `CAxxxx` rules to suggestion/silent — don't treat as warnings.
- Do not add Rust dependencies to the C# projects.
