# Netplay

Multiplayer for the port: a player hosts a world and others join it - for a
card table, a golf round, prop hunt or roaming a shared map. Retail has no
networking at all (it reads only pad port 0), so everything here is the
port's own. The code is [`crates/netplay`](../../crates/netplay/README.md)
(`legaia_netplay`): a versioned wire protocol, two session cores, the
transports, and the `legaia-relay` server.

The crate names no engine crate. A session drives anything that implements
one of two small traits, so a game-mode session from `engine-modes` and a
whole `World` plug in the same way, and the protocol and session cores do
no I/O, which keeps them buildable for the browser host.

## At a glance

| Question | Answer |
|---|---|
| Simulation contract | `lockstep::Simulation` (`tick(&[u16])`, `state_hash()`) or `authority::EventSim` (`apply_event`, `state_hash`). |
| Simulation models | Lockstep (every peer simulates from every seat's pad) and host-authoritative (the host simulates; the others send actions). |
| Transports | In-memory loopback with simulated latency / jitter / loss, native raw TCP and WebSocket, browser `WebSocket` (feature `web`, wasm32 only). |
| Server | `legaia-relay` binary; the same server in-process via `RelayServer::spawn(addr)`. |
| Access control | Room codes only - LAN and friends use. See [Security posture](#security-posture). |
| Dependencies | None beyond the workspace's `clap` (CLI) and `web-sys` family (browser); the WebSocket handshake's SHA-1 and base64 are in-crate. |

## The layers

```text
 game host (play-window, web-play)
   |  pad words in, Step / events out
 LockstepSession<S> | AuthorityHost<S> / AuthorityReplica<S>   no I/O
   |  Message values
 Transport (Loopback | StreamTransport | WebSocketTransport)
   |  bytes
 relay (RelayCore state machine, driven by RelayServer or LocalRelay)
```

Every layer polls. A host calls the session once per frame - push the local
pad, `advance()`, `pump(&mut transport)` - and nothing blocks the frame. The
relay's [`RelayCore`](../../crates/netplay/src/relay/mod.rs) is the whole
relay as a state machine with no sockets; `RelayServer` runs it on threads
and `LocalRelay` runs it over any transports from the caller's loop (the
test harness, or a same-machine room with no sockets).

## Wire format

One relay port serves two stream framings, told apart by the first four
bytes a client sends:

| First bytes | Framing |
|---|---|
| `LGNP` | Raw TCP: then `u32 LE length` + message bytes, both directions. |
| `GET ` | WebSocket (RFC 6455): the opening handshake, then one binary frame per message. Client frames are masked, server frames are not; text frames and extensions are refused. |
| anything else | The connection is closed. A `GET` that is not an upgrade gets `400` and is closed. |

A message is a one-byte tag followed by its fields. Frame numbers, ids and
lengths are LEB128 varints; hashes are fixed 8-byte little-endian; pad words
are 2-byte little-endian. Every variable-length field has a cap, checked
before anything is allocated, and a whole message is at most 16 KiB.
Trailing bytes after a message are an error. The protocol version is
negotiated once, in `Hello`: a relay refuses a client of another version, so
no later message repeats it.

| Field | Cap |
|---|---|
| Player name | 24 bytes |
| Chat line | 256 bytes |
| Engine build id, pack id, mode id | 64 bytes |
| Start configuration | 4 KiB |
| Action / event / state payload | 8 KiB |
| Seats | 8 |
| Room members (seated + spectators) | 16 |
| Pad words per input packet | 32 |

### Messages

| Tag | Message | Direction | Purpose |
|---|---|---|---|
| `0x01` | `Hello` | client to relay | Protocol version, engine build id, player name, pack identity, create-or-join. |
| `0x02` | `Welcome` | relay to client | Admitted: the room code and the client's player id. |
| `0x03` | `Reject` | relay to client | A refused request and why; some reasons also close the connection. |
| `0x04` / `0x05` | `Ping` / `Pong` | either | Latency probe; the relay answers pings itself. |
| `0x10` | `Lobby` | relay to room | The whole lobby (host, seat count, members), re-sent after every change. |
| `0x11` | `SetReady` | client to relay | Ready toggle. |
| `0x12` | `AssignSeat` | host to relay | Move a member to a seat or to spectating. |
| `0x13` | `Chat` | both | A chat line; the relay stamps the real sender and strips control characters. |
| `0x14` | `Leave` | client to relay | Leave the room; the connection closes. |
| `0x15` | `PlayerLeft` | relay to room | A member left or dropped. |
| `0x16` | `HostChanged` | relay to room | Host migration. |
| `0x17` | `RoomClosed` | relay to room | The room is gone (host of a host-authoritative room left, or relay shutdown). |
| `0x20` | `Start` | host to relay to room | Seed, mode id, configuration, simulation model; the relay fills in the seat map. |
| `0x21` | `Input` | peer to relay to peers | One seat's pad words for a run of frames. |
| `0x22` | `Checkpoint` | peer to relay to peers | A state hash after a frame. |
| `0x23` | `SeatController` | relay to room | A seat changes hands (a dropped player's seat goes to the AI) from a given frame. |
| `0x24` / `0x25` | `Pause` / `Resume` | peer to relay to peers | Session pause; the relay stamps the sender. |
| `0x30` | `Action` | client to relay to host | A player's intention in a host-authoritative session. |
| `0x31` | `Event` | host to relay to room | A validated event with its sequence number and post-event state hash. |
| `0x32` | `StateRequest` | client to relay to host | Ask for the full state. |
| `0x33` | `StateBlob` | host to relay to one client | The state after the first `seq` events. |
| `0x34` | `Snapshot` | host to relay to room | Avatar positions and animation state for shared-world roaming. |

## Rooms and the lobby

The first message on a connection is `Hello`. To **create** a room it names
a seat count (1 to 8); the relay picks a six-character code from a 32-symbol
alphabet without the look-alikes `0`/`O` and `1`/`I`, makes the sender host
and seats them at seat 0. To **join** it names a code; the relay admits the
client only if the room is still in its lobby, has room, and the client's
engine build id and pack identity (pack id plus SHA-256 of its bundle)
equal the host's byte for byte - two different builds or packs would run
different rules and diverge on the first frame that touches them. A joiner
takes the lowest free seat, or spectates when the seats are full; the host
can move anyone with `AssignSeat`.

`Start` is the host's alone, and only once every seated member other than
the host is ready. The relay replaces the start order's seat map with the
lobby's (seat index to player, empty seats to the AI) and sends it to the
whole room, host included, so every peer starts from the same order. A
started room admits no one.

When the host leaves the lobby, the earliest-joined remaining member becomes
host (`HostChanged`). During a lockstep session the same migration happens;
the host has no simulation privilege there. A host-authoritative session
ends instead (`RoomClosed(HostLeft)`): the state lives on the host and
nobody else holds it. An empty room is freed at once.

Refusals that only concern the request (no such room, room full, wrong
build or pack, not ready, not host) leave the connection open so the client
can try again; a protocol-version mismatch, a malformed frame, the
connection or room cap, and the rate limits close it.

## Lockstep

Every peer runs the same deterministic simulation from every seat's pad
word, frame by frame. `LockstepSession<S: Simulation>` is the core:

- **Input delay.** A peer samples its pad for frame `f + input_delay` while
  simulating frame `f`, and sends it. Frames `0..input_delay` run on zero
  pads everywhere, so nobody waits at the start. The delay hides that much
  one-way latency through the relay; beyond it the slowest link sets the
  pace.
- **Stall.** Frame `f` is simulated only once every human seat's word for
  `f` is known; until then `advance()` returns `Stalled` with the seats
  being waited on. `wants_local_input()` stops the local pad from running
  more than the delay ahead, so a stalled peer queues nothing.
- **Redundancy.** Every input packet repeats the sender's last K words
  (default 8, at most 32). After a few consecutive stalled calls the latest
  packet is sent again, so a lost packet is covered even when the stall
  itself stops new packets. The relay transports are reliable streams; the
  redundancy is there for an unreliable input channel and is exercised by
  the loopback link's input loss.
- **Checkpoints.** Every `checkpoint_interval` frames each peer hashes its
  state after the tick and sends the hash. A hash that differs from a
  peer's hash for the same frame is a desync, reported once with the frame,
  both hashes and the peer's seat; by default the session then halts.
  Spectators verify but send nothing.
- **Pause.** `pause()` stops the local pad and tells the room; every peer
  stops sampling, and the simulations stop within the input delay, all on
  frames fixed by the inputs already sent. `resume()` clears it.
- **Dropped players.** When a seated player disconnects, the relay knows the
  end of the last input frame it forwarded for that seat. It re-sends that
  seat's last packet (so a peer that lost it is not left waiting on a
  sender that is gone) and then announces `SeatController { seat, Ai,
  from_frame }` with that frame. No peer can have simulated `from_frame`
  without an input that never comes, so every peer switches the seat to the
  AI at the same frame (`Simulation::set_seat_controller`). The
  `engine-modes` sessions support AI seats; an AI seat's pad word is zero
  and ignored.

### What a simulation owes

`state_hash()` must be equal for equal states on every platform: no
pointer values, no `HashMap` iteration order, no floating point that a
target can round differently. `tick()` must depend on nothing but the
state and the pad words - no wall clock, no host RNG. Both are the same
contract the replay harness ([determinism-replay](../tooling/determinism-replay.md))
already holds `World::tick` to: it is deterministic at 60 Hz from a `u16`
pad word. A `World`-backed simulation additionally has to give each seat's
word somewhere to go - retail reads only pad port 0 - which is the
multi-avatar work of the hosts, not of this crate.

### Rollback

Rollback - predict missing inputs, simulate ahead, restore a snapshot and
re-simulate on a misprediction - needs a snapshot cheap enough to take
every frame. The mode sessions could provide one; a whole `World` cannot
(it is large and holds scene resources), which is why the core is
delay-based. Input packets already carry frame numbers, so rollback would
add a `RollbackSimulation` trait beside `Simulation` (`snapshot` /
`restore`) and leave the wire format unchanged.

## Host-authoritative

One player owns the state; the rest send intentions and follow. It suits
what is turn-based or what only the host can resolve.

- **Turn-based events** (cards, poker). A client sends `Action`; the relay
  carries it to the host stamped with the sender. `AuthorityHost` turns it
  into an event through a host-supplied validator (which can refuse it, for
  example out of turn), applies it, and broadcasts `Event` with a sequence
  number and the post-event hash. `AuthorityReplica` applies events in
  sequence order, buffering early ones, and checks each hash. A replica
  whose simulation can serialise itself (`EventSim::save_state` /
  `load_state`) recovers from a mismatch, or from joining late, by asking
  for a `StateBlob` and holding events until it arrives.
- **Shared-world roaming.** The host broadcasts every avatar's scene,
  position, facing and animation frame as a `Snapshot` at 10 to 20 Hz.
  `AvatarInterpolator` keeps an estimate of the host's frame clock and
  draws each avatar two snapshot intervals in the past, between the two
  snapshots that bracket that moment: positions interpolate linearly,
  facing the short way round the 4096-unit circle, and a scene change
  snaps. Past the newest snapshot it holds the last pose rather than
  extrapolating, so a stalled link freezes an avatar instead of walking it
  through a wall.

## Host a world

The relay runs in two places:

- **Standalone:** `legaia-relay --bind 0.0.0.0:7462` (the default port),
  for a dedicated machine on a LAN or a small server.
- **In-process:** the hosting game calls `RelayServer::spawn(addr)` and gets
  a `RelayHandle` with the bound address; it then connects to
  `127.0.0.1:<port>` like any other client, creates the room and shows the
  room code and its address. Dropping the handle (or `shutdown()`) closes
  every room, tells each client `RoomClosed(ServerShutdown)`, and joins every
  thread; nothing is left listening.

Native clients connect with `StreamTransport::connect_tcp` or `connect_ws`;
the browser host connects with `WebSocketTransport::connect("ws://host:port/")`.
A page served over `https://` may only open `wss://` sockets (loopback
excepted), and the relay does not do TLS itself, so a relay reached from the
published site sits behind a TLS-terminating reverse proxy that forwards the
WebSocket upgrade.

## Security posture

The relay is for LAN and friends. Its only access control is the room code,
and it never touches the file system or the game: it speaks the netplay
protocol and nothing else, refuses every HTTP request that is not a
WebSocket upgrade, and holds no game data.

| Bound | Default | What it stops |
|---|---|---|
| Message size | 16 KiB | Memory blow-up from one frame (checked before buffering). |
| Message rate | 240/s sustained, 480 burst, per connection | Flooding. Over it, the connection is dropped. |
| Bad room codes | 5 per connection | Code guessing; the connection is dropped. |
| Connections | 256 total, 16 per IP address | Socket exhaustion. |
| Rooms | 64 | Room-table growth. |
| Members per room | 16 | Lobby growth. |
| Outbound queue | 1024 messages per connection | A client that stops reading is dropped, not buffered. |
| Handshake | 5 s | Half-open connections. |

Per room the relay keeps only the lobby rows, the seat map and, per seat,
one frame counter and the last input packet - no frame history - so a room's
memory is bounded by the caps above whatever a session's length.

What it does not do: there is no authentication beyond the code, no
encryption (use the TLS proxy), and no cheat prevention. The relay stamps
every input, checkpoint, chat line and action with the sender's real seat
and id, so a peer cannot speak for another, but a lockstep peer chooses its
own seat's inputs freely and a host-authoritative host decides everything.
Play with people you trust.

## Tests

`crates/netplay/tests/` (disc-free, all of it):

- `codec_roundtrip.rs` - every message variant round-trips, every strict
  prefix of an encoding is refused, random bytes never panic the decoder.
- `lockstep_loopback.rs` - 2, 3 and 4 peers through the relay core over
  links with latency, jitter (so reordering) and 5% input loss agree on the
  state after 10 000 frames of a toy simulation; a peer that diverges is
  caught by everyone at the next checkpoint; a dropped player's seat goes to
  the AI on the same frame everywhere; pause and resume.
- `relay_lobby.rs` - admission checks, seats and spectators, readiness, host
  migration, sender stamping, rate limits and caps.
- `authority_rooms.rs` - replicas follow the host through validated
  turn-based events, a stale replica recovers from a state blob, snapshots
  reach the others, the room closes when its host leaves.
- `socket_smoke.rs` - the threaded relay on an ephemeral `127.0.0.1` port
  with one raw-TCP and one WebSocket client playing lockstep; every wait has
  a deadline and the port is checked closed after shutdown.
