# Handoff: Rust Control Station

## Current state (2026-11-06)

Legacy C# Mission Planner plus the new Rust/Tauri app in `rust/`. Phases 1-6 are committed and pushed to `master`: `6535baf80` (P1), `27fb0aa42` (P2), `dc597b77b` (P3), `286e407c8` (P4a MAVFTP), `ad2b8063c` (P4b Firmware), `fc7fcb7b1` + `2d6067d5a` (P5 Motor Test), `70c7c108a` (P6a), `4523c7fe0` (P6b), `ad239dc97` (P6c DroneCAN) and `a4335754c` (beta installers). **Phase 4 was built on a Jetson Orin running Ubuntu, and the user tested it personally against a real Cube Orange.** The details of what worked or failed there were not recorded in the repo, so the "Not verified" lists below may be partly outdated (see "Hardware testing status"). The original plan file (`C:\Users\renan\.claude\plans\floofy-sauteeing-toast.md`) lives on the Windows machine.

Checks on 2026-11-06 (Linux ARM64, Jetson): `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (298 passed, 0 failed), `npm run build` and `npm run lint` all clean. The UI build warns that one chunk is over 500 kB (harmless, code-splitting would fix it).

## SpeedLOG: log plotter in Tools (branch `feature/speedlog`, 2026-11-12)

SpeedLOG (github.com/Humerlok/SpeedLOG, the JS offline log viewer) rewritten in Rust and added as Tools > SpeedLOG.
Phase 1 is the plotter plus downloading logs from the connected vehicle; **Hardware Report and PID Review (FFT,
step response) are phases 2 and 3, not started**.

- `crates/dataflash`: offset index per message id (`records_of`, `count`, `id_of`), `column(id, col)` decodes one
  column only, `column_info(id)` gives units/multipliers from FMTU/UNIT/MULT and the `#` instance column.
- New pure crate `crates/speedlog`: `SpeedLog::open/from_bytes` -> `info`, `tree(lang)` (instances apart, text-only
  and empty types left out, untimed types flagged), `series` (cached; FMTU multiplier applied to raw integer
  columns only, since `cCeEL` are already scaled by the reader), `series_view` (min/max of N buckets for a window,
  one sample of margin, raw min/mean/max), `modes` (names from `ardupilot-meta`), `params` (first/last, changed,
  docs and meaning of options/bitmask), `messages` (ERR/EV texts ported from SpeedLOG `i18n.js`), `presets`.
  Descriptions are SpeedLOG's `descriptions.js` chunks at commit `ee8253d6`, imported by
  `crates/speedlog/tools/import_descriptions.cjs`; parameter docs are embedded too (offline, no pdef download).
- `link::LogService` (`crates/link/src/logs.rs`, tests in `crates/link/tests/logs.rs`): the LOG_ENTRY id is a list
  position, not the file number (ArduPilot `AP_Logger_Backend::log_num_from_list_entry`: `oldest_log + list_entry - 1`,
  wrapping at the max log count), so an FTP path is never built from it: when `/APM/LOGS` lists over MAVFTP that
  listing is used, else LOG_REQUEST_LIST + LOG_REQUEST_DATA (gap re-requests, LOG_REQUEST_END). Shares the
  `FtpService` of the file screen. Replay answers "no logs".
- Desktop: `speedlog_*` commands (`speedlog_cmds.rs`, the log stays in Rust; `speedlog_open` uses a native open
  dialog from `save_dialog.rs`), `logs_list/open/save/cancel` (`log_cmds.rs`, downloads cached in the app cache dir
  under `vehicle-logs/`), event `logs-progress`. UI: `views/SpeedLog.tsx`, `components/speedlog/` (uPlot panels with
  synced cursor, shared drag-zoom, mode shading, message marker, chips with window stats), `mockSpeedLog.ts`.
- Intentional difference from SpeedLOG web: JsDataflashParser only prefixes the unit with the FMTU multiplier
  (raw integer shown as e.g. `cm`), speedlog scales raw integer columns and shows the base unit (`m`). A side by
  side comparison will show this on fields such as integer altitudes; it is not a regression.
- Verified: unit tests (dataflash, speedlog incl. a synthetic log with instances/units/modes/params/ERR/EV),
  LogService loopback tests, `?mock=1` in a browser (presets, zoom, readout, messages -> marker, vehicle logs
  download). **Not verified**: a real `.BIN` in the Tauri app (no sample log on this machine), download from a real
  vehicle over MAVFTP or LOG_DATA, very large logs (memory: the whole file is kept in RAM, as SpeedLOG does).

## Beta adjustments from `reajustes.md` (2026-11-09)

- **Save dialogs**: MAVFTP Download, Flight Plan "Save file" and Parameters "Save to file" open a native "Save as" dialog (`tauri-plugin-dialog`, opened from Rust in `src-tauri/src/save_dialog.rs`) and write the file there. The commands are `ftp_download_save`, `mission_save_file`, `params_save_file`; each returns the saved path or `null` on cancel. They replace `ftp_download`, `mission_format_file` and `params_format_file`. The webview gets no generic "write a path" command. In `?mock=1`, saving falls back to a browser download (`mockDownload.ts`).
- **MAVFTP Places**: built from the vehicle the first time the screen is shown on a connection. It lists `/` and the APM directory (two requests only, so it does not compete with the `param.pck` download), then shows `/`, `/APM`, the real subfolders spelled as the vehicle spells them (`LOGS`/`logs`, `scripts`) and any `@...` virtual trees listed in `/`. The fixed `/APM/lua` entry is gone.
- **Quick View**: `vehicle-state/src/fields.rs` holds 352 fields with the legacy `CurrentState` keys and `[GroupText]` groups. The picker groups them. New sources:
  - GPS2_RAW (`satcount2`, `gpsstatus2`, `gpshdop2`, `lat2`...) and the GPS_RAW_INT extras
  - BATTERY_STATUS by id (batteries 1-9, cells) and BATTERY2
  - POWER_STATUS, HWSTATUS, MCU_STATUS, EKF_STATUS_REPORT, vibration clipping, WIND
  - RANGEFINDER, DISTANCE_SENSOR, RADIO_STATUS, SCALED_PRESSURE 1/2, TERRAIN_REPORT, AHRS2, RPM
  - FENCE_STATUS, GENERATOR_STATUS, EFI_STATUS, MOUNT_STATUS
  - RC 1-16, servo 1-16, IMU 1-3, ESC 1-16, PID_TUNING, OPTICAL_FLOW, EXTENDED_SYS_STATE, velocities and climb rate
  - computed time in air, distance traveled and turn rate

  Not ported: Transponder, the CAN/UAVCAN entries, `Software`/version strings, home-derived values (HomeAlt, ToH/ToT, km left), the sensor-health booleans, and the rest of the legacy properties that are not plain message fields.
- **HUD**: the flight mode is drawn large under the horizon and turns red for 2 s after a change (legacy behaviour).
- **Message banner** (`MessageBanner.tsx`): shows STATUSTEXT at WARNING or worse, plus `PreArm:`/`Arm:`/`Tuning:`, over the bottom of the map. Colours: red for 0-2, orange for 3, yellow for 4 and pre-arm. Up to 3 messages, repeats counted, 10 s each, dismissable. Only the selected system's messages are shown. STATUSTEXT from a component that never sent a heartbeat is now emitted too.
- **Vehicle/component selection**: every non-GCS heartbeat (not MAV_TYPE_GCS, not compid 190, not our sysid) becomes a selectable entry, so gimbals and companions show up.
  - `VehicleSummary` gained `component` (MAV_TYPE label) and `autopilot`.
  - Automatic selection prefers the first autopilot. A non-autopilot picked automatically gives way to a later autopilot, but a manual choice sticks.
  - Stream requests go to autopilots only. A non-autopilot reports no flight mode.
  - The selector uses legacy labels (`1-1 QUADROTOR`, `1-154 GIMBAL`). A toast appears once per connection when more than one SYSID is seen.
  - Switching vehicle downloads the parameters again, also when a download was still running.
- Verified: fmt, clippy, `cargo test --workspace` (new tests for the field decoders, the selection rules and STATUSTEXT without a heartbeat), `npm run lint`, `npm run build`. A Firefox headless screenshot of `?mock=1` showed the HUD mode, the `1-1 QUADROTOR` selector and the yellow PreArm banner.
- **Tested on a real Cube Orange (ArduCopter 4.7.2, USB, nothing else attached), 2026-11-09**:
  - Worked: serial connect, the HUD mode, the PreArm/EKF banner, the grouped Quick View picker, and the parameter download (1078) with Save to file through the native dialog.
  - MAVFTP: Places (`/`, `/APM`, `LOGS`, `scripts`, `STRG_BAK`, `TERRAIN`, `@ROMFS`, `@SYS`) and Download through the dialog (`@SYS/uarts.txt`).
  - Flight Plan Save file through the dialog.
  - Bugs found and fixed:
    - 4.7.2 sends two NULs after every directory entry, which the listing parser read as bogus entries ("DLOGS", "Ffence.stg 65535").
    - `@SYS` files open with a nominal 100000 bytes and end with an EOF NAK, which was reported as "no answer".
    - The banner dropped PreArm bursts behind repeated EKF messages; it now ranks by severity and keeps PreArm for 35 s.
    - With ADSB enabled, a heartbeat from component 0 appeared in the selector; component 0 is now ignored.
  - Writing to `/dev/ttyACM1` (the second USB interface, not MAVLink here) from a test script wedged the USB CDC until the cable was replugged.
- **Previously not verified** (now partly covered above): the debug app starts with the dialog plugin on the Jetson and runs without errors for 20 s, but the native Save dialog itself was never opened. Also, the MAVFTP Places and the grouped picker were not clicked through. GPS2, component selection with two SYSIDs, and the banner were not tried with real telemetry.

## Beta installers (workflow committed, first release NOT yet built)

- `.github/workflows/beta-release.yml` builds on a `beta-*` tag (or manually from Actions > Beta release > Run workflow, which publishes nothing and keeps the files as run artifacts). Matrix: `windows-latest` (x64), `windows-11-arm`, `ubuntu-22.04` (x86_64), `ubuntu-22.04-arm`. Outputs: NSIS `.exe` + `.msi` (Windows), `.deb`, `.rpm`, `.AppImage` (Linux), attached to a GitHub pre-release by `softprops/action-gh-release`. Unsigned (Windows SmartScreen warns).
- `tauri.conf.json` bundle targets are `nsis, msi, deb, rpm, appimage` and the icon set in `src-tauri/icons/` is now tracked (generated with `tauri icon` from `src-tauri/icon-source.svg`, a placeholder quadcopter drawn for the beta; regenerate with `tauri icon icon-source.svg -o icons` after replacing it, then delete `icons/android` and `icons/ios`). Android/iOS icon folders were removed.
- `Beta/BETA.md` holds the tester instructions (download, install per OS, serial `dialout` group, what to test, how to report) and the maintainer recipe for a new version.
- Not verified: no `tauri build` has run in CI yet. Open questions: the bundle output path assumed to be `rust/target/release/bundle/` (workspace target dir), the `windows-11-arm` runner, Linux package names on Ubuntu 22.04 (ARM included), and `.rpm` generation. A `beta-0.1.0` tag was pushed once by mistake from an unmerged commit and deleted; it does not exist now.
- Release recipe: version must match in `rust/Cargo.toml`, `src-tauri/tauri.conf.json` and `ui/package.json` (all 0.1.0), then `git tag beta-X.Y.Z && git push origin beta-X.Y.Z`. The app identifier is `rust-control-station`; this build is not Speedbird-branded yet (a Speedbird identifier and icon come later).

**Phase 4b, Firmware**:
- `firmware` crate (pure): `manifest` (manifest.json.gz parse, `select` by board/vehicle/channel, `boards`, `is_trusted_url` = firmware.ardupilot.org only), `apj` (JSON + base64 + zlib image, padded to 4 bytes, size checks), `bootloader` (request bytes, status check, bootloader CRC32 and `crc32_pad`, `boards_compatible`; board 33 accepts fmuv2/9 images).
- `link::FirmwareService`: `catalog`, `download` (trusted URL only), `reboot_to_bootloader` (MAV_CMD_PREFLIGHT_REBOOT_SHUTDOWN param1=3, then drops the link), `identify(port)` (read-only), `flash(port, firmware)`, `cancel`, `flash-progress` events. Flash order: sync, GET_DEVICE, then every refusal that exists happens BEFORE the erase (board id mismatch, external flash image, unknown or too small flash), then chip erase, PROG_MULTI 252 B chunks, GET_CRC compared with the CRC of image + erased flash (a mismatch does not reboot), reboot. Refuses to run while the MAVLink link is connected. No board-id override exists.
- Desktop: `firmware_*` commands (the decoded image stays in Rust, `FirmwareState`), Firmware screen (list, board/vehicle/channel, download or local `.apj`, reboot to bootloader, port + identify, mismatch banner, danger confirm, progress, cancel). `mockFirmware.ts` backs `?mock=1`. `transport::SerialPortInfo` gained `vid`/`pid`.
- Tests: 15 unit tests in the crate and 6 integration tests (`link/tests/firmware.rs`) against an in-memory fake bootloader: happy path (image byte-exact, erased, rebooted), wrong board and oversized image refused with no erase, CRC mismatch not rebooted, silence gives a timeout, untrusted URL refused.

**Phase 4a, MAVFTP**:
- `mavftp` crate (pure): frame encode/decode (12 byte header + 239 data), opcodes, NAK -> `FtpError`, `parse_listing` (plain and `ListDirectoryWithTime`), `crc32` (ArduPilot variant: reflected 0xEDB88320, init 0, no final xor, *not* zlib).
- `params::unpack_param_pck` decodes `@PARAM/param.pck` (magic 0x671b/0x671c, common-prefix names, pad bytes; embedded defaults are skipped).
- `link::FtpService`: list (timed listing with automatic fallback, remembered), read (burst read, re-requests from the first missing byte, falls back to plain `ReadFile` when the vehicle NAKs burst), write (CreateFile + 80 byte WriteFile + optional CRC32 verification), remove/mkdir/rmdir/rename/crc32, cancel, `ftp-progress` events, one operation at a time (`Busy`). Replies are matched on `req_opcode` and `seq + 1`.
- `ParamService::with_ftp`: `refresh` tries `@PARAM/param.pck?withdefaults=1` first and falls back to PARAM_REQUEST_LIST; a vehicle where it failed once is not tried again until the app restarts. The desktop shell wires it.
- Replay serves an in-memory file system (`link/src/replay/ftp.rs`: `/APM`, `/APM/scripts`, `/APM/logs`), so the screen works without a drone. It does not serve `param.pck`.
- Desktop: `ftp_*` commands (file contents cross IPC as base64), MAVFTP screen (places, directory path, list with size/mtime, upload with overwrite confirmation and CRC check, download, CRC32, rename, delete with confirmation, new folder, cancel, progress). `mockFtp.ts` backs `?mock=1`.

**Phase 3**:
- **MAVLink Inspector**: `link::InspectorService` (per sysid/compid/message count, Hz and bytes/s over a 3 s window, last decoded fields, watched numeric series with every sample, ~4 Hz snapshots only while the screen is active) on top of `LinkManager::subscribe_packets()`. UI: tree System -> Component -> message, search by message/field/value, pinning, sort, field table with Graph buttons (arrays per element), uPlot live graph with 10/30/60 s/All windows, pause, clear, copy JSON/text.
- **Mission planning**: `mission` crate (`MissionItem`, `.waypoints` QGC WPL 110 parse/format), `ardupilot-meta` command catalog from `data/mavcmd.xml` (AC2 for Copter, APM for Plane, APRover for Rover), `link::MissionService` (MISSION_ITEM_INT only; download with per-item retries, upload with REQUEST_INT and legacy REQUEST, ACK handling, clear; mission/fence/rally via `mission_type`). UI: map with draggable numbered waypoints and path, editable grid with per-command parameter labels and Dist/Az/Grad columns, frames Relative/Absolute/Terrain, Read/Write/Clear on vehicle, Load/Save `.waypoints`, progress bar, home row.
- Replay keeps an in-memory mission/fence/rally store and speaks the mission protocol; it was split into `link/src/replay/`.
- The `mavlink` crate now has `mav2-message-extensions` (needed for `mission_type`); this added extension fields to some messages, so literals use `..Default::default()`.

## Phase 5: Motor Test (committed `fc7fcb7b1`, `2d6067d5a`)

- `commands::motor_test` (`MAV_CMD_DO_MOTOR_TEST`), `commands::set_message_interval`, `MotorThrottleType`.
- `ardupilot-meta::motor_layout`: embedded `data/motor_layout.json` (copy of the legacy `APMotorLayout.json`, 75 frames) giving each motor's output number, test-order letter and CW/CCW; generic 4 motors (8 for non-copter/rover) when the frame is unknown.
- `link::MotorTestService`: start one / all (one command per motor, like the legacy tool) / sequence (vehicle-run), `stop_all` (always allowed), `MOT_SPIN_ARM/MIN` writes (0-30 %), ESC telemetry (`ESC_TELEMETRY_*`, `ESC_STATUS`) with `watch_esc` asking the vehicle to stream it while the screen is open. Rules enforced in Rust: disarmed only, throttle 0-100 % with >20 % needing `allowHigh`, duration 1-120 s, one test at a time.
- Desktop: `motor_*` commands, `motor-test` event; UI `views/MotorTest.tsx` (Setup > Motor Test) with a props-off confirmation, confirmation of throttle changes above 10 %, always-enabled STOP ALL, per-motor buttons, ESC table, spin thresholds. `mockMotor.ts` backs `?mock=1`.
- Frames outside the layout table (heli, scripting matrix class 17, coaxial...) get generic motors A, B, C...: the screen names the unknown class/type and offers a "Number of motors" field (1-12, passed to `MotorTestService::frame` as `generic_count`). Output number and rotation are unknown for those.
- Tested: unit tests, 9 loopback tests (`link/tests/motor_test.rs`), the mock flow in a browser. Not tested on the real window or on a Cube Orange (props off, bench only).

## Phase 6c: DroneCAN Inspector (committed `ad239dc97`)

- `link::dronecan_inspector` (used by `DroneCanService`): every transfer that comes off the bus is counted per (source node, kind, type id) with count, rate and payload bandwidth over a 3 s window, the last decoded value flattened to rows (`status.health`, a union under its variant such as `value.integer_value`, arrays of numbers as arrays, byte arrays that read as text shown as strings, arrays of structs as `cells[0].volts`) and watched numeric fields recorded as series (`node/kind/typeId/field`, arrays as `field[3]`). Kind is 0 broadcast, 1 service request, 2 service response; the node is the "system", the kind the "component". It emits the same `InspectorSnapshot` as the MAVLink inspector (types and `parse_key` are shared), about four per second and only while enabled; stopping DroneCAN disables it. Our own frames are not echoed by the autopilot, so requests we send do not show, only what the bus carries.
- Desktop: `dronecan_inspector_{enable,watch,unwatch,clear}`, event `dronecan-inspector`. The DroneCAN screen has Nodes and Inspector tabs; the inspector reuses the MAVLink inspector's tree, field table, pinning, search, graph and copy through `InspectorPanel` (`views/Inspector.tsx`, with `originLabel`, `emptyText`, `storagePrefix`) and an `InspectorSource` (`components/inspector/useInspector.ts`), so the MAVLink screen is unchanged. Mock traffic in `mockDroneCan.ts`.
- Tested: 6 unit tests for the inspector module, 2 more loopback tests in `link/tests/dronecan.rs` (17 in all: counts and decoded fields per node and kind, the series, off means silent, clear, stop disables), the mock flow in a browser including the MAVLink inspector still working.
- Not done: the legacy stats popup (its `dronecan.protocol.Stats`/`CanStats` messages show in the inspector when a node sends them) and the message filter (`CAN_FILTER_MODIFY`, which only saves link bandwidth; how ArduPilot treats an empty filter was not verified, so every frame is still forwarded). Subscribe-style JSON dump is covered by Copy JSON.

## Phase 6b: node firmware update and node id allocation (committed `4523c7fe0`)

- `dronecan::image::app_descriptor_crc`: the 64-bit image CRC after the AP_Periph descriptor signature (`D7 E4 F7 BA D0 0F 9B EE` or `40 A2 E4 F1 64 68 91 06`), the value a node reports as `image_crc`.
- `DroneCanService::start_update(node, bytes, force)`: accepts an `.apj` (decoded with `firmware::parse_apj`) or a raw `.bin`; refuses when the vehicle is armed, the node is offline or busy, or another update runs; does nothing (with a message) when the image CRC equals the node's unless `force`; sends `BeginFirmwareUpdate` (source node 127, path `fw.bin`, `ERROR_OK` and `ERROR_IN_PROGRESS` accepted, a node already in `SOFTWARE_UPDATE` is not asked again) and then serves the node's `file.Read` requests (256 bytes per answer, aliases `fw.bin`/`fw.bi`, `NOT_FOUND` otherwise). The update ends `Done` when the last chunk is read, or `Failed` when the node never asks (30 s), stalls (10 s, both in `DroneCanTiming`), refuses, or the user cancels. Progress is `DroneCanState.update`. The node is not re-verified after it reboots.
- `inspect_image` describes a file (format, size, descriptor CRC) before sending. No download from `firmware.ardupilot.org` yet: files are local.
- Node id allocator (`set_allocation`, off by default and reset when the relay stops): the three-stage anonymous protocol (6 + 6 + 4 bytes of unique id, replies carry what was collected), ids granted in this order: the id already given to that unique id, the id of a node on the bus with that unique id, the requested id if free, else the highest free id from 125 down (never 127 or one in use). `DroneCanState.allocation` lists the last 20.
- Desktop: `dronecan_inspect_firmware`, `dronecan_update_firmware`, `dronecan_cancel_update`, `dronecan_set_allocation`; the DroneCAN screen has a per-node Update button (file picker, then a danger confirmation showing the file, its CRC and the node's, with a "send anyway" box when they match), a progress panel with Cancel, and an "Assign node IDs to new devices" box. Mock in `mockDroneCan.ts`.
- Tested: 15 loopback tests in `link/tests/dronecan.rs` (8 new: whole image served byte for byte, already-current image skipped and forced, refusals for empty/bad files, unknown node, node refusing, armed vehicle, stall and never-asks timeouts, cancel and one update at a time, descriptor CRC, three-stage allocation with preferred ids, same device getting the same id, known node keeping its id), unit tests for the descriptor, the mock flow in a browser. **Never run against a real node: a bad update can leave a device needing bootloader recovery. Try it first on a spare peripheral, props off.**
- Not done: DroneCAN Inspector and the stats/filter popups (6c), downloading peripheral firmware from the ArduPilot server, verifying the CRC after the node reboots.

## Phase 6a: DroneCAN nodes, parameters, restart (committed `70c7c108a`)

- New pure crate `dronecan`: the DSDL files of the legacy tool copied to `crates/dronecan/dsdl/` and embedded by `build.rs`; `dsdl` (parser, constants and `voidN` handled, `OVERRIDE_SIGNATURE`, 64-bit signatures: **all 155 match** the legacy `messages.cs` table, kept as `tests/golden_signatures.txt`; constants are not part of the signature and `voidN` normalises without a trailing space), `codec` (data-driven encode/decode into `Value`; little-endian bytes with the partial byte last and bits MSB first, unions with a tag, tail array optimisation on in classic CAN and off in CAN FD, float16, zero extension of short payloads), `transfer` (29-bit id, tail byte, CRC16 seeded with the signature, multi-frame, reassembly keyed by id with a 2 s timeout, CAN FD padding before the tail byte).
- `link::DroneCanService`: `MAV_CMD_CAN_FORWARD` renewed every second (stop sends 0), `CAN_FRAME`/`CANFD_FRAME` filtered by bus and vehicle, ground station as node 127 (NodeStatus 1 Hz, answers GetNodeInfo), node table (NodeStatus + GetNodeInfo, offline after 3 s), node `LogMessage`s (100), `list_params` (GetSet by index), `set_param` (checked against the echo), `save_params` (ExecuteOpcode SAVE), `restart_node` (silence is not an error). Requests are retried 4 times with a 1 s timeout; one operation per node at a time.
- Desktop: `dronecan_*` commands, `dronecan` event; Setup > DroneCAN screen (CAN1/CAN2 + Connect, node grid, Parameters editor with search / non-default filter / write with `*_ENABLE` last then save to flash / commit, Restart with confirmation, node messages). `mockDroneCan.ts` backs `?mock=1`.
- Tested: 23 unit tests in the crate, 7 loopback tests (`link/tests/dronecan.rs`: node info over several frames, parameters, retries, dead node, other bus ignored, offline), the mock flow in a browser. Not tested on a real CAN bus or peripheral.
- CAN FD is untested (CRC over padded payloads is accepted with or without the padding).

## Next planned

Order proposed: finish organising the repo, then build the first beta installers and have testers report. Candidates after that: test DroneCAN on a real CAN bus and peripheral, download peripheral firmware from the ArduPilot server, verify the node CRC after an update, the DroneCAN message filter, SLCAN/multicast transports. From earlier phases: MAVFTP tree view, recursive delete/folder download, resume and reset-to-default using the `param.pck` defaults; firmware auto-select of the board from USB ids, `.abin`/`.px4` files, external flash images, DFU/BLE; STATUSTEXT chunk reassembly, `.plan`/KML import and survey grid, Copter-only tuning, the Inspector 81 px overflow at 1024 px width, bitflag decoding. Logs and SITL from the legacy app are not started.

## Verified

- `npm run lint` works: ESLint was never a project dependency (the script fell through to an unrelated global 6.4 with no config). Added `eslint@9`, `typescript-eslint`, `eslint-plugin-react-hooks` and `eslint.config.js` (flat config; unused vars are errors unless prefixed `_`). The whole `src` tree passes clean, and a planted unused variable is reported.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (all pass), `npm run build`. MAVFTP: 15 loopback tests against a fake vehicle with an in-memory FS (lost burst chunk, no-burst firmware, old firmware without timed listing, corrupted upload caught by CRC, cancel, busy, silent vehicle), 3 Replay server unit tests, 3 `param.pck` decoder tests and 2 fast-parameter tests (success, and fallback that is not retried). The MAVFTP mock was exercised in Node; the screen itself was **not** opened in a browser or the Tauri window (no display on the machine used), so layout and clicks are untested.
- The real Tauri window was driven over CDP with the Replay of the sample log: Inspector showed 6-8 message types with Hz/B/s/counts, decoded fields and a live graph with roll/pitch/yaw; the watched `roll` series returned 118 points in 3 s and a malformed key returned a clear error. Missions: catalog of 52 commands with labels, an upload of 5 items followed by a download returned identical items, `.waypoints` format/parse worked and a bad file gave a clear error, a fence upload and a clear worked. The Flight Plan screen read the 5 items from the "vehicle", drew waypoints and path on the Esri map and showed the grid with computed distance/azimuth/gradient.

## Hardware testing status

- 2026-11-05, Windows: after the Phase 4 commits, `cargo fmt --check` and `cargo test --workspace` pass on the Windows machine (all crates, including 18 FTP and 6 firmware integration tests). Clippy, `npm run build` and `npm run lint` were not re-run there.
- The user ran the app on Ubuntu (Jetson Orin) connected to a Cube Orange. **Firmware update: reported by the user as working "in principle" (a primeira vista funcionou)**, i.e. a real flash of the Cube Orange through the Firmware screen succeeded. Exact firmware version/channel, bootloader revision, timings and any rough edges were not recorded. MAVFTP and the other screens: what was exercised and with what result is **not recorded**. Update this section with those details and trim the "Not verified" sections accordingly.

## Not verified (needs the real drone; may be outdated, see above)

MAVFTP against real ArduPilot: burst read pacing and `burst_complete` behaviour, write chunk size over radios, `ListDirectoryWithTime` on current firmware, `param.pck?withdefaults=1`, and the `replay_serves_files` integration test, which skips when `exemplos/Teste 02 - tunning.BIN` is absent (it was absent when this was written).

Everything from Phases 1-2 plus: mission upload/download against a live ArduCopter (item ordering, MISSION_REQUEST_INT vs legacy MISSION_REQUEST, terrain frame, `mission_type` fence/rally on a real vehicle), the Inspector with real telemetry rates, drag-editing on a very long mission, and downloading files in WebView2 (Save file uses a Blob download; only checked in a browser). Test with props off.

## Testing MAVFTP on a real drone (next step for the user)

Launch the desktop app (see "Linux / Jetson" below), connect (UDP/serial/TCP), open **MAVFTP**. Suggested order, props off:
1. Browse `/APM` (and `/APM/scripts` on a Lua-enabled build). Check names, sizes and mtimes; if mtimes are blank the firmware lacks `ListDirectoryWithTime` (expected on old builds).
2. Download a small file, then a large one (a log under `/APM/logs`); watch for stalls (burst pacing over a radio link) and compare sizes.
3. Upload a tiny `.lua` to `/APM/scripts` with Verify CRC on, then CRC32 it, then delete it. Over a slow radio try lowering `FtpTiming::write_chunk`/`read_chunk` if it times out.
4. Parameters screen, Refresh: it should use the fast `param.pck` path (a second or two for ~1300 params). If it silently takes the slow path, MAVFTP params failed once and are remembered as unavailable until the app restarts; check the vehicle's `FTP` capability.
5. Report: firmware version/board, link type, and any NAK text shown in the red banner.

## Linux / Jetson development

- Rust is installed with rustup in `~/.cargo` (`export PATH=$HOME/.cargo/bin:$PATH`); `webkit2gtk-4.1` is present, so `control-station-desktop` compiles.
- The icon set in `rust/desktop/src-tauri/icons/` is tracked and listed in `tauri.conf.json`, so Linux builds need nothing extra.
- Run: `cd rust/desktop/ui && npm ci && npm run tauri` (starts Vite on 5173; the first Rust build takes a few minutes). UI only: `npm run dev -- --port 5199`, open `/?mock=1`.
- CDP driving works with Vivaldi (`--remote-debugging-port=9222`, Node 18 needs the `ws` package outside the repo): `Runtime.evaluate` and tab clicks work, but every `Page.*` call (`Page.enable`, `captureScreenshot`) hangs, headless or not, and `/json/new` also hangs. Attach to the already-open page from `/json` instead. When killing helpers use `pkill -f '[v]ivaldi'`-style patterns so the pattern does not match the calling shell.

## Known rough edges

- Mission items with a command or frame the MAVLink dialect does not know are rejected on upload and dropped on download (`UnknownCommand`/`UnknownFrame`).
- STATUSTEXT chunk reassembly (`id`/`chunk_seq`) is not implemented, so a long vehicle message arrives as several lines.
- Fence/rally editing is basic (same grid; default commands 5001/5100). `.plan`/KML/SHP import and survey grid generation from the legacy planner are not ported. Items without a position (TAKEOFF, RTL, DO_*) have no marker on the map.
- The Inspector decodes bitflags as text (`"A | B"`), so they cannot be graphed; `clear()` keeps the series time base.
- MAVFTP: no recursive delete or folder download, no truncate/resume, no tree view (places list + path box instead), `param.pck` defaults are ignored (no reset-to-default yet), uploads capped at 16 MB in the UI, and `window.prompt` is deliberately not used (name entry is an in-app dialog).
- Parameters/Calibration/Extended Tuning limitations from Phase 2 remain (no reset-to-default, Copter-only tuning). The `param.pck` fast download now exists but is untested on a real vehicle.
- The Inspector view shows a ~81 px horizontal overflow at 1024 px width whose source was not found.

## Firmware: partly verified on a real board (see "Hardware testing status")

The user reports that a firmware update on a Cube Orange (Ubuntu/Jetson Orin) worked at first sight. The details below were written before that test and are still the list of things not individually confirmed or recorded. Unverified: bootloader timing (sync tries, 60 s erase timeout), optional GET_CHIP/GET_CHIP_DES/GET_SN on real bootloaders, reboot-to-bootloader across boards (USB port name/id often changes after the reboot; the screen refreshes ports after 3 s), the manifest `USBID` auto-detection (parsed, but the UI does not match boards by USB id yet; vid/pid are exposed on ports), boards with external flash (refused), and `.apj` download from the real server. First test with a spare board, props off, and report the board, bootloader revision and any error text.

## Working rules

Keep the C# application as reference only. Protocol and safety behavior live in Rust crates, never in React. Use a licensed/self-hosted offline map provider; never add OSM public-tile prefetch (live viewing with attribution is fine). After changes run `cargo fmt`, Clippy with `-D warnings`, `cargo test --workspace` and `npm run build`, then update this file. Launch with `cd rust/desktop/ui && npm run tauri`; if port 5173 is busy an orphaned Vite from an earlier run holds it (stop only that PID). `tauri dev` does not rebuild when a new capability file appears: touch `src-tauri/src/main.rs`.
