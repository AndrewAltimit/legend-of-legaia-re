# Lane G - `engine-core` share of the replay never-entered set

Slice: the 165 `crates/engine-core/` addresses in the *Live but never entered*
section of `scripts/ci/replay-port-coverage.py`'s union report. Per-address
verdicts are committed at `docs/tooling/reach-triage.md` (the delimited
`engine-core` section). This file carries what does not belong in a public doc:
the bucket arithmetic, the full `(c)` list with its evidence, the ranked ladder
proposals, and one request for the lanes that own the gates.

## Bucket counts

| bucket | n |
|---|---|
| (a) NO-LADDER | 104 |
| (b) GATED | 10 |
| (c) HOST-DEAD | 45 |
| (d) NOT-PLAYTHROUGH | 6 |
| **total** | **165** |

`(a)` splits 46 harness-blind / 58 content-not-driven. The harness-blind half is
the finding that reframes the whole number: those 46 are wired and reached in
real play, on hosts the instrument structurally cannot execute.

## The measurement's two blind spots

**A `--test` binary never runs the native window.** Exports are built
`cargo llvm-cov -p legaia-engine-shell --test <ladder>`, so the crate's `[[bin]]`
is compiled and instrumented but never executed. Measured directly in each of
the five exports: 31 files under `crates/engine-shell/src/bin/legaia-engine/`,
762 instrumented functions, **0 executed**. `web-viewer` is worse - it does not
appear in any export at all. Any `engine-core` port whose only caller lives in
either place is permanently never-entered, and no pad fixture changes that.

**The row names `sites[0]`, not the anchor that made the address live.** For a
multi-anchor address the report picks the first anchor found; 51 of the 165 have
more than one. `8002174c` names `apply_morph_weights` (inert) while the address
is `live` through `MorphWeightEnvelope::tick`. `800467e8` names
`remap_pad_direction` (inert, disclosed) while the address is `live` through the
`//! PORT:` block in `world.rs`. A tag on a plain data `struct` with no `impl`
is worse still: liveness falls back to module scope, coverage falls back to the
next function in the file, and neither symbol is the port.

## Every `(c)`, with evidence

Verdict method for all of them: symbol scan across `crates/**/*.rs` for
`(\.|::|\b)<sym>\s*\(` and for the bare identifier, then classify each hit as
production / `#[cfg(test)]`-or-`tests/` / doc-comment, including hits inside the
defining file (an in-file caller is a real caller and rules the row out). A row
is `(c)` only when the production count is zero or is the symbol's own `impl`
line. This is a Rust-side check; the disassembly reads below are separate and
are noted as such.

### Undisclosed - 21

| addresses | symbol / why nothing reaches it |
|---|---|
| `8003de7c` `8003e800` `8003e8a8` `8003eb98` `8003f128` `8005ea84` `8003dda0` | `cd_dma.rs`. `ProtCdDmaHost` is the only non-test implementor of `CdDmaHost` and of `overlay_loader::OverlayLoaderHost`, and it is constructed only in `#[cfg(test)]` and `tests/cd_dma_real_prot.rs`. No crate outside `engine-core` names `cd_dma`. `battle_stage_overlay_entry` likewise has zero production callers. |
| `800558fc` `80055a5c` `800559ec` `80055ac8` `8003e964` | `stream_file.rs`. `StreamFileHost`'s only production mention is its own `impl` line; every construction is a unit test or `tests/stream_file_real.rs`. |
| `80017978` `80025eec` `80025f2c` `80025f74` | `mode.rs`. `ModeDriver` is named by no code outside `mode.rs` - the two other mentions are `///` links in `world/state.rs` and `mode_trace_oracle.rs`. It is the only caller of `per_frame_stage`. `CARD_FRAME_BODY` is read only in that file's tests. `GameMode` / `TABLE` / `other_warp_init_stage` are used; the *driver* is not. |
| `80020038` | `sound_state.rs`. `DRAW_ENV_INIT` is read at three sites, all after that file's `#[cfg(test)]`. |
| `80020118` | `scene_bundle.rs`. `field_load_entry_plan` is called at three sites, all after `#[cfg(test)]`. |
| `801dc1cc` | `prize_exchange.rs`. `PrizeExchangeSession`'s only production mention is its own `impl`. Mechanism: `field_submode_screen::slot_for_op49_sub_op` collapses every non-dedicated op-`0x49` sub-op to `slot::CLOSE_TICK`, so no script can select sub-screen `0x20`. |
| `8001d7f8` | `scene_name_sync.rs`. `sync_scene_name` is called only from that file's tests; the `fn` anchor was disclosed, the module tag that carries liveness was not. |
| `801e3294` | `save_select.rs`. `card_frame_tick`, the only thing that advances a `CardIoMachine`, has zero production callers and carried the disclosure - on the function, not on the type anchor the address is keyed to. |

Three of these were checked on the disc side with
`scripts/ghidra-analysis/find-address-word-refs.py`, to separate "port of a
retail-live routine that no host reaches" from "port of dead code":

- `8003de7c` - **127** `jal` sites across `SCUS_942.54` and eleven overlay
  images (field, menu, battle-action, every minigame overlay, gameover).
- `800558fc` - 4 `jal`: `0x80052dfc`, `0x80052e70` in SCUS, `0x801f18f0`,
  `0x801f1928` in the battle-action overlay.
- `80025eec` - 12 literal-word hits in the mode table at `0x800707b4` onward,
  spaced `0x30` apart, i.e. every other 24-byte record - which matches the
  port's own claim of "12 of the 14 per-frame modes".

`80020038` scans to 2 `jal` (`0x8001dcac`, `0x8001dcb4`, the boot init) plus one
incidental unaligned word. All four scans are reads of the disc bytes, not of
decompiled C.

### Disclosed - 24

Same verdict, already stated in the source, so no disclosure work: `801e4f40`
`801dd12c` `801dd26c` `801d98f0` `801dae24` `801daef4` `801dafd4` `801dbc5c`
(`save_subscreen`, `SaveScreenMachine` test-only); `801e0598` `801e3d68`
`801e380c` `801e435c` (`card_bu_io`); `801d5d60` `801d6058` `801d27e0`
(`cutscene_script_elements`, no element-actor dispatch); `801db7f4` `801dbd94`
(`shop` retail sub-screens, test-only); `8002149c`; `801e13b8`; `801cfa48`;
`80024190`; `80021934`; `8002174c`; `800467e8`.

## What was done with them

**Fixed - four disclosures added**, each naming the specific missing
prerequisite rather than the generic gap:

| address | site | prerequisite named |
|---|---|---|
| `8003dda0` | `cd_dma.rs` `StreamLoadQueue` | a host that owns a `ProtCdDmaHost` across frames |
| `801dc1cc` | `prize_exchange.rs` module | a per-sub-screen route off the entry-context byte, since `slot_for_op49_sub_op` collapses to `CLOSE_TICK` |
| `8001d7f8` | `scene_name_sync.rs` module | repeats the `fn`-level disclosure onto the tag that carries liveness |
| `801e3294` | `save_select.rs` `CardIoMachine` | a host keeping the machine across frames; says why the disclosure on `card_frame_tick` did not cover this anchor |

Non-vacuity: these are disclosures, not wires - there is no call site to
disable, so the observable check is the audit's own verdict. All four resolve to
`live_strict = false` after the edit (verified by running `port-catalog.py`'s
`compute_live` against both graphs), so none of them appears in
`--live-audit`'s *tagged `NOT WIRED` but analysed live* section. `cargo test -p
legaia-engine-core --lib` passes (2647), `cargo fmt` clean, both doc gates green.

**Not fixed - 17 undisclosed rows, and the reason is a gate interaction, not
effort.** Their anchors are module-scoped or type-scoped and analysed *live*
under the receiver-gated graph, so a `NOT WIRED:` tag on them lands in the
stale-tag section as a false accusation - the exact failure mode
`docs/tooling/stale-not-wired-triage.md` exists to clean up, and adding 17 rows
to it would be a net loss. No wire was attempted for any of them: wiring
`ModeDriver`, `cd_dma` or `stream_file` into a host is a boot-path change well
outside a triage lane's scope.

## Request for the lanes that own the gates (E/F)

Two changes would let the 17 be disclosed truthfully. Either is sufficient.

1. **Per-anchor re-key.** Move each address's `PORT:` tag from the data
   `struct` / module header onto the function that implements it - e.g.
   `80025eec` onto `per_frame_stage` rather than onto `PerFrameStage`. That
   makes the anchor a `fn` anchor, its liveness the function's own (false here),
   and the disclosure silent in the stale-tag test. It also removes a class of
   spurious `live` verdicts across the whole catalog.
2. **Module-scope exemption in the stale-tag test.** Skip `kind == "module"` and
   `kind == "type"`-without-`impl` anchors when deciding staleness, on the
   grounds that neither one's liveness is a statement about the tagged address.

A third, smaller thing: the never-entered rows carry `sites[0]`. Emitting the
anchor whose liveness carried the address (or all of them) would have saved most
of the attribution work in this lane.

## Ladder proposals, ranked by addresses reached

Yield is over this lane's `engine-core` slice only; every proposal reaches rows
in other crates too.

| # | proposal | reaches | note |
|---|---|---|---|
| 1 | A coverage export that executes the **native window** - or move the bin's wiring down into the shared session layer | 46 | Not a ladder. Highest yield by a factor of three, and the only thing that touches the harness-blind half at all. The session-layer move is the better version: it also closes the browser side and gives all three hosts one model. |
| 2 | Field-VM **screen-effect / widget breadth** fixture: scenes whose scripts run op `0x43` widget sub-ops and op `0x4C` n6 arms | 15 | `screen_fx` 10, `clut_fx` 2, `text_balloon` 2, `register_ramp` 1. The ending scenes the `screen_fx` module doc names are the cheapest carriers. |
| 3 | **Minigame depth rungs** - play each minigame to its scoring screen instead of stopping at the round trip | 14 | `fishing` session kernels 6, `muscle_dome` 4, `baka_fighter` tally + intro 4. Extends `minigame_replay` rather than adding a ladder. |
| 4 | **Menu operate rungs** - use the rows the menu ladder currently only opens | 10 | Equip candidate hover + Best Equipment, Items Arrange, the three special Use routes, a shop stock list confirm. |
| 5 | **Battle depth** - an action that reaches its effect script, a built HUD row, round-start seeding | 6 | Extends `battle_depth_replay`. |
| 6 | A **BGM transition** rung - a scene change that pauses and resumes BGM | 4 | `scene/host` plumbing; the audio oracles already call these, the pad ladders do not. |
| 7 | Field-VM **host-arm** coverage (`0x4C` n-E sub-A, motion `start_motion`, take-item unequip) | 3 | Needs scripts that carry those ops; overlaps proposal 2's fixture work. |
| 8 | Promote the existing **`training_battle`** test to a ladder export | 2 | Cheapest item on the list - the test exists and drives `battle_tutorial`; only the export set changes. |
| 9 | A **world-map entry** rung through the real transition | 2 | |
| 10 | An **opening-prologue** ladder (`opdeene` / `opstati` / `opurud`) | 2 (+1 gated) | Also converts the `name_entry` GATED row, since the prologue is where the naming prompt lives. |

The eight `(b)` GATED rows are not on this list on purpose: they need seeded game
state (story flags, a capture-class boss, a status effect landed, a Seru spell at
its XP threshold), not more pad input.

## For the orchestrator

`docs/tooling/reach-triage.md` is not linked from `CLAUDE.md`'s tooling table.
That row was deliberately not added here: Lane H writes into the same page and
would add the same row, so a one-line edit to a shared file would collide on
cherry-pick. Add it once at integration, between the `stale-not-wired-triage`
and `call-target-integrity` rows.

The page is delimited by `<!-- BEGIN engine-core -->` / `<!-- END engine-core -->`
around this lane's section; everything above `BEGIN` is the shared header
(buckets and the two instrument shapes), which Lane H's section can rely on
rather than restate.
