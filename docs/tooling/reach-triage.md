# Replay-reach triage

[`replay-port-coverage.py`](../../scripts/ci/replay-port-coverage.py) joins the
five pad-only replay ladders' `cargo llvm-cov` exports against the port
catalog's `// PORT:` anchors and closes with a section titled *Live but never
entered*: addresses the static graph calls reachable that no ladder executed.
That section is the largest number the report produces and it arrives
unclassified, so it reads as one worklist when it is really four different
things stacked on top of each other.

This page is the per-address verdict, keyed so the follow-up work is mechanical
rather than re-derived. It is a snapshot of a worklist, not a specification - a
row leaves it when a ladder reaches the code, when a host wires it, or when the
disclosure lands. What outlives the rows is the bucket definition plus the two
instrument shapes below, both of which will keep producing rows.

## Buckets

- **NO-LADDER** - reachable in real play; no existing ladder drives that
  content. The fix is a fixture, not a code change.
- **GATED** - reachable only behind a story flag, scene or game state the
  ladders cannot currently reach. The fix is a seeded state.
- **HOST-DEAD** - nothing on any of the three hosts reaches it in play. This is
  the defect bucket, and the audit that would normally catch it cannot: these
  addresses are `live` in the static graph, so they never appear in
  `--live-audit`'s *undisclosed inert ports* section.
- **NOT-PLAYTHROUGH** - not playthrough-shaped: a dev tool, a CLI-only path, or
  a `const` data table that no coverage instrument can report either way.

## Two instrument shapes that keep producing rows

**A `--test` binary never executes the native window.** The exports are built
with `cargo llvm-cov -p legaia-engine-shell --test <ladder>`, and a test target
links the crate's *library*, not its `[[bin]]`. The bin is still compiled and
instrumented, so its files appear in the coverage data and the join treats them
as observable - but across every export, all 31 files under
`crates/engine-shell/src/bin/legaia-engine/` have **zero** executed functions.
Anything wired only from `window/hud.rs`, `window/minigames.rs` or a `commands/`
subcommand is therefore unreachable by this instrument no matter how far a
ladder gets, and no new pad fixture can change that. The browser hosts are
further out still: no `web-viewer` file appears in any export at all, because
that crate is not in the build.

The consequence for reading the report: *live but never entered* is not one
worklist. A row wired only into the native bin or the browser is a statement
about the harness; a row wired into `engine-core` and still unentered is a
statement about the ladders.

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
