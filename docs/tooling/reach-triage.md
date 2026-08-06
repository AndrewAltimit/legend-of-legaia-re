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

### A `--release` export cannot tell "never called" from "inlined"

`-C instrument-coverage` emits one counter per function. An optimised build
inlines the small ones and leaves the out-of-line record at zero, and nothing
downstream can distinguish that zero from a function no run entered. Measured
on one ladder over the same disc, default profile against `--release`:
`advance_slice` 40 executions against 0, `slice_word_count` 39 against 0, three
more at 1-3 against 0. Two of those five were on the never-entered worklist for
no other reason.

This lands on the two bucket kinds very differently, and blanket-caveating the
page would lose the distinction:

- an **(a)** or **(b)** row whose only evidence is "no run entered it" is a
  hypothesis when it was measured off a `--release` export - the routine may
  have run and been inlined out of its own record;
- a **(c)** row is unaffected. Its evidence is the source-side caller scan
  above, which no compiler profile participates in; the coverage number only
  ever corroborated it.

So: re-measure an (a)/(b) row on the default profile before spending a fixture
on it, and read a (c) row as it stands.

## What a pad-only ladder structurally cannot execute

The *headless* ladders drive `BootSession`, which constructs no renderer, no
audio device and no draw list; `crates/engine-shell/src/boot.rs` names neither
`engine-ui` nor `engine-render`. Under their union alone, four whole crates
report **zero** executed regions - which is a fact about the harness, not the
port, and it is what kept the largest NO-LADDER cluster on this page invisible
to the reach report.

The union now carries a rendering host: `play_compose_ladder`
(`crates/web-viewer/tests/`) drives the browser play page's `LegaiaRuntime` by
pad and composes the page's whole per-frame read surface, so the draw-list
builders execute under coverage. Per crate, what that converts and what it
structurally cannot:

| crate | files in the coverage data | executed, headless union only | executed, with the composition ladder |
|---|---|---|---|
| `engine-ui` | 42 | 0 | 22 |
| `engine-render` | 28 | 0 | 0 - a hard wgpu link `web-viewer` does not carry |
| `engine-audio` | 20 | 0 | 3 - the page's SFX channel; the mixer output path has no producer in the union (see below) |
| `mdec` | 6 | 0 | 0 - the play page has no STR playback (its FMV arm auto-skips) |

Three more structural exclusions matter as much and are easy to misread as port
gaps:

**No `#[test]` can *call* into a `bin/` target - but it can still cover one.**
The call half is a real exclusion: `crates/engine-shell/src/bin/legaia-engine/`
holds the native window's whole composition layer and no integration test links
against it, so nothing there can be invoked directly.

The coverage half of that claim was wrong and is corrected here, because it is
the half that put rows on this page. `LLVM_PROFILE_FILE` is **inherited by
child processes**: a test that spawns `CARGO_BIN_EXE_legaia-engine` gets the
child's own profile written and merged into the same export, measured at 40
executions of a function whose only driver was such a spawn. So a `bin/`-
resident address is reachable by a ladder that *runs the subcommand*, and a
`bin/` row on this page names a fixture that could exist rather than a
structural impossibility. The browser hosts never had either problem: their
composition is in `crates/web-viewer/src`, a library, which is the seam the
composition ladder drives.

**The browser minigames page is outside the union entirely.** `minigame_replay`
drives the *engine-shell* minigame path, and `play_compose_ladder` drives the
*play page* - neither is the standalone minigames page, so a port wired only on
that page is never-entered by construction. Its own oracles live in
`crates/web-viewer/tests/`.

**No ladder in the union holds a `BgmDirector`, so the whole BGM route is
unreachable from it.** This is sharper than "the ladders have no audio device",
and it is two separate exclusions stacked. `critical_path_replay`,
`minigame_replay` and `play_compose_ladder` drive `SceneHost` / `LegaiaRuntime`
directly and never call `SceneHost::route_bgm_events` at all; the `BootSession`
ladders, which do reach `boot.rs`'s call site, every one construct their session
with `enable_audio: false`, so `BootSession::bgm` is `None` and the call site is
skipped. Whether a cpal device could be opened never enters into it. Every
`0x35` sub-op arm - the SEQ-byte resolve, the pause/resume pair, the volume
re-apply, the swap-commit - is therefore never-entered by construction, and the
same is true one layer down: nothing in the union attaches a sequencer, so the
`engine-audio` mixing path has no producer either. The session ladders
`crates/engine-core/tests/w1e_scene_bgm_transition_ladder.rs` and
`crates/engine-audio/tests/w1e_audio_session_ladder.rs` supply that producer -
the latter through `legaia_engine_audio::TestAudioSink`, the device-free twin of
the cpal mixing core (see the crate README) - and both export as their own
ladder JSONs. They are session-shaped rather than pad-driven, which the union
should keep visible: they measure "code a mixer-attached frame loop executes",
not "code a player pressing buttons executes".


**An anchor is attributed to its first site, and a data anchor has no site.**
The report emits one row per address carrying `sites[0]`, so the file and symbol
it names may not be the anchor that made the address `live`, and for a
multi-anchor address it is simply the first one the source walk found.
`8002174c` is the worked example: the row names `apply_morph_weights`, which the
liveness pass calls inert, while the address is `live` through the sibling
`MorphWeightEnvelope::tick` in the same file. Separately, a tag on a plain data
`struct` with no `impl` falls back to *module* scope for liveness and to *the
next function in the file* for coverage - two different symbols, neither of them
the port. The `mode.rs`, `sound_state.rs` and `scene_bundle.rs` rows below were
that shape until their tags were re-keyed onto implementing functions;
`new_game.rs` still is.

**Three ways a tag ends up file-scoped, and only one of them looks like it.**
A `//!` tag is file-scoped by definition and reads that way. A `///` tag on a
data `struct` with no `impl` falls back, and reads like a type anchor. The
third is the quiet one: a `//` tag whose *next* item is outside the collector's
lookahead - it stops at the first line that is neither comment nor attribute,
and a `pub const` is not an item kind it recognises, so the tag silently
becomes file-scoped while looking like a function tag.
`crates/engine-core/src/cutscene_narration.rs` was that case for `80037174`
(tag at the foot of the module doc, first following line a `pub const`); it now
sits on `pub struct CutsceneNarration`, which has an `impl`, so the anchor is
the type the port actually is.

The same fallback also produces **pseudo-entries** - an address the report
counts as *entered* whose routine never ran, because a module-scope anchor's
entry verdict is "any region in the file executed". Two measured cases, both
surfaced the first time a coverage source contained their files:

- the seven `engine-vm/src/lib.rs` addresses (the actor VM and its SCUS
  helpers) read entered under the composition ladder while the only executed
  function in that file is `Position::new` - the exact one-type import
  [`vm-inventory.md`](../subsystems/vm-inventory.md#ported-but-inert) names.
  The interpreter's coverage record is unexecuted; the HOST-DEAD verdict below
  stands.
- `801d603c` (the casino prize confirm painter) reports as a **disclosed
  `NOT WIRED` anchor executed**, which would be a red-flag row - but its
  anchor resolution fell back to module scope (the tag sits above a
  60-line disclosure block, past the collector's item lookahead), and
  `choice_panel_draws_for`'s own record is unexecuted with no production
  caller in the workspace. The disclosure is correct; the report row is the
  fallback.


<!-- BEGIN engine-core -->

## `engine-core`

`engine-core` carries the largest crate share of the never-entered set. Every
address in it is accounted for below.

| bucket | addresses |
|---|---|
| NO-LADDER | 85 |
| GATED | 9 |
| HOST-DEAD | 45 |
| NOT-PLAYTHROUGH | 5 |

### The escape pair reads as a contradiction, and both halves are true

`801e791c` is filed NO-LADDER below while the `engine-vm` table further down
says the flee roll is "ladder-covered
(`crates/engine-core/tests/battle_flee_ladder.rs`)". Both statements hold, and
the join between them is the denominator: `CANONICAL_LADDERS` in
`replay-port-coverage.py` is five named test binaries, and neither
`battle_flee_ladder` nor `seru_cast_magic_xp_ladder` is one of them. Membership
is "drives the engine with `set_pad` and nothing else"; both of those build a
`World` by hand first, which is what puts them outside the union however
pad-driven the rest of the run is.

So "a pad ladder drives it" and "no run in the denominator entered it" are
different claims, and a row can satisfy the first while failing the second. For
such a row the fix is not a new fixture: it is promoting an existing seeded
oracle into the canonical set, or driving the same content from a ladder that
is already in it. Rows of that shape name their existing driver.

### HOST-DEAD, and what an anchor's scope was hiding

Every row here now carries a `NOT WIRED:` disclosure naming its own
prerequisite, and the way they got one is the reusable part.

Each was `live` for a reason that had nothing to do with the routine: its tag
was file-scoped, by one of the three routes above, and a file-wide verdict
answers "does anything in this file run", not "does this port run". Two
consequences, and the second is why the rows sat here:

- the address reads `live`, so `--live-audit`'s *undisclosed inert ports*
  section cannot see it, however dead it is;
- a truthful disclosure on it becomes **unwritable**, because the stale-tag
  test reads the same file-wide verdict and would report the disclosure as a
  false accusation.

The fix was a per-anchor re-key: each `PORT:` tag moved onto the function that
implements that address, with the disclosure on the same item. The data
descriptors the tags used to sit on keep a `REF:` pointing at the new home, so
the address is still findable from the shape it describes. Where no function
existed to key onto - `FUN_80020038`, whose port was a values-only `const` -
the missing half of the routine was written instead: `DrawEnvInit::stores`
carries the three *pair offsets* the values are stored at, which a value-only
descriptor drops.

| group | n | addresses | why |
|---|---|---|---|
| `cd_dma.rs` | 7 | `8003de7c` `8003e800` `8003e8a8` `8003eb98` `8003f128` `8005ea84` `8003dda0` | `ProtCdDmaHost` is constructed only inside `#[cfg(test)]` and the disc-gated `cd_dma_real_prot` test; no crate outside `engine-core` names `cd_dma`, and `overlay_loader`'s only non-test implementor is that same test-only host. |
| `stream_file.rs` | 5 | `800558fc` `80055a5c` `800559ec` `80055ac8` `8003e964` | `StreamFileHost` has exactly one production mention - its own `impl` line. Every construction is a unit test or the disc-gated `stream_file_real` oracle. |
| `mode.rs` | 4 | `80017978` `80025eec` `80025f2c` `80025f74` | `ModeDriver` - the port of the 28-entry game-mode state table, and the only caller of `per_frame_stage` - is named by no code outside `mode.rs`. `CARD_FRAME_BODY` is read only in that file's tests. |
| `sound_state.rs` | 1 | `80020038` | `DRAW_ENV_INIT` is read at three sites, all inside that file's `#[cfg(test)]` block. |
| `scene_bundle.rs` | 1 | `80020118` | `field_load_entry_plan` is called at three sites, all in that file's `#[cfg(test)]` block. |
| `prize_exchange.rs` | 1 | `801dc1cc` | `PrizeExchangeSession` has one production mention - its own `impl` line. |
| `scene_name_sync.rs` | 1 | `8001d7f8` | `sync_scene_name` is called only from that file's tests. Two anchors share the address - the `fn` and a `//! PORT:` module tag - and it is the module tag that carries the liveness verdict, so both need the disclosure. |
| `save_select.rs` | 1 | `801e3294` | `card_frame_tick` - the only thing that advances a `CardIoMachine` - is disclosed on the function *and* on the type anchor the address is keyed to; the function alone left the verdict on the type. |

Three of these name routines that are heavily used on the disc, so the gap is a
port that is not reached rather than a port of dead code. A five-form
[address-reference scan](address-reference-scan.md) puts `FUN_8003DE7C` at 127
`jal` sites spanning `SCUS_942.54` and eleven overlay images, `FUN_800558FC` at
four (two in SCUS, two in the battle-action overlay), and `FUN_80025EEC` in
twelve slots of the game-mode table at `0x8007078C` - every other entry, which
is the odd-indexed per-frame modes the port's own tag claims.

One rename fell out of the re-key pass and is worth knowing about, because it
is a graph property rather than a style choice. `StreamFileHost::seek` was the
workspace's **only** in-tree definition of that name, and the call graph's
receiver gate deliberately declines to resolve a one-definition name - so every
`File::seek` in every crate linked to the retail seek shim and made it
reachable from a host root in **both** graphs. It is `seek_bytes` now.

Six rows stay permissively `live` after the re-key for the mirror of that
reason - a name with *many* in-tree definitions, where the permissive graph
keeps every edge the gate would drop. They are `read` and `close` in
`stream_file.rs`, and the four handler addresses now keyed to
`mode::per_frame_stage`, reached through a `.tick(` collision on `ModeDriver`.
The receiver-gated graph calls all six inert, which is what the stale-tag test
reads, so their disclosures stand; what is left is an over-count in the
permissive `ported + live` figure, not a contested verdict.

The reading to resist is that a disclosure retires the row. It does the
opposite: it moves the address into `--live-audit`'s *disclosed inert ports*
list, which **is** the declared wiring worklist, and each disclosure names the
one prerequisite that would let a wire be real rather than synthesised. For
`cd_dma` and `stream_file` that prerequisite is the same shape twice - a
production owner of the trait / host type, which means routing the engine's
loaders through them instead of through `ProtIndex` whole-entry reads. For
`mode` it is a seat for `ModeDriver`, the port of the 28-entry mode table,
which the engine's hosts currently bypass entirely.

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

Wired, and reached in real play on a host the ladder harness cannot execute.
The composition ladder converted the rows whose host is the browser *play
page* (the shop panel kernels, the fishing prize rows, the demo tile board);
what remains here is reached only on the native window, on the standalone
browser minigame pages, or on the browser cards page - hosts still outside the
union.

| group | n | addresses | host |
|---|---|---|---|
| `dance.rs` (HUD + banner) | 7 | `801d231c` `801d3e28` `801d32f8` `801d2524` `801d2d98` `801d2f38` `801d387c` | native `window/hud.rs`, `window/minigames.rs`, `window/minigame_fx.rs` |
| `fishing_chrome.rs` | 6 | `801d03b0` `801d78c0` `801d74b0` `801d7a5c` `801d70ec` `801d7c30` | native `window/minigames.rs`, `window/hud.rs` |
| `fishing_actors.rs` | 4 | `801d2050` `801d765c` `801d2278` `801d4948` | native fishing block |
| `baka_fighter.rs` (digit strips) | 3 | `801d6a18` `801d6f44` `801d69e4` | native `window/hud.rs` |
| `save_select.rs` (card directory) | 3 | `801e1208` `801e3af0` `801e3ba0` | browser `web-viewer::cards` |
| `minigame_floor.rs` | 2 | `801d2a10` `801d6028` | native `window/minigames.rs` |
| `dance.rs` (sting + clip gate) | 2 | `801d3d78` `801d4098` | browser dance page |
| `pause_screens.rs` / `save_select.rs` (panel modes) | 2 | `801d6a54` `801e3f74` | both rendering hosts |
| `fishing.rs` (prize row remainder) | 1 | `801d092c` | the one prize-row kernel the play page's venue read does not fold |
| `shop.rs` (panel kernel remainder) | 1 | `801d5510` | native menu draw path |
| `baka_fighter.rs` (widget quad) | 1 | `801d5ed0` | browser `minigames_baka`, deliberately one-host |
| `dance_tutorial.rs` | 1 | `801d0750` | native tutorial run |
| `slot_machine.rs` | 1 | `801e6f70` | native casino entry |
| `dialog.rs` | 1 | `80038050` | native keyboard handler |
| `cutscene.rs` | 1 | `801cea3c` | native `run` subcommand |

### NO-LADDER, content not driven

Wired through `engine-core` and reachable by a headless ladder in principle -
these are the rows a new or deeper fixture would actually convert. (The
composition ladder converted the battle-HUD row bake, the dialog-atlas bake, a
`4C 60` stamp, a scripted CLUT-cell arm and the effect-script reader; the
rows below are what it did not reach.)

| group | n | addresses | what would reach it |
|---|---|---|---|
| `screen_fx.rs` | 10 | `801de4c8` `801f8d4c` `801f811c` `801f8004` `801f7a9c` `801f88fc` `801f8e6c` `801f849c` `801f8f28` `801f8a34` | a scene whose script spawns an iris mask, letterbox or image panel - the ending scenes the module doc names |
| `fishing.rs` (session kernels) | 6 | `801d5298` `801d0474` `801d0f5c` `801d26cc` `801d3db4` `801d746c` | a fishing rung past rung 4: rod select, a full cast, a landed catch |
| `muscle_dome.rs` | 4 | `801cf074` `801d1184` `801d1510` `801d9bbc` | a dome leg played to its between-leg tally |
| `baka_fighter*.rs` (tally + intro) | 4 | `801d6710` `801d239c` `801d2a28` `801d59d4` | a duel played through its intro card to the end-of-match tally |
| `equip_session.rs` / `menu_arrange.rs` / `menu_item_category.rs` | 4 | `801d9c14` `801cf760` `801d64a8` `801dd0c0` | operating the Equip and Items rows deeper than the menu ladders' browse-and-confirm |
| `pause_screens.rs` (special Use) | 4 | `801d7e50` `801d8a58` `801d8b90` `801d8d94` | a Use confirm on Door of Light / Door of Wind / Incense |
| `world/vm_hosts.rs` + `equipment.rs` | 2 | `8003c7ec` `800430ac` | field-VM scripts exercising those op arms |
| `battle_round.rs` / `other_game_overlay.rs` | 2 | `801db8b4` `801d14b0` | one call deeper into round start and the arena |
| `text_balloon.rs` | 2 | `8003c764` `801da7f0` | a scene running field-VM `4C E1` |
| `battle_tutorial.rs` | 2 | `801f6b70` `801f747c` | promoting the existing `training_battle` test to a ladder export |
| `shop.rs` (buy list) | 2 | `801db21c` `801db380` | opening a shop stock list and confirming a row through the *retail* menu-overlay list, not the engine session the play page drives |
| `world_map.rs` | 2 | `800196a4` `801d8258` | entering a kingdom overworld through its own transition |
| `cutscene_narration.rs` | 1 | `80037174` | an opening-prologue ladder (`opdeene` / `opstati` / `opurud`) |
| `register_ramp.rs` | 1 | `8003c6a4` | a script running field-VM op `0x43` sub-3..6 |
| `world/narration.rs` | 1 | `8003cf7c` | an inline field-VM conversation rather than the dialog panel |
| `world/battle/stats.rs` + `battle_formulas/escape.rs` | 1 | `801e791c` | a canonical ladder pressing **Run**. Wired at `world/battle/command_flow.rs`'s `Resolution::RunAway` arm and driven end to end by `battle_flee_ladder`, which is outside `CANONICAL_LADDERS` - promote it, or flee in the composition ladder's fight |
| `fade.rs` | 1 | `80020b00` | the same flee, one beat later: `FadeState::load` stages the state-`0x66` white-out from `fold_battle_event`'s `BattleEnd { Escaped }` arm, so it needs the escape to *succeed* and reach teardown |

Five rows left this table through the scene-session ladder
(`crates/engine-core/tests/w1e_scene_bgm_transition_ladder.rs`): the four BGM
plumbing addresses (`80019898` `800243f0` `800266e0` `80026520`) and the
scripted CLUT-cell cross-fade arm (`801e4c58`). The BGM four needed a
`BgmDirector` more than they needed a scene - see the structural exclusion
above - and driving the sub-ops the scenes' own MANs carry, in the order a
transition performs them, is what makes the pause/resume pair falsifiable
rather than four independent hook calls.

Driving them surfaced a wiring gap the reach number cannot show, because the
gap is on the far side of the trait. `BgmDirector::reattach_volume` has a
default no-op body, and **neither** rendering host overrides it -
`AudioBgmDirector` implements `pause` / `resume` / `stop` / `unhalt_pause` and
not this one, and the browser runtime's director matches. So sub-op 8 computes
retail's level (`FUN_80019898`'s `(raw << 15) >> 16`) and every host discards
it. The primitive it would drive already exists
(`legaia_engine_audio::Sequencer::set_master_vol`), so this is a two-host wire,
not a port.

Reachable only from a game state the ladders do not seed. The fix is a seeded
save or a longer spine, not a pad stream.

| group | n | addresses | gate |
|---|---|---|---|
| `field_actor_program.rs` | 2 | `801d4a60` `801d5a24` | the `MAN_LOAD_RESUME` story flags that arm the four voice-over programs |
| `world/battle/casting.rs` | 2 | `801dd4b0` `801dd6b4` | a capture-class boss cast. The gate now has a seeded oracle - `world/tests/battle_capture_class_disc.rs` folds Guilty Cross / Neo Star Slash off the real spell + move-power tables and pins the folded damage to each wrapper's own roll - but no pad ladder seeds a boss encounter, so the rows stay |
| `world/vm_hosts.rs` | 1 | `801d2d38` | system flag `0xD`, the three-actor talk lock |
| `world/battle/monster_ai.rs` | 1 | `801e7320` | a monster whose `field_flags & 0x380` is set |
| `world/field_movement.rs` | 1 | `801d2404` | a scene with a ledge-hop trigger |
| `world/battle/capture.rs` + `battle_formulas/victory.rs` | 1 | `801e70bc` | a party member **casting a Seru summon spell**. `accrue_summon_spell_xp` fires only under `is_party_summon_cast` (`world/battle/casting.rs`), and a new-game party knows no Seru magic, so no from-boot pad stream reaches it; `seru_cast_magic_xp_ladder` seeds the spell and is outside `CANONICAL_LADDERS` |
| `magic_xp.rs` | 1 | `801e92dc` | a battle that **captures a Seru**. `learn_spell_prepend` is the record-side commit of `seru_learning::record_capture`'s accepted learns, so the fight has to seat a monster carrying a `seru_id` and the capture roll has to take - one gate deeper than the summon-cast row above |

Three former rows of this table converted. The `town01` opening naming prompt
(`801f03f0`) left through the composition ladder, whose opening rung drives
the prompt to its commit instead of booting past it. `battle_status_clut.rs`
(`8004ce2c`) was gated on "a Stone landed on an actor", and no gameplay path
could land one: the impact-selector ladder carries only Venom / Toxic / Rot,
and Stone's applier is `FUN_800402F4`'s class-9 arm, reached only by the
streamed capture-class boss modules with the class as a code literal. The arm
is now ported and wired (`World::apply_enemy_agl_status`, roll kernel
`status_effects::agl_status_inflict_roll`),
`world/tests/battle_stone_gaze.rs` drives cast -> Stone -> `sync_status` ->
CLUT-row grey end to end, and the composition ladder's driven fight lands an
ailment stamp in play. `magic_xp.rs` (`801f452c`) is driven by the pad ladder
`seru_cast_magic_xp_ladder` - round prompt to spell submenu to the threshold
cross and the level-up banner.

### NOT-PLAYTHROUGH

| group | n | addresses | why |
|---|---|---|---|
| `dev_menu.rs` | 2 | `801dbd04` `801db8f4` | the overlay-0897 developer EVENT FLAG editor |
| `new_game.rs` | 1 | `8001ffa4` | `GAME_STATE_COLD_RESET` is a `const`, read in production by `scene/host/lifecycle` and `world/frame_tick` - wired, but a `const` has no coverage record, so no instrument can report it either way |
| `baka_cabinet.rs` | 1 | `801d553c` | the developer action-table dump retail writes as `ot5stat.txt` |
| `cutscene_script_elements.rs` | 1 | `801d841c` | reached only from the dev world-map panel's fade/flash actor |

The character-parameter editor (`801d6e18`) left this bucket: the play page's
dev-menu opt-in is itself pad-driven library code, so the composition ladder
walks its rows and the "not playthrough-shaped" reading no longer holds for
the browser host.

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
| `baka_hub_actors.rs` | 13 | (a) | baka-hub | `801f0adc` `801f1138` `801f16c0` `801f17d8` `801f1890` `801f1950` `801f1a1c` `801f1ab0` `801f1b64` `801f1d90` `801f1e48` `801f1fdc` `801f20b0` |
| `battle_action/overlay_rng.rs` | 1 | (c) | disclosed | `801d0290` |
| `battle_action/pool_ops.rs` | 3 | (a) | battle-target | `801d8a88` `801d8d00` `801db124` |
| `battle_burst.rs` | 1 | (c) | disclosed | `801f30c4` |
| `battle_cast_dispatch.rs` | 3 | (c) | disclosed | `801dba90` `801f1ed4` `801f2160` |
| `battle_formulas/stat_init.rs` | 1 | (a) | minigames-page | `80053cb8` |
| `battle_gauge_rearm.rs` | 1 | (a) | battle-render | `801f44a0` |
| `battle_helpers.rs` | 1 | (c) | disclosed | `80046870` |
| `battle_intro_particles.rs` | 2 | (a) | battle-render | `801cfbb4` `801d0164` |
| `battle_intro_styles.rs` | 1 | (a) | battle-render | `801d11d0` |
| `battle_intro_swirl.rs` | 2 | (a) | battle-render | `801d1564` `801d1888` |
| `battle_intro_transition.rs` | 1 | (a) | battle-render | `801cf1b0` |
| `battle_party_panel.rs` | 3 | (a) | battle-render | `801d84c0` `801dbb8c` `801dbc30` |
| `battle_stream_slot.rs` | 2 | (c) | disclosed | `80055b4c` `801f17f8` |
| `battle_target_group.rs` | 1 | (a) | battle-target | `801dceac` |
| `camera_mover.rs` | 1 | (a) | field-render | `801dd310` |
| `code_lock_actor.rs` | 1 | (c) | disclosed | `801eed58` |
| `dev_equip_commit.rs` | 1 | (a) | dev-menu | `801e5a08` |
| `effect_vm/pool.rs` | 1 | (a) | field-actors | `801de914` |
| `escape_timer.rs` | 1 | (b) | timed-flags scene | `801d2ebc` |
| `field_ledge_hop_arc.rs` | 1 | (b) | ledge-hop | `801d2298` |
| `field_party_cursor.rs` | 1 | (c) | disclosed | `801f1278` |
| `lib.rs` | 7 | (c) | **actor VM** (pseudo-entered - see the attribution note above) | `800319a8` `800326ac` `80035334` `800357fc` `80035978` `80035a4c` `801d6628` |
| `scus_battle_helpers.rs` | 2 | (c) | disclosed | `80046978` `80055854` |
| `scus_core_helpers.rs` | 5 | (c) | disclosed | `8001fa68` `800203ec` `80020424` `80020454` `800204a4` |
| `travel_art_actor.rs` | 2 | (b) | quick-travel | `801ee094` `801ee328` |
| `world_map.rs` | 1 | (a) | world-map | `801e3e00` |
| `world_map_clut_fade.rs` | 1 | (a) | world-map | `801e4d8c` |
| `world_map_dim.rs` | 1 | (a) | world-map | `801e75dc` |
| `world_map_horizon.rs` | 2 | (a) | world-map | `801c9688` `801d7ea0` |
| `world_map_overlay.rs` | 1 | (a) | dev-menu | `801e5b4c` |
| `world_map_panel.rs` | 3 | (a) | world-map-panel | `801e9b3c` `801e9dc8` `801ea9b0` |
| `world_map_panel_actors.rs` | 5 | (a) | world-map-panel | `801ed590` `801edf00` `801ee5d4` `801ee90c` `801ef014` |
| `world_map_particle_burst.rs` | 1 | (a) | world-map | `801e5338` |

The `disclosed` rows carry a `NOT WIRED` disclosure at their own tag, so they
are inert by the source's own account; they appear here only because the
permissive graph also calls them live. The `lib.rs` row was measured before
the widget-script resolver landed and is superseded - written up under
[The actor VM: a resolved bytecode source](#the-actor-vm-a-resolved-bytecode-source).

Four former "spirit-cast" / "summon-cast" rows left the table when the wiring
landed: the live loop's battle **Item** command now arms the action SM's
category-1 band instead of parking at `EndOfAction`, which puts
`battle_action/spirit.rs` (`801f3990`, the cast-audio cue),
`battle_cue_group.rs` (`801e22c8`, the cue-group expansion) and - through the
SummonFlute reroute - `battle_action/summon.rs` (`801f3c34`, the queued-magic
guard) on the playthrough path of all three hosts; the pad ladder is
`crates/engine-core/tests/battle_item_cast_band.rs`. The three
`battle_cast_dispatch.rs` addresses were re-bucketed on their own module's
account: the two dispatchers are disclosed `NOT WIRED` (they resolve to retail
VAs with no engine channel), and `801dba90` is a retail-dead entry point whose
instruction-identical twin (`FUN_801D8DE8` case `0x59`) is the wired one -
none of that is a "spirit-cast" gate.

`escape_timer.rs` is kept but its gate was misnamed: `FUN_801D2EBC` is the
field-VM `4C D3` scripted countdown (chitei2's collapsing-dungeon clock), not
the battle flee. Fleeing a battle runs the action SM's run band and the
`FUN_801E791C` roll, both ladder-covered
(`crates/engine-core/tests/battle_flee_ladder.rs`); what no ladder reaches is
a *scene* whose script arms the timed-flags countdown. The world-tick join is
oracle-covered (`escape_timer_world`, `w2_timed_flag_scheduler_chain`).

The world-map cluster splits three ways and the split is worth keeping: the
`dev-menu` rows sit behind a host hotkey a pad ladder cannot press, the
`world-map-panel` rows behind the panel-actor screens the spine ladder does not
open, and the plain `world-map` rows behind the overworld render pass.

### engine-ui

The crate used to be the largest one-reason cluster on this page: with no
rendering host in the union, every anchored builder read never-entered at
once. The composition ladder dissolved that bulk - the pause-menu screens,
field panels, window painters, name entry, dev-menu list, records screen and
the fishing HUD's live rows all execute under it now. What remains is no
longer one reason; each surviving row names the content or host still outside
the union.

| module | n | bucket | reach | addresses |
|---|---|---|---|---|
| `battle_intro.rs` | 5 | (a) | the intro styles + curtain trail the driven fights did not roll | `801cfda0` `801d0370` `801d1a20` `801d1cfc` `801d1d9c` |
| `other_game_hud.rs` | 4 | (a) | native / minigames-page muscle-dome HUD | `801d02f0` `801d050c` `801d08ec` `801d15c8` |
| `ui_fishing.rs` | 5 | (a) | catch / miss / strike event banners a short session does not land | `801d6f10` `801d71d4` `801d7528` `801d75dc` `801d78ec` |
| `ui_menu/pause_lists.rs` | 1 | (a) | the Items throw-confirm arm | `801d1b20` |
| `ui_menu/system_menus.rs` | 2 | (a) | the system confirm prompts | `801d1dac` `801d1f10` |
| `ui_menu/target_panel.rs` | 1 | (a) | the field item-use target panel | `801d0520` |
| `ui_menu_window_painters.rs` | 5 | (a) | specific retail windows no driven screen opens | `801d56fc` `801d61b0` `801d6360` `801dccb4` `801dcf14` |
| `ui_menu_window_painters_large.rs` | 3 | (a) | the stat-compare window chain | `801cf5d0` `801d1290` `801d4c28` |
| `ui_title_save/save_select.rs` | 1 | (a) | deeper Load beats | `801e3ff0` |
| `ui_title_save/slot_grid.rs` | 2 | (a) | the block-grid render beat | `801e06c0` `801e0fd0` |
| `ui_title_save/slot_info.rs` | 1 | (a) | the slot-info caption | `801e3ee0` |
| `gte/math.rs` | 1 | (c) | disclosed | `8004629c` |

The casino prize-exchange confirm window (`801d603c`) stays disclosed-inert;
the report now *misreports* it as an executed disclosure - see the
pseudo-entry note above for why that row is the anchor fallback and not a
finding.

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
| `seq_calc.rs` | 5 | (d) | differential | `80062f98` `8006320c` `8006352c` `80063aa8` `800649b0` |
| `seq_events.rs` | 5 | (d) | differential | `800638d8` `80063974` `800639a0` `80063cec` `8006418c` |
| `seq_slots.rs` | 1 | (a) | no owner | `8001ff58` |
| `shout.rs` | 1 | (a) | arts-swing | `8004c140` |

The ten `seq_calc` / `seq_events` addresses are the SsAPI per-frame calc tier.
`Sequencer` is the engine's clean-room replacement and drives playback on its
own clock, so nothing on the audio output path calls these kernels; their host
is the `note-trace` differential CLI, which is what makes a divergence localise
to one kernel. That is stated at their own tags, and it makes them (d) rather
than a wiring gap.

The footstep cadence and the SFX delay ring left this table through the
composition ladder - both tick on the browser play page's frame path, which is
exactly the host asymmetry the footstep module doc records against the native
window.

The seven `sequencer` / `sfx` / `vab_bind` rows left it through the audio
session ladder. What they needed was a mixer-attached tick - SFX enqueue, VAB
upload and voice allocation only run when something is pulling frames - and
`legaia_engine_audio::TestAudioSink` supplies one without a device by driving
the same mixing core the cpal callback drives. The ladder stages a real bank,
plays a real track, and matures a real cue through the frame scheduler, each
asserted on the PCM that came out rather than on the call being made.

`seq_slots.rs`'s `8001ff58` is the one that stays, and the reason is an
**owner**, not a tick. It is the SEQ resource-slot release keyed on the
12-byte-stride table at `0x80091508`, and `SeqResourceTable` is instantiated
nowhere in the workspace: the two hosts holding an open VAB
(`AudioBgmDirector::bank`; the browser runtime's `bgm_bank` / `sfx_vabs`) model
no slot table, and there is no `VabBank` close for the release to call. Both
halves are within reach - `SpuAllocator::free` exists and `VabBank::samples`
retains each `UploadedVag`'s `(addr, size)`, so a faithful `FUN_80068C80` has
its primitives - but wiring it is a three-host change, and until an owner
exists a release call would hand back resources nothing holds.

### asset

| module | n | bucket | reach | addresses |
|---|---|---|---|---|
| `boot_overlay.rs` | 4 | (d) | cli | `8001eef0` `8002574c` `80025ba0` `8003e360` |
| `character_pack.rs` | 1 | (a) | field-render | `8001ebec` |
| `face_anim.rs` | 1 | (a) | battle-render | `8004c7b4` |
| `minigame_slot_scene.rs` | 5 | (b) | slot-bonus | `801cec94` `801cfff0` `801d069c` `801d0fa8` `801d3230` |
| `save_icon.rs` | 1 | (a) | menu-render | `801e1934` |
| `summon_readef.rs` | 2 | (a) | battle-render | `801f12d0` `801f19ec` |

The `summon_readef` rows moved from the summon-cast gate to the render
ladder: a live Seru cast now stages its spawn request on the playthrough path
(`World::request_summon_spawn`, ladder `seru_cast_magic_xp_ladder`), and what
consumes the side-band slots is the render half - the native window's
`spawn_summon_creature` (`bin/`, structurally test-unreachable) and the
browser summon page (`web-viewer::summon_view`, outside the union). The parse
layer has its own disc oracle (`crates/asset/tests/summon_readef_real.rs`).

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

## The actor VM: a resolved bytecode source

`FUN_801D6628` is the actor / sprite VM - the first VM ported and the `Host`
trait shape every later VM port follows. Its interpreter is
`legaia_engine_vm::run`. This section previously graded the seven tagged
addresses "(c): no host reaches it" and pinned the reason precisely: **the
missing prerequisite was a bytecode source, not a call site** - nothing
resolved the VM's programs out of the disc, so any new call site could only
synthesize its operands, and a call proving the interpreter runs on invented
input would have been a fake wire.

The prerequisite is resolved. The programs are **data resident in the menu
overlay itself** (PROT 0899 - a program table in the overlay's data segment,
[`window-script.md`](../formats/window-script.md)), which also dissolves the
"per-scene lookup" framing: the carrier is per-boot overlay data, and what
selects a program is the menu code path, not a field-VM or scene-entry
event. The wired chain: `legaia_asset::widget_script` parses + scans the
programs, `World::install_menu_overlay_tables` (both hosts call it with the
real overlay bytes) resolves them, and `MenuRuntime::tick` feeds them into
`legaia_engine_vm::run` over the `engine-core::menu_widget` window-list host
on the shop picker entry / Sell transition edges - the transitions retail's
`FUN_801DAFD4` drives. Disc-gated pins:
`crates/engine-core/tests/menu_widget_scripts_real.rs`.

What stays true from the old verdict: `World::run_actor_bytecode`
(`crates/engine-core/src/world/effects.rs`), the field-actor host, is still
reached only from `FieldDemoHandler::run` in
`crates/engine-core/src/mode.rs` - a handler that synthesizes its bytecode
and is constructed nowhere outside the `#[cfg(test)]` module in the same
file. That edge is a demo, disclosed at both ends; the production route is
the menu-widget one above. A rerun of the replay reach report is what moves
the seven addresses out of the (c) table rows above, which record the
pre-resolver measurement.

## Ladder proposals

Ranked by how many of this page's (a) rows each would move. The counts are for
the non-`engine-core` slice only, so a shared ladder moves more than its row
here says.

The top proposal of this table's earlier state is **built**:
`crates/web-viewer/tests/play_compose_ladder.rs` is the draw-composition
ladder, a canonical member of the report's union, and it also swallowed most
of what the *battle render*, *menu render* and *dev menu* proposals covered -
its driven fights build the intro, the ground grid, the party HUD, the
assembled meshes and the attack camera, its menu rungs run the ink and sprite
builders, and its opt-in rung walks the dev list and records page. What each
remaining proposal would still move:

| ladder | rows | what it drives |
|---|---|---|
| FMV | 17 | any `fmv_id`; export the coverage of the existing `av_decode_oracle` / `w5_fmv_handoff` |
| Baka Fighter hub | 13 | the PROT 0977 contest hub screen, not the duel the current ladder plays |
| audio | 9 | a mixer-attached tick, so SFX enqueue, VAB upload and voice alloc run |
| world-map panels | 8 | the panel-actor screens: sub-list, fill fade, text box, flag window |
| world map | 5 | the overworld render pass: horizon, dim, CLUT fade, particle burst |
| field actors | 4 | an effect that spawns a child actor through the allocator |
| field render | 2 | posed field characters: camera mover, pack apply |

The native window's composition still cannot be driven this way until it
leaves `bin/`; the standalone browser minigames page and the `cards` page
remain outside the union and keep their harness-blind rows above.

### Battle render, battle target and arts swing are built

Three proposals left this table together, as two files:
`crates/web-viewer/tests/w1c_battle_render_ladder.rs` (the intro styles and
the attack-target ring) and `crates/engine-shell/tests/w1c_arts_swing_ladder.rs`
(the shout bank, the facial animator and the XA-clip census). Each needs its
own `cargo llvm-cov` export joined into the union, **without `--release`**.

Why the styles needed a ladder at all is worth keeping: the four non-default
transition styles are not a beat a player reaches, they are a *data* arm.
`select_intro_style` keys on the formation's first monster id, and the ids
that select the confetti / curtain / swirl belong to formations no scene the
composition ladder enters registers - so the driven fights all took the
default `TileShatter` arm and four ported style bodies never ran once.

Four addresses did **not** move, and each is a different shape:

| address | why it stayed |
|---|---|
| `801f44a0` | orphan: nothing in the workspace calls `DamagePopupRing::push` |
| `801d84c0` `801dbb8c` `801dbc30` | `battle_party_panel.rs` emits no coverage record at all - every item is an unused `const fn`, so the module is not in the binary |
| `801e1ab0` | content-gated: the streak needs a move-FX scene whose move-power record carries a non-zero trail texture page (`+0x0b`) |

The party-panel row is the sharper finding: `engine-ui` reproduces
`panel_anchors`' constants as its own `party_panel_stage_x` rather than
calling the port, and an `engine-shell` test pins the two equal - so the gate
passes, the numbers agree, and the ported kernel is dead. `DamagePopupRing` is
the same shape one level down: it models retail's **8-slot wrapping** ring
while the live HUD keeps an unbounded `Vec<DamagePopup>`, so retail's
"a ninth popup overwrites the first" is not reproduced.

Four `PORT` tags moved off module scope in the same pass, onto the routines
they name (`pick_channel`, `build_afterimage_quad`, `LabelState::opened`,
`cross_out_mark`, `panel_labels`). At module scope each would have resolved,
under the anchor fallback above, to an unrelated neighbouring function -
`ArtsShoutBank::new`, `streak_half_width`, `name_field_ptr` - so constructing
a bank would have read as "the arts-voice selector ran".

## Gates behind the (b) rows

| gate | rows | what has to happen |
|---|---|---|
| slot-bonus | 5 | the casino slot machine's bonus round and its marquee |
| quick-travel | 2 | a world-map quick-travel with at least one visited destination |
| timed-flags scene | 1 | a scene whose script arms the `4C D3` countdown (chitei2) |
| ledge-hop | 1 | a field ledge with a hop arc |

The former spirit-cast (5 rows) and summon-cast (3 rows) gates opened with
the item-band wiring and the summon-spawn ladder; the "battle-escape" gate
was a misnomer for the timed-flags scene countdown - see the engine-vm
section's conversion notes.
