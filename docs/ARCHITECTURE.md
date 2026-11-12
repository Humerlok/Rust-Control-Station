# Architecture and contributor guide

This is the map for people new to the code. For commands and style rules see [CLAUDE.md](../CLAUDE.md) and
[AGENTS.md](../AGENTS.md); for what is verified on real hardware see [HANDOFF.md](../HANDOFF.md).

## The big picture

```
 ArduPilot vehicle
      │  MAVLink (serial / TCP / UDP / WebSocket)  or a Replay of a .BIN log
      ▼
 transport ──► mavlink (parser) ──► link::LinkManager ──► vehicle-state (named numeric fields)
                                         │  broadcast buses: events + every packet
                                         ▼
                  link services: params, calibration, mission, ftp, firmware,
                  motor test, inspector, dronecan   (built only on LinkManager's public API)
                                         │
                                         ▼
              desktop/src-tauri  (thin shell: #[tauri::command] + event pumps)
                                         │  invoke / emit
                                         ▼
              desktop/ui  (React; src/api.ts is the only code that talks to Tauri)
```

Rules that keep it maintainable:

- **Domain crates** (`rust/crates/*`) never depend on Tauri or React. The ones marked "pure" in
  CLAUDE.md (`params`, `mission`, `mavftp`, `firmware`, `dronecan`, ...) do no I/O at all, so they are tested with
  plain unit tests.
- **The Tauri shell is thin.** A command translates a UI request into a call on `LinkManager` or a service and
  turns the error into a string. Protocol logic, safety rules and timeouts live in the crates.
- **React does not own protocol logic.** Screens render state and call `api.*`.
- **Safety rules live in the service**, for example `MotorTestService` refuses to spin motors on an armed vehicle,
  whatever the UI does.

## Where things are

| You want to... | Look at |
|---|---|
| change how a connection is opened or kept alive | `crates/link/src/manager.rs` (module docs explain the tasks), `crates/transport` |
| add a telemetry value for Quick View or the HUD | `crates/vehicle-state/src/fields.rs` (`FIELDS`, `apply`), then read `telemetry.fields.<key>` in the UI |
| add a vehicle action (a button) | `crates/commands` (builder + `ACTIONS`), `LinkManager` method, command in `src-tauri/src/main.rs` |
| change a screen | `desktop/ui/src/views/<Screen>.tsx`, with the helpers in `components/` |
| change the JSON the UI receives | `crates/link/src/types.rs` (tests pin it), mirror in `ui/src/types.ts` and `ui/src/mockBackend.ts` |

## Adding a feature end to end

1. **Domain code** in the right crate (pure logic first, with unit tests next to it).
2. **A service in `link`** if it needs the vehicle: build it on `LinkManager` (`send_command`, `subscribe_packets`),
   never on the transport. Add an integration test in `crates/link/tests/` against a fake vehicle (loopback UDP/TCP).
3. **Tauri commands** in `src-tauri/src/<feature>_cmds.rs`; register them in `generate_handler![...]` in `main.rs`.
   For progress/state events, spawn a pump task in `main()` like the existing ones.
   Listening to events needs the grant in `src-tauri/capabilities/default.json`; a capability change needs a rebuild
   (`touch src/main.rs`) that `tauri dev` does not trigger by itself.
4. **UI**: add the call to `ui/src/api.ts` (real and mock sides), the types to `ui/src/types.ts`, a `mock*.ts` for the
   fake backend, then the screen. Keep the mock realistic: it is what lets people work without a drone.
5. **Docs**: update `HANDOFF.md` (what is verified and what is not) and the crate table in `CLAUDE.md` if a crate or
   service was added.

## Running without a drone

- **Replay (log):** connection type "Replay (log)" plays `exemplos/Teste 02 - tunning.BIN` (git-ignored; put a log
  there). It serves the log's parameters and an in-memory mission/fence/rally store, and answers commands with
  "unsupported".
- **UI only:** `npm run dev` in `rust/desktop/ui`, then open `/?mock=1`. No Rust needed.

## Checks before a pull request

```
cd rust
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd desktop/ui && npm run lint && npm run build
```

CI runs the same on Windows and Ubuntu (`.github/workflows/rust-control-station.yml`).

## Releases

Version must match in `rust/Cargo.toml`, `rust/desktop/src-tauri/tauri.conf.json` and `rust/desktop/ui/package.json`
(plus the lockfiles). A `beta-X.Y.Z` tag pushed to GitHub triggers `.github/workflows/beta-release.yml`, which builds
the Windows and Linux installers. Tester-facing steps are in [Beta/BETA.md](../Beta/BETA.md).

## Comment conventions

- Every file starts with a short `//!` (Rust) or `/** */` (TypeScript) saying what it is for.
- Comments explain **why** (units, safety rules, protocol quirks, workarounds), not what the next line does.
- Keep them one line when possible, and update them in the same change as the code.
