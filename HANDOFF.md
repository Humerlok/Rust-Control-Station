# Handoff: Rust Control Station

## Current state (2026-11-04)

Legacy C# Mission Planner plus the new Rust/Tauri app in `rust/`. Phase 1 is committed (`6535baf80`). **Phase 2 is implemented and verified but uncommitted.** The approved plan is `C:\Users\renan\.claude\plans\floofy-sauteeing-toast.md` (4 phases, stop after each). Next is Phase 3.

**Phase 1** (committed): connection types, Flight Data, Actions, Quick View, messages, reboot, Nord theme.

**Phase 2** (this change):
- **Parameters**: `params` crate (table, `.param` files, natural sort), `ardupilot-meta` pdef metadata (downloaded from autotest.ardupilot.org, cached 7 days, stale cache used offline), `link::ParamService` (PARAM_REQUEST_LIST with re-requests, per-index reads, PARAM_SET with echo check). UI: prefix tree like the legacy tool, search, Modified/Favorites/Advanced filters, value selects/bitmask editors from metadata, Write (ENABLE params last, confirm over 20 changes), Save/Load/Compare `.param`, virtualized list.
- **Calibration**: `link::CalibrationService` (accel full/level/simple, gyro, baro, compass with MAG_CAL progress/report, radio min/max/trim written through the parameter service, ESC via `ESC_CALIBRATION=3`). Refuses while armed and while another calibration runs. Gyro/baro/level use a 30 s single-shot ACK wait (a resend would restart them).
- **Extended Tuning**: Copter groups from the legacy `ConfigArducopter.cs` in `ui/src/tuning/copter.ts`, each control resolving the first candidate parameter name that exists.
- `LinkManager::subscribe_packets()` is the packet bus services build on (the Phase 3 inspector will use it too); `send_command_with` takes explicit ACK timing.
- Replay now loads in ~2.7 s (single pass) and serves parameters from the log's PARM records (1166 distinct names).
- Removed dead parameter/calibration types from `commands`.

## Verified

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `npm run build` all pass.
- The real Tauri window was driven over CDP with the Replay of the sample log: 1166 parameters downloaded in 2.5 s, `param_set` echo, unknown-name error, official metadata fetched from the internet (5803 entries), `.param` export/parse, gyro/radio calibration commands. Screenshots of Parameters (tree with counts, units, ranges, descriptions) and Extended Tuning (the drone's real PID values) looked right.
- UI bug found that way and fixed: the store now reloads the list/metadata on every finished download and retries metadata when it came back empty (no vehicle yet at connect time).

## Not verified (needs the real drone)

Everything from Phase 1 plus: parameter download/write over a real link (ArduPilot sends PARAM_VALUE as C-cast floats; large INT32 values lose precision in the float), PARAM_SET echoes on a live vehicle, the accelerometer/compass/radio/ESC flows against a live ArduCopter, and the Calibration screen with a real armed/disarmed vehicle. Test with props off. The radio flow could not be exercised through Replay (the log has no RC_CHANNELS); it is covered by the loopback tests.

## Known rough edges

- MAVFTP `@PARAM/param.pck` (fast download) is not implemented; it arrives with MAVFTP in Phase 4. Metadata has no offline fallback bundle: first use needs the internet.
- No "Reset to default" / commit-to-storage buttons yet; compass-mot, motor test and DroneCAN parameters are not ported.
- Extended Tuning flags a filter set to 0 (disabled) as out of range because the metadata range starts at 5 Hz.
- Takeoff on a Copter sets Guided first; if rejected, no takeoff is sent. The Force dialog appears after any failed arm/disarm.
- The Extended Tuning screen is Copter only.

## Next: Phase 3 (stop after it)

MAVLink Inspector (tree sysid -> compid -> message with Hz, bytes/s, count, search, pin, graph with uPlot; use `subscribe_packets()`), and mission planning (map editing, command grid from `mavcmd.xml`, upload/download protocol, `.waypoints`). See the plan file.

## Working rules

Keep the C# application as reference only. Protocol and safety behavior live in Rust crates, never in React. Use a licensed/self-hosted offline map provider; never add OSM public-tile prefetch (live viewing with attribution is fine). After changes run `cargo fmt`, Clippy with `-D warnings`, `cargo test --workspace` and `npm run build`, then update this file. Launch with `cd rust/desktop/ui && npm run tauri`; if port 5173 is busy an orphaned Vite from an earlier run holds it (stop only that PID). `tauri dev` does not rebuild when a new capability file appears: touch `src-tauri/src/main.rs`.
