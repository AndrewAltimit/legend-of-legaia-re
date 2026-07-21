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
| Super / Miracle Arts trigger logic | mostly resolved | [details ↓](#super--miracle-arts-trigger-logic) |
| First boss trigger -> Battle | resolved | The scripted-battle arm is the field-VM op `3E FF <formation_row>` ([battle.md](../subsystems/battle.md#scripted-battle-entry-3e-ff-row)): Zeto = garmel `P2[12]` row 9 (lone `0x4B`), Caruban = rikuroa stager `P1[3]` row 17 (lone `0x49`, `World::run_boss_stager_record`). `DAT_8007b7fc` closed: writer-less across `SCUS_942.54` + every static overlay (validated absolute + gp-relative + address-materialisation sweep); readers pin it as the debug forced-battle formation id - battle init `FUN_80055b6c` -> `FUN_8005567c` seeds the formation cells `DAT_8007BD0C+` from it, and `FUN_80046A20` routes a nonzero value to its mode-0 debug-menu exit. Retail never sets it. See [battle.md](../subsystems/battle.md). |
| Enemy-ally charm battle softlock | mostly resolved (cause pinned; one live repro left) | The freeze is the state-`0x5A` victory arm's party-slot assumption, **not** the `FUN_801E7320` reroll (falsified - [`re-do-not-re-walk.md`](re-do-not-re-walk.md#battle--arts--level-up)). The charm-victory widen (`0x801E6638` -> mask `0x384`) desyncs the wipe scan from the scheduler's `0x4` predicate, so a living charmed ally can be the acting actor at victory; the alive-skip at `0x801E6690` keeps the monster slot and `0x801E6770` indexes the 3-byte roster `DAT_8007BD10` OOB, arming a garbage win-pose stream. Chain + port: [battle.md](../subsystems/battle.md#enemy-ally-charm-at-the-end-of-action-gate-the-charm-battle-softlock). Open: live repro + disc-side rando fix. |
| Battle-actor `+0x16E` bit `0x400` applier (guard-disabling status) | open (low value; two consumers pinned) | Bit `0x400` of the battle-actor flags word `+0x16E` reads as a guard-disabling Sleep/Numb-like status; the sibling AI-delegated bits `0x380` are pinned (enemy-ally charm hook, `FUN_801E7320`). Two *consumers* of the `0x80..0x800` class are pinned live: the on-hit strip `0x801EDA60` in overlay 0898 (`andi 0xF07F` clears the class on any hit, so `0x400` is a volatile hit-cleared status) and the battle-exit per-party clear `0x80046EB0` in `FUN_80046A20`. The *setter* is still uncaught (no infliction in the wipe capture); probe `autorun_status_word_writer.lua` is armed for a status-inflicting battle state. |

### Super / Miracle Arts trigger logic

*Status:* partial

The find/replace matcher **is** ported (`legaia_art::{MiracleMatcher,SuperMatcher}`, applied by `legaia_engine_vm::battle_action::resolve_action_queue`).

**Miracle is now wired into the live player-driven Arts submenu**: `battle_arts::miracle_for_chain` flags a saved chain whose directional string is the caster's Miracle Art, and `World::build_battle_arts_rows` resolves the finisher-replacement queue into a per-strike profile (real `ArtRecord` power where staged, synthetic `x12` per component art otherwise).

**Super is now wired into the live submenu, with the queue connectors abstracted.** `legaia_art::recognize_art_sequence` tokenizes a saved chain's flat directional string into its ordered named arts (each identified by its own `ArtRecord::commands`), and `SuperMatcher::trigger_by_art_sequence` tail-matches that ordering against each Super's `SuperArt::art_sequence()` - the `find` pattern projected to art constants only (`[0x27,0x1F,0x27]` for Tri-Somersault), with starters + connectors stripped. `battle_arts::super_for_chain` / `World::build_battle_arts_rows` flag the row (`ArtRow::super_art`) and resolve the `replace`-queue strike profile (shared with the Miracle path).

**Byte-exact queue strings: closed (capture).** The other-13-Supers residue is
retired. The battle overlay keeps the full Miracle/Super trigger table resident,
and a live battle-RAM read (static-recomp endgame battle savestate, scene
`jou ene`, mode `0x15`) captured all of it: `0x801F64F4/6504/6514` = the 3
Miracle replacement strings (art-data.md's pinned VAs), byte-exact vs
`miracle.rs`; `0x801F6524` = 15 Super `find` entries (13-byte stride) in
`super_art.rs` order; `0x801F65E8` = 15 Super `replace` strings (16-byte
stride). All 30 are byte-identical to the modeled tables, and every replace
obeys `replace = find[..len-2] ++ [1A, finisher…]` (locked by test
`replace_preserves_find_prefix_and_finisher_tail`). So the combo-specific
connectors are established as **resident table data**, not derivation. The
byte-exact matcher (`SuperMatcher::try_trigger_at_tail`) is ported, exercised by
`resolve_action_queue`. See `docs/subsystems/battle-action.md` § "Miracle /
Super in the live player-driven Arts submenu".

**What (narrowly) stays open:** per-Super *live-executed* queue captures for the other 13 (driving each combo and watching the tail-replace at `actor[+0x1DF]`) - now purely confirmatory, since the resident strings, queue location, dequeue site (`0x801D89D8`), and two end-to-end executions (Noa Miracle + Vahn Tri-Somersault) are all pinned. The connector-emitting queue-builder function entry is still unpinned as a code address.

**Queue location pinned; Miracle path validated (capture).**
The action queue is the per-actor **`actor[+0x1DF..+0x1F2]`** action-parameter byte stream (not `ctx[+0x274]` - a capture showed that is the turn-order active-actor index written by `recompute_battle_order` `FUN_801DABA4`).
The directions/connectors encode as `0x0C/0x0D/0x0E/0x0F` = Left/Right/Down/Up, `0x1A` = `SpecialStarter`, `0x1B..0x32` = art constants.
A `battle_noa_miracle_art_combo` capture (probe `autorun_super_art_action_queue.lua`, runbook [`docs/tooling/super-art-queue-capture.md`](../tooling/super-art-queue-capture.md)) read Noa's resident Miracle queue and it matches `crates/art/src/miracle.rs`'s modeled replacement string **byte-exact** - runtime-validating the queue + `ActionConstant` encoding that were previously spreadsheet-sourced.
**Super path also validated:** a `battle_vahn_tri_somersault_super` capture read Vahn's resident Tri-Somersault queue (`…19 27 0F 19 1F 0E 1A 2B 2B 2B`) whose matched/replaced tail is **byte-identical** to `super_art.rs`'s `Tri-Somersault` `replace` - confirming the combo-specific connectors (`Somersault 0x27 → 0F`, `Cyclone 0x1F → 0E`) and the finisher tail. The dequeue site is pc `0x801D89D8`. The only residue is the other 13 Supers' replace strings (each a one-capture check through the same probe).

## Field / locomotion

| Thread | Status | What would close it |
|---|---|---|
| What transitions retail into game over? | resolved | Retail has **no** mode-`0x12` transition. A party wipe exits battle to mode 2; MAIN INIT `FUN_8003AEB0`'s back-from-battle arm (gated on the `DAT_8007BD60 & 0x80` survivor latch **and** story-flag idx 0 = the scripted-loss latch, raised by field-VM op `4C EA`) stores `game_mode = 0x16` (CARD INIT) with `_DAT_8007BB00 = 1` at `0x8003B5D4`, landing on the **title screen with CONTINUE preselected** - no GAME OVER art, no dedicated menu. Every store PC captured live (`autorun_gameover_mode_writer.lua`). Mode 18/19 + PROT 0902 confirmed an unreachable dev harness. The port's three-row session stays an engine invention. [details](../subsystems/battle.md#party-wipe--the-game-over-overlay) |
| Region story-flag gate families (record-header C1/C2 gates) | partial - structure mapped across the chapter-2/3 regions; play order for the dungeons the capture corpus never walked is still owed | [details ↓](#region-story-flag-gate-families) |
| Mid-visit NPC re-arrangement beats (dolk2 market crowd; garmel pre-Zeto staging) | open | dolk2: a mid-visit record parks the day cohort (prologue-seated at market tiles) and seats the crowd cohort P1[53..60] (bare idle-loop prologues, header parks) - the `dolk2_market_noa` capture holds the post-swap arrangement while its bank still has P1[2]'s `44 72` spawn latch `0x2FE` clear, so the swap runs through a different path than P1[2]'s own `SpawnRecord`. garmel: the pre-Zeto capture stands P1[3]/P1[4] beside the player; cold entry parks them. Closing either: a fresh-entry capture of the same scene (enter, save state before any beat fires), or tracing which record's choreography performs the seats. |
| Extraction-0874 §2 (`player.lzs`) F-variant pixels | mostly resolved (corpus-pinned) | The exact pause-menu-path writer PC; a `LoadImage`/draw trace would pin it. [details ↓](#extraction-0874-2-playerlzs-f-variant-pixels---pause-menu-lineage-not-boot) |

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
Writer-less gates worth a capture: `0x50A` (koin1 toggle) and `0x5D6` (koin4) — no clean script SET disc-wide even under the fixed `4C 0xE_` widths **and** the full-nibble decoder audit (all sixteen `0x4C` outer nibbles now decode; see [script-vm.md](../subsystems/script-vm.md) § width blindness), so the static path is exhausted: a reader-watch burst during a Karisto-castle walk is what settles them. Guarded by `man_variant_carrier_census_disc.rs::koin_gates_0x50a_0x5d6_remain_script_writer_less`, which fails loudly if a future decoder fix surfaces a static writer. (Nivora's `0x370` left this list — its writer surfaced statically under the pinned widths; see the Nivora Ravine row.)
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

### Extraction-0874 §2 (`player.lzs`) F-variant pixels - pause-menu-lineage, not boot

*Status:* mostly resolved (corpus-pinned; exact writer PC needs a GPU trace)

The earlier "a freshly booted game holds the `0xFFFF` variant" premise is **refuted** by a full save-catalog bisect: the title screen holds the band all-zero (not yet uploaded), and the new-game field-entry load (the mode-2 `FUN_80025B64` → `FUN_801D6704` stage) uploads the **disc** bytes - the whole intro chain through name entry holds `0x3333`. The F-variant is **pause-menu-lineage** instead: every pause-menu capture (CARD-mode init from field, six of six) holds it, the casino prize shop (also mode `0x17`, but hosted via the dialog/door path rather than the pause-menu init) holds the disc bytes, and the first battle **effect use** (not battle entry itself) restores the disc value.

The variant is exactly **3 words** of row 271 (`(853, 271)` `3333→ffff`, `(856, 271)` `3333→fff3`, `(857, 271)` `1e33→1e3f`), each equal to the disc word two rows down at `(x, 273)` - both row variants are consecutive rows of the *same* disc TIM (no sibling copy involved). Remaining residue: the exact pause-menu-path writer (a `LoadImage`/draw trace would pin the PC - low value; the oracle handles the band via the cross-scene shared-band refinement).

## Text / fonts / dialog

| Thread | Status | What would close it |
|---|---|---|
| Pause Items/Magic screens: remaining sub-flows | resolved (one capture-diff residual) | All four sub-flows traced from disassembly and ported: the **window-14 target panel** (`FUN_801D0520`; the preview modes are the permanent-stat Water previews, superseding the "HP-restore" reading), the **PAGE sprite** (UI-icon `0x76`), the **SCUS kind-4 list kernel** (`FUN_80032A44` + allocator `FUN_80030104`), and the **class-`0x80..0x82` Use routes** (submenus 0xA..0xD: single-target apply `FUN_801D8308`, Door of Light/Wind `FUN_801D8A58`/`FUN_801D8B90`, Incense `FUN_801D8D94`). Engine `engine-ui`/`pause_screens`. See [field-menu.md](../subsystems/field-menu.md#items-screen). Residual: which overlay path sets the row `0x800` dim bit across a focused Use-list page (one capture diff). |

## Audio

| Thread | Status | What would close it |
|---|---|---|
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
| New-Game opening chain + narration roller | partial (chain + caption + roller + prologue gold grade resolved; one far-geometry-brightness residual open) - the gold grade is a capture-pinned palette-space collapse, superseding the per-node depth-cue reading | [details ↓](#new-game-opening-chain--narration-roller) |
| Slot-B overlay cluster (`0900..0969`) per-entry identity | mostly resolved | [details ↓](#slot-b-overlay-cluster-09000969-per-entry-identity) |
| Overlay-loader index off-by-2 - remaining ripple | partial (core finding resolved; two stager bindings unpinned) | One mid-cast capture each for the attack-titled stagers 0924 / 0925 binds them to their casts. [details ↓](#overlay-loader-index-off-by-2---remaining-ripple) |

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
- **0924/0927** = attack-titled stager-shaped overlays ("Ultimate Rave" / "Dark Eclipse");
  loader callsites computed, action-id assignment open. **0957** summon-effect strings
  (**NOT** a dance song).


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

*Status:* partial - the chain, caption, roller, and prologue gold grade are resolved; one far-geometry-brightness residual below is open

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
of retail wall-time to within ~4 %, pinned by `opening_chain_wall_time`. **Residual:** the `map01`
fly-in leg now runs ~22 % long against a headless retail capture while the other three legs sit
within ~6 % - a per-leg thread in the world-map fly-in choreography, not a global rate.

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
- **Far-geometry brightness (open).** Matched-region measures: the tableau ground is identical both sides, but the retail spires/wings read `B/R ≈ 0.15..0.16` at brightness `~51` vs the engine's `0.27` at `~80`. The `B/R` direction is the law's integer floor at small `L`; the open question is why retail's far geometry draws that much darker. Also open: pinning the retail load-time asset-grade pass to a function (cutscene-host overlay 0970 load hooks are the candidates).

### Overlay-loader index off-by-2 - remaining ripple

*Status:* core finding resolved; per-spell summon identity + engine mirrors open

The overlay loaders (`FUN_8003EBE4`/`FUN_8003EC70` → `FUN_8003E8A8(param + 0x381)`) resolve against the in-RAM TOC at `0x801C70F0`, which is **raw `PROT.DAT` from byte 0** (byte-verified vs the `door_warp_town01_to_map01` state); the extraction index space slices entry starts 2 words higher, so the loaded entry is **extraction `param + 0x37F`** - every historical `param + 0x381` PROT attribution is 2 high. Slot A is fully reconciled (field 0897 = mode 2, battle 0898, menu 0899 = mode 22, STR-path 0969, cutscene 0970, debug menu 0971 = mode 0, the seven `0x3E` minigame slots, efect-test 0979 = mode 8 - each content/prologue-anchored; see [`boot.md`](../subsystems/boot.md)). Open:

1. **Per-spell summon-stager identity (slot B) - Gimard leg pinned from existing states; other ids open.**
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
   `938..966`. **Evolved-Seru block - resolved (8/10 capture-pinned).** All ten
   evolved-Seru entries (`0x8C..0x95` - Gola Gola / Mushura / …) → `914..923` trim to
   clean move-VM stagers (4..67 spawn sites; `EVOLVED_SUMMON_STAGER_PROT`, disc-gated
   `summon_overlay_block`), so the "they may be move-FX-path casts instead" alternative is
   falsified - they ride the stager mechanism, on the same `(id − 0x81) + 903` run as the
   base block. **Eight legs are now capture-pinned** by mid-cast states (loader-B id +
   slot-B residency; disc+library-gated `evolved_summon_binding`): `0x8C` Gola Gola → 914,
   `0x8D` Mushura → 915, `0x8E` Aluru → 916, `0x8F` Barra → 917, `0x92` Slippery → 920,
   `0x93` Iota → 921, `0x94` Puera → 922, `0x95` Gilium → 923; only `0x90 → 918` and
   `0x91 → 919` stay arithmetic-predicted (no mid-cast captured). The two `0x4000`
   render-mode carriers (`0x8E → 916` Aluru, `0x93 → 921` Iota) are both pinned as player
   casts - so neither seats a live render-mode part (still the F-RENDERMODE blocker below).
   The attack-titled 0924 "Ultimate Rave" + 0925 are likewise confirmed stager-shaped
   (arithmetic ids `0x1D..0x1F` under the enemy `895 + id` formula - likeliest **other
   enemies'** specials; one mid-cast each still pins the binding), while **0926 is a
   single-sector non-stager** (1 spawn site, 0 records - no real scene-graph there).
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
