# legaia-engine-core

Where the engine's kernels meet. This crate owns `World` - the one struct
that holds a running game - and everything that has to read or write it: the
VM host implementations, the scene host that ticks a loaded scene, the battle
loop, the menu runtime and the save round-trip. It draws nothing and plays
nothing.

## What lives here

- **`World` and its state.** The VM contexts, the actor table and the
  scene-flow latches are direct fields; everything else is one plain-data
  sub-struct per subsystem, reached as `world.<group>.<field>`
  ([layout](../../docs/subsystems/engine.md#how-world-is-laid-out)).
- **The tick.** `World::tick` is one retail vsync: the effect pool, the
  per-actor move-VM pass, then the scene mode's own step. The rules that
  turn a host's display frames into ticks (`frame_step`) are here too, so
  every host paces the simulation the same way.
- **The VM hosts.** Each interpreter in `engine-vm`, `engine-motion-vm` and
  `engine-battle-vm` reaches the game through a host trait; this crate
  implements those traits over `World`.
- **The scene host.** `SceneHost` owns the `World` and the `ProtIndex`,
  enters scenes, ticks them and drains the queues a tick fills: door warps,
  BGM routing through `BgmDirector`, the FMV hand-off.
- **The stateful side of battle, menus and minigames.** The round loop, the
  command flow, the pause-menu runtime, the minigame sessions' entry points
  and the scene assembly they need.
- **Saves.** `World::save_full` / `load_full` over the LGSF container in
  `legaia-save`, and the retail memory-card write.
- **Scene staging and export.** The scene questions that take simulation
  state (which placed objects are live, which records the story hides) and
  the `.glb` export built on them.

## The split line

A rule or a state machine that never touches `World` does not belong here. It
lives in the kernel crate for its area, where it can be unit-tested alone:

| Area | Kernel crate |
|---|---|
| Battle arithmetic, monster AI, catalogs, encounters, level-up | [`engine-battle`](../engine-battle/README.md), [`engine-battle-vm`](../engine-battle-vm/README.md) |
| Field actor programs, camera, regions, world map | [`engine-field`](../engine-field/README.md) |
| Menus, title, shop, inn, memory card | [`engine-menus`](../engine-menus/README.md), [`engine-dialog`](../engine-dialog/README.md) |
| Minigame rules and their 3D surfaces | [`engine-minigames`](../engine-minigames/README.md), [`engine-fishing`](../engine-fishing/README.md), [`engine-minigame-scenes`](../engine-minigame-scenes/README.md) |
| Effects | [`engine-effects`](../engine-effects/README.md) |
| Mode table, pad input, streaming, fades | [`engine-mode`](../engine-mode/README.md), [`engine-system`](../engine-system/README.md) |
| Scene resolution and loading, the virtual filesystem | [`engine-scene`](../engine-scene/README.md) |

A module here that shares its name with a kernel module holds only the
functions that need a `World` or a loaded scene; the rules are in the kernel
crate. Paths name the owning crate - nothing is re-exported.

## What it must not depend on

- **No renderer, window or audio device.** No `wgpu`, `winit` or `cpal`, and
  none of `engine-render`, `engine-audio`, `engine-ui` or `engine-screens`.
  That is what lets the browser play page run this crate unchanged.
- **Nothing above it.** `engine-session`, `parity` and the hosts depend on
  this crate, never the reverse.
- **No patcher or emulator crate in the library.** The patcher and the
  save-state readers appear only under `[dev-dependencies]`, for the
  disc-gated runtime oracles.

## Place in the layering

```text
engine-shell / web-viewer / asset-viewer      hosts
engine-session, engine-screens, parity        host-shared top
engine-core                                   World, scene host, battle loop
engine-scene                                  disc-facing scene loading
engine-* kernel crates, render-kernels        World-free rules
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
