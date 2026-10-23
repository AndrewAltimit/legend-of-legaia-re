# legaia-engine-core

Where the engine's kernels meet. This crate owns `World` - the one struct
that holds a running game - and everything that has to read or write it: the
VM host implementations, the tick, the battle loop and the party-record
half of a save.
It draws nothing and plays nothing. The two layers that drive a `World` from
outside sit directly above it: the scene host
([`engine-scene-host`](../engine-scene-host/README.md)) and the pause-menu
runtime ([`engine-menu-runtime`](../engine-menu-runtime/README.md)).

## What lives here

- **`World` and its state.** The VM contexts, the actor table and the
  scene-flow latches are direct fields; everything else is one plain-data
  sub-struct per subsystem, reached as `world.<group>.<field>`
  ([layout](../../docs/subsystems/engine.md#how-world-is-laid-out)).
- **The tick.** `World::tick` is one retail vsync: the effect pool, the
  per-actor move-VM pass, then the scene mode's own step. The rules that
  turn a host's display frames into ticks are `legaia_engine_field::frame_step`,
  over the `FrameStepWorld` seat this crate implements for `World`.
- **The VM hosts.** Each interpreter in `engine-vm`, `engine-motion-vm` and
  `engine-battle-vm` reaches the game through a host trait; this crate
  implements those traits over `World`.
- **The `World` half of scene flow.** New Game seeding (`new_game`), the
  live-loop arming (`live_loop`), the player-rig source
  (`World::player_rig_source`) and the constants the scene host and the tick
  share (`world::EFFECT_MODEL_LIBRARY_BASE`, `world::BGM_SUB_OP_ENGINE_STOP`).
  `SceneHost`, which owns the `World` and the `ProtIndex` and enters and
  ticks scenes, is `engine-scene-host`.
- **The stateful side of battle and minigames.** The round loop, the
  command flow, the battle HUD state (the glides and banners `BattleState`
  carries) and the minigame sessions' entry points. The minigame venues and
  surfaces they drive, and the minigame seat itself (`MinigameState`), are
  `engine-minigame-scenes`. What the battle screen reads back off that state
  is [`engine-battle-hud`](../engine-battle-hud/README.md), above this
  crate.
- **Saves.** The party-record half (`World::save_party` / `load_party`),
  which the tick and New Game run too. The full LGSF round trip over it
  (`save_full` / `load_full`) is
  [`engine-save-state`](../engine-save-state/README.md), and writing a save
  into a mounted memory-card image is `engine-menu-runtime::card_write`, both
  above this crate.
- **What the hosts read back** is not here. The host-facing reads of a
  `World` - the per-frame camera resolution (`camera_view`), the
  visible-tile crop policy, scene staging, the world-map markers and sky, the
  field party HUD and the menu / minigame view models - are
  [`engine-world-views`](../engine-world-views/README.md), and the battle
  screen's are [`engine-battle-hud`](../engine-battle-hud/README.md), both
  above this crate. The `.glb` export is
  [`engine-export`](../engine-export/README.md), above it too.

## The split line

A rule or a state machine that never touches `World` does not belong here. It
lives in the kernel crate for its area, where it can be unit-tested alone:

| Area | Kernel crate |
|---|---|
| Battle arithmetic, monster AI, catalogs, encounters, level-up | [`engine-battle`](../engine-battle/README.md), [`engine-battle-vm`](../engine-battle-vm/README.md) |
| Field actor programs, camera, regions, world map | [`engine-field`](../engine-field/README.md) |
| The shared GTE camera, its tweens and shake | [`engine-camera`](../engine-camera/README.md) |
| Slot-B cast / summon module bodies | [`engine-cast`](../engine-cast/README.md) |
| The field overlay's developer-menu bodies | [`engine-dev-menu`](../engine-dev-menu/README.md) |
| Menus, title, shop, inn, memory card | [`engine-menus`](../engine-menus/README.md), [`engine-dialog`](../engine-dialog/README.md) |
| Minigame rules, their 3D surfaces and venues, the minigame seat | [`engine-minigames`](../engine-minigames/README.md), [`engine-fishing`](../engine-fishing/README.md), [`engine-minigame-scenes`](../engine-minigame-scenes/README.md) |
| Effects | [`engine-effects`](../engine-effects/README.md) |
| Mode table, pad input, streaming, fades | [`engine-mode`](../engine-mode/README.md), [`engine-system`](../engine-system/README.md) |
| Scene resolution and loading, the NPC catalog, the virtual filesystem | [`engine-scene`](../engine-scene/README.md) |

A module here that shares its name with a kernel module holds only the
functions that need a `World` or a loaded scene; the rules are in the kernel
crate. Paths name the owning crate - nothing is re-exported.

## What it must not depend on

- **No renderer, window or audio device.** No `wgpu`, `winit` or `cpal`, and
  none of `engine-render`, `engine-audio`, `engine-ui` or `engine-screens`.
  That is what lets the browser play page run this crate unchanged.
- **Nothing above it.** `engine-scene-host`, `engine-menu-runtime`,
  `engine-session`, `parity` and the hosts depend on this crate, never the
  reverse. A `World` field the scene host seeds at scene entry is `pub` for
  that reason; the scene host is not named here.
- **No patcher or emulator crate in the library.** The patcher and the
  save-state readers appear only under `[dev-dependencies]`, for the
  disc-gated runtime oracles.

## Place in the layering

```text
engine-shell / web-play / asset-viewer        hosts
engine-session, engine-screens, parity        host-shared top
engine-menu-runtime                           the pause menu over World
engine-scene-host                             SceneHost, resume, live preview
engine-world-views, engine-battle-hud         reads of World
engine-core                                   World, the tick, battle loop
engine-minigame-scenes                        minigame venues + surfaces
engine-scene, engine-* kernels, render-kernels   World-free rules + scene loading
format crates                                 typed disc data
```

The full edge set and the gate over the kernel crates are in
[`docs/subsystems/engine.md`](../../docs/subsystems/engine.md#crate-layering).

## Tests

`tests/*.rs` build as one `integration` binary; select a file by module path
(`cargo test -p legaia-engine-core --test integration foo::`). Tests that
read a disc skip and pass when `LEGAIA_DISC_BIN` is unset.

## Where the behaviour is documented

| Topic | Page |
|---|---|
| Architecture, frame model, hosts, fidelity knobs | [`engine.md`](../../docs/subsystems/engine.md) |
| Field scenes, locomotion, the field VM | [`field-locomotion.md`](../../docs/subsystems/field-locomotion.md), [`script-vm.md`](../../docs/subsystems/script-vm.md) |
| Battle: round loop, command flow, action SM, HUD | [`battle.md`](../../docs/subsystems/battle.md) and the pages it maps |
| Cutscenes and movies | [`cutscene.md`](../../docs/subsystems/cutscene.md) |
| Pause menu, shop, inn, save screen | [`field-menu.md`](../../docs/subsystems/field-menu.md), [`shop.md`](../../docs/subsystems/shop.md), [`inn.md`](../../docs/subsystems/inn.md), [`save-screen.md`](../../docs/subsystems/save-screen.md) |
| World map | [`world-map.md`](../../docs/subsystems/world-map.md) |
| Ambient effects | [`field-ambient-fx.md`](../../docs/subsystems/field-ambient-fx.md) |
| `.glb` export | [`vrchat-world-export.md`](../../docs/tooling/vrchat-world-export.md) |
| Keeping the hosts in step | [`host-drift.md`](../../docs/tooling/host-drift.md) |
