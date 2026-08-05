# Runtime reach triage

[`replay-port-coverage.py`](port-catalog.md) joins `cargo llvm-cov` output for
the pad-only replay ladders against the port catalog's `// PORT:` anchors and
reports three sets. Two of them are defect lists and are normally empty. The
third - *live but never entered* - is neither empty nor a defect list, and it is
by far the largest: the static live count is several times what a playthrough
executes.

This page is the per-row verdict for that third set, so the question "is this a
gap in the port, a gap in the ladders, or neither" is answered once per address
instead of re-derived. It is a snapshot of a worklist: a row leaves the page
when a ladder reaches it or the wiring lands. What outlives the rows is the
bucket definitions plus the structural facts below about what a pad-only ladder
can and cannot execute at all.

## Buckets

- **(a) NO-LADDER** - reachable in real play on at least one host, but no
  existing ladder drives that content. The fix is a replay fixture, not a wire.
- **(b) GATED** - reachable only behind a story flag, scene or game state the
  ladders do not reach. The gate is named per row.
- **(c) HOST-DEAD** - the static graph finds a caller, and nothing on any of the
  three hosts reaches it in play. The valuable bucket: these are `live` in the
  permissive graph, so `--live-audit`'s *undisclosed inert ports* section cannot
  see them.
- **(d) NOT-PLAYTHROUGH** - not playthrough-shaped. A preservation parser whose
  host is a CLI subcommand is *wired* (a CLI subcommand is a host root, per
  [`stale-not-wired-triage.md`](stale-not-wired-triage.md)); "no engine consumer"
  is a different claim and is written as one.

Liveness is an upper bound, so bucket (c) is never assigned off the static
verdict. Each (c) row below rests on a strict caller scan of the workspace with
comment lines dropped and `#[cfg(test)]` bodies excluded - "the only caller is a
unit test" is the finding, and it is not visible to a scan that counts doc
comments as references.

## What a pad-only ladder structurally cannot execute

Four whole crates report **zero** executed regions across every ladder, and the
reason is the harness rather than the port. The ladders drive the headless
`BootSession`, which constructs no renderer, no audio device and no draw list;
`crates/engine-shell/src/boot.rs` names neither `engine-ui` nor `engine-render`,
and neither does any of the five ladder sources.

| crate | source files in the coverage data | files with an executed region |
|---|---|---|
| `engine-ui` | 30 | 0 |
| `engine-render` | 37 | 0 |
| `engine-audio` | 20 | 0 |
| `mdec` | 6 | 0 |

Two more structural exclusions matter as much and are easy to misread as port
gaps:

**A `bin/` target is unreachable from any `#[test]`.** `cargo llvm-cov --test`
still builds and instruments the binary, so all 33 files of
`crates/engine-shell/src/bin/legaia-engine/` appear in the coverage data with
nothing executed. The native window's entire composition layer lives there, so
no integration test can drive it. The browser hosts do not have this problem:
their composition is in `crates/web-viewer/src`, a library.

**The browser minigames page is outside the union entirely.** `minigame_replay`
drives the *engine-shell* minigame path, not the standalone page, so a port
wired only on that page is never-entered by construction. Its own oracles live
in `crates/web-viewer/tests/`.


**An anchor is attributed to its first site, and a data anchor has no site.**
The report emits one row per address carrying `sites[0]`, so the file and symbol
it names may not be the anchor that made the address `live`, and for a
multi-anchor address it is simply the first one the source walk found.
`8002174c` is the worked example: the row names `apply_morph_weights`, which the
liveness pass calls inert, while the address is `live` through the sibling
`MorphWeightEnvelope::tick` in the same file. Separately, a tag on a plain data
`struct` with no `impl` falls back to *module* scope for liveness and to *the
next function in the file* for coverage - two different symbols, neither of them
the port. Every `mode.rs`, `new_game.rs`, `sound_state.rs` and `scene_bundle.rs`
row below is that shape.


<!-- BEGIN engine-core -->

## `engine-core`

`engine-core` carries the largest crate share of the never-entered set. Every
address in it is accounted for below.

| bucket | addresses |
|---|---|
| NO-LADDER | 104 |
| GATED | 10 |
| HOST-DEAD | 45 |
| NOT-PLAYTHROUGH | 6 |

### HOST-DEAD, undisclosed

The rows worth acting on. Each is `live` because a module-scope or type-scope
`PORT:` tag widened the verdict to a whole file, while the symbol the address
names has no production caller anywhere in the workspace - only unit tests and
disc-gated oracles. None of them carries a `NOT WIRED:` disclosure, so nothing
in the existing gate set reports them.

| group | n | addresses | why |
|---|---|---|---|
| `cd_dma.rs` | 7 | `8003de7c` `8003e800` `8003e8a8` `8003eb98` `8003f128` `8005ea84` `8003dda0` | `ProtCdDmaHost` is constructed only inside `#[cfg(test)]` and the disc-gated `cd_dma_real_prot` test; no crate outside `engine-core` names `cd_dma`, and `overlay_loader`'s only non-test implementor is that same test-only host. |
| `stream_file.rs` | 5 | `800558fc` `80055a5c` `800559ec` `80055ac8` `8003e964` | `StreamFileHost` has exactly one production mention - its own `impl` line. Every construction is a unit test or the disc-gated `stream_file_real` oracle. |
| `mode.rs` | 4 | `80017978` `80025eec` `80025f2c` `80025f74` | `ModeDriver` - the port of the 28-entry game-mode state table, and the only caller of `per_frame_stage` - is named by no code outside `mode.rs`. `CARD_FRAME_BODY` is read only in that file's tests. |
| `sound_state.rs` | 1 | `80020038` | `DRAW_ENV_INIT` is read at three sites, all inside that file's `#[cfg(test)]` block. |
| `scene_bundle.rs` | 1 | `80020118` | `field_load_entry_plan` is called at three sites, all in that file's `#[cfg(test)]` block. |
| `prize_exchange.rs` | 1 | `801dc1cc` | `PrizeExchangeSession` has one production mention - its own `impl` line. |
| `scene_name_sync.rs` | 1 | `8001d7f8` | `sync_scene_name` is called only from that file's tests. The `fn` anchor was already disclosed; the `//! PORT:` module tag on the same address was not, and it is the module tag that carries the liveness verdict. |
| `save_select.rs` | 1 | `801e3294` | `card_frame_tick` - the only thing that advances a `CardIoMachine` - carried a full disclosure, but on the function rather than on the type anchor the address is keyed to. |

Three of these name routines that are heavily used on the disc, so the gap is a
port that is not reached rather than a port of dead code. A five-form
[address-reference scan](address-reference-scan.md) puts `FUN_8003DE7C` at 127
`jal` sites spanning `SCUS_942.54` and eleven overlay images, `FUN_800558FC` at
four (two in SCUS, two in the battle-action overlay), and `FUN_80025EEC` in
twelve slots of the game-mode table at `0x8007078C` - every other entry, which
is the odd-indexed per-frame modes the port's own tag claims.

`8003dda0`, `801dc1cc`, `8001d7f8` and `801e3294` now carry `NOT WIRED:`
disclosures naming their specific prerequisite. The other seventeen do not, and
the reason is mechanical: their anchors are module-scoped and analysed live
under the receiver-gated graph, so a disclosure on them would appear in
`--live-audit`'s *tagged `NOT WIRED` but analysed live* section as a false
accusation. Disclosing them needs either a per-anchor re-key onto the function
that implements each address, or a module-scope exemption in the stale-tag test.

### HOST-DEAD, disclosed

Same verdict, already stated in the source. No further disclosure work; they are
listed so the bucket count is the whole of what no host reaches.

| group | n | addresses | why |
|---|---|---|---|
| `save_subscreen.rs` | 8 | `801e4f40` `801dd12c` `801dd26c` `801d98f0` `801dae24` `801daef4` `801dafd4` `801dbc5c` | `SaveScreenMachine` - the graph every sub-screen hangs off - is constructed only in that file's tests. |
| `card_bu_io.rs` | 4 | `801e0598` `801e3d68` `801e380c` `801e435c` | The engine has no `bu` device layer under the save screen. |
| `cutscene_script_elements.rs` | 3 | `801d5d60` `801d6058` `801d27e0` | No element-actor dispatch; the three `step` bodies have no production caller. |
| `shop.rs` | 2 | `801db7f4` `801dbd94` | The retail menu-overlay quantity sub-screens, distinct from the engine's own shop session. |
| `camera_rel_glide.rs` | 1 | `8002149c` | No producer for the family's 20-halfword spawn record. |
| `card_flow.rs` | 1 | `801e13b8` | Nothing owns the state word `CardWriteMachine` drives. |
| `effect_ribbon.rs` | 1 | `801cfa48` | The only production mention of the module is its `pub mod` line. |
| `field_save_screen_actor.rs` | 1 | `80024190` | The engine reaches the save UI as host screen state, so there is no overlay swap to sequence. |
| `scene_transition_actor.rs` | 1 | `80021934` | Scenes load as `Scene` resources, not as a streamed raw bundle, so nothing seats the actor. |
| `morph_weight_apply.rs` | 1 | `8002174c` | Both anchors disclosed; `MorphWeightEnvelope` is test-only. |
| `world/field_movement.rs` | 1 | `800467e8` | Declined with proof - see below. |

`800467e8` is the one to read before proposing a wire. `remap_pad_direction` is
a faithful port of the retail 45-degree camera-relative pad remap, and the tag
declines to route the live pad path through it because the two implementations
agree on every even `rot` - that is, on every camera a retail field scene
installs. Wiring it would be a provable identity, which is worse than the gap.

### NO-LADDER, harness-blind

Wired, and reached in real play on a host the ladder harness cannot execute. No
new pad fixture reaches these; the options are a windowed-host coverage export,
or moving the wiring down out of the bin into the shared session layer where all
three hosts and the ladders would share it.

| group | n | addresses | host |
|---|---|---|---|
| `dance.rs` (HUD + banner) | 7 | `801d231c` `801d3e28` `801d32f8` `801d2524` `801d2d98` `801d2f38` `801d387c` | native `window/hud.rs`, `window/minigames.rs`, `window/minigame_fx.rs` |
| `fishing_chrome.rs` | 6 | `801d03b0` `801d78c0` `801d74b0` `801d7a5c` `801d70ec` `801d7c30` | native `window/minigames.rs`, `window/hud.rs` |
| `fishing.rs` (prize rows) | 5 | `801d0c3c` `801d6f90` `801d712c` `801d092c` `801d06c8` | native + browser fishing pages |
| `fishing_actors.rs` | 4 | `801d2050` `801d765c` `801d2278` `801d4948` | native fishing block |
| `shop.rs` (panel kernels) | 4 | `801d4868` `801d5de0` `801d5510` `801d5ae8` | both rendering hosts' menu draw paths |
| `baka_fighter.rs` (digit strips) | 3 | `801d6a18` `801d6f44` `801d69e4` | native `window/hud.rs` |
| `save_select.rs` (card directory) | 3 | `801e1208` `801e3af0` `801e3ba0` | browser `web-viewer::cards` |
| `minigame_floor.rs` | 2 | `801d2a10` `801d6028` | native `window/minigames.rs` |
| `dance.rs` (sting + clip gate) | 2 | `801d3d78` `801d4098` | browser dance page |
| `tile_board.rs` / `frame_tick.rs` | 2 | `801e0f3c` `801e0b1c` | both; no ladder installs a board |
| `pause_screens.rs` / `save_select.rs` (panel modes) | 2 | `801d6a54` `801e3f74` | both rendering hosts |
| `baka_fighter.rs` (widget quad) | 1 | `801d5ed0` | browser `minigames_baka`, deliberately one-host |
| `dance_tutorial.rs` | 1 | `801d0750` | native tutorial run |
| `slot_machine.rs` | 1 | `801e6f70` | native casino entry |
| `dialog.rs` | 1 | `80038050` | native keyboard handler |
| `world/effects.rs` | 1 | `80058490` | both; no ladder runs a `4C 60` stamp |
| `cutscene.rs` | 1 | `801cea3c` | native `run` subcommand |

### NO-LADDER, content not driven

Wired through `engine-core` and reachable by a headless ladder in principle -
these are the rows a new or deeper fixture would actually convert.

| group | n | addresses | what would reach it |
|---|---|---|---|
| `screen_fx.rs` | 10 | `801de4c8` `801f8d4c` `801f811c` `801f8004` `801f7a9c` `801f88fc` `801f8e6c` `801f849c` `801f8f28` `801f8a34` | a scene whose script spawns an iris mask, letterbox or image panel - the ending scenes the module doc names |
| `fishing.rs` (session kernels) | 6 | `801d5298` `801d0474` `801d0f5c` `801d26cc` `801d3db4` `801d746c` | a fishing rung past rung 4: rod select, a full cast, a landed catch |
| `muscle_dome.rs` | 4 | `801cf074` `801d1184` `801d1510` `801d9bbc` | a dome leg played to its between-leg tally |
| `baka_fighter*.rs` (tally + intro) | 4 | `801d6710` `801d239c` `801d2a28` `801d59d4` | a duel played through its intro card to the end-of-match tally |
| `scene/host*.rs` (BGM plumbing) | 4 | `80019898` `800243f0` `800266e0` `80026520` | a scene transition that pauses and resumes BGM |
| `equip_session.rs` / `menu_arrange.rs` / `menu_item_category.rs` | 4 | `801d9c14` `801cf760` `801d64a8` `801dd0c0` | operating the Equip and Items rows the menu ladder only opens |
| `pause_screens.rs` (special Use) | 4 | `801d7e50` `801d8a58` `801d8b90` `801d8d94` | a Use confirm on Door of Light / Door of Wind / Incense |
| `world/vm_hosts.rs` + `equipment.rs` | 3 | `8003c7ec` `800358c0` `800430ac` | field-VM scripts exercising those op arms |
| `battle_round.rs` / `other_game_overlay.rs` / `save_menu_atlas.rs` | 3 | `801db8b4` `801d14b0` `8002c69c` | one call deeper into round start, the arena and the dialog atlas bake |
| `action_effect_script.rs` | 2 | `801dea50` `80026be0` | a battle action that reaches an effect-script record |
| `text_balloon.rs` | 2 | `8003c764` `801da7f0` | a scene running field-VM `4C E1` |
| `battle_tutorial.rs` | 2 | `801f6b70` `801f747c` | promoting the existing `training_battle` test to a ladder export |
| `clut_fx.rs` | 2 | `801e4c58` `801e4794` | a scene carrying a scripted CLUT-cell effect |
| `shop.rs` (buy list) | 2 | `801db21c` `801db380` | opening a shop stock list and confirming a row |
| `world_map.rs` | 2 | `800196a4` `801d8258` | entering a kingdom overworld through its own transition |
| `cutscene_narration.rs` | 1 | `80037174` | an opening-prologue ladder (`opdeene` / `opstati` / `opurud`) |
| `register_ramp.rs` | 1 | `8003c6a4` | a script running field-VM op `0x43` sub-3..6 |
| `world/narration.rs` | 1 | `8003cf7c` | an inline field-VM conversation rather than the dialog panel |
| `battle_hud.rs` | 1 | `8002c2e4` | building a battle HUD row |

### GATED

Reachable only from a game state the ladders do not seed. The fix is a seeded
save or a longer spine, not a pad stream.

| group | n | addresses | gate |
|---|---|---|---|
| `field_actor_program.rs` | 2 | `801d4a60` `801d5a24` | the `MAN_LOAD_RESUME` story flags that arm the four voice-over programs |
| `world/battle/casting.rs` | 2 | `801dd4b0` `801dd6b4` | a capture-class boss cast |
| `world/vm_hosts.rs` | 1 | `801d2d38` | system flag `0xD`, the three-actor talk lock |
| `name_entry.rs` | 1 | `801f03f0` | `town01`'s opening naming prompt, which every ladder boots past |
| `world/battle/monster_ai.rs` | 1 | `801e7320` | a monster whose `field_flags & 0x380` is set |
| `battle_status_clut.rs` | 1 | `8004ce2c` | a Stone or Rot status landed on an actor |
| `magic_xp.rs` | 1 | `801f452c` | a Seru spell crossing its XP threshold |
| `world/field_movement.rs` | 1 | `801d2404` | a scene with a ledge-hop trigger |

### NOT-PLAYTHROUGH

| group | n | addresses | why |
|---|---|---|---|
| `dev_menu.rs` | 2 | `801dbd04` `801db8f4` | the overlay-0897 developer EVENT FLAG editor |
| `new_game.rs` | 1 | `8001ffa4` | `GAME_STATE_COLD_RESET` is a `const`, read in production by `scene/host/lifecycle` and `world/frame_tick` - wired, but a `const` has no coverage record, so no instrument can report it either way |
| `baka_cabinet.rs` | 1 | `801d553c` | the developer action-table dump retail writes as `ot5stat.txt` |
| `debug_char_editor.rs` | 1 | `801d6e18` | the menu overlay's developer character-parameter editor |
| `cutscene_script_elements.rs` | 1 | `801d841c` | reached only from the dev world-map panel's fade/flash actor |

<!-- END engine-core -->

## Per-crate verdicts

Rows are grouped by module: one address is rarely a distinct decision from its
neighbour in the same file, and a table of 243 one-line rows would hide that.
The `reach` column names the ladder that would enter an (a) row, the gate that
blocks a (b) row, or the disclosure state of a (c) row.

### engine-vm

| module | n | bucket | reach | addresses |
|---|---|---|---|---|
| `actor_alloc.rs` | 3 | (a) | field-actors | `80024c88` `80024d78` `80024dfc` |
| `baka_hub_actors.rs` | 15 | (a) | baka-hub | `801f0adc` `801f1138` `801f159c` `801f16c0` `801f17d8` `801f1890` `801f1950` `801f1a1c` `801f1ab0` `801f1b64` `801f1d90` `801f1e48` `801f1fdc` `801f20b0` `801f2134` |
| `battle_action/overlay_rng.rs` | 1 | (c) | disclosed | `801d0290` |
| `battle_action/pool_ops.rs` | 3 | (a) | battle-target | `801d8a88` `801d8d00` `801db124` |
| `battle_action/spirit.rs` | 1 | (b) | spirit-cast | `801f3990` |
| `battle_action/summon.rs` | 1 | (b) | summon-cast | `801f3c34` |
| `battle_attack_camera.rs` | 1 | (a) | battle-render | `801d71b8` |
| `battle_burst.rs` | 1 | (c) | disclosed | `801f30c4` |
| `battle_camera.rs` | 2 | (a) | battle-render | `801d829c` `801d9d30` |
| `battle_cast_dispatch.rs` | 3 | (b) | spirit-cast | `801dba90` `801f1ed4` `801f2160` |
| `battle_cue_group.rs` | 1 | (b) | spirit-cast | `801e22c8` |
| `battle_formulas/stat_init.rs` | 1 | (a) | minigames-page | `80053cb8` |
| `battle_gauge.rs` | 1 | (a) | battle-render | `80046a20` |
| `battle_gauge_rearm.rs` | 1 | (a) | battle-render | `801f44a0` |
| `battle_ground_grid.rs` | 1 | (a) | battle-render | `801d02c0` |
| `battle_helpers.rs` | 1 | (c) | disclosed | `80046870` |
| `battle_intro_particles.rs` | 2 | (a) | battle-render | `801cfbb4` `801d0164` |
| `battle_intro_styles.rs` | 2 | (a) | battle-render | `801ce8cc` `801d11d0` |
| `battle_intro_swirl.rs` | 2 | (a) | battle-render | `801d1564` `801d1888` |
| `battle_intro_tiles.rs` | 3 | (a) | battle-render | `801d081c` `801d0d24` `801d0e54` |
| `battle_intro_transition.rs` | 1 | (a) | battle-render | `801cf1b0` |
| `battle_party_panel.rs` | 3 | (a) | battle-render | `801d84c0` `801dbb8c` `801dbc30` |
| `battle_stream_slot.rs` | 2 | (c) | disclosed | `80055b4c` `801f17f8` |
| `battle_target_group.rs` | 1 | (a) | battle-target | `801dceac` |
| `battle_value_readout.rs` | 1 | (a) | battle-render | `801e805c` |
| `camera_mover.rs` | 1 | (a) | field-render | `801dd310` |
| `code_lock_actor.rs` | 1 | (c) | disclosed | `801eed58` |
| `dev_equip_commit.rs` | 1 | (a) | dev-menu | `801e5a08` |
| `effect_vm/pool.rs` | 1 | (a) | field-actors | `801de914` |
| `escape_timer.rs` | 1 | (b) | battle-escape | `801d2ebc` |
| `field_ledge_hop_arc.rs` | 1 | (b) | ledge-hop | `801d2298` |
| `field_party_cursor.rs` | 1 | (c) | disclosed | `801f1278` |
| `lib.rs` | 7 | (c) | **actor VM** | `800319a8` `800326ac` `80035334` `800357fc` `80035978` `80035a4c` `801d6628` |
| `menu.rs` | 1 | (a) | menu | `801dd35c` |
| `scus_battle_helpers.rs` | 2 | (c) | disclosed | `80046978` `80055854` |
| `scus_core_helpers.rs` | 5 | (c) | disclosed | `8001fa68` `800203ec` `80020424` `80020454` `800204a4` |
| `travel_art_actor.rs` | 2 | (b) | quick-travel | `801ee094` `801ee328` |
| `world_map.rs` | 1 | (a) | world-map | `801e3e00` |
| `world_map_clut_fade.rs` | 1 | (a) | world-map | `801e4d8c` |
| `world_map_dev_menu.rs` | 1 | (a) | dev-menu | `801e9f64` |
| `world_map_dim.rs` | 1 | (a) | world-map | `801e75dc` |
| `world_map_horizon.rs` | 2 | (a) | world-map | `801c9688` `801d7ea0` |
| `world_map_overlay.rs` | 2 | (a) | dev-menu | `801e5b4c` `801eca08` |
| `world_map_panel.rs` | 3 | (a) | world-map-panel | `801e9b3c` `801e9dc8` `801ea9b0` |
| `world_map_panel_actors.rs` | 5 | (a) | world-map-panel | `801ed590` `801edf00` `801ee5d4` `801ee90c` `801ef014` |
| `world_map_particle_burst.rs` | 1 | (a) | world-map | `801e5338` |

The `disclosed` rows carry a `NOT WIRED` disclosure at their own tag, so they
are inert by the source's own account; they appear here only because the
permissive graph also calls them live. The `lib.rs` row is the one that does
not, and it is written up under [The actor VM has no host caller](#the-actor-vm-has-no-host-caller).

The world-map cluster splits three ways and the split is worth keeping: the
`dev-menu` rows sit behind a host hotkey a pad ladder cannot press, the
`world-map-panel` rows behind the panel-actor screens the spine ladder does not
open, and the plain `world-map` rows behind the overworld render pass.

### engine-ui

Sixty-two of the sixty-three rows are (a), for one reason stated once: no ladder
builds a draw list, so the crate reports zero executed regions. Each anchored
builder was checked against the two browser hosts and the native window, and
every one is reached - `check-ui-host-drift.py` agrees, and its four orphan
waivers cover symbols that are separately disclosed. So the crate is a ladder
gap almost end to end, and the single exception is the casino prize-exchange
confirm window, which is disclosed as unreached at its own tag.

| module | n | bucket | reach | addresses |
|---|---|---|---|---|
| `other_game_hud.rs` | 5 | (a) | draw | `801d02f0` `801d050c` `801d08ec` `801d1308` `801d15c8` |
| `ui_fishing.rs` | 10 | (a) | draw | `801d13f0` `801d1580` `801d1870` `801d1a90` `801d6f10` `801d71d4` `801d7528` `801d75dc` `801d76e0` `801d78ec` |
| `ui_menu/dev_menu_list.rs` | 1 | (a) | draw | `801ead98` |
| `ui_menu/equipment.rs` | 2 | (a) | draw | `801d2094` `801d21c0` |
| `ui_menu/field_panels.rs` | 7 | (a) | draw | `8002c0b0` `801cfd68` `801d0148` `801d030c` `801d30a4` `801d31ec` `801d33d8` |
| `ui_menu/name_entry.rs` | 1 | (a) | draw | `801e6b34` |
| `ui_menu/pause_lists.rs` | 6 | (a) | draw | `801d0d18` `801d0f1c` `801d1b20` `801d2c98` `801d2e74` `801dcb60` |
| `ui_menu/records_screen.rs` | 1 | (a) | draw | `801ed710` |
| `ui_menu/system_menus.rs` | 4 | (a) | draw | `801d1dac` `801d1f10` `801d2b44` `801dcef0` |
| `ui_menu/target_panel.rs` | 1 | (a) | draw | `801d0520` |
| `ui_menu_window_painters.rs` | 16 | (a) | draw | `801d4a80` `801d56fc` `801d5944` `801d61b0` `801d6360` `801dca0c` `801dca50` `801dca94` `801dcad8` `801dcb1c` `801dccb4` `801dce20` `801dcf14` `801dcf84` `801dcfe4` `801dd028` |
| `ui_menu_window_painters.rs` | 1 | (c) | disclosed | `801d603c` |
| `ui_menu_window_painters_large.rs` | 3 | (a) | draw | `801cf5d0` `801d1290` `801d4c28` |
| `ui_title_save/save_select.rs` | 2 | (a) | draw | `801e02a4` `801e3ff0` |
| `ui_title_save/slot_grid.rs` | 2 | (a) | draw | `801e06c0` `801e0fd0` |
| `ui_title_save/slot_info.rs` | 1 | (a) | draw | `801e3ee0` |

One near-miss is worth recording because the grep that finds it is wrong. The
window-25 / window-41 stat-compare chain in `ui_menu_window_painters_large.rs`
reads like an orphan cluster: `compare_panel_draws_for` and
`party_compare_panel_fields` have no caller outside their own file, and their
in-file call sites sit at line numbers past the module's test boundary. They are
not in a test - both are inside `recipient_picker_draws_for`, which both hosts
call from their shop recipient window. Only the *window-25* half
(`equip_compare_panel_fields` and its category chain) is genuinely orphaned, and
that half is disclosed at its tag and waived by the drift gate.

### engine-audio

| module | n | bucket | reach | addresses |
|---|---|---|---|---|
| `anim_cue.rs` | 1 | (c) | disclosed | `800508dc` |
| `footstep.rs` | 1 | (c) | disclosed | `80018db0` |
| `seq_calc.rs` | 5 | (d) | differential | `80062f98` `8006320c` `8006352c` `80063aa8` `800649b0` |
| `seq_events.rs` | 5 | (d) | differential | `800638d8` `80063974` `800639a0` `80063cec` `8006418c` |
| `seq_slots.rs` | 1 | (a) | audio | `8001ff58` |
| `sequencer.rs` | 2 | (a) | audio | `80066b00` `80067550` |
| `sfx.rs` | 2 | (a) | audio | `80035b50` `8004fcc8` |
| `sfx_ring.rs` | 1 | (a) | audio | `8001698c` |
| `shout.rs` | 1 | (a) | arts-swing | `8004c140` |
| `vab_bind.rs` | 3 | (a) | audio | `80066d8c` `80066e50` `80068d94` |

The ten `seq_calc` / `seq_events` addresses are the SsAPI per-frame calc tier.
`Sequencer` is the engine's clean-room replacement and drives playback on its
own clock, so nothing on the audio output path calls these kernels; their host
is the `note-trace` differential CLI, which is what makes a divergence localise
to one kernel. That is stated at their own tags, and it makes them (d) rather
than a wiring gap.

`footstep.rs` is the one audio row that is a real host asymmetry: the cadence
ticks on the browser play page and not in the native window. Its module doc
already says so and names the caller that would close it.

### asset

| module | n | bucket | reach | addresses |
|---|---|---|---|---|
| `battle_char_assembly/assembly.rs` | 2 | (a) | battle-render | `800536bc` `80053898` |
| `battle_char_assembly/texture.rs` | 2 | (a) | battle-render | `80053a28` `80053b9c` |
| `boot_overlay.rs` | 4 | (d) | cli | `8001eef0` `8002574c` `80025ba0` `8003e360` |
| `character_pack.rs` | 1 | (a) | field-render | `8001ebec` |
| `face_anim.rs` | 1 | (a) | battle-render | `8004c7b4` |
| `me_archive.rs` | 2 | (a) | arts-swing | `8002a9cc` `8002b28c` |
| `minigame_slot_scene.rs` | 5 | (b) | slot-bonus | `801cec94` `801cfff0` `801d069c` `801d0fa8` `801d3230` |
| `monster_archive/mesh.rs` | 1 | (a) | battle-render | `80055468` |
| `player_anm.rs` | 1 | (a) | field-render | `8001be80` |
| `save_icon.rs` | 1 | (a) | menu-render | `801e1934` |
| `summon_readef.rs` | 2 | (b) | summon-cast | `801f12d0` `801f19ec` |

`asset` is not bulk-classifiable and the bulk reading would have been wrong: 72
of its 114 files in the coverage data do execute, because the ladders resolve
scene bundles and battle records through them. What never executes is the
*render-facing* half - mesh assembly, texture uploads, the save-icon sheet, the
pose decoders - which is a consequence of the headless harness, not of the
parsers. `boot_overlay` is the only genuinely tooling-only module here; its sole
consumer is the `asset boot-overlay` subcommand.

### mdec

| module | n | bucket | reach | addresses |
|---|---|---|---|---|
| `lib.rs` | 2 | (a) | fmv | `801d0378` `801d0604` |
| `st_ring.rs` | 7 | (a) | fmv | `8005bbf8` `8005ecd4` `8005edc4` `8005ee4c` `8005ef40` `8005f004` `8005f024` |
| `str_player.rs` | 8 | (a) | fmv | `801cf098` `801cf56c` `801cf740` `801cf8b0` `801cf988` `801cfa14` `801cfd84` `801cfebc` |
| `strv2_table.rs` | 1 | (d) | no-input | `801f1a00` |

Seventeen of the eighteen are one fact: no pad-only ladder plays a movie. They
are not untested - `av_decode_oracle`, `fmv_table_roundtrip`, `w5_fmv_handoff`
and the `mdec` crate's own `str_player_segment` / `st_ring_real_str` all drive
them - those oracles are simply not in the coverage union.

`strv2_table` is the exception, and it is (d) on disc evidence rather than on
harness evidence. Retail's play loop does expand this table once per FMV, but
the port never selects the STRv2 arm: only slots 9 and 10 clear the Iki flag and
neither file is on the released disc, so no playthrough of a retail image can
reach it. The `strv2_decode` module states the same prerequisite chain.

### engine-render

| module | n | bucket | reach | addresses |
|---|---|---|---|---|
| `afterimage.rs` | 1 | (a) | battle-render | `801e1ab0` |
| `attach_swap.rs` | 1 | (c) | disclosed | `8004ccd4` |
| `battle_intro.rs` | 5 | (a) | battle-render | `80026988` `801cfda0` `801d0370` `801d1a20` `801d1cfc` |
| `gte/math.rs` | 1 | (c) | disclosed | `8004629c` |
| `lib.rs` | 5 | (a) | menu-render | `8002b994` `800349ec` `80034e4c` `80035ea8` `8003c1f8` |

The `lib.rs` rows are module-scope tags whose real bodies are the `engine-ui`
menu-ink and sprite builders the crate re-exports, so they close with the same
draw ladder as the `engine-ui` table above.

### save

| module | n | bucket | reach | addresses |
|---|---|---|---|---|
| `card.rs` | 1 | (d) | cli | `801e38d8` |
| `retail_inventory.rs` | 6 | (d) | cli | `800421d4` `80042310` `800423e0` `80042f4c` `80043048` `8004313c` |

Bulk-classified, on the module's own statement plus a caller scan that agrees
with it: nothing on the engine's frame path constructs a `RetailInventory`,
deliberately, because the gameplay inventory is `engine-core`'s typed item list
and swapping in a bug-compatible fixed window would be a regression. The
preservation host is `save-tool items`; the SC block checksum's is `save-tool`
and the memory-card round-trip oracle.

### engine-shell

| module | n | bucket | reach | addresses |
|---|---|---|---|---|
| `bin/legaia-engine/window/field_render.rs` | 1 | (a) | native-bin | `8001ada4` |
| `xa_clip.rs` | 1 | (a) | arts-swing | `8003d53c` |

The ocean-animation row is the worked example of the `bin/` exclusion above: it
has a disc-gated oracle of its own (`ocean_anim_real`), and no `#[test]` can
enter the module it is tagged in.

### prot

| module | n | bucket | reach | addresses |
|---|---|---|---|---|
| `cdname.rs` | 1 | (d) | cli | `8001d8fc` |

`retail_name_table` is deliberately the *lossy* CDNAME reader and must never sit
on a resolution path; its two consumers are the disc-gated parity oracle and
`prot-extract retail-names`.

## The actor VM has no host caller

`FUN_801D6628` is the actor / sprite VM - the first VM ported and the `Host`
trait shape every later VM port follows. Its interpreter is
`legaia_engine_vm::run`, and the six SCUS helper addresses tagged beside it on
the same module doc share its fate. No ladder executes any of the seven, and
neither does any host.

The chain is short enough to state completely. `World::run_actor_bytecode`
(`crates/engine-core/src/world/effects.rs`) is the only production wrapper. Its
only caller is `FieldDemoHandler::run` in `crates/engine-core/src/mode.rs`, and
that handler both synthesizes its own bytecode - a `SpawnDefault` per actor
followed by `End`, not disc bytes - and is constructed nowhere outside the
`#[cfg(test)]` module in the same file. `ModeHandler` has no registry: the trait
name appears in `mode.rs` and in no other file in the workspace.

So the permissive graph is right that an edge exists, and the edge leads to a
handler nothing installs. That is why this cluster is (c) and not (a): a ladder
would not reach it however far it walked.

[`vm-inventory.md`](../subsystems/vm-inventory.md#ported-but-inert) already
grades the actor VM inert, which is the right verdict; what it gets wrong is the
supporting detail, and the detail is the part someone wiring this would act on.
"Only its `Position` type is imported elsewhere; the interpreter itself has no
caller" understates the problem in one direction and overstates it in another -
there *is* a caller, and its existence is exactly what keeps the address out of
the disclosed-inert audit.

**The missing prerequisite is a bytecode source, not a call site.** Adding a
call from a live host would be a fake wire: nothing currently resolves a scene's
actor-VM programs out of the disc into a byte slice, so any new call site would
have to synthesize its operands the way `FieldDemoHandler` does, and the call
would then prove only that the interpreter runs on input the port invented. What
has to exist first is the per-scene program lookup - which carrier holds the
programs, and which field-VM or scene-entry event selects one. Until that
resolver exists there is nothing to feed [`run`](../subsystems/actor-vm.md).

No `NOT WIRED` token is added at the tag. The token's audit is a *static*
one, and this port is statically live through a real edge, so tagging it would
file seven fresh rows in the stale-`NOT WIRED` list -
[the shape that page calls FALSE-EDGE](stale-not-wired-triage.md) - and move the
finding into a queue that is about tag hygiene rather than about wiring. The
disclosure lives here and in the module doc instead.

## Ladder proposals

Ranked by how many of this page's (a) rows each would move. The counts are for
the non-`engine-core` slice only, so a shared ladder moves more than its row
here says.

| ladder | rows | what it drives |
|---|---|---|
| draw composition | 62 | compose a frame per tick from the browser play page's builders, which are library code |
| battle render | 32 | the encounter intro, gauges, party panel and assembled battle meshes |
| FMV | 17 | any `fmv_id`; export the coverage of the existing `av_decode_oracle` / `w5_fmv_handoff` |
| Baka Fighter hub | 15 | the PROT 0977 contest hub screen, not the duel the current ladder plays |
| audio | 9 | a mixer-attached tick, so SFX enqueue, VAB upload and voice alloc run |
| world-map panels | 8 | the panel-actor screens: sub-list, fill fade, text box, flag window |
| menu render | 6 | the pause-menu ink and sprite builders |
| world map | 6 | the overworld render pass: horizon, dim, CLUT fade, particle burst |
| dev menu | 4 | the host dev-menu hotkey and the records screen behind it |
| arts swing | 4 | an art that swings: shout bank, XA clip, ME archive, face stamps |
| battle target | 4 | the target picker's cycle and sweep-group arms |
| field actors | 4 | an effect that spawns a child actor through the allocator |
| field render | 3 | posed field characters: camera mover, pack apply, ANM decode |

The first row is the largest single win and the cheapest: the browser hosts'
composition is library code, so a `web-viewer` test can drive a pad ladder and
call `play_overlay_draws_json` per frame. The native window cannot be driven the
same way until its composition leaves `bin/`.

## Gates behind the (b) rows

| gate | rows | what has to happen |
|---|---|---|
| spirit-cast | 5 | a multi-cast / spirit-gauge cast reaching its cue group |
| slot-bonus | 5 | the casino slot machine's bonus round and its marquee |
| summon-cast | 3 | a summon cast, which streams its own side-band slots |
| quick-travel | 2 | a world-map quick-travel with at least one visited destination |
| battle-escape | 1 | fleeing a battle, so the escape countdown arms |
| ledge-hop | 1 | a field ledge with a hop arc |
