# legaia-engine-netplay

Netplay in the play hosts: [`legaia-netplay`](../netplay/README.md)'s
protocol and session cores joined to a running `World`. Both hosts - the
native play window and the browser play page - drive one
`room::NetPlay` per player, once per frame, before the world's tick.

| Module | What it is |
|---|---|
| `room` | `NetPlay`: the connection, the lobby, chat and emotes, the room screen's rows and input, the three phases (lobby roaming, lockstep mode, view-stream mode). |
| `mode_net` | Which model each mode kind runs under; `NetMode::build` (one instance from a start order, seats from the room); `ModeSim` (lockstep adapter); the view stream's payloads and poker redaction. |
| `view_stream` | The host-authoritative view stream: `ViewHost` ticks the session from every seat's reported pad, `ViewGuest` reports its pad and draws the host's frames. |
| `roam` | Shared-world roaming: avatar reports, the host's snapshots, interpolated remote avatars. |
| `screens` | The room screen and roaming panel as `legaia_engine_ui` draw lists. |
| `avatar_pose` | A remote avatar's pose off the local leader's locomotion clips. |

A networked mode runs in the world's own mode seat
(`PackModeSeat::networked`), so it draws through the same screens as an
offline one; it grants nothing.

The flow, the per-mode models and their hidden-information trade-off, and
the limits of roaming are in
[`docs/subsystems/netplay.md`](../../docs/subsystems/netplay.md#in-the-hosts).

Tests (`tests/`, disc-free): poker over the view stream without leaking
hole cards, lockstep golf and prop hunt ending with equal hashes on every
peer, a desync caught and both peers back in the lobby, a dropped player's
seat going to the AI, roaming avatars, chat and emotes, and a real-socket
smoke test on an ephemeral `127.0.0.1` port.
