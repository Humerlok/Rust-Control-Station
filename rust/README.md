# Rust Control Station

This workspace is the incremental Rust successor to the legacy Mission Planner application. The first milestone supports ArduPilot Copter and Plane telemetry, parameter workflows, guided calibration, and offline-capable maps on Windows and Linux.

## Development

Install Rust stable, Node.js 20+, and the platform prerequisites required by [Tauri v2](https://v2.tauri.app/start/prerequisites/). Then run `cargo test --workspace` from this directory. To launch the shell, run `npm ci && npm run tauri dev` in `desktop/ui`.

The public `tile.openstreetmap.org` service is interactive-only. Offline regions must use a provider explicitly licensed for prefetch/offline use, or a self-hosted tile endpoint.
