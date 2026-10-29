# legaia-netplay

Multiplayer for the port: the wire protocol, the lockstep and
host-authoritative session cores, the transports, and the `legaia-relay`
server that joins players into rooms. The design - protocol, both
simulation models, the "host a world" flow and the security posture - is
[`docs/subsystems/netplay.md`](../../docs/subsystems/netplay.md).

The crate names no engine crate. A session drives anything implementing
`lockstep::Simulation` (per-frame pad words and a state hash) or
`authority::EventSim` (discrete events), so a game-mode session and a whole
`World` plug in alike.

## Modules

| Module | What it holds | Builds for wasm32 |
|---|---|---|
| `protocol` | `Message` and every payload type, the caps, the binary codec (`encode` / `decode`). | yes |
| `codec` | The varint / fixed-width reader and writer under it. | yes |
| `lockstep` | `Simulation`, `LockstepSession` (delay, stall, redundancy, checkpoints, desync, pause, AI handover). | yes |
| `authority` | `EventSim`, `AuthorityHost` / `AuthorityReplica`, `AvatarInterpolator`. | yes |
| `wire` | Raw (length-prefixed) and WebSocket stream framing. | yes |
| `websocket` | The RFC 6455 subset: handshake (with in-crate SHA-1 and base64) and frames. | yes |
| `transport` | The polling `Transport` trait; `loopback` (simulated latency / jitter / loss); `stream` (native TCP and WebSocket); `web` (browser, feature `web`). | `loopback`, and `web` with the feature |
| `relay` | `RelayCore` (the relay as a state machine), `local::LocalRelay`, `server::RelayServer`. | `RelayCore` and `LocalRelay` |

## Binary

```bash
legaia-relay --bind 0.0.0.0:7462
```

Behind the default `cli` feature (clap). Library users take the crate with
default features off, as the workspace table does. In-process hosting is
`relay::server::RelayServer::spawn("0.0.0.0:7462")`.

## Features

- `cli` (default) - the `legaia-relay` binary.
- `web` - the browser transport `transport::web::WebSocketTransport`; its
  `web-sys` / `wasm-bindgen` / `js-sys` dependencies are declared for
  wasm32 only, so the feature is inert natively.

```bash
cargo test -p legaia-netplay --profile release-test
cargo check -p legaia-netplay --target wasm32-unknown-unknown --features web
```

No new third-party dependencies: the WebSocket handshake's SHA-1 and base64
are in-crate, and the server is `std::net` plus threads.
