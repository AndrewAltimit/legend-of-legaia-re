# legaia-engine-minigames

The minigame **rules engines** of the engine port, split out of
`legaia-engine-core`: each is a headless state machine driven by disc-parsed
tables, with no `World`, no scene loading and no renderer. `engine-core`
depends on this crate, never the other way round, and re-exports every module
at its old path (`legaia_engine_core::dance`, `legaia_engine_core::slot_machine`,
...), so hosts and tests name the same paths they always did.

## What lives here

| Module | Covers |
|---|---|
| `dance` / `dance_tutorial` | Noa's dance rhythm minigame: beat clock, timing-window judge, groove gauge, the floor cast and sprite-part pools, the camera keyframe track, the tutorial. See [`minigame-dance.md`](../../docs/subsystems/minigame-dance.md). |
| `baka_fighter` / `baka_cabinet` / `baka_fighter_chrome` / `baka_impact_fx` / `baka_duel` | The Baka Fighter duel: round state machine, rock-paper-scissors resolver, the cabinet shell, the round chrome, impact effects, and the duel's camera and fighter-clip kernels. See [`minigame-baka-fighter.md`](../../docs/subsystems/minigame-baka-fighter.md). |
| `slot_machine` | The casino slot machine: reel state machine, dual RNG, five-payline payout. See [`minigame-slot-machine.md`](../../docs/subsystems/minigame-slot-machine.md). |
| `fishing` / `fishing_actors` / `fishing_chrome` / `fishing_hub` | The fishing minigame's `PondSession`, its rod / lure / line actors, the venue chrome and the hub screen. See [`minigame-fishing.md`](../../docs/subsystems/minigame-fishing.md). |
| `minigame_actor` / `minigame_fx` / `minigame_floor` | The overlay band's shared actor record, effect-part pool and venue floor grid. |
| `other_game_overlay` | PROT 0977 kernels shared by the hub-band games (step scaling, voice cues, the score-tally ramp). |

## What stays in `engine-core`, and why

The split line is `World`. A module moves only when nothing in it needs the
simulation crate:

- **World glue** - `World::enter_dance` / `tick_fishing` / the hub and
  exchange methods, `minigame_entry` (mode-24 door warps), `minigame_status`,
  `casino_coin_bank`, `fishing_exchange_input`, `timed_fight`.
- **Scene assembly** - `dance_venue`, `dance_cast_scene`, `baka_duel_scene`,
  `fishing_scene`, `fishing_venue`, `muscle_dome_scene`: they build meshes and
  cameras out of a loaded `Scene` / `SceneResources`. Where such a module
  carried a pure kernel (the duel camera, the dance camera track), the kernel
  moved here and the module re-exports it.
- **Disc and scene readers** - `dance::stage_dance_hud_vram` (reads the HUD
  art through a `ProtIndex`) and `fishing_actors::rod_mesh_from_scene` (lifts
  the rods out of a loaded `Scene`). `engine-core`'s `dance`, `fishing_actors`
  and `fishing_hub` are thin modules that glob-re-export the crate's module
  and add only these, so `legaia_engine_core::dance::*` still names both
  halves.
- **Muscle Dome** - `muscle_dome` and `muscle_ringside` are an ordinary battle
  under the hood (spells, the battle command menu, the arts input), so they
  sit with the battle modules.
- **`prize_exchange` / `tile_board`** - both drive the world's `ItemBag` /
  `World` directly.

The pure leaves these engines share with the rest of the engine (the BIOS
`rand()` LCG, both pad-word layouts, the `.MAP` region tables, the field clip
step, the retail camera globals and the camera-relative glide) live one layer
lower, in `legaia-engine-vm`.
