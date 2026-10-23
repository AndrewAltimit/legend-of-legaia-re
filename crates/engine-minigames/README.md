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
| `muscle_ringside` | The Muscle Dome contest hub's screens (`FUN_801CF870`): the first-visit strip, the ROUND card, the INTERVAL tally and its cue ring, the ringside-still backdrop, and `HubTimers`, which steps them off a `HubTickInputs`. |
| `prize_exchange` | The casino prize-exchange sub-screen `0x20`: the menu overlay's prize table, its 4-state browse / confirm / commit machine, and `apply_redeem` over any bag implementing `RedeemBag`. See [`shop.md`](../../docs/subsystems/shop.md). |
| `tile_board` | The op-`0x49` tile board: cell codes, header, walk state machine, fades, prompt. See [`tile-board.md`](../../docs/subsystems/tile-board.md). |
| `tile_board_state` | The tile board's state as one unit held on the composite `World`. |

The fishing minigame (`PondSession`, its actors, the venue chrome and the hub
screen) is a sibling crate, [`legaia-engine-fishing`](../engine-fishing/README.md);
neither depends on the other. The minigame floor both stand on and the dance
floor's step markers (`minigame_floor`, `dance_marker`) are
`legaia-engine-field`'s.

## What lives elsewhere, and why

The split line is `World` (and, for scene assembly, `engine-scene`):

- **World glue** - `World::enter_dance` / `tick_fishing` / the hub and
  exchange methods, `casino_coin_bank` and `fishing_exchange_input`
  (`engine-core`); the status caption is `engine-world-views`'
  `WorldMinigameCaption`. The mode-24 door warps
  (`minigame_entry`) are `engine-mode`'s, the timed fight's gate and numbers
  `engine-world-views::timed_fight`.
- **Scene assembly and the minigame seat** - `dance_venue`,
  `dance_cast_scene`, `fishing_scene`, `fishing_venue` (meshes and cameras
  out of a loaded `Scene` / `SceneResources`), the duel and arena surfaces,
  the disc readers `dance_hud_art` and `fishing_rod`, and `MinigameState`
  live in [`legaia-engine-minigame-scenes`](../engine-minigame-scenes/README.md),
  which needs `engine-scene` and `engine-menus` and so cannot sit in this
  crate. The pure kernels those modules run on (the duel camera, the dance
  camera track) are in this crate. `engine-core` keeps only the `World`
  half (`dance_venue::sync_dance_venue`) and `engine-scene-host` the
  `SceneHost` one (`fishing_venue::tick_fishing_venue_on_host`); the fishing
  hub screen is `engine-fishing::fishing_hub`.
- **The Muscle Dome leg** - a leg is an ordinary battle under the hood
  (spells, the battle command menu, the arts input), so `MuscleDomeSession`,
  its command menu, the ring / magic gates and the loadouts built off a live
  roster stay in `engine-world-views::muscle_dome`. The contest hub's
  screen timers (`muscle_ringside`) are here: `HubTimers::tick` takes the four
  things it reads off the simulation as `HubTickInputs`, which
  `World::tick_muscle_hub` fills.
- **The engine bag** - `prize_exchange::apply_redeem` is generic over
  `RedeemBag`; `engine-menus` implements it for the world's `ItemBag`.
  `tile_board`'s per-cell draw assembly reads the `World` and is
  `engine-world-views::tile_board`.

The pure leaves these engines share with the rest of the engine (the BIOS
`rand()` LCG, both pad-word layouts, the `.MAP` region tables, the field clip
step, the retail camera globals, the camera-relative glide and the menu cursor
navigator `menu_input`) live lower: pad words, `rand` and `menu_input` in
`legaia-engine-system`, the camera kernels in `legaia-engine-camera` and
the clip step in `legaia-engine-vm` (`actor_tick`). The `.MAP` region tables
are `legaia-engine-field`'s, read by the shared minigame floor there; this
crate reaches `legaia-engine-field` for that floor's step markers only, and
does not depend on `legaia-engine-battle` or `legaia-engine-fishing`.
