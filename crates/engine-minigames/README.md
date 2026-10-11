# legaia-engine-minigames

The minigame **rules engines** of the engine port: each is a headless state
machine driven by disc-parsed tables, with no `World`, no scene loading and
no renderer. `engine-core` depends on this crate, never the other way round.
Hosts and tests name the modules here directly
(`legaia_engine_minigames::dance`, `legaia_engine_minigames::slot_machine`, ...).

## What lives here

| Module | Covers |
|---|---|
| `dance` / `dance_tutorial` | Noa's dance rhythm minigame: beat clock, timing-window judge, groove gauge, the floor cast and sprite-part pools, the camera keyframe track, the tutorial. See [`minigame-dance.md`](../../docs/subsystems/minigame-dance.md). |
| `baka_fighter` / `baka_cabinet` / `baka_fighter_chrome` / `baka_impact_fx` / `baka_duel` | The Baka Fighter duel: round state machine, rock-paper-scissors resolver, the cabinet shell, the round chrome, impact effects, and the duel's camera and fighter-clip kernels. See [`minigame-baka-fighter.md`](../../docs/subsystems/minigame-baka-fighter.md). |
| `slot_machine` | The casino slot machine: reel state machine, dual RNG, five-payline payout. See [`minigame-slot-machine.md`](../../docs/subsystems/minigame-slot-machine.md). |
| `minigame_actor` / `minigame_fx` | The overlay band's shared actor record and effect-part pool. |
| `other_game_overlay` | PROT 0977 kernels shared by the hub-band games (step scaling, voice cues, the score-tally ramp). |
| `muscle_dome` | The Muscle Dome's World-free half: the PROT 0977 course ladder and score tables, the contest ladder above a leg (`DomeContest`: cursor word, course gating, leg scoring, coin settlement), the fighter stat profiles and `DomeDamageModel`, and the hub screen envelopes. See [`minigame-muscle-dome.md`](../../docs/subsystems/minigame-muscle-dome.md). |
| `prize_exchange` | The casino prize-exchange sub-screen `0x20`: the menu overlay's prize table, its 4-state browse / confirm / commit machine, and `apply_redeem` over any bag implementing `RedeemBag`. See [`shop.md`](../../docs/subsystems/shop.md). |
| `tile_board` | The op-`0x49` tile board: cell codes, header, walk state machine, fades, prompt. See [`tile-board.md`](../../docs/subsystems/tile-board.md). |

The fishing minigame (`PondSession`, its actors, the venue chrome, the hub
screen and the venue floor grid `minigame_floor`) is the crate below this
one, [`legaia-engine-fishing`](../engine-fishing/README.md).

## What stays in `engine-core`, and why

The split line is `World`. A module moves only when nothing in it needs the
simulation crate:

- **World glue** - `World::enter_dance` / `tick_fishing` / the hub and
  exchange methods, `minigame_entry` (mode-24 door warps), `minigame_status`,
  `casino_coin_bank`, `fishing_exchange_input`, `timed_fight`.
- **Scene assembly** - `dance_venue`, `dance_cast_scene`, `fishing_scene`,
  `fishing_venue`: they build meshes and cameras out of a loaded `Scene` /
  `SceneResources`. The duel and arena surfaces (`baka_duel_scene`,
  `muscle_dome_scene`) load through a `read_prot` closure instead and live in
  [`legaia-engine-minigame-scenes`](../engine-minigame-scenes/README.md), which
  needs `engine-menus` and so cannot sit in this crate. The pure kernels
  those modules run on (the duel camera, the dance camera track) are in this
  crate.
- **Disc and scene readers** - `dance::stage_dance_hud_vram` (reads the HUD
  art through a `ProtIndex`) and `fishing_actors::rod_mesh_from_scene` (lifts
  the rods out of a loaded `Scene`). `engine-core`'s `dance`, `fishing_actors`
  and `fishing_hub` modules hold only these; the rules are named in the
  owning crate.
- **The Muscle Dome leg** - a leg is an ordinary battle under the hood
  (spells, the battle command menu, the arts input), so `MuscleDomeSession`,
  its command menu, the ring / magic gates and the loadouts built off a live
  roster stay in `engine-core::muscle_dome`; `muscle_ringside` stays too.
- **The engine bag** - `prize_exchange::apply_redeem` is generic over
  `RedeemBag`; `engine-core` implements it for the world's `ItemBag`.
  `tile_board`'s per-cell draw assembly reads the `World` and is
  `engine-core::tile_board`.

The pure leaves these engines share with the rest of the engine (the BIOS
`rand()` LCG, both pad-word layouts, the `.MAP` region tables, the field clip
step, the retail camera globals, the camera-relative glide and the menu cursor
navigator `menu_input`) live lower: pad words, `rand` and `menu_input` in
`legaia-engine-system`, the camera kernels in `legaia-engine-battle-vm`, and
the region tables and clip step in `legaia-engine-field`.
