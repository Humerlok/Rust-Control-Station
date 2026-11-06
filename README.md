# Rust Control Station

Ground control station for ArduPilot vehicles, written in Rust (Tauri 2 + React). It connects over MAVLink (serial, TCP, UDP, WebSocket) and offers Flight Data, parameters, calibration, Extended Tuning, a MAVLink Inspector, mission planning, MAVFTP, firmware flashing, Motor Test and DroneCAN tools.

It is the successor of ArduPilot's Mission Planner. The original C# code was removed from this tree; it is kept in git history under the tag `legacy-mission-planner`.

## Download (beta)

Installers for Windows and Linux (x86_64 and ARM64) are published in [Releases](https://github.com/humerlok/rust-control-station/releases). See [Beta/BETA.md](Beta/BETA.md) for install steps and what to test.

## Development

See [rust/README.md](rust/README.md) and [HANDOFF.md](HANDOFF.md) (state of each phase, what is verified on hardware).

```
cd rust
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd desktop/ui && npm ci && npm run tauri
```

## License

GPLv3, see [COPYING.txt](COPYING.txt).
