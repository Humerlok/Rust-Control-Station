# Handoff: Rust Control Station

## Current state (2026-11-05)

Legacy C# Mission Planner plus the new Rust/Tauri app in `rust/`. Phases 1-4 are all committed: `6535baf80` (P1), `27fb0aa42` (P2), `dc597b77b` (P3), `286e407c8` (P4a MAVFTP) and `ad2b8063c` (P4b Firmware). Nothing is pushed. **Phase 4 was built on a Jetson Orin running Ubuntu, and the user tested it personally against a real Cube Orange.** The details of what worked or failed there were not recorded in the repo, so the "Not verified" lists below may be partly outdated (see "Hardware testing status"). The original plan file (`C:\Users\renan\.claude\plans\floofy-sauteeing-toast.md`) lives on the Windows machine and has only 4 phases; there is no Phase 5 plan yet, see "Next".

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

## Phase 5: Motor Test (implemented, uncommitted)

- `commands::motor_test` (`MAV_CMD_DO_MOTOR_TEST`), `commands::set_message_interval`, `MotorThrottleType`.
- `ardupilot-meta::motor_layout`: embedded `data/motor_layout.json` (copy of the legacy `APMotorLayout.json`, 75 frames) giving each motor's output number, test-order letter and CW/CCW; generic 4 motors (8 for non-copter/rover) when the frame is unknown.
- `link::MotorTestService`: start one / all (one command per motor, like the legacy tool) / sequence (vehicle-run), `stop_all` (always allowed), `MOT_SPIN_ARM/MIN` writes (0-30 %), ESC telemetry (`ESC_TELEMETRY_*`, `ESC_STATUS`) with `watch_esc` asking the vehicle to stream it while the screen is open. Rules enforced in Rust: disarmed only, throttle 0-100 % with >20 % needing `allowHigh`, duration 1-120 s, one test at a time.
- Desktop: `motor_*` commands, `motor-test` event; UI `views/MotorTest.tsx` (Setup > Motor Test) with a props-off confirmation, confirmation of throttle changes above 10 %, always-enabled STOP ALL, per-motor buttons, ESC table, spin thresholds. `mockMotor.ts` backs `?mock=1`.
- Frames outside the layout table (heli, scripting matrix class 17, coaxial...) get generic motors A, B, C...: the screen names the unknown class/type and offers a "Number of motors" field (1-12, passed to `MotorTestService::frame` as `generic_count`). Output number and rotation are unknown for those.
- Tested: unit tests, 9 loopback tests (`link/tests/motor_test.rs`), the mock flow in a browser. Not tested on the real window or on a Cube Orange (props off, bench only).

## Phase 6b: node firmware update and node id allocation (implemented, uncommitted)

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

6c: DroneCAN Inspector (node -> message tree with Hz and bytes/s, decoded fields, graph, subscribe), stats popup and message filter. See the plan file.

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
- Tauri needs `rust/desktop/src-tauri/icons/icon.png` on Linux (the config lists only an `.ico`). It is not tracked; create any 32x32 RGBA PNG locally. Delete it before committing if you do not want it in the tree.
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

## Next

Phase 4 is committed. There is no Phase 5 plan; pick it from the legacy app (logs, SITL, more setup screens) and from the gaps below. Candidate items:
- MAVFTP: tree view, recursive delete/folder download, resume, reset-to-default using the `param.pck` defaults.
- Firmware: auto-select the board from USB ids, ArduPilot `.abin`/PX4 `.px4` files, external flash images, DFU/BLE.
- Earlier phases: STATUSTEXT chunk reassembly, `.plan`/KML import and survey grid, Copter-only tuning, Inspector 81 px overflow at 1024 px width, bitflag decoding.

## Working rules

Keep the C# application as reference only. Protocol and safety behavior live in Rust crates, never in React. Use a licensed/self-hosted offline map provider; never add OSM public-tile prefetch (live viewing with attribution is fine). After changes run `cargo fmt`, Clippy with `-D warnings`, `cargo test --workspace` and `npm run build`, then update this file. Launch with `cd rust/desktop/ui && npm run tauri`; on Linux Tauri needs `icons/icon.png` (the config only lists an `.ico`), so a local untracked one is required to compile `control-station-desktop`; if port 5173 is busy an orphaned Vite from an earlier run holds it (stop only that PID). `tauri dev` does not rebuild when a new capability file appears: touch `src-tauri/src/main.rs`.
