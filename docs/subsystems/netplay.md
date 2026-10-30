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
| `0x26` | `EndSession` | peer to relay to room | The running session is over: the room returns to its lobby. |
| `0x30` | `Action` | client to relay to host | A player's intention in a host-authoritative session, or a roaming avatar report in the lobby. |
| `0x31` | `Event` | host to relay to room | A validated event with its sequence number and post-event state hash. |
| `0x32` | `StateRequest` | client to relay to host | Ask for the full state. |
| `0x33` | `StateBlob` | host to relay to one client | The state after the first `seq` events. |
| `0x34` | `Snapshot` | host to relay to room | Avatar positions and animation state for shared-world roaming (lobby or host-authoritative session). |

`Action` and `Event` payloads open with a tag byte that says what they carry:
`0x01` a roaming avatar, `0x02` a pad word, `0x03` a card trade, `0x04` a
party-night message ([Party night](#party-night)).

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

`EndSession`, from any member of a started room, returns it to the lobby:
the relay clears everyone's readiness, sends the notice (stamped with the
sender) and the lobby, and admits joiners again. The lobby is not idle
time: shared-world roaming runs there, so the relay carries guests'
`Action`s to the host and the host's `Snapshot`s and `Event`s (card
trades) to the room in the lobby
as well as in a host-authoritative session.

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

## In the hosts

[`crates/engine-netplay`](../../crates/engine-netplay/README.md) joins the
protocol to a running `World`. Both play hosts drive one
`room::NetPlay` per player, once per tick, before the world's tick: it
pumps the transport, runs the room screen when it is open (the pad goes to
the screen, not the world), and drives whichever phase the room is in.

### The flow

| Step | Native play window | Browser play page |
|---|---|---|
| Host | `play-window --host [PORT]` starts `RelayServer::spawn` on `0.0.0.0:PORT` (default `7462`) and joins it over `127.0.0.1`. | Cannot host: a page cannot listen on a socket. A native play window or `legaia-relay` hosts. |
| Join | `play-window --join ADDR --room CODE --name NAME`. | The Online panel: relay URL (`ws://host:port/`), room code, name. |
| Room screen | `Tab` (and, in a room, the backquote key); `T` types a chat line. | The Room button or `Tab`; the panel's chat box. |
| Avatars | The leader mesh posed per avatar in the actor pass. | Copies of the leader rig, mesh ids above the player's. |

The room screen lists the players (seat colour, ready, host), and rows for
the host's game pick and Start, a guest's Ready, an emote, a quick-chat
line, Leave and Close; chat and room notices run along its bottom. The
title shows the room code and the address others join at - the host's LAN
address for a hosting window. Closed, a small panel over the field names
the room, its players and the newest chat lines.

### Modes

The host picks an instance from the world's mode list (the active pack's,
or the built-in practice set) and starts it; every peer builds that
instance from the start order's id and seed. The first room seats fill
the mode's seats; a mode seat with no player is the AI, and a player whose
room seat the mode lacks roams instead. Golf seats the whole room. The
session runs in the world's own mode seat (`PackModeSeat::networked`), so
it draws through the offline screens, with "You" on each player's own
seat; it grants nothing.

| Kind | Model | Why |
|---|---|---|
| Golf, racing, prop hunt | Lockstep | Real-time; only pad words and checkpoint hashes travel. |
| Poker, Grid Duel | Host-authoritative view stream | Turn-based; poker hides hole cards. |

**Lockstep.** `ModeSim` is the `Simulation`: the real session lives in the
world's seat and is swapped into the lockstep core around each advance, so
the screens draw the simulated state. A desync ends the game for everyone
with the frame and seat on the room screen; a dropped player's seat goes to
the AI on the same frame everywhere.

**View stream.** Only the host runs the session. A guest sends its pad
word when it changes; the host feeds one change per tick to that seat, so a
press and its release arriving together still give the session a pressed
frame. After a tick that changed what a guest sees, the host sends it a
numbered `StateBlob`: the table as that seat may see it plus the tick's log
lines worded for it. A guest's own copy of the instance is built from
another seed and only labels the screen. The hidden-information trade-off:
under lockstep every peer holds the whole deal, so a modified client could
read every hole card; under the view stream a poker guest never receives
another seat's hole cards before the showdown, and then only the live
hands. Grid Duel is played with open hands and passes through unredacted.
The price is a round trip of input latency, which a turn-based table does
not notice, and that the session ends if its host leaves.

Leaving a mode (the quit prompt, or the results screen) sends
`EndSession`; another player still mid-game is returned to the lobby with
a notice, while one reading the results keeps them until they leave.

### Roaming

Players roam their own worlds. A guest reports its avatar (scene id - the
scene's first PROT entry -, position, facing, idle or walk clip and frame)
to the host at 15 Hz; the host broadcasts all of them, its own included, as
one snapshot; every peer draws the others in its scene through the
`AvatarInterpolator`. Outside the field (a menu, a battle, a mode) an
avatar is reported with no scene and drawn nowhere.

The limits, by design: nothing but avatars is shared. Each world runs its
own scripts, encounters, doors, dialogue and battles, so two players see
each other walk but not each other's fights or story state; there is no
collision between avatars. A remote player is drawn as the local party
leader (posed with the local leader's clips at the reported frame), since
a field scene loads only its own leader's rig. Emotes are chat lines
(`* Vahn waves`), not animations. The practice prop hunt from the mode
list runs on its own flat arena in lockstep; the one played in the world
is the field prop hunt below.

### Prop hunt in the field

The host's room screen offers **Prop hunt here** while it stands in a field
scene with props to hide as. The scene is the arena: its placed objects
small enough to hide as (a horizontal half-extent of 24 to 200 world units
- a barrel or a cart, not a house) are the disguises and the decoys, one
prop kind per environment-pack slot, so a prop names its mesh by the slot
every peer that loaded the scene from the same disc agrees on. The arena
is framed around those props and the players.

Only the host runs the session (`legaia_engine_modes::prop_hunt` through
`ActiveMode::tick_with_positions`), from every player's real field
position: its own player's and each guest's avatar report, scaled four
world units to a session unit. The session alone decides roles, timers
and tags, and its view rides the view stream to the guests. Nothing else
changes in a world: it stays in the field, with its own collision, camera
and movement model (action movement too, when that toggle is on). While a
hunt runs the world sees only movement - Cross tags, so it must not also
open a door or talk to a villager - and no movement at all while everyone
picks a prop, or while a hunter waits out the hiding time.

| Player | Controls |
|---|---|
| Hider, choosing | Left / Right cycles the prop, Cross takes it. |
| Hider, hiding | Walk to a spot among the props of that kind and keep still; Triangle taunts. |
| Hunter | Waits out the hiding time, then Cross next to a prop tags it: a hider is found, a decoy costs health. |

A hider is drawn as its prop's mesh in place of the leader, in every world
including its own; the HUD (round, phase and time left, role, hiders still
hidden, the newest event) is an `engine-ui` panel both hosts draw. The
match runs two rounds with the hunter role passing on; four seconds after
it ends the host closes it and the room is back in its lobby.

### Trading cards

With a card economy (a pack that defines cards), the room screen gains
Trade with / Give / For / Offer rows. Trades run on the cards rules'
`TradeTable`, settled by the host:

1. Every player reports its holdings to the host when they change; the
   host relays them, so a proposer sees what a partner owns.
2. An offer (one card for one card) goes to the host, which puts it on
   the table and confirms it for its proposer; the partner sees Accept /
   Decline, the proposer Withdraw.
3. With both confirmed, the host settles the table against the holdings it
   was told and broadcasts the result. Each side applies it to its own
   collection, validating again, so the cards move on both worlds or on
   neither; the collection is saved with the rest of the card progress.

Messages ride the same `Action` / `Event` pair as a host-authoritative
mode, tagged apart from roaming reports and pad words; the relay carries
the host's events in the lobby for this.

### Hosting and joining from the window

Offline, `Tab` opens the Multiplayer screen: host on a port (Left / Right,
or Cross to type one), or join by address, room code and name, each typed
after Cross on its row and kept with Enter. The launcher's **Play online**
button starts the game with that screen open (`play-window
--multiplayer`). The browser page joins from its Online panel; it cannot
host.

## Party night

A party night chains short rounds of the modes - a few poker hands, a
golf hole, a lap, a prop hunt round, a Grid Duel ladder - into one
points-based evening with a scoreboard between rounds and prizes at the
end. The night's rules are a `World`-free kernel,
`legaia_engine_modes::party` ([expansion-modes.md](expansion-modes.md#party-night));
this section is how a room plays one
([`crates/engine-netplay/src/party.rs`](../../crates/engine-netplay/src/party.rs),
driven from `room::NetPlay`).

| Question | Answer |
|---|---|
| Who plays | Every seated room member when the host starts it, in seat order; anyone else watches. |
| Where it runs | The room's lobby. Between rounds everyone is back in their own field under the party card; a round is an ordinary networked start. |
| Round models | The same as a picked game: lockstep for golf, racing and prop hunt, the view stream for poker and Grid Duel. |
| Who directs | The room's host: its copy of the night moves on, the others run only the timers down and verify. |
| Messages | `Event` (host to room) and `Action` (a guest's pick) with payload tag `0x04`; no new wire message. |
| Prizes | Each machine grants its own player's prize from the shared scoreboard. Nothing about a prize travels. |

### Starting and the rounds

The room screen's Game row lists the active pack's nights by name
ahead of the single games - the built-in **Practice Night** with no pack.
Start needs every seated guest ready, as a game does. The host builds the
night from the seated players and sends it to the room; from then on the
party card holds every player's pad between rounds (the room screen still
opens over it).

A round's start order carries the instance id `party:<night>:<round>:<match>`.
Every peer builds the round from its own copy of the night:
`party::round_section` cuts the base instance short (hands and stack,
holes, laps, rounds) and widens a table, a grid or an arena to the room's
four seats; a seat with no player is the AI. A Grid Duel match seats its
two ladder players and streams the table to everyone else, who watches.
The room screen's own rows give way to **End the night** for the host.

Guests ready themselves when an intro card comes up, and the host starts
the round once the relay's lobby shows them ready (re-sending every second;
a round nobody can start within 20 seconds is void). The world's own pad is
withheld through a round, so no player can close it - or its results -
under the others: the host holds the results three seconds, ends the
session, scores the placements and sends the night on.

### One scoreboard

Every change the host makes travels as a `PartyEvent`: the operation (a
round's placements, a duel's winner, a void, a player leaving, a pick, the
end) and the whole night after it, with the night's scoreboard hash in the
event's `hash_after`. A guest applies the operation to its own copy and
compares hashes, so the scoreboard each player sees is one their machine
computed; a guest whose copy does not reach the host's hash, or that has no
copy (a late joiner), adopts the host's night and counts a resync. Placements
are re-ranked among the night's players, so an AI seat that beats a player
takes no place of theirs.

### When something goes wrong

| Event | What happens |
|---|---|
| A player drops mid-round | The AI plays their seat out (the relay's seat hand-over); they leave the night before the round's placements count, scoring nothing more. Their total stands on the board; they win no prize. |
| A desync in a lockstep round | The round ends for everyone with the desync noted; the host records it void (no points, no wager stakes) and the night goes on. |
| Someone ends a round early | The same: a round whose results never came up is void. |
| The host leaves between rounds or during a lockstep round | The relay migrates the room; the new host directs from its own copy, marking the old host gone. |
| The host leaves during a view-stream round | The room closes (the state lived on the host): every copy ends early on the standings so far, with no prizes. |
| A player joins mid-night | The relay admits joiners between rounds only. The host moves them off the seats and sends them the night: they watch lockstep rounds as spectators and view-stream rounds as watchers (every hidden card stays hidden). |

### Reactions

Emotes reach everyone as chat lines and also float over a game in play and
over the party card as small bubbles for a few seconds. They are sent from
the room screen's Emote row, the native window's number keys `1` to `5`
(when not bound to a pad button; mid-game too), and the browser page's
emote buttons.

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

`crates/engine-netplay/tests/` (disc-free) runs whole `World`s, each with
its own `NetPlay`, through the relay core on loopback links:

- `modes.rs` - two poker hands over the view stream, the guest never
  holding the host's hole cards before a showdown and ending on the host's
  table; a golf round and a prop hunt match in lockstep with every shared
  frame's hash equal and one result; a desync caught on both peers, both
  back in the lobby and able to start again; a racer who leaves handing
  the seat to the AI while the rest race on.
- `roaming.rs` - a guest walking moves its avatar on the host, trailing it
  and never backwards; chat and emotes reach the room.
- `sockets.rs` - a host and a guest on the threaded relay over real TCP on
  an ephemeral port play poker frames; the port is closed after shutdown.
- `party.rs` - the built-in night end to end: four peers through golf,
  poker, a race and the Grid Duel ladder ending on one scoreboard hash with
  every guest verifying every round and each world granted its own prize; a
  player dropping mid-round (total kept, nothing more scored, no prize); a
  desynced golf round voided while the night goes on; a late joiner moved
  off the seats and watching to the end.
