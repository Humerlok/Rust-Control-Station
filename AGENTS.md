# Repository Guidelines

## Project Structure

The legacy Mission Planner application is C#/.NET Framework. Its WinForms entry points live at the repository root; `GCSViews/` contains screens, `ExtLibs/` contains protocol and shared libraries, `MissionPlannerTests/` contains MSTest tests, and `Resources/` stores assets. Treat that code as a behavioral reference during the migration.

The new control station lives entirely in `rust/`. Its workspace separates MAVLink parsing, transports, vehicle state, command workflows, map caching, and the Tauri desktop shell. Keep domain crates independent of Tauri and React.

## Build, Test, and Development

Run Rust commands from `rust/`:

```powershell
cargo fmt --check        # formatting
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace   # unit and integration tests
cd desktop/ui; npm ci; npm run dev
cd ..; cargo tauri dev   # launch the desktop application
```

The legacy application still builds through `MissionPlanner.sln` with Visual Studio/MSBuild; do not add Rust dependencies to its projects.

## Style and Boundaries

Use Rust 2021, `rustfmt`, and Clippy-clean code. Use `snake_case` for modules/functions, `PascalCase` for types, and explicit domain types rather than UI-shaped JSON. Keep MAVLink decoding, I/O, commands, and cached map data in their respective crates. Tauri commands translate UI requests into domain calls; React must not own protocol logic.

Use a configured provider or self-hosted tiles for offline downloads. Never prefetch from `tile.openstreetmap.org`; show required map attribution in the UI.

## Testing

Place pure behavior tests alongside their Rust modules and cross-crate tests under each crate's `tests/` directory. Cover packet parsing, state transitions, parameter conversion, calibration workflows, and map-cache boundaries. Use loopback UDP/TCP or fakes for transport tests; hardware and SITL tests must be opt-in.

## Commits and Pull Requests

Use concise imperative subjects with an area prefix, for example `desktop: add connection status`. Keep commits focused. Pull requests must explain the user-visible change, validation performed, and any protocol or provider assumptions. Include screenshots for UI changes and logs for connection/calibration issues. Never commit credentials, vehicle telemetry, or downloaded map regions.
