# Open reverse-engineering threads

An index of reverse-engineering **questions** about Legaia's runtime that are
still live. Rows are questions, not progress markers: each says what is
settled, what remains, and what evidence would close it.

## What this page is for

Before starting a hunt, look for it here. If the question is not on this page,
it is probably already answered or already disproved - the two companion pages
below hold those, and checking them first is cheaper than re-deriving them.

| Page | Holds | Read it when |
|---|---|---|
| This page | Live hunts: `open`, `partial`, `mostly resolved` | You are picking up work, or want to know whether a question is still contested. |
| [`re-settled-threads.md`](re-settled-threads.md) | Answered questions, each carrying an evidence grade | You need the answer to something, or you are about to build on a claim and want to know how firmly it is pinned. |
| [`re-do-not-re-walk.md`](re-do-not-re-walk.md) | Falsified hypotheses, reasoning intact | A reading of the bytes looks obvious and you want to check nobody has already spent a week disproving it. |

A falsified row is kept forever, with its reasoning: "the world-map slot-4
bodies are coastline wireframes" is a very plausible reading of those bytes,
and knowing *why* it is wrong is worth more than the row it occupies.

Nothing on any of the three pages counts ports, tests, or coverage. Detailed
captures and decompiler dumps live in the linked docs and under
`ghidra/scripts/funcs/`.

## What an evidence grade means

Every settled row carries one of four grades, naming what its own stated
evidence actually rests on:

| Grade | The row cites |
|---|---|
| `disassembly` | Instructions, addresses, opcode encodings, branch or store sequences. The strongest grade. |
| `capture` | A runtime capture, save state, probe, firehose, or disc-derived oracle. |
| `decompiled-C` | Ghidra's C output, a `FUN_x(...)` call signature, a Ghidra label or plate comment, or a claim about store order / store count / a boolean operator with no instruction behind it. |
| `inference` | Reasoning from surrounding facts, corpus absence, or analogy, with no direct evidence cited. |

`decompiled-C` marks a claim **nobody has confirmed against instructions** - not
a claim known to be wrong. Most of them are probably right. But the C is a
rendering, and every claim falsified in the last audit wave would have graded
`decompiled-C`: dropped register arguments, `||` printed as nested `if`s,
reordered or omitted stores, and hand-written Ghidra annotations read as fact
have each already put a wrong statement on these pages. The catalogue of the
seven rendering artifacts is
[`ghidra.md` § decompiler artifacts](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims);
it is also the grading rubric. When a `decompiled-C` row is load-bearing for
something you are about to build, re-derive it from the disassembly first.

## Status conventions

| Status | Meaning |
|---|---|
| **open** | Active hunt. A concrete next step exists; the row names it. |
| **partial** | The main result is pinned; a residual sub-question remains. |
| **mostly resolved** | The mechanism is pinned; one leg is unconfirmed. |

Many rows qualify the status in parentheses - `partial (transcode closed)`,
`open (narrowed)` - naming *how far* it got. Read the parenthetical.

## How a thread is laid out

Each area below opens with a table of one-line rows. A thread whose write-up
outgrows a table cell keeps its one-liner in the table and links to a `###`
section immediately after that table via **[details ↓]**; the full
analysis - every address, capture, and falsification - lives in that section,
under its own *Status:* line.

## Recently corrected

Rows the last audit wave overturned. They are listed here rather than filed
silently into the settled page, because a claim that was wrong once is the
cheapest place to look for a claim that is still wrong.

- **Debug flag `_DAT_8007B8C2` had its branch sense backwards**, and one arm
  was named for the wrong loader. Every site reads the flag with `lh` and takes
  the **zero** arm to the debug-station host trap, the **non-zero** arm to the
  index resolver. The flag is re-opened on that polarity - see its row below.
- **The VA-aliasing corollary was too narrow.** It read as an `0x801Fxxxx`
  problem; `801e23ec` is a settled casualty in the `0x801E` band, and its
  aliased reading had silently dropped all three initiative modifier terms.
- **The op-`0x2F` "seven byte-identical dumps" shorthand was compressing.** The
  capture-derived dumps agree with each other; the static 0897 dump is a strict
  *subset* of them, not a twin.
- **`FUN_8001EBEC`'s second pose-copy arm loads seven words**, so its range ends
  at `+0x15C`, not `+0x158`.

Two rows a prior audit flagged as the highest-risk `decompiled-C` claims on the
register - the narration-roller op's operand decode and the item-add OOB
store-order claim - have both now been **re-derived from the disassembly and
confirmed** (grade `disassembly`). The store orders and operand shapes stand as
written; the instruction evidence is cited on each row below.

---

## World map / kingdom bundles

| Thread | Status | What would close it |
|---|---|---|
| Kingdom slot 4 - per-record semantic | partial (transcode closed - read in place; no actionable next step) | [details ↓](#kingdom-slot-4---per-record-semantic) |

### Kingdom slot 4 - per-record semantic

*Status:* **consumer pinned - slot-4 is read in place, no transcode** (Drake capture); residual = the per-record field semantic

The **consumer is fully decoded** ([`world-map-overlay.md`](../formats/world-map-overlay.md#cluster-a-internals)): `FUN_80043390` walks an 8-byte-header **command stream** (`kind` = bits 17–31, `count` = bits 0–15), tail-calling per-`kind` GTE primitive emitters (kinds 8–19 across 4 banks via the `0x8007657C` table; each reads two packed vertex indices per word `& 0x7FF8` into a vertex pool and emits a `POLY_F3/G3/G4/GT3/GT4` GP0 packet - dispatcher + the kind-12 flat-triangle handler spot-verified against `ghidra/scripts/funcs/{80043390,slot4_k12_bank0_80043658}.txt`).

**The handlers read the slot-4 RAM payload in place - there is no transcode.** A Drake warp capture (`scripts/pcsx-redux/autorun_slot4_source_map.lua`; 365 rows) shows 363 reads of the slot-4 window with the cluster-A GTE prim path (`0x80044C70 = lw …,0x10(a1); … andi …,0x7FF8`, the exact packed-vertex-index extraction) holding slot-4 pointers in `a1`/`a2` (`0x8011A608`, `0x80121614`, …), under return addresses `0x801F78D4` (the world-map top-view overlay renderer, 276 reads) and `0x8001BC8C` (SCUS render, 78). The streaming-chunk processor `FUN_8001E54C` fired only twice and on a non-slot-4 buffer (`0x80184BD0`). So the earlier "`FUN_8001E54C` distributes the slot-4 records into a working buffer the handlers walk" reading is **falsified**:
the slot-4 sub-body payloads *are* the command stream + vertex pool, walked directly. (The working-buffer writers the prior hunt saw - `FUN_80028158` at `0x801BA000` - are unrelated procedural meshes, as that hunt already found.)

**Cross-kingdom: confirmed.** The slot-4 resident base is byte-pinned for all three kingdoms (Drake `0x8011A624`, Sebucus `0x80119CE4`, Karisto `0x80108D84` - it varies per kingdom; `locate_slot4_base.py` matches the disc payload against a post-warp RAM dump, all bodies unanimous). Re-read against the correct Sebucus base, 171/177 of the Sebucus `slot4_source_map` reads land inside the verified window - in-place there too.

**Per-record semantic - decoded.** Each 8-byte record is a **GTE vertex**: the per-kind handler `FUN_80044c14` loads a record's two words into the GTE vertex registers (`VXYn = x | y<<16`, `VZn = z`) and `RTPT`-transforms them, so `x/y/z` are model-space coordinates (the parser's field layout is confirmed) and `attr` (the `VZn` word's high half) is **not** a coordinate. Each body is an object-local vertex pool; the triangle topology lives in a separate cluster-A command stream that indexes the pool by byte offset (`& 0x7ff8`). The transcode question is closed (there is none - the pool is read in place).

**`kind` + `attr` - characterized (consumer is the open tail).** `kind` (1/2/4) tags a body's class/scope: hashing bodies across kingdoms shows `kind 1` = the three leading bodies, **byte-identical across all three kingdoms** (a shared universal mesh set); `kind 2` = full-3D kingdom objects (one cluster also globally shared, others shared between kingdom pairs); `kind 4` ⟺ `flag_a = 1` (widest-extent meshes). So slot 4 is a per-kingdom assembly from a shared mesh library + kingdom-specific bodies. `attr` is genuinely per-vertex (not per-group), **not** position-correlated (`corr ≈ 0.1`), varies smoothly across groups, and rides the unused `VZn` high half.

**Consumer sweep - no reader (was "read by some non-render path"):** widening the search beyond the render family, `attr` is read by **nothing** in the dumped corpus. The pool base flows only to the cluster-A GTE renderers; all 43 `>> 0x10` sites in that family extract a *command*-word vertex index, and each record's `z|attr` word is loaded whole into GTE `VZn` (high half never masked); `grep puVar[1]>>0x10` = zero hits. So `attr` is **render-unused reserved per-vertex data**, not a live non-render input. (Dump note: `ghidra/scripts/funcs/80059de4.txt` is mislabeled - its entry is `FUN_80059BD4`, a VRAM `LoadImage` DMA, not a slot-4 reader.)
**`kind`/`count` consumer - pinned.** A Read-watchpoint on body 0's header during the Drake warp catches the cluster-A handler chain reading it **in place**: `ra = 0x801F78D4` (the world-map renderer), PC `0x8004568C`/`0x800456F4` (`FUN_80045584`), record pointers also in slot-4. The handler reads `count`/`kind` and `andi 0x40`-tests a header bit. So there is **no separate command-stream builder** - each slot-4 body is a self-contained render packet (header + indexed vertex records) walked in place (the `FUN_8001ada4` → `FUN_80058490` candidate was falsified: `FUN_80058490` is a libgpu `MoveImage`). **`attr`** is render-unused - a full sweep of the cluster-A handler family (`FUN_80043658`..`FUN_80045988`) confirms every `>> 0x10` is a vertex-index extraction or output-packet write,
none reading the pool `word1` high half. So `attr` (real per-vertex data) is ignored by the entire world-map render path - reserved/authoring data or a non-render-subsystem consumer; nothing in the render family reads it.

## Battle / arts / level-up

| Thread | Status | What would close it |
|---|---|---|
| Xain "Bloody Horns"/"Terio Punch" ignore elemental guards (community mystery) | resolved (grade `disassembly`+`capture`) | Not an element drop - a **resist-ladder bypass**. Capture-class casts (spell byte `+0` = `'c'`) run per-spell modules (PROT 944..966) whose damage calls pass the caster's seat but pick one of two wrappers: `FUN_801DD4B0` (finisher `param_5=0`, resist ladder runs) or `FUN_801DD6B4` (`param_5=1`, the whole party-defender jewel/guard block is skipped). BH (952) / TP (953) use the bypass wrapper for their main hits; enemy ESM (966) uses the respecting one (hence Cort reads as Dark). Element attribution law + live confirmation: [battle-formulas.md](../subsystems/battle-formulas.md); cast classes: [spell-table.md](../formats/spell-table.md#cast-classes-record-byte-0). |
| Endless camera orbit after a battle action (community-reported on Gaza 2 magic) | mostly resolved (grade `disassembly`+`capture`); retail trigger open, candidates narrowed | The park is state `0x51`; the orbit is only the idle azimuth sweep. Both first-pass generators are measured out on the Gaza 2 fight itself ([re-do-not-re-walk.md](re-do-not-re-walk.md#battle--arts--level-up)). [details ↓](#endless-orbit---what-remains-open) |
| Super / Miracle Arts trigger logic | resolved (grade `disassembly`+`capture`) | The full chain is pinned (preseed `FUN_801DA34C`, queue-builder `FUN_801EED1C`, Super applier `FUN_801EF9E4`) and all 15 Supers are **live-executed**: an applier-entry injection probe (`autorun_super_art_queue_inject.lua`) drives the retail applier over each `find` string and reads the tail-replace at `actor[+0x1DF]` back byte-exact, 15/15; per-character library states re-checked by `crates/pcsxr/tests/super_art_queue_replace.rs`. Full chain + derived pins: [`re-settled-threads.md`](re-settled-threads.md#super--miracle-arts-trigger-chain) and [battle-action.md](../subsystems/battle-action.md#the-retail-queue-builder-fun_801eed1c-and-super-applier-fun_801ef9e4). |
| First boss trigger -> Battle | resolved | The scripted-battle arm is the field-VM op `3E FF <formation_row>` ([battle.md](../subsystems/battle.md#scripted-battle-entry-3e-ff-row)): Zeto = garmel `P2[12]` row 9 (lone `0x4B`), Caruban = rikuroa stager `P1[3]` row 17 (lone `0x49`, `World::run_boss_stager_record`). `DAT_8007b7fc` closed: writer-less across `SCUS_942.54` + every static overlay (validated absolute + gp-relative + address-materialisation sweep); readers pin it as the debug forced-battle formation id - battle init `FUN_80055b6c` -> `FUN_8005567c` seeds the formation cells `DAT_8007BD0C+` from it, and `FUN_80046A20` routes a nonzero value to its mode-0 debug-menu exit. Retail never sets it. See [battle.md](../subsystems/battle.md). |
| Enemy-ally charm battle softlock | resolved (both tracks fixed; grade `disassembly`) | The state-`0x5A` victory arm's party-slot assumption OOB-indexes the win-pose roster `DAT_8007BD10` (via `0x801E6770`) when a living charmed ally is the acting actor at monster-wipe victory - the `FUN_801E7320` reroll theory is falsified ([`re-do-not-re-walk.md`](re-do-not-re-walk.md#battle--arts--level-up)). Fixed on both tracks: engine `victory_pose_fixup`/`charm_widen`, and the disc-side `legaia_patcher::charm_fix` guard - a single-word detour at the `0x801E6690` keep-branch into a SCUS dead-space liveness guard. Full chain + port: [battle.md](../subsystems/battle.md#enemy-ally-charm-at-the-end-of-action-gate-the-charm-battle-softlock). |
| Action-SM state `0xFF` treated as battle end by the port | open (retail half graded `disassembly`) | Retail `0xFF` is the **round boundary**: its only writer is the non-wipe arm of `0x5A`, and wipes signal through `DAT_8007BD71 = 0xFE` without writing a state byte ([battle-action.md](../subsystems/battle-action.md#0xff-is-the-round-boundary-not-the-battles-end)). The port maps it to `ActionState::BattleComplete` → `battle_end(MonsterWipe)` → `finish_battle`. Close it by settling whether a live battle reaches that path: it turns on how `action_queue_counter` accumulates, given `dispatch.rs` re-stamps it from `ctx.queued_action`. If it does, the symptom is a spurious victory after one round. Drive a battle past a full round with both sides alive. |
| Battle-actor `+0x16E` bit `0x400` applier (guard-disabling status) | resolved - exhaustive negative (grade `disassembly`) | Bit `0x400` has **no retail setter**: a word-level decode of `SCUS_942.54` + every static-overlay image (all stores covering `+0x16C..+0x171`, pointer precomputes, `ori`/`sllv` bit-set shapes, the `+0x6F6` mirror, the `+0x21F` deferral) finds only clears - accessory cure `FUN_8004CE2C`, the per-round RNG waker `FUN_801F45A4`, item cures, the on-hit strip, battle-exit. The appliers (hit leg `FUN_801EC3E4`, cast leg `FUN_801E09F8`) map kinds 3/4/5/6 → `0x1`/`0x2`/random-`0x38`/`0x1000`, kinds 1-2 → the `0x380` deferral; none reaches `0x400`. Latent content. Writer inventory: [battle.md](../subsystems/battle.md#the-0x16e-status-halfword---retail-writer-inventory). |

### Endless orbit - what remains open

The orbit is only the unconditional idle azimuth sweep (`FUN_801D0748`
stepping `_DAT_8007B792`); behind it sit **two distinct park classes**, and
the one caught from ordinary play is not the one this thread started on:

- **State `0x19` (attack approach) - caught live, no interventions.** A
  human playing the Gaza 2 fight at dynarec speed under the poll-only
  `autorun_gaza2_park_hunter.lua` hit it and savestated the frozen moment
  (scenario `battle_gaza2_park_0x19`). The boss's physical attack reached
  `0x19` still ~556 units from its target with the walk phase never having
  engaged; `0x19` has no movement code and its not-in-range path only bumps
  the stall counter `ctx[+0x6D4]`, so it re-polls `FUN_8004E2F0` forever.
  Full anatomy:
  [battle-action.md](../subsystems/battle-action.md#the-0x19-attack-approach-park---a-second-distinct-softlock-class).
  Open: why `0x14`/`0x16` handed off without walking (the `0x0C -> 0x14 ->
  0x19` transition took ~3 vsyncs), and what built the wedged round queue
  (all four combatants simultaneously parked in approach states, two of
  them holding target `8`, which the range check rejects by construction).
- **State `0x51` (HP-readout settle)** - mechanism fully decoded and
  reproducible by injection
  ([battle-action.md](../subsystems/battle-action.md#the-0x51-exit-gate-and-the-hp-bar-settle-invariant)),
  generators measured out on this fight; still unobserved from retail-only
  play. The community live-park exhibit (JP screenshot) is now more likely
  a `0x19`-class park - its healthy-looking readout needs no HP desync
  under that reading.

What fell: the clamp asymmetry only amplifies a pre-existing offset, and a
three-capture Lost-Grail campaign (twelve retail revives, zero harness HP
writes) found every `FUN_800402F4` assign landing on an already-drained
accumulator - both entries with reasoning in
[re-do-not-re-walk.md](re-do-not-re-walk.md#battle--arts--level-up).

What would close the thread now:

- ~~audit the capture-class module HP stores~~ - **done, negative**. The
  campaign's party-wide double-kill casts credited live HP and the
  accumulator through none of the armed census writers, which briefly made
  the capture-class per-spell modules (PROT 944..966) the prime suspect. A
  static store audit of the whole family
  (`scripts/asset-investigation/audit_module_hp_stores.py`) finds every
  actor-accumulator store to be the **paired** accumulate + live-HP shape -
  module-local copies of the safe applier (e.g. 0944 `+0x808`/`+0x824`;
  the remaining `+0x10` matches are handle-table and fade-struct false
  positives) - so the "unarmed" seeding was a census blind spot, not an
  unpaired writer, and the module family follows the safe convention;
- the **drain-vs-tail race**: minimum last-credit-to-`0x51` gap on Gaza 2 is
  ~27 rendered frames vs a 23-frame drain from a ~1300 readout (30 from
  `9999`; +1 per doubling) - Gaza 2 misses by ~4 frames; a higher readout or
  faster-tailed move crosses
  ([battle-action.md](../subsystems/battle-action.md#where-the-desync-comes-from-two-seeding-conventions)
  has the numbers). The community live-park exhibit (JP version, reported on
  both regions) draws a healthy `1476/1476` target panel - measured: the HUD
  draws `+0x172` raw (an injected overshoot renders `1439/1289` on screen),
  so the desynced slot in the exhibit is off-panel and the parked action is
  the all-target arm (`+0x1DD == 8`), i.e. a party-wide cast;
- a retail path through the `FUN_801EC3E4` commit-skip guards
  (`0x801EE988` / `0x801EE9AC` / `0x801EE9EC`) with a non-zero credit
  already applied - credit-without-commit is the one shape that leaves a
  settled desync;
- or evidence that the community orbit reports trace to a different
  absorbing state than this park.

## Field / locomotion

| Thread | Status | What would close it |
|---|---|---|
| What transitions retail into game over? | resolved | Retail has **no** mode-`0x12` transition. A party wipe exits battle to mode 2; MAIN INIT `FUN_8003AEB0`'s back-from-battle arm (gated on the `DAT_8007BD60 & 0x80` survivor latch **and** story-flag idx 0 = the scripted-loss latch, raised by field-VM op `4C EA`) stores `game_mode = 0x16` (CARD INIT) with `_DAT_8007BB00 = 1` at `0x8003B5D4`, landing on the **title screen with CONTINUE preselected** - no GAME OVER art, no dedicated menu. Every store PC captured live (`autorun_gameover_mode_writer.lua`). Mode 18/19 + PROT 0902 confirmed an unreachable dev harness. The port's three-row session stays an engine invention. [details](../subsystems/battle.md#party-wipe--the-game-over-overlay) |
| Region story-flag gate families (record-header C1/C2 gates) | partial - structure mapped across the chapter-2/3 regions; play order for the dungeons the capture corpus never walked is still owed | [details ↓](#region-story-flag-gate-families) |
| Mid-visit NPC re-arrangement beats (dolk2 market crowd; garmel pre-Zeto staging) | resolved (grade `disassembly`+`capture`) | dolk2: the swap is `P2[11]`, spawned by the `.MAP` fallback walk-on-trigger rows (C1=[`0x27C`], C2=[`0x142`]) - eight `CC <crowd> E3 <day>` seats (op `4C` nE sub-3, `0x801E3108`) put P1[53..60] on the day cohort's tiles and `A3` parks the day cohort at `(127,127)`. garmel: the Zeto stager `P2[12]` materializes P1[3]/P1[4] beside the player (n3 sub-7 player-coord copy `0x801E0FB0`); post-battle re-entries run `P1[0]`'s flag-consume arms. See [script-vm.md](../subsystems/script-vm.md#mid-visit-npc-re-arrangement-beats-dolk2-market-swap--garmel-boss-staging); pinned by `engine-core/tests/man_midvisit_rearrangement_disc.rs`. |
| Extraction-0874 §2 (`player.lzs`) F-variant pixels | resolved - installing event named (grade `capture`+`disassembly`) | The variant is a one-shot face-frame stamp from the town01 opening record, not a scroll phase and not a menu writer. [details ↓](#extraction-0874-2-playerlzs-f-variant-pixels---a-one-shot-opening-face-frame-stamp-not-a-menu-writer) |

### Region story-flag gate families

*Status:* structure resolved; a residual play-order question remains for the dungeons the capture corpus never walked.

Every field scene's MAN carries one **partition-2 record** per cutscene or story beat, and each record's *header* holds two flag lists that the spawn evaluator `FUN_8003BDE0` checks before running it: a **C1** one-shot list (the record is suppressed once any listed flag is set) and a **C2** requires-all list (the record spawns only when every listed flag is set). Regional progression is expressed almost entirely through these header gates.

Because they live in the record header rather than as inline `0x50`/`0x60`/`0x70` opcodes, the inline flag census (`man-scripts --system-flag-census`) cannot see them — the recurring cause of several "write-only flag" false alarms. `legaia_engine_core::man_field_scripts::partition2_record_gates` decodes them, and the census-file anchor tests named below pin each region's exact lists.

Two reader-only flags first exposed the pattern. `0x1BE` (Jeremi's arrival at `geremi`) is a self-latch: `geremi P2[0]` both sets it and lists it as its own C1 gate (anchor `geremi_p2_0_is_the_0x1be_self_latch`). `549`/`0x225` (the Rim Elm opening) is read the same way across the Rim Elm variants and turned out to be the same self-latch shape once the `4C 0xE_` op widths were fixed — see its row above.

**Chapter 2 — Sebucus (`map02` and its dungeon spokes).** The progression spine needs no chapter-specific engine code: each beat's script latches its flag through the ordinary field-VM `SysFlag.Set` path, so the generic seeder drives the whole arc. The chain runs `teien` (`0x1C8` → `0x1C9` → `0x332`) into `tower` (`0x1C7`, gated on the teien arc) into a post-tower `geremi` beat, with `balden` self-latching `0x5B3` and `map02 P2[9]` mirroring the teien arc onto the overworld. Proven by `chapter2_sebucus_spine_oracle`, `chapter2_sebucus_gate_spine`, and `chapter2_sebucus_hub_sweep_disc`, which drives the arc through real `0x3F` scene transitions. Each spoke's family is pinned disc-static:

- **`taiku` / `doman` / `rayman`** — self-latch pairs plus a linear `0x201` → `0x1FB` → `0x200` → `0x1FC` chain in `rayman`; `rayman2` is the same MAN with a shared C1 on the low flag `0x7`, a variant discriminator. `rayman`'s streaming variant adds a `P2[18..20]` tail latching `0x34D`/`0x34C` (`P2[18]` body `+0x2C2`, at a `JmpRel` branch-arm after `0x1FE`/`0x1FF` tests). The taiku variant's `P2[16]` beat SETs the pair `0x380` + `0x382` at its head (body `+0x11`/`+0x21`, between `SceneFade` and the particle emitters) — `0x382` is a **cross-chapter gate**: `son P1[14]` branches its NPC dialogue on it (body `+0x4A`), and the clean census reads span `doman(V)`/`retockin`/`ropeway`/`ropeway2`/`map03`/`koin2`/`korout`. Anchor `chapter2_dungeon_gate_families`.
- **`balden` / `balden2` / `station`** — `balden` is an arc around its reached-flag `0x1D5`; `balden2` is a sibling carrier with an identical gate family, so the variant is selected by the streaming slot rather than a flag. Cross-scene: `balden` gates on the `ropeway2` switches, and `station`/`station3` gate on `taiku`'s `0x38F`. Anchor `chapter2_balden_station_gate_families`.
- **`ropeway` / `ropeway2` / `jiji`** — the only spokes the capture corpus walked organically, so their play order is confirmed. `ropeway2` hosts a four-bit switch puzzle (`0x3FF`–`0x402`); its payoff records `P2[31..=34]` are gated via C2 on all four switches plus the `0x359` commit, an internal consumer the inline census had earlier mistaken for an external one. `jiji P2[8]` latches `0x304` from three branch arms of one cutscene (each `4C CD` → `Set` → `JmpRel` to the shared tail; bodies `+0x912`/`+0xCD6`/..). Anchor `chapter2_ropeway_jiji_gate_families`.
- **`retona`** — its own five-step ladder `0x353` → `0x354`/`0x355` → `0x356` → `0x357`: `P2[8..14]` gate on `0x353`/`0x354`/`0x356`, `P2[15]` chains C2=`0x354`/C1=`0x355`, `P2[17]` (C1=`0x357`, C2=`0x356`) is the pre-beat rendition and `P2[18]` (C2=`0x356`) the beat that SETs `0x357` (body `+0x5EF`, after the `4C 73` tile run + BGM cue).
  The entry script `P1[0]` carries a normalization backstop (`Test 0x357` → skip; `Test 0x3AD` → `Set 0x357` at `+0xF4`; `0x3AD` is also the C2 of `map02 P2[10]`, the overworld mirror `0x357` retires). **`0x357` is the Jeremi-arc cross-scene gate** — clean reads in `retock`/`retockin`/`map02`/`geremi`/`edretoin` — so the `0x357` half of retock's `0x357 → 0x502` chain is *retona's* output, not retock-internal. `P2[10]` separately latches `0x354` (`+0x673`), read by `rugi`.
- **`dohaty` / `retock` / `retockin` / `stone`** — `dohaty` opens with a six-record `0xF` first-visit group; `retock`'s progression depends cross-scene on `balden`'s `0x1D5` and gates on retona's `0x357` before its own `0x502`; `retockin` is the `0x7`-gated interior variant, sharing `0x502`/`0x357` with `retock`; `stone` is a single one-shot whose partition-0 walk-on scripts also latch a local band — `P0[2]`→`0x32B`, `P0[3]`→`0x32A`, `P0[4]`→`0x32D`, `P0[5]`→`0x32C` (`+0xB7F`, then `SpawnRecord 0x1E`).
  `0x32C` is a **write-only latch — no reader exists anywhere**: every census read (~50 scenes) is the ASCII `s,` bigram in dialogue (see [script-vm.md](../subsystems/script-vm.md) § ASCII dialogue aliases), no C1/C2 list in the pinned regions carries it, and the code side is swept negative too —
  a word-aligned scan of `SCUS_942.54` plus all 15 static overlay images (`crates/asset/data/static-overlays.toml`) finds no immediate `0x32C` load into any register, no access to the flag byte `0x800857BD` under any viable `lui`/`addiu` encoding, and no constant `0x32C` argument at any flag-helper call site (`FUN_8003CE08` set / `FUN_8003CE34` clear / `FUN_8003CE64` test) across the dump corpus.
  Residual reachability is data-driven readers only (script ops and C1/C2 gates, both already swept) and the 0897 dev-menu flag browser, which reads any flag on demand. Anchor `chapter2_dohaty_retock_stone_gate_families`.
- **`tunnelb` / `tunnelc`** (the range tunnels) — small internal one-shots: `tunnelb P2[34]` latches `0x322`/`0x326`, `tunnelc P1[4]` latches `0x360` + `0x362` from two branch arms (bodies `+0x107..+0x110` / `+0x2AB..+0x2B4`) and `P2[6]` latches `0x34A`; read back only by the tunnels themselves.
- **`map02` hub** — a router: only two gated records, both overworld mirrors of a dungeon-arc completion. Anchor `chapter2_map02_hub_gate_family`.

**Rim Elm town variants.** `town01`, `town0b`, and `town0c` share the Rim Elm opening chain (`549` → `0x226` → `0x227`, plus sub-chains) byte-for-byte in `P2[3..=11]`; they are story-state renditions of the one town, not separate places. A `town0c` visit in the chapter-2 capture is therefore a revisit, and the "scene" that appears beside it in the poll is the capture CSV's column header, not a map. `town0d` is the `0x7`-gated later variant. Anchor `town0c_is_a_rim_elm_state_variant_not_a_ch2_spoke`.

**Rim Elm revisit chain (`town0b` band `0x228..0x233`).** The revisit story state is a second flag band alongside the opening chain. `town0b P2[7]` (C1=`[0x22B,0x141]`, C2=`[0x147]`) is the revisit beat: it self-latches `0x22B` at its head (`+0x26`, before the flash + waits) and SETs `0x228`/`0x229`/`0x22A` from its branch arms (`+0x377`/`+0x804`/`+0x8F9`, each at a `JmpRel` boundary inside camera/emitter choreography).
Every Rim Elm variant ships a `P2[7]` under the same gate shell (`town01`/`town0c`/`gameover_data`: C1=`[0x22B]`, C2=`[0x147]`); town0b's copy adds `0x141` to C1 and is the rendition whose arms mint the band.
The successors chain through the band — `P2[8]` (C1=`0x231`, C2=`0x22F`) sets `0x231`, `P2[9]` (C1=`0x232`, C2=`0x141`) sets `0x232`, `P2[10]` (C1=`0x233`, C2=`0x232`) sets `0x233`, `P2[11]` (C1=`0x141`, C2=`0x231`) — while `P1[1]` is the state seeder (sets `0x22F` + `0x147`, clears `0x141`; same record in `town0c`).
The reads are cross-variant and real: `town01 P0[1]` (the entry walk-on) branches on `0x22F`/`0x229` (`+0x69`/`+0x6D`) and the NPC record `town0b P1[39]` selects dialogue over `0x22F`/`0x148`/`0x147`/`0x228`/`0x229`/`0x22A` in sequence. Late one-shots `town0b P2[30]` / `town0c P2[29]` latch `0x5C4` (`+0x3CD`, behind a `Test 0x35` battle-victory guard), read by the ending scene `edlast`.

**Rim Elm final variant (`town0e`) per-NPC band `0x5DC..0x5F0` + `0x6DC`.** Every `town0e` NPC interaction record `P1[1..24]` opens with the same head — `Test <own flag>` → skip, `Set <own flag>`, then `Test` the *neighbouring* NPCs' flags (`P1[2]`: `Set 0x5DC` at `+0x20`, then tests `0x5D8..0x5DB`) — a talked-to-everyone tracker whose dialogue changes as the rest of the cast is visited. Scene-local flavor state, not progression; the record indices map 1:1 onto the band.

**Uru Mais (`uru`/`uru2`) beat band.** `uru`'s cutscene tail latches `0x3BE` (`P2[30]`), `0x3BF` (`P2[34]`), `0x3C0` (`P2[32]`), and `0x3FC` (`P2[38]`, body `+0x8B7` after a BGM cue). `P2[30]` is the party-recompose beat: `PartyAdd char 1` + `Set 0x11`, `PartyAdd char 2` + `Set 0x12`, then `Set 0x3BE` (`+0x72`) under a camera reconfigure — the low party-presence flags and the story latch written by the same record. All four flags read back only within `uru`.

**Nivora Ravine (`nilboa`).** An entry group sharing `0x456`, a `0x47x` puzzle cluster, and a cross-scene successor gated on `0x370`; `nilboa2` is the `0xF`-gated variant carrier. `0x456`'s writer is pinned: `nilboa P2[11]` both SETs and CLEARs it (`Set 0x455` + `Set 0x456` at `+0x37..+0x39`, inside a `CC .. C3` per-actor run). `0x370`'s writer is **pinned static**: `doman` variant `P1[15]` at MAN offset `0x06397` — a `53 70` SET in a clean
choreography run whose loop-back `JmpRel` re-enters the record's gate-test head, with the head's own
`Test 0x370 -> +0x301E` jump landing on the very next op (the Dr. Usha "Do you understand? The first TimeSpace…"
briefing branch) — the town01/549 self-latch shape. The record's other three `53 70` occurrences are the
"Time**Sp**ace Bomb" prose aliases the earlier hand-check adjudicated (that check predated the nibble-width
pinning and never saw this site). The doman `P1[3..=18]` clean head TESTs are the reader family (arc-gate
dispatch chain, alias-immune operands). Pinned by
`man_variant_carrier_census_disc.rs::flag_0x370_writer_is_the_doman_p1_15_usha_latch`; a live organic SET
(the poll auto-snapshots flag 880) confirms play-order. Anchor `nilboa_nivora_ravine_gate_family`.

**Chapter 3 — Karisto (`map03` and its spokes).** `map03` is a pure router with no gated records at all. Its spokes are `bubu2` (a small requires-all chain), `son` and `deroa` (sparse one-shots; `deroa` leads to the underground `chitei2`), and `korb3`, the Karisto castle approach, whose nine-record collection group `P2[5..=13]` — each record gated on a distinct flag under one shared `0x403` "all done" latch — is the most elaborate family found. `bubu1` carries no field MAN.
Ungated hub state does exist as inline latches: `map03 P2[15]` SETs `0x378` (`+0x9E`, between a 180-frame camera hold and the particle emitters), read back by `doman` and `map03` itself. `son`'s NPC records use the per-NPC one-shot head (`P1[14]`: `Test 0x62E` → skip / `Set 0x62E` at `+0x52`) and branch on taiku's `0x382`. Anchor `map03_karisto_region_gate_families`.

**Chapter 3 — Karisto castle depth (`kor`/`koin` cluster + `chitei2`).** `kor` holds one-shot beats (`0x408` read by `korout`, self-latches `0x409`/`0x40A`) plus a
**door group** C2-gated on `0x612` — an *arm-then-consume* mechanic: the partition-0 entry scripts SET `0x612`, each door record clears it back; `kor3`/`kor4` gate
their doors on the same flag. `kor5` is a three-step chain `0x43A → 0x436 → 0x6C4`. `koin1b` is `koin1`'s story-state sibling (same gate shape + a spliced `0x00B`
toggle pair; it owns the `0x3DA` SET koin1 gates on); koin1's `P2[9..10]` are a `0x50A` set/clear **toggle pair**. `chitei2` holds the `0x470`/`0x4F0` and
`0x4C4`/`0x4C6`/`0x4C8`/`0x4C9` families — `0x4C8` is co-written by `map03 P2[19]` (the hub co-writes the underground beat). `korb2`/`koin2`/`koin6` are gateless.
`koin3 P2[8]` and its stale sibling copy `other7 P2[5]` co-latch `0x430` (`koin3` body `+0xA40`, a `JmpRel` branch-arm set inside `CC` camera choreography), read by the ending scene `edlast` — an epilogue-visible castle beat.
`0x50A` is **resolved** - the Sol game-hall minigame result toggle, written **natively by
the mode-24 minigame overlays** (a space the MAN script census is structurally blind to):
the Muscle Dome module (PROT 0977) CLEARs it in the post-match settle (`0x801D0FF8`) and
win-re-SETs it (`0x801D101C`, labeled by the overlay's own `WIn on`/`WIn off` debug
strings), and the dance trio (0978..0980) SETs it at session start (`0x801CF968`) / CLEARs
on a missed goal (`0x801CFF10`); koin1 hosts the Muscle Dome + Baka doors (`3E 69`/`3E
68`), koin3 the dance doors (`3E 6A`), and koin1 `P2[9]` (C2=[`0x50A`]) is the returned-
victorious beat. `0x5D6` is **resolved as writer-less** (the `0x482` class): negative
across the script census, a disc-wide operand-classified sweep of every native flag-helper
caller (`scripts/asset-investigation/flag_helper_call_sweep.py`), the move-VM ext flag
sub-ops, the motion-VM census and raw MAN operand scans; only the dev-menu flag editor
(index cell `0x801F2AA0`) reaches it, so koin4's `0x5D6` content is dev residue,
unreachable in retail. See [script-vm.md](../subsystems/script-vm.md) § native flag-bank
writers. The guard `koin_gates_0x50a_0x5d6_remain_script_writer_less` stays correct as
stated (script-writer-less).
(Nivora's `0x370` left this list — its writer surfaced statically under the pinned widths; see the Nivora Ravine row.)
Anchors `chapter3_karisto_castle_gate_families` + `chapter3_koin_family_and_writer_pins`. Runtime oracle: `chapter3_karisto_spine_oracle.rs` — the Conkram→deroa→chitei2
bridge, the kor5 chain, the door arm-then-consume, and the koin toggle all sequence through `p2_record_gates_pass` + `install_gated_p2_record` with no
chapter-specific engine code (the chapter-2 shape holds).

**Chapter 3 — Conkram (`conc*`, the "past" arc).** The pivot pair is `0x3E1`/`0x3E5`: `conc2 P2[12]` SETs `0x3E1` — the flag `deroa` C2-gates the `chitei2` descent
on (the cross-region bridge) — and `conc3` self-latches `0x3E5` (`P2[10]`) + SETs `0x3F9` (ungated `P2[9]`); `conc P2[10]` chains on both. `conc`/`concnow` carry
`r1..rN` **soldier rows** all C1-gated on the low flag `0x007` (SET by `concnow P0[34]` + `conc2 P0[21]` — a "soldiers disperse" beat); `conc` has eleven doors on
`0x6DE`, armed by the entry script's player-position BBoxTest run (same mechanic as kor's `0x612`) — and the arm is not conc-exclusive: all four carriers'
entry scripts (`conc`/`conc2`/`conc3`/`concnow P1[0]`) SET `0x6DE`. `concend` is a single ungated epilogue record.

The `concnow` one-shot ladder's writers are pinned — each C1 gate is a self-latch in its own record: `P2[13]`→`0x3ED`, `P2[14]`→`0x3EE`, `P2[15]`→`0x3D2`
(at its tail `+0x1483`), `P2[16]`→`0x3CE`, `P2[18]`→`0x423`, plus `P2[20]`→`0x3CF`. Two of them are more than latches:

- **`0x3EF` is the chapter-wide "Conkram revelation" gate.** `P2[15]` SETs it from a branch arm (`+0xDDD`, after the emitter run + BGM cue, jumping straight
  to the record tail). Its operand byte is outside ASCII, so the census reads are alias-immune: clean `Test` sites in fifteen scenes spanning Sebucus
  (`balden`/`balden2`/`bylon`/`dolk2`/`geremi`/`jiji`/`rayman`/`rayman2`/`retock`/`ropeway`) and Karisto (`koin1`/`koin2`/`son`/`doman`) — world-wide NPC
  dialogue reacts to the beat.
- **`0x423` is a cross-scene message, not a one-shot.** `conc2 P1[0]` *consumes* it on entry (`Test 0x423` → `Clear 0x423`, `Set 0x664`, `SpawnRecord 0x69`
  at `+0xDB..+0xE8`): the concnow beat posts the flag, and the next `conc2` visit converts it into `0x664` (read by `conc`) plus a spawned follow-up record.
  The pre-fix census could not see the consume side, so the ladder read as five identical latches.

Anchor `chapter3_conkram_gate_families`.

**Cross-cutting patterns.** Two low-numbered flags recur as variant discriminators, gating nearly every record of an alternate or interior carrier: `0x7` (`rayman2`, `retockin`, `town0d`) and `0xF` (`dohaty`, `nilboa`, `nilboa2`) — most likely party- or chapter-state globals that select which rendition of a scene is live. Region hubs hold little or no gate state of their own; the progression logic lives in the spoke dungeons.
Two traps when reading the census against these families: the story-numbered band `0x522..0x531` is engine scratch (a one-hot exit selector + fade handshake repeated in nearly every scene's entry script — [script-vm.md](../subsystems/script-vm.md) § the `0x527..0x531` scene-transition scratch band), and clean-tagged rows over flags whose operand byte is printable ASCII can be dialogue bigrams (`ta`/`s,`/`Sp`) — the wide reader lists of `0x461` and `0x32C` dissolve entirely under that check ([script-vm.md](../subsystems/script-vm.md) § ASCII dialogue aliases).

**Residual.** The families for the dungeons the capture corpus never walked (`taiku`/`doman`/`rayman`, `station`, `dohaty`/`retock`, the Karisto spokes) are proven as structure, but their in-game play order is not yet confirmed against a live capture. The generic C1/C2 seeder already drives them, so one dungeon-walk capture per region would close the residual.

### Extraction-0874 §2 (`player.lzs`) F-variant pixels - a one-shot opening face-frame stamp, not a menu writer

*Status:* resolved - the installing event is named

The earlier "a freshly booted game holds the `0xFFFF` variant" premise was already refuted
(title screen all-zero; the mode-2 field-entry load uploads the disc bytes). The successor
"pause-menu-path writer" premise is **falsified** (grade `capture`, exhaustive): with
every DMA2 kick chain-walked for `A0/80/E3/E4/E5` packets *and* GP0 PIO stores hooked, the
whole pause walk issues **zero** image transfers and the band is byte-identical before and
after; a 49-state library census shows plain field saves carrying the F-variant with no
menu in their lineage while `s1/s2` hold disc bytes - the flip brackets inside the town01
opening (s2→s3), and the 6/6 pause-capture correlation was session history, not causation.

The wrap-scroll-phase reading fell next. The 3 words (`(853,271)` `3333→ffff`, `(856,271)`
`3333→fff3`, `(857,271)` `1e33→1e3f`) equal the disc words at `(x,273)` by **frame-content
coincidence only**: the Noa strip (TIM 2 at `(852,256)` 20×128; rows 271/273 = its rows
15/17) is not shift-invariant, so a parked +2-row rotation would move dozens of rows, and
the wrap-scroll installer ops (move-VM op `0x1E`, body `0x80023694`; op `0x45` sibling)
plus the `FUN_80021DF4` dispatch-4 arm never fire across a full s2→s3 replay while the
flip reproduces (`autorun_s2s3_scroll_installer.lua`).

The installer is **town01 MAN `P2[3]` (`★ＯＰ`, the Rim Elm opening timeline record,
C1-gated on the opening latch `0x225`)**, body `+0x392`/`+0x3A0`: after the opening's
white flash + 60-frame wait it stamps the Noa face cell once via field-VM op **`4C 60`**
(literal-operand MoveImage `[4C 60 src_x src_y w h dst_x dst_y]`, six misaligned u16s via
`FUN_8003CE9C`, handler arm `0x801E1B28..0x801E1B90`, `jal FUN_80058490` at `0x801E1B84`)
- `MoveImage (852,336,6,16) → (852,268)` and `(852,368,4,8) → (853,284)`. The parked
alternate frame differs from the boot cell at exactly the three F-variant halfwords (row
271 cols 1/4/5); the live catch at `ra = 0x801E1B8C` reproduces the s3 anchor band byte-
exact (`autorun_s2s3_atlas_stamp.lua`), and the two ops sit on the disc at MAN offsets
`0x735A`/`0x7368` (PROT 0004 §1, LZS at container `0x25BEB`) - the misaligned-u16 operands
are why every aligned scan missed them. The `0x225` C1 gate fires once per game, which is
why every post-opening save carries the variant; the first battle effect-texture re-upload
restores the disc bytes. See [character-mesh.md](../formats/character-mesh.md#runtime-
scroll-cell-residue-why-a-live-vram-dump-can-differ-from-the-tim).

## Text / fonts / dialog

| Thread | Status | What would close it |
|---|---|---|
| Pause Items/Magic screens: remaining sub-flows | resolved (one capture-diff residual) |
All four sub-flows traced from disassembly and ported: the **window-14 target panel**
(`FUN_801D0520`; the preview modes are the permanent-stat Water previews, superseding the
"HP-restore" reading), the **PAGE sprite** (UI-icon `0x76`), the **SCUS kind-4 list
kernel** (`FUN_80032A44` + allocator `FUN_80030104`), and the **class-`0x80..0x82` Use
routes** (submenus 0xA..0xD: single-target apply `FUN_801D8308`, Door of Light/Wind
`FUN_801D8A58`/`FUN_801D8B90`, Incense `FUN_801D8D94`). Engine `engine-
ui`/`pause_screens`. See [field-menu.md](../subsystems/field-menu.md#items-screen). The
`0x800` dim-bit residual is closed (grade `disassembly`+`capture`): it is set at **build
time** by the SCUS content builder `FUN_80030628`'s content-id-3 case (dispatch on live
window `+0x1C`, copied from descriptor byte `+0x0` at create, `0x80032990`) - equipment
always-dim, Door ids `0x88/0x89` scratchpad-gated, field-usable bit `0x2`, then the
`FUN_8003043C` applicability probe (battle context gates bit `0x4`). No focus-dependent
write exists - the white→grey flip is the kernel mode-4 park override; a capture shows the
row words bit-identical across focus states. See [field-menu.md](../subsystems/field-
menu.md#use-list-row-build-content-id-3-fun_80030628). |

## Audio

| Thread | Status | What would close it |
|---|---|---|
| `_DAT_8007B910` carries two incompatible roles | open | The corpus calls it "live brightness" (seeded `0xD7` beside the brightness reference `_DAT_8008457C` by `FUN_8001FFA4`; ramped as a screen fade by the battle-action SM states `0x35`/`0x51`/`0x6F`) **and** feeds it as an audio scalar: `FUN_80026478` passes `_DAT_8007B910 >> 1` to the pan primitive `FUN_8002657C`, and `FUN_800267A8` passes the same halved value to the libsnd wrapper `FUN_80062004`. Both readings are already committed, in different pages. Closing it needs a live watch on the cell across a summon cast (brightness ramp) with the audio mix observed, or the identification of `FUN_80062004`'s libsnd entry - if its second argument is a volume, one of the two labels is wrong. |
| XA clip-table writer + `(clip_id, chan)` cue census | resolved | Writer pinned statically: `FUN_801CFA78` in PROT 0895 `init.pak` (base `0x801CE818`, recovered from four in-blob string refs) sprintf-generates `\XA\XA%d.XA;1` per slot and fills `[BCD-MSF][size]` via ISO9660 lookup `FUN_8005DBB4`; called once from the init boot tick `0x801CF500`. Full deduped one-shot + streamed cue census in [`audio.md`](../subsystems/audio.md); grade `disassembly` (byte-level, base self-consistent). Census note: PROT-entry over-read aliases callsites into neighbouring overlays - dedupe by true entry extent (gameover 0902 / world-map 0901 have zero genuine XA calls). [details ↓](#xa-clip-table-writer--clip_id-chan-cue-census) |

### XA clip-table writer + `(clip_id, chan)` cue census

*Status:* resolved (writer pinned; cue census below)

The `0x801C6ED8` clip-table content is pinned (34 `[CdlLOC][len]` slots = `XA1..XA34`, title-capture byte-exact vs the disc files). The filler is **`FUN_801CFA78`** in PROT 0895 `init.pak` (base `0x801CE818`, recovered from four in-blob string refs): it sprintf-generates `\XA\XA%d.XA;1` per slot and fills `[BCD-MSF][size]` via the ISO9660 lookup `FUN_8005DBB4`, called once from the init boot tick `0x801CF500`. The earlier "filler is an untraceable DMA/computed write" framing was the SCUS-only sweep's blind spot - the two `lui 0x801c` materialisation sites in SCUS (`FUN_8003D53C`/`FUN_8003EAE4`) are the **readers**, and the writer is overlay-resident, so no absolute-form scan of SCUS could see it.

A caller census of `FUN_8003D53C`/`FUN_8003EAE4` names each `(clip_id, chan)` cue. Decoded: menu voice `FUN_8004FCC8`; the normal-move grunt (`XA30` chan 0/4/6, overlay `0x801EEB44`); the **arts shout** (`FUN_8004C140` → `XA2`/`XA4`/`XA6` per character, per-art channel pool, capture-verified; [battle-action.md](../subsystems/battle-action.md)); SM state-`0x6E` (`XA9` via `0x800787AF`); slot machine `XA1`. Full deduped one-shot + streamed cue census in [`audio.md`](../subsystems/audio.md). Census note: PROT-entry over-read aliases callsites into neighbouring overlays - dedupe by true entry extent (gameover 0902 / world-map 0901 have zero genuine XA calls).

## Title / boot / overlays

| Thread | Status | What would close it |
|---|---|---|
| Debug flag `0x8007B98F` | resolved | `0x8007B98F` has no byte-granular reader: it is byte +3 (MSB, little-endian) of the 32-bit debug-mode word `_DAT_8007B98C`, and *that word* is the consumer surface. Its sibling `0x8007B8C2` is now **settled** - see [`re-settled-threads.md`](re-settled-threads.md#_dat_8007b8c2-polarity-and-its-writer). [details ↓](#debug-flags-0x8007b8c2--0x8007b98f) |
| Full-window item-add OOB primitive: reachability | resolved (moved to re-settled) | Primitive real (grade `disassembly`): id store `sb t0,0x1818(a0)` @ `0x800422BC` is unconditional, before the guard that gates only the count store. But **unreachable through the retail add call sites in normal play** - each caller `jal`s the helper with no room pre-check, and the helper's free-slot scan cannot reach the `i == end` OOB exit (a `[0,256)` window holds ≤255 distinct ids, so a hole always remains). See [`re-settled-threads.md`](re-settled-threads.md#full-window-item-add-oob-reachability). |
| New-Game opening chain + narration roller | resolved (chain + caption + roller + prologue gold grade; far-geometry residual closed resolved-negative) - the gold grade is a capture-pinned palette-space collapse, superseding the per-node depth-cue reading | [details ↓](#new-game-opening-chain--narration-roller) |
| Slot-B overlay cluster (`0900..0969`) per-entry identity | mostly resolved | [details ↓](#slot-b-overlay-cluster-09000969-per-entry-identity) |
| PROT 0977 / 0978 are not in the extracted overlay set | open | [details ↓](#prot-0977--0978-are-not-in-the-extracted-overlay-set) |
| Phantom-VA sweep of the PROT 0897 imports | partial | The two deltas are measured and nine addresses are re-keyed against base-tagged dumps - see [`overlay-va-aliases.md`](overlay-va-aliases.md). What remains: the boundary band near `0x801E5000` (where the "0897 own content" and "over-read into 0898" readings both land inside a dumped body, and neither dump carries enough instructions to decide), the doubly-aliased `0x8020D05C`, and whether PROT 0896's imports obey a law of their own (one `0x9000` step is measured, which is not a law). Closing it means a byte-level sweep of both images at every printed VA rather than the per-address spot checks done so far. |
| Overlay-loader index off-by-2 - remaining ripple | resolved | Slot A reconciled; slot-B per-spell identity fully capture-pinned across every block, incl. the flute summons 0924/0925 (Lippian/Spikefish) and the 0926 unused-`0x98` stub; engine mirrors carry the extraction-space constant. [details ↓](#overlay-loader-index-off-by-2---remaining-ripple) |

### PROT 0977 / 0978 are not in the extracted overlay set

*Status:* open

Dumps prefixed `overlay_0977_*` / `overlay_0978_*` resolve to no extracted
image, so their printed addresses cannot be corrected against a known base.

Five rows (`801c2b58`, `801c3004`, `801c39b8`, `801c614c`, `801c6804`) close as
mis-based prints on the **base argument alone**: every slot-A overlay bases at
`0x801CE818` and every slot-B overlay at `0x801F69D8`, so a VA below
`0x801CE818` names no overlay function whatever its dump looks like. That
disposes of the addresses; it does not recover the functions. The real VA of
each body stays unknown and the bodies stay undocumented.

Two hints at where they came from: four `overlay_0978_*` siblings resolve into
`dance_0980` at a constant `+0x9818`, and one `overlay_0977_*` into
`baka_fighter_0976` at `+0x5710`. That the deltas are constant per program
suggests the images those imports were taken from are not the overlays their
filenames claim - the same class of error measured in
[`overlay-va-aliases.md`](overlay-va-aliases.md) for the PROT 0897 imports.

Closing it needs `asset overlay` runs for 0977 and 0978, then a re-run of
`scripts/ghidra-analysis/check-dump-base-integrity.py`. Evidence grade:
`disassembly`.

### Slot-B overlay cluster (`0900..0969`) per-entry identity

*Status:* mostly resolved

The slot-B buffer (link base `0x801F69D8`) timeshares the `0900..0969` summon/dance/minigame
blobs; static extraction at the link base is the clean path, each base cross-checked by in-file
self-pointer resolution (`static_overlay::pointer_resolution`, ≥70%). Pinned:

- **0900/0901** = the slot-B *default* pair - `FUN_80025BA0` loads param 5 or 6 by flag
  `DAT_8007B6A8`, agreeing with 0900's byte-residency in mid-cast saves (the summon-render
  overlay).
- **0903** = the Gimard `0x81` arithmetic slot; the deep-dived 38-spawn-call stager file is
  extraction **0905** = the `0x83` slot. The summon arithmetic range is extraction
  `0903..=0913` (raw `0x389..=0x393`) - **fully capture-pinned per spell id**, incl. 0907 =
  Nighto on the `0x85` slot (head title "Hell's Music" = the attack's display name; the
  dance-song reading is refuted).
- **0902** = GAME OVER (content pin, corroborated by the loader census: `FUN_8003EBE4(7)`
  inside the mode-18 init).
- **0924/0925/0926** = the rare-Seru **flute summon** block, capture-pinned (states
  `flute_lippian_midcast` / `flute_spikefish_midcast`): 0924 = **Lippian** (spell `0x96`;
  head title "Ultimate Rave" is the attack's failed-kill banner - the landed 1/128 kill
  shows "Ultimate Death", the summon.dat slot-65 actor name), 0925 = **Spikefish** (spell
  `0x97`, attack "Blowfish", untitled pre-linked head), 0926 = the unused `0x98` slot (one
  sector, a `jr ra` stub). The "Dark Eclipse" text inside 0925/0926 extractions is 0927's
  head bleeding through the over-read. **0957** summon-effect strings (**NOT** a dance
  song).


### Debug flags `0x8007B8C2` / `0x8007B98F`

*Status:* `_DAT_8007B98F` is **resolved** (the MSB of the debug-mode word `_DAT_8007B98C`, whose consumer is statically pinned at `FUN_8001822c` + the resident field-overlay gates, no capture required). `_DAT_8007B8C2` is now **settled** and has moved to [`re-settled-threads.md`](re-settled-threads.md#_dat_8007b8c2-polarity-and-its-writer); the corrected branch sense below stands, and the writer that made it self-consistent has been found.

Neither address is BIOS-zeroed: the PS-X EXE header carries `b_addr = 0, b_size = 0`, so no BSS is cleared for this executable at all. The earlier "zero-initialised at boot" framing was wrong independently of the polarity question.

**`_DAT_8007B8C2` - the branch sense, corrected.** `_DAT_8007B8C2` is the dev/retail asset-load selector, and the polarity is the opposite of what this row long recorded. Every site reads it with `lh` and takes the **zero** arm to the host-PC debug station, the **non-zero** arm to the PROT-index resolver:

| Site | `== 0` arm | `!= 0` arm |
|---|---|---|
| `FUN_8003E360` (`bnez` at `0x8003E37C`) | `jal 0x800608F0`, whose body is `break 0x103` | `0x8003E49C`: index `0x3D5` into `FUN_8003E8A8` |
| `FUN_8001D8FC` (`bnez` at `0x8001D91C`) | `jal 0x8003E6BC` on an `h:\` path | the ISO loader `FUN_8003D3C4` |
| `FUN_800558FC` (`bnez` at `0x80055938`) | `jal 0x800608F0` | `sll/sra a3` → `FUN_8003E8A8` |

So the old sentence - "routes through ISO9660 when `_DAT_8007B8C2 == 0` (retail) and through the PROT-index loader when non-zero (dev)" - had the two arms swapped, and named ISO9660 for a branch that is the index resolver. The `FUN_8001F7C0` / `FUN_8001FA88` / `FUN_8001FC00` citations were also loose: the sound pair's *opening* gate is the unrelated word `_DAT_8007B868`; they reach `_DAT_8007B8C2` further into the body.

**The apparent paradox, and its resolution.** A word-aligned scan of `SCUS_942.54`
finds 40 reads of `0x8007B8C2` and, in the **absolute** `lui`+offset form, zero
stores - which made the flag look writer-less and therefore stuck on the arm
retail cannot service. The writer exists and is **gp-relative**: `main()`
(`FUN_80015E90`) stores it once at cold boot, `0x80015F08 sh v0,0x5aa(gp)` with
`gp = 0x8007B318`, taking the return of `FUN_8003F084` - a two-instruction leaf
(`jr ra` / `addiu v0,zero,0x1`) that returns the constant `1`. A sweep searching
only the absolute form cannot see it. The
[`summon.dat` row](re-settled-threads.md#summondat--readefdat-side-band-streaming)
recording `_DAT_8007B8C2 != 0` as *verified live* was right all along. Full
account, including the 60/60 save-state confirmation:
[`re-settled-threads.md`](re-settled-threads.md#_dat_8007b8c2-polarity-and-its-writer).
and [`boot.md`](../subsystems/boot.md#debug-flags).

**Corpus sweep (`_DAT_8007B98F`).** The dump sweep across SCUS + every captured overlay finds zero references - read or write - to `_DAT_8007B98F`.

`_DAT_8007B98F` has zero references in the captured corpus because it is **not
read byte-granularly at all**: it is byte +3 (the MSB, little-endian) of the
32-bit debug-mode word `_DAT_8007B98C`, and that word is the real consumer
surface. Grep of `ghidra/scripts/funcs/` for `8007b98f` returns 0 hits;
`_DAT_8007B98C` is read as the debug gate in SCUS (`FUN_8001822c` at
`8001822c.txt:500/533`, plus `80016230`/`80016444`/`800173bc`/`800188c8`/
`8003cbf8`/`8004ad80`/`80025cb4`) and across the field/dialog/world-map overlays
(an aligned word-search of the 23 static overlays finds 14 genuine
`lw ...,-0x4674(reg)` reads of `0x8007B98C` in the field overlay 0897, base reg
= `0x80080000`), with the sole `sw` writer in the shared menu/title/save-init
routine (`overlay_menu_801de234`/`overlay_title_801ddccc` internal offset
`0x4158`). So `SELECT+START` / GameShark writing `0x8007B98F = 1` sets the MSB
of the word, and every `_DAT_8007B98C != 0` gate then reads the debug mode
active. The earlier "stripped at link time / inert" AND "consumer in an
uncaptured overlay" framings are both superseded: the consumer is `FUN_8001822c`
+ the resident field-overlay gates, statically pinned, no capture required. See
[`subsystems/boot.md` § Debug flags](../subsystems/boot.md#debug-flags) and
[`reference/builds.md` § Debug input bindings](builds.md#debug-input-bindings)
for the combo table.

**Runtime confirmation.** The static model above was derived without ever opening the
menu; driving it under the static recomp then reproduced every part of it, and the
three details that only a live run could show all fall out of the static reading
rather than contradicting it:

- Asserting the debug word and pulsing `SELECT + △` **on controller port 2** opens the
  game-owned developer menu. Port 2 is not an extra fact to learn - it is forced by the
  `_DAT_8007B850 &= 0xFFFF` mask, which puts every debug binding in the upper half.
- The gate **does not survive scene initialisation** and has to be held asserted for
  the session. That is the single `sw` writer doing its job: scene transitions run the
  shared menu/title/save-init routine, which clears the word.
- Forcing game **mode 0** does *not* reach that menu - it loads PROT 0971's full-screen
  configuration tester, exactly as the `CONFIG INIT` mode-table reading in
  [`boot.md`](../subsystems/boot.md#game-mode-state-machine) predicts. The developer
  menu's MAP CHANGE appliers are field-overlay-0897-resident, matching the 14 gate
  reads found there.

### New-Game opening chain + narration roller

*Status:* resolved - the chain, caption, roller, and prologue gold grade are resolved; the far-geometry-brightness residual below closed resolved-negative

**One sub-claim was graded `decompiled-C` and flagged for re-audit: the roller
config op's operand decode. It has now been re-derived from the field-overlay
disassembly and confirmed** (handler `0x801E3378` in `overlay_0897_801e0c3c.txt`;
reader `80037174.txt`; grade `disassembly`). Sub-thread 2 below says `CC F8 E8 …`
carries **four** signed-16 LE words and describes **three** globals being written
(`+0x4C`, `+0x4E`, `+0x50`), with the fourth word said to select a mode. `word3`
is a pure selector that is never stored, and the handler writes exactly three
`_DAT_801C6EA4` globals (`sh` at `0x801E34B0`/`34B4`/`34BC`) - so the
four-read/three-write shape is genuine, **not** the "only N of M slots" artifact.
The earlier `4C 88` label was the wrong op (see sub-thread 2); the confirmed
handler is the nibble-`E` sub-8 `0xE8` form, and `RollerParams::for_scene`'s
operand mapping is pinned. The five-scene chain, the caption TIM, and the
camera-mover law rest on captures and are unaffected.

**The opening is a five-scene chain, live-probe + pixel-capture pinned** - `opdeene` → `opstati` → `opurud` → `map01` → `town01`, all master mode 3, zero input; the `FUN_801D1344` `town01` packet is the **intro skip** (its earlier reading as the required hand-off gate is superseded). Each leg's record spawn is pinned (exec-BP on `FUN_8003BDE0`, exactly 5 hits): op `0x44` SPAWN_RECORD in the first three legs' entry scripts (the old op-`0x44` "COUNTER" reading is superseded), the walk-on tile trigger (`FUN_801D1EC4` → `FUN_801D5630`) for `map01`/`town01`. Full mechanics: [`cutscene.md`](../subsystems/cutscene.md#in-engine-3d-opening-the-five-scene-new-game-chain).

**The narration is a bottom-up scrolling crawl** (roller actor `FUN_80037174`, spawned as a **child context** so the parent timeline keeps executing and the between-block camera cuts play under the scroll; per-scene capture-pinned geometry/speed), not a one-caption-at-a-time presenter - the prior one-line model described the separate `4C E1` balloon op (`FUN_8003C764` / `FUN_801DA7F0`) and is superseded. A cold-boot crawl-1 capture (`scripts/pcsx-redux/autorun_crawl1_capture.lua`) confirms the eye cuts through the Genesis-grove foliage to the villager tableau *while* the creation crawl scrolls; the engine ports this as a non-blocking crawl (blocking only the last block of a scene before its terminal SceneChange).
The name-entry auto-open stays pinned: op `0x49` STATE_RESUME sub-op 3 at town01 P2[3] body offset `0x02c6` (`_DAT_8007B450` parks there while name entry is up); the retail town01 order is establishing pan → name entry → Vahn's walk-out.
The op-`0x45` camera param→global map, the GTE rotation build (`FUN_8001CF50`), and the eye-back depth (the offset-trio slot 5, `0x800840B8` - no separate eye-distance scalar) are all pinned; `play-window` renders through `psx_camera_mvp`.

**The per-frame camera mover is `FUN_801DC0BC`, not `FUN_801DB510`** (that is the follow / scroll
camera - a different mode of the same globals). `FUN_801DD310` attaches ten `(start, end)` pairs plus
one shared progress / duration / curve to a dedicated mover actor, so a glide runs **in parallel**
with the record that staged it, and a beat landing mid-tween re-seeds every axis from the live pose.
All four ease curves are decoded, and the port (`legaia_engine_vm::camera_mover`) reproduces a live
retail capture on 2471 of 2480 sampled axis values, the rest resolving under the probe's own read
skew. Falsified with it: the "mode 1 eases the angles but runs the eye trio linear" per-axis curve
split - retail applies one curve to all ten axes, so mode 1 is **linear on pitch/yaw too** (measured
on three independent beats, incl. a 2000+-frame yaw dolly). Frame-exact recomp captures of the whole
opening chain re-confirm the law per display frame: the env-gated oracle
`camera_mover_recomp_oracle` (`LEGAIA_RECOMP_TRACE_DIR`) replays the staged snap / mode-1 / mode-2 /
mode-4 beats bit-exact, and pins the `town01` arrival H glide (`P2[3] +0x00C4`, `apply` 600,
H 412 → 512) as **mode 4** ease-in-out (`op0 0x13 >> 2`; an earlier mode-2 reading of that beat is
falsified - disc pin `town01_arrival_camera`). Full law in
[`cutscene.md`](../subsystems/cutscene.md#in-engine-3d-opening-the-five-scene-new-game-chain).

**Retired: the "field-VM step-parallelism" dead-air thread.** Retail runs no hidden parallelism the
engine has to catch up with - `FUN_8002519C` walks the actor lists in full every frame, so every
context already gets one run-until-yield slice per frame
([`script-vm.md`](../subsystems/script-vm.md#per-frame-scheduling)). The measured inter-crawl gap was
a units error: record durations count retail **display** frames (op-`0x4A` and the mover both
accumulate `DAT_1F800393`), and the engine stepped its timeline once per 100 Hz sim tick. Pacing the
timeline off the existing 60 Hz sub-clock moved the whole zero-input opening chain from ~10 % short
of retail wall-time to within ~4 %, pinned by `opening_chain_wall_time`. **The `map01` fly-in
overhang is closed** (grade `capture` - frame-tagged recomp camera trace of the whole chain): the
engine was serializing the final narration crawl against the record's authored tail - it parked at
the last crawl's *open* op until the roller drained, then ran the authored `4A` waits, double-
counting. Retail opens every crawl non-blocking and holds only at the record's **terminal `0x3F`
SceneChange** while narration is active; the retail leg decomposes exactly into scene-load/init +
the authored waits with the 3-page crawl scrolling concurrently. The other three legs hid the
misplacement because their last crawl sits directly before the SceneChange - `map01` was the
discriminating case. With the hold moved to the SceneChange, every leg runs one-sidedly *short* by
its un-modeled retail scene-load window (the engine loads scenes instantly by design), and
`opening_chain_wall_time` pins asymmetric bands so running long is the hard regression signal. See
[`cutscene.md`](../subsystems/cutscene.md#narration-playback---the-crawl-roller-fun_80037174).

**Data-source sub-threads - both resolved:**

1. **The *"It was the Seru."* caption's data source - it is not text.** The caption is a **pre-rendered 112×32 4bpp TIM** (two CLUT palettes = the fade steps) baked into the `opdeene` geometry pack **PROT entry 0749** at LZS-decoded offset `0x01EC30` (VRAM `fb=(384,0)`), drawn by the scene renderer as a screen-space textured quad - not a `4C E1` balloon, not a MES id, not any font string. Pinned by cold-boot probes (`autorun_text_census.lua` + `autorun_seru_blit_probe.lua` + a full-RAM dump): every UI text/image draw path fires **zero** times in the caption window and the string is in RAM in **no** encoding. `tim-scan extracted/PROT/0749_opdeene.BIN` renders it. See [`cutscene.md`](../subsystems/cutscene.md#narration-playback---the-crawl-roller-fun_80037174).
2. **The retail roller config op's parameter decode - decoded (Ghidra-traced).** Two sub-ops of field-VM op `0x4C`: the spawner `CC F8 80 N` (`N` = page count) allocates the roller child on `FUN_80037174`, and `CC F8 E8 …` (four signed-16 LE words) seeds the per-scene crawl globals at `_DAT_801C6EA4`: `+0x4C` = window top Y, `+0x4E` = visible line count, `+0x50` = scroll-cadence divisor (`word3` selects seed/pause/resume/kill). The earlier `4C 88`-shaped label was a **mis-attribution** (op0 `0x88` writes `_DAT_80084628/…`, not the crawl geometry; the seed is the nibble-`E` sub-8 `0xE8` form). So `RollerParams::for_scene` is derivable from the scene bytecode, not just the pixel capture. Full decode in [`cutscene.md`](../subsystems/cutscene.md#roller-op-operands-ghidra-traced).

**Render-fidelity residuals (mostly resolved):**

- **Prologue gold grade = palette-space collapse (settled, grade `capture`).**
  Both former residuals ("per-node depth-cue crush", "tableau ground texture
  chroma") had one root cause, and it is neither a depth cue nor a texture
  binding. A live recomp capture (cold boot, VRAM-peek vs the disc TIMs) shows
  the cutscene host rewrites every CLUT the `opdeene` bundle uploads,
  entry-for-entry, to `L = max(r,g,b) → (L, max(L-1,0), L>>1)` (5-bit, STP
  preserved; 0 mismatches across graded terrain rows 509/508/501, 768 entries),
  and collapses the loaded TMDs' authored colour packets to the amber family
  `~(M, 0.94M, 0.43M)`, while runtime-emitted neutral `0x80` ground quads stay
  neutral. Walking all render-node heads (`0x8007C34C..`) across the whole
  opening, node `+0x78` (`IR0`) is **0 on every node at every beat** - the
  per-node depth-graded-IR0 model is **falsified** (see
  [`re-do-not-re-walk.md`](re-do-not-re-walk.md#field--locomotion)). The ground
  divergence was the same law: retail binds the same green page / row-509 CLUT
  the engine binds, seen through the collapsed palette. Engine port
  `Renderer::set_palette_grade` (`palette_law_word` / `palette_collapse_prim`),
  staged by play-window when `World::scene_color_grade` is active; tableau
  ground lands `G/R 0.890` vs retail `0.88` (was `~1.07`). See
  [`cutscene.md`](../subsystems/cutscene.md#full-scene-sepia-grade-the-gold-prologue-look).
- **Far-geometry brightness (resolved-negative, grade `disassembly`+`capture`).**
  Matched-region measures: the tableau ground is identical both sides, but the
  retail spires/wings read `B/R ≈ 0.15..0.16` at brightness `~51` vs the engine's
  `0.27` at `~80`. This is **not** a missing separable palette/depth law. A
  signature scan for the collapse arithmetic across overlay 0970 (28 funcs), field
  0897 (690), and `SCUS_942.54` (945) finds **no CLUT-rewrite loop** - 0970 is pure
  MDEC/STR code, so the earlier "0970 load hooks are the candidate grade host" is
  **falsified**; the load-time CLUT rewrite is a table/DMA upload, not a pinnable
  CPU pass (same shape as the XA-clip-table writer). With `IR0 = 0` on every node
  and both grade halves reproduced, the residual gap is un-darkened neutral packets
  on lit-descriptor prims (the mesh builder feeds `0x80`, and
  `palette_collapse_prim`'s neutral guard leaves them alone) vs retail drawing those
  same prims through the scene GTE far/back colour `FUN_80029888` loads - opdeene's
  dim ambient `DAT_8007B788 = 0x00202020` vs town01's `0x00FFFFFF`. That GTE ambient
  is the port's standing **no-field-light-op boundary** (see
  [`re-settled-threads.md`](re-settled-threads.md#field-decoration-path---does-it-dispatch-the-ncc-light-handlers)),
  made visible only by opdeene's unusually dim ambient plus the port's lack of
  distance culling widening the sampled far region. Reproducing it faithfully would
  mean porting a GTE ambient/light op that contradicts that boundary, so no engine
  change was warranted.

### Overlay-loader index off-by-2 - remaining ripple

*Status:* resolved - slot A reconciled, per-spell summon identity capture-pinned across every block (player, evolved, flutes, enemy), engine mirrors updated

The overlay loaders (`FUN_8003EBE4`/`FUN_8003EC70` → `FUN_8003E8A8(param + 0x381)`) resolve against the in-RAM TOC at `0x801C70F0`, which is **raw `PROT.DAT` from byte 0** (byte-verified vs the `door_warp_town01_to_map01` state); the extraction index space slices entry starts 2 words higher, so the loaded entry is **extraction `param + 0x37F`** - every historical `param + 0x381` PROT attribution is 2 high. Slot A is fully reconciled (field 0897 = mode 2, battle 0898, menu 0899 = mode 22, STR-path 0969, cutscene 0970, debug menu 0971 = mode 0, the seven `0x3E` minigame slots, efect-test 0979 = mode 8 - each content/prologue-anchored; see [`boot.md`](../subsystems/boot.md)). Open:

1. **Per-spell summon-stager identity (slot B) - resolved; every id capture-pinned.**
   The whole player span `0x81..=0xA0` is one unbroken linear run
   (`extraction = spell_id - 0x79 + 895`, i.e. `903 + (id - 0x81)`) with no
   special-cased gap, and the enemy arm is pinned separately. Engine mirror:
   `engine-core::summon::summon_stager_prot_entry`. The detail below is kept
   because the method - reading loader-B out of catalogued states rather than
   live-probing - is the reusable part.
   The loader-B current-id (`gp+0x934` = `0x8007BC4C`) read straight out of the catalogued PCSX save
   states (no live probe - `scripts/pcsx-redux/match_prim_groups_to_disc.py::extract_ram` walks the
   gzipped-protobuf `.sstate` to the RAM blob): all three player-Gimard cast states
   (`gimard_summon_start` / `_visible` / `_burning_attack`) hold `id = 8` → **extraction 0903**,
   byte-confirming the `spell − 0x79` arithmetic for `0x81` across the whole cast (spawn window,
   steady-state render, attack move). The "0900 overwrites the stager mid-cast" concern does **not**
   ride loader-B on the player path (the id never moves off 8). The **enemy** Gimard "Fire Tail"
   frames (`battle_gimard_tail_fire_a/_b`, mednafen) instead hold loader-B `id = 5` → **extraction
   0900** - the enemy special pages the move-FX module, not a stager. Caveat: the id is a
   *last-load* tracker (an idle Begin/Run-menu state holds a stale `6`), so only in-cast states are
   evidential. The whole spell block `0x81..=0x8B` is now capture-pinned to `903..=913` (one mid-cast
   state per spell, zero exceptions; 0907 = Nighto, whose "Hell's Music" head title is the
   attack's display name - the dance-song / dual-use reading is refuted, the dance overlay has
   no slot-B loader callsite). The **whole high block `0x99..0xA0` is capture-pinned too**
   (one mid-cast mednafen state per cast, loader-B id read + the predicted entry
   byte-resident at slot B `0x801F69D8`): an Evil Seru Magic cast (spell id `0x99`,
   creature Juggernaut) drives id `0x20` → **0927** ("Dark Eclipse" is that attack's
   display name, the same pattern as Nighto's "Hell's Music"), the Sim-Seru summons
   Palma / Mule / Horn / Jedo (`0x9A..0x9D`) drive ids `0x21..0x24` → **0928..0931**, and
   the Ra-Seru summons Meta / Terra / Ozma (`0x9E..0xA0`) drive ids `0x25..0x27` →
   **0932..0934** (the untitled entries head with a pre-linked slot-B pointer table). The
   linear arithmetic (`loader = spell − 0x79`, `extraction = loader + 895`) holds across
   every pinned leg of both blocks. **The enemy arm is capture-pinned too** (six
   catalogued final-boss Cort mid-cast states): boss specials stream their own stagers
   through the same loader - Mystic Circle `0x2B` → **938**, Mystic Shield `0x2D` →
   **940**, Guilty Cross `0x31` → **944**, evolved-form Final Crisis / Ultra Charge
   `0x42`/`0x43` → **961/962**, and Cort's Evil Seru Magic `0x47` → **966**, *distinct*
   from the player-side Juggernaut stager 0927 - the player and enemy arms of the same
   spell ship separate stagers, and the enemy-special id band sits at `0x2B..0x47` →
   `938..966`. **Evolved-Seru block - resolved (10/10 capture-pinned).** All ten
   evolved-Seru entries (`0x8C..0x95` - Gola Gola / Mushura / …) → `914..923` trim to
   clean move-VM stagers (4..67 spawn sites; `EVOLVED_SUMMON_STAGER_PROT`, disc-gated
   `summon_overlay_block`), so the "they may be move-FX-path casts instead" alternative is
   falsified - they ride the stager mechanism, on the same `(id − 0x81) + 903` run as the
   base block. **Eight legs are now capture-pinned** by mid-cast states (loader-B id +
   slot-B residency; disc+library-gated `evolved_summon_binding`): `0x8C` Gola Gola → 914,
   `0x8D` Mushura → 915, `0x8E` Aluru → 916, `0x8F` Barra → 917, `0x92` Slippery → 920,
   `0x93` Iota → 921, `0x94` Puera → 922, `0x95` Gilium → 923, and the last two legs
   are pinned by *injected* casts (probe `autorun_evolved_cast.lua` writes the spell
   into the caster's record spell list + MP into record and battle-actor `+0x150`,
   then pad-scripts the cast; states `evolved_0x90_midcast` / `evolved_0x91_midcast`):
   `0x90` Kemaro ("Canine Fangs") → 918 and `0x91` Spoon ("Holy Eyes") → 919, each
   loader-B-id-confirmed mid-cast with the slot-B image a 100 % byte-match over the
   entry's full LBA footprint. Capture nuance the probe encodes: loader-B flips when
   the slot-B load is *queued*, so an at-flip save holds a partial image - the probe
   saves 90 frames after the flip, when both stagers are fully resident. A side pin
   from the injection: the battle Magic submenu reads the character-record spell list
   live, while the MP gate reads battle-actor `+0x150`. The two `0x4000`
   render-mode carriers (`0x8E → 916` Aluru, `0x93 → 921` Iota) are both pinned as player
   casts - so neither seats a live render-mode part (still the F-RENDERMODE blocker below).
   The attack-titled 0924 + 0925 are **capture-pinned as the rare-Seru flute summons**
   (states `flute_lippian_midcast` / `flute_spikefish_midcast`, probe
   `autorun_flute_cast.lua`): loader-B `0x1D`/`0x1E` mid-cast with the slot-B head
   byte-identical to the disc entry - **Lippian** (spell `0x96`; "Ultimate Rave" = the
   failed-kill banner, the landed kill shows "Ultimate Death") and **Spikefish** (spell
   `0x97`, attack "Blowfish"). They extend the *player* run `loader = spell − 0x79`
   unbroken (Gilium `0x95→923`, Lippian `0x96→924`, Spikefish `0x97→925`, unused
   `0x98→926`, Evil Seru Magic `0x99→927`) - the earlier "likeliest other enemies'
   specials" guess is refuted, and **0926** is the unused-`0x98` one-sector `jr ra` stub.
   SummonFlute items (effect classes 126/127) enqueue the spell id directly, so the
   flutes ride the same stager mechanism as Seru magic.
2. **The 0977 sub-id-5 minigame - resolved.** `0977` ("Ronginus") is the mode-24 case-5 **door/init** slot: the `0x801CEA6C` init prologue + the arena monster-name roster + `other6` dev paths. The Muscle Dome **match SM `FUN_801D0748` + all its data lives in the battle-action overlay (PROT 0898)**, not in `0977` and not in a separate aliasing overlay - the arena is a *mode of the battle engine* (fighters are battle actors, cards resolve through the battle-action path).
   Pinned by `asset overlay find-sig` of the controller prologue (`lui v0,0x8008; lw v0,-0x42dc(v0)` reading the ctx `_DAT_8007bd24`) → 0898 @ base `0x801CE818` file offset `0x1F30`, plus the deck/sub-draw/victory tables resolving in-overlay (`legaia_asset::muscle_dome::verify_resident`; the Duckstation `overlay_muscle_dome.bin` capture was that overlay's slot).
3. **Engine mirrors - resolved.** `OVERLAY_PROT_BASE` now carries the extraction-space `0x37F` (the engine host chain - `prot_one_shot_load` → `entry_start_lba_retail`, whose `toc` array starts at raw dword 2 - consumes extraction indices, so the raw `+ 0x381` loaded entries 2 high); `summon.rs` maps `0x81..=0x8B → 903..=913` directly. The constant's unit test documents the raw-vs-extraction shift.

## Adding a thread

A thread belongs here when:

1. There is something *specific* that would close it - a probe to run, a dump to read, a function to port. "Generally understand X better" is not closable; skip.
2. The next step is non-obvious from the code or git log. If `grep` would surface it, no row needed.
3. The detail lives elsewhere (a memory entry, a docs page, a Ghidra dump). The row is the pointer, not the analysis.

When the thread closes, rewrite the row to a `falsified` or `done - kept for reference` line if the path was instructive enough to warrant a "do not re-walk" marker; otherwise delete the row. Rotating the page is part of using it.

## Related pages

- [`re-settled-threads.md`](re-settled-threads.md) - the answered questions, each with an evidence grade. Check here before opening a hunt.
- [`re-do-not-re-walk.md`](re-do-not-re-walk.md) - the falsified hypotheses, reasoning intact.
- [`docs/tooling/port-catalog.md`](../tooling/port-catalog.md) - per-function dumped × documented × ported × ignored axes. `port-catalog.py --missing-ports` is the function-level companion to this page's question-level index.
- [`docs/reference/functions.md`](functions.md) - canonical function directory; the place to learn what a `FUN_<addr>` mentioned in a row actually does.
- [`scripts/ci/port-catalog-ignore.toml`](../../scripts/ci/port-catalog-ignore.toml) - addresses explicitly *not* worth investigating (statically-linked PsyQ infra). Disjoint from this page.
- [`docs/tooling/worklist-classification.md`](../tooling/worklist-classification.md) - classifies each `--missing-ports` row by whether it is a portable function entry at all. Read it before treating a bare address on the worklist as an open question: `INTERIOR`, `SHARED_TAIL`, `DUPLICATE` and `VA_ALIASED` rows are not work.
- [`docs/tooling/call-target-integrity.md`](../tooling/call-target-integrity.md) - why a decoded `jal` target is a property of the bytes, not the load base, and the one dump window whose targets are therefore untrustworthy.
- [`docs/subsystems/vm-inventory.md`](../subsystems/vm-inventory.md) - every VM-shaped subsystem with its op space, port status and whether anything live calls the port. Several rows on this page are questions about one of its entries.
- [`docs/tooling/ghidra.md` § decompiler artifacts](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims) - the seven C-rendering artifacts that have each already put a false claim into these docs. A `resolved` row whose evidence is decompiled C rather than instructions has not been audited against this list.
