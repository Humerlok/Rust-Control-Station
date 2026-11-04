# Handoff: Rust Control Station

## Current state (2026-11-04)

Legacy C# Mission Planner plus the new Rust/Tauri app in `rust/`. Phase 1 is committed (`6535baf80`), Phase 2 is committed (`27fb0aa42`). **Phase 3 is implemented and verified but uncommitted.** The approved plan is `C:\Users\renan\.claude\plans\floofy-sauteeing-toast.md` (4 phases, stop after each). Next is Phase 4 (MAVFTP and firmware).

**Phase 3** (this change):
- **MAVLink Inspector**: `link::InspectorService` (per sysid/compid/message count, Hz and bytes/s over a 3 s window, last decoded fields, watched numeric series with every sample, ~4 Hz snapshots only while the screen is active) on top of `LinkManager::subscribe_packets()`. UI: tree System -> Component -> message, search by message/field/value, pinning, sort, field table with Graph buttons (arrays per element), uPlot live graph with 10/30/60 s/All windows, pause, clear, copy JSON/text.
- **Mission planning**: `mission` crate (`MissionItem`, `.waypoints` QGC WPL 110 parse/format), `ardupilot-meta` command catalog from `data/mavcmd.xml` (AC2 for Copter, APM for Plane, APRover for Rover), `link::MissionService` (MISSION_ITEM_INT only; download with per-item retries, upload with REQUEST_INT and legacy REQUEST, ACK handling, clear; mission/fence/rally via `mission_type`). UI: map with draggable numbered waypoints and path, editable grid with per-command parameter labels and Dist/Az/Grad columns, frames Relative/Absolute/Terrain, Read/Write/Clear on vehicle, Load/Save `.waypoints`, progress bar, home row.
- Replay keeps an in-memory mission/fence/rally store and speaks the mission protocol; it was split into `link/src/replay/`.
- The `mavlink` crate now has `mav2-message-extensions` (needed for `mission_type`); this added extension fields to some messages, so literals use `..Default::default()`.

## Verified

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (all pass), `npm run build`.
- The real Tauri window was driven over CDP with the Replay of the sample log: Inspector showed 6-8 message types with Hz/B/s/counts, decoded fields and a live graph with roll/pitch/yaw; the watched `roll` series returned 118 points in 3 s and a malformed key returned a clear error. Missions: catalog of 52 commands with labels, an upload of 5 items followed by a download returned identical items, `.waypoints` format/parse worked and a bad file gave a clear error, a fence upload and a clear worked. The Flight Plan screen read the 5 items from the "vehicle", drew waypoints and path on the Esri map and showed the grid with computed distance/azimuth/gradient.

## Not verified (needs the real drone)

Everything from Phases 1-2 plus: mission upload/download against a live ArduCopter (item ordering, MISSION_REQUEST_INT vs legacy MISSION_REQUEST, terrain frame, `mission_type` fence/rally on a real vehicle), the Inspector with real telemetry rates, drag-editing on a very long mission, and downloading files in WebView2 (Save file uses a Blob download; only checked in a browser). Test with props off.

## Known rough edges

- Mission items with a command or frame the MAVLink dialect does not know are rejected on upload and dropped on download (`UnknownCommand`/`UnknownFrame`).
- STATUSTEXT chunk reassembly (`id`/`chunk_seq`) is not implemented, so a long vehicle message arrives as several lines.
- Fence/rally editing is basic (same grid; default commands 5001/5100). `.plan`/KML/SHP import and survey grid generation from the legacy planner are not ported. Items without a position (TAKEOFF, RTL, DO_*) have no marker on the map.
- The Inspector decodes bitflags as text (`"A | B"`), so they cannot be graphed; `clear()` keeps the series time base.
- Parameters/Calibration/Extended Tuning limitations from Phase 2 remain (no param.pck fast download, no reset-to-default, Copter-only tuning).
- The Inspector view shows a ~81 px horizontal overflow at 1024 px width whose source was not found.

## Next: Phase 4 (stop after it)

MAVFTP (crate `mavftp`: list/read/burst read/create/write/remove/mkdir/rename/CRC32, tree + list UI, upload with CRC check to `/APM/scripts` for Lua; also unlocks the `@PARAM/param.pck` fast parameter download) and Firmware (manifest `firmware.ardupilot.org/manifest.json.gz`, board detection by USB ID and bootloader `GET_DEVICE`, `.apj` flashing via the px4/ArduPilot bootloader protocol with reboot-to-bootloader). Firmware flashing can brick a board: explicit confirmations, `board_id` check, no automated test on real hardware. See the plan file.

## Working rules

Keep the C# application as reference only. Protocol and safety behavior live in Rust crates, never in React. Use a licensed/self-hosted offline map provider; never add OSM public-tile prefetch (live viewing with attribution is fine). After changes run `cargo fmt`, Clippy with `-D warnings`, `cargo test --workspace` and `npm run build`, then update this file. Launch with `cd rust/desktop/ui && npm run tauri`; if port 5173 is busy an orphaned Vite from an earlier run holds it (stop only that PID). `tauri dev` does not rebuild when a new capability file appears: touch `src-tauri/src/main.rs`.
