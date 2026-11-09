# Rust Control Station

This workspace is the Rust successor to the legacy Mission Planner application. It supports ArduPilot vehicles over MAVLink (serial, TCP, UDP, WebSocket) with Flight Data, parameters, calibration, Extended Tuning, a MAVLink Inspector, mission planning, MAVFTP, firmware flashing, Motor Test and DroneCAN tools, on Windows and Linux. The project status and what is verified on hardware are in the root [README](../README.md) and [HANDOFF.md](../HANDOFF.md).

## Development

Install Rust stable, Node.js 20+, and the platform prerequisites required by [Tauri v2](https://v2.tauri.app/start/prerequisites/). Then run `cargo test --workspace` from this directory. To launch the shell, run `npm ci && npm run tauri dev` in `desktop/ui`.

The public `tile.openstreetmap.org` service is interactive-only. Offline regions must use a provider explicitly licensed for prefetch/offline use, or a self-hosted tile endpoint.
