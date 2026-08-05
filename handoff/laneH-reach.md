# Lane H - runtime reach triage, non-`engine-core` slice

Scope: the 243 *live but never entered* addresses of
`scripts/ci/replay-port-coverage.py` whose reported site is outside
`crates/engine-core/`. Lane G owns the other 165.

Public write-up: `docs/tooling/reach-triage.md` (per-crate sections; Lane G adds
`engine-core`). This file carries the bucket counts, every `(c)` in full, and
the ladder proposals ranked.

## Bucket counts

| crate | (a) no-ladder | (b) gated | (c) host-dead | (d) not-playthrough | total |
|---|---|---|---|---|---|
| engine-vm | 64 | 10 | 21 | 0 | 95 |
| engine-ui | 62 | 0 | 1 | 0 | 63 |
| engine-audio | 10 | 0 | 2 | 10 | 22 |
| asset | 11 | 7 | 0 | 4 | 22 |
| mdec | 17 | 0 | 0 | 1 | 18 |
| engine-render | 11 | 0 | 2 | 0 | 13 |
| save | 0 | 0 | 0 | 7 | 7 |
| engine-shell | 2 | 0 | 0 | 0 | 2 |
| prot | 0 | 0 | 0 | 1 | 1 |
| **total** | **177** | **17** | **26** | **23** | **243** |

## Method, and what it does not rest on

Per-address evidence came from three passes, all reproducible without a disc:

1. **Per-file execution from the five ladder exports.** Union of
   `target/cov-*.json`, collapsed to "did any region of this file execute". This
   is what turns four whole crates into one fact each.
2. **A strict caller scan of the workspace.** Comment lines dropped,
   `#[cfg(test)]` module bodies dropped, a reference counted only as `name(` or
   `name::`. Callers are tagged by layer: native `bin/`, `web-viewer/src`,
   `asset-viewer/src`, other library, test.
3. **Reading the module docs and the disassembly citations they carry**, for
   every row the first two passes could not settle.

**The MIPS address-reference scan was not run, deliberately.** That instrument
answers "does any reference to this address exist in the retail images", which
is a question about retail. Every `(c)` here is a claim about the *port* - does
a host reach the ported Rust symbol - so the strict caller scan is the matching
instrument. Where a retail-side claim is repeated below it is cited to the
module's own disassembly reference, not re-derived.

## The `(c)` rows in full

26 rows. 19 carry a `NOT WIRED` disclosure at their own tag and are listed
compactly; the remaining 7 are one cluster and are the finding.

### Undisclosed: the actor / sprite VM, 7 addresses

`crates/engine-vm/src/lib.rs` - `801d6628` `800319a8` `800326ac` `80035334`
`800357fc` `80035978` `80035a4c`.

Evidence chain, each link checked in the source:

- `legaia_engine_vm::run` is the interpreter. Its only production wrapper is
  `World::run_actor_bytecode` (`engine-core/src/world/effects.rs:175`).
- That wrapper's only caller is `FieldDemoHandler::run`
  (`engine-core/src/mode.rs:478`), which builds its operand bytes inline - a
  `SpawnDefault` per actor then `End` - rather than reading disc bytes.
- `FieldDemoHandler` is constructed at `mode.rs:1462` and `mode.rs:1480` only.
  `#[cfg(test)]` opens at `mode.rs:1031`, so both are test constructions.
- `ModeHandler` appears in `mode.rs` and in no other file in `crates/`. There is
  no handler registry, so no host installs it.
- `crates/engine-vm/src/lib.rs` reports zero executed regions across all five
  ladders, which is consistent rather than additional evidence.

**Declined to wire, with the reason.** A call from a live host would be a fake
wire: nothing resolves a scene's actor-VM programs into a byte slice, so the new
call site would have to synthesize operands exactly as `FieldDemoHandler` does,
and the wire would prove only that the interpreter runs on port-invented input.
The missing prerequisite is the **program source** - which carrier holds the
per-scene actor programs, and what selects one on scene entry.

**No `NOT WIRED` token added, and the reason is a gate interaction, not
squeamishness.** The token's audit is static, and this port *is* statically live
through a real edge, so tagging it would file seven fresh rows in
`--live-audit`'s "tagged `NOT WIRED` but analysed live" section - the FALSE-EDGE
shape - moving a wiring finding into a tag-hygiene queue. **This is the one
decision in the lane that another lane may want to reverse**; lanes E/F own that
gate and its baseline. If the tag is wanted, `docs/tooling/reach-triage.md`
§ *The actor VM has no host caller* is the text to lift, and
`docs/tooling/stale-not-wired-triage.md` needs seven FALSE-EDGE rows.

What was changed instead: a `## Wiring state` block in the `engine-vm` module
doc (phrased to avoid the marker regex - verified, `not_wired_tag` stays
`False` for all seven), and a correction to
`docs/subsystems/vm-inventory.md`, which claimed "the interpreter itself has no
caller". It has one; its existence is precisely what keeps the cluster out of
the disclosed-inert audit, so rounding it to zero hid the interesting part.

### Disclosed at their own tag, 19 addresses

Each is inert by its source's own account and appears in the never-entered set
only because the permissive graph also calls it live. No action taken; listed so
the count reconciles.

| module | addresses |
|---|---|
| `engine-vm/scus_core_helpers.rs` | `8001fa68` `800203ec` `80020424` `80020454` `800204a4` |
| `engine-vm/scus_battle_helpers.rs` | `80046978` `80055854` |
| `engine-vm/battle_stream_slot.rs` | `80055b4c` `801f17f8` |
| `engine-vm/battle_helpers.rs` | `80046870` |
| `engine-vm/battle_action/overlay_rng.rs` | `801d0290` |
| `engine-vm/battle_burst.rs` | `801f30c4` |
| `engine-vm/code_lock_actor.rs` | `801eed58` |
| `engine-vm/field_party_cursor.rs` | `801f1278` |
| `engine-render/attach_swap.rs` | `8004ccd4` |
| `engine-render/gte/math.rs` | `8004629c` |
| `engine-audio/anim_cue.rs` | `800508dc` |
| `engine-audio/footstep.rs` | `80018db0` |
| `engine-ui/ui_menu_window_painters.rs` | `801d603c` |

`801d603c` is the casino prize-exchange confirm window, and its tag is the most
actionable disclosure in the table: the missing prerequisite is the sub-screen
`0x20` host, spelled out as three concrete pieces - an extended-read reader for
the prize block at PROT 0899 file `0x15D00`, a pending-session slot on `World`
beside `pending_field_shop`, and routing for op-`0x49` sub-op `7`, which today
falls through `slot_for_op49_sub_op` to the generic close tick and is discarded.

`footstep.rs` (`80018db0`) is the only one of these that is a live host
asymmetry rather than a dead port: `FootstepCadence::tick_cadence` is called
from `web-viewer/src/play_sfx.rs:530` and from nothing in the native window. Its
module doc already says so and names the caller that would close it - the
native field-mode per-frame audio update. It is a candidate for a cheap wire in
a lane that owns the native window's audio path; it was left alone here because
the module also records that the speed input is a port-side convention a second
host has to choose deliberately, so the wire is a decision, not a line.

## Near-misses worth not re-deriving

**The window-25 / window-41 stat-compare chain is not an orphan cluster.**
`compare_panel_draws_for` and `party_compare_panel_fields` in
`engine-ui/src/ui_menu_window_painters_large.rs` have no caller outside their own
file, and their in-file call sites are at line numbers that look past the test
boundary. They are inside `recipient_picker_draws_for`, which both hosts call
from their shop recipient window. Only the window-25 half is orphaned and it is
already disclosed and waived.

**`FUN_80053CB8` being browser-only is not drift.** `init_party_battle_stats` is
called only from `web-viewer/src/minigames_muscle.rs`, but that is the
standalone minigames page synthesizing a fighter from a level because it has no
save; `MuscleFighter` is a `web-viewer`-local type. The native window seats the
real party instead.

**`engine-vm::battle_value_readout` looks host-dead to a module-path grep and is
not.** Both hosts import it aliased (`use ... as vr`), so `vr::value_cells(...)`
is the call. Any future scan of this shape needs to resolve aliases or it will
manufacture a finding.

## Ladder proposals, ranked

Counts are this slice only - a shared ladder moves Lane G rows too.

| rank | ladder | rows | note |
|---|---|---|---|
| 1 | draw composition | 62 | drive a pad ladder in `web-viewer` and compose a frame per tick; the browser hosts' composition is library code |
| 2 | battle render | 32 | encounter intro, gauges, party panel, assembled battle meshes; also 6 `asset` rows |
| 3 | FMV | 17 | cheapest of all: export coverage for the existing `av_decode_oracle` / `w5_fmv_handoff` / `str_player_segment` oracles |
| 4 | Baka Fighter hub | 15 | the PROT 0977 contest hub, not the duel `minigame_replay` already plays |
| 5 | audio | 9 | attach a mixer to the ladder tick so enqueue / VAB upload / voice alloc run |
| 6 | world-map panels | 8 | sub-list, fill fade, text box, flag window |
| 7 | menu render | 6 | folds into rank 1 |
| 8 | world map | 6 | overworld render pass |
| 9 | dev menu | 4 | needs a host hotkey a pad ladder cannot press |
| 10 | arts swing | 4 | shout bank, XA clip, ME archive, face stamps |
| 11 | battle target | 4 | target picker cycle + sweep-group aim |
| 12 | field actors | 4 | an effect that spawns a child actor |
| 13 | field render | 3 | posed field characters |

Ranks 1-3 together move 111 of the 177 `(a)` rows.

## Implications for tooling other lanes own

Not acted on here; `scripts/ci/` measurement files are lanes E/F.

1. **`replay-port-coverage.py`'s never-entered list mixes two populations.** An
   address in a `bin/` target and an address in a headless-unreachable crate can
   never be entered by a `--test` run, however far the ladders walk. Reporting
   them beside rows a longer ladder would reach makes the worklist read longer
   than it is. A `structurally-unobservable` split - "the anchor's file has no
   executed region in any source *and* no `#[test]` can enter it" - would be a
   truer denominator than the current `unobservable` rule, which only fires when
   the file is absent from the coverage data entirely.
2. **The union has no host-3 member.** The browser minigames page is a shipped
   host with its own oracles in `crates/web-viewer/tests/`, and nothing in the
   canonical ladder set drives it, so ports wired only there are never-entered
   by construction. `CANONICAL_LADDERS` naming a `web-viewer` member would fix
   that; today the report cannot distinguish "no host reaches it" from "the one
   host that reaches it is not measured".
3. **A `NOT WIRED` tag on a statically-live address has no home.** It is neither
   a disclosed-inert row nor a clean wiring row - it lands in the stale-tag
   queue. The actor-VM cluster is the worked case, and it is why that tag was
   withheld rather than added.
