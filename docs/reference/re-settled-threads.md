# Settled reverse-engineering threads

Reverse-engineering questions about Legaia's runtime that have an answer, with
the evidence that answer rests on. This is the archive half of the register:
the live hunts are on [`open-rev-eng-threads.md`](open-rev-eng-threads.md), and
the disproved readings on [`re-do-not-re-walk.md`](re-do-not-re-walk.md).

Read a row before starting work that depends on it. `resolved` is a claim, not
a warranty - which is what the evidence column is for.

## The evidence column

Every row is graded by what its own stated evidence actually rests on. Where a
row cites more than one kind, it is graded by the **weakest load-bearing**
claim, because that is the one that breaks the conclusion if it is wrong.

| Grade | The row cites |
|---|---|
| `disassembly` | Instructions, addresses, opcode encodings, branch or store sequences. The strongest grade. |
| `capture` | A runtime capture, save state, probe, firehose, or disc-derived oracle. |
| `decompiled-C` | Ghidra's C output, a `FUN_x(...)` call signature, a Ghidra label or plate comment, or a claim about store order / store count / a boolean operator with no instruction behind it. |
| `inference` | Reasoning from surrounding facts, corpus absence, or analogy, with no direct evidence cited. |

**`decompiled-C` is the re-audit bucket, not a wrong-answer bucket.** It marks
a claim nobody has confirmed against instructions. Most are probably right;
the point is that none of them has been checked, and every claim falsified in
the last audit wave would have graded `decompiled-C`. Three shapes carry most
of that risk - evidence citing a `FUN_x(a, b)` call signature or a
`funcs/<addr>.txt` dump rather than instructions; any claim about store
*order* or store *count*; and any claim about which boolean operator a
predicate uses. The artifact catalogue is
[`ghidra.md` § decompiler artifacts](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims).

`inference` is not weaker than `decompiled-C` so much as differently exposed:
an inference row usually says so out loud ("the structural rule supersedes the
snapshot", "no image claims them as functions"), and its failure mode is a
missing counter-example rather than a misread instruction.

## How a thread is laid out

Each area below opens with a table of one-line rows. A thread whose write-up
outgrew a table cell keeps its one-liner in the table and links to a `###`
section immediately after that table via **[details ↓]**; the full analysis -
every address, capture, and correction - lives in that section, under its own
*Status:* line.

---

## World map / kingdom bundles

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| World-map walk-view continent ground render | resolved | `capture` | [details ↓](#world-map-walk-view-continent-ground-render) |
| `DAT_8007C018[45..53]` mid-load vertex-pool pointers | resolved (structural) | `inference` | The liveness rule settles it without a snapshot: `DAT_8007C018[i]` is meaningful only for `i <= DAT_8007BB38` (the walker/install counter). Entries above the counter - which includes `45..53` in small field scenes like town01 - are stale carryover from prior game state, never dereferenced; there is no per-index "vertex-pool pointer" semantic. The historical "`[45..53]` = `(-6,-6)` vertex data" reading was a Drake mid-warp snapshot taken *past* the counter. The `field_load_first_town` state the probe would use was never actually captured (no file in the catalogue), so the structural rule supersedes it. |
| Field decoration path - does it dispatch the NCC light handlers? | resolved (no field light; depth-cue only) | `capture` | [details ↓](#field-decoration-path---does-it-dispatch-the-ncc-light-handlers) |
| Kingdom slot 4 - per-record semantic + consumer | resolved (read in place; `attr` render-unused) | `capture` + `disassembly` | [details ↓](#kingdom-slot-4---per-record-semantic) |
| MAN sections 2 and 5 - what do `_DAT_801C6EA0` / `DAT_80073EE0` carry? | resolved (both are place-name carriers) | `disassembly` + `capture` | Section 2's body is the **scene display name** the on-entry banner draws (and the save screen's location row); section 5, the universal zero-length terminator, leaves its pointer on the **world-map location table** the kingdom MANs trail - 29 records of `region + map x/y + discovery flag + 24-byte name`, walked by the label pass that draws each place's name at its map position. Together with the SCUS quick-travel cells that makes **three** independent carriers of one place name, which is why a rename has to edit all three. Layout + provenance: [place-names.md](../formats/place-names.md). |

### World-map walk-view continent ground render

*Status:* resolved - heightfield geometry + per-cell terrain-type-keyed multi-page texturing (tile=`+0x14`, page=`+0x15`, clut=`+0x16`), shipped in engine

**The continent ground is a procedural heightfield, not instanced meshes** - confirmed by **`FUN_80019278`** (SCUS, always-resident, no overlay aliasing): the bilinear ground-height sampler reads an entity's XZ, gates on the object-grid `0x1000` cell bit, and **bilinearly interpolates** the floor height from the 2×2 block of `+0x4000` nibbles (`grid[0],[1],[0x80],[0x81]`, each `& 0xf` → `DAT_1f80035c[nibble]` LUT, weighted by the sub-tile position, `>>0xe`). So the `+0x4000` grid is terrain elevation and the `0x1000` continent is a smooth heightfield surface.

**The slot-1 pack meshes are only the sparse placed landmarks** (`pool = record[+0x10] + prefix`, resolved 14/14 against the live render list via `FUN_8001ADA4` case 5 / `FUN_80024d78` / `FUN_80020f88`; spawned by `FUN_8003A55C`, gated on `flags & 0x4`, ~6 objects → pools 36/34/11/7/19/21). The `0x1000`-gated bulk cells are heightfield ground, not pack-mesh draws.

**`.MAP` source - raw (no compression):** the walk `.MAP` records+grid is a raw `0x10000` region at PROT.DAT `0x655800` (the loader's retail branch resolves it by PROT index `*(0x80084540) = 0x55 = 85` → `toc[87] = 3243` → `0x655800`; the per-entry extractor mis-slices it - its `0085_map01.BIN` count=46 pack at `0x668000` is the field object/script pack, and the real `.MAP` is under the overlapping manifest entry 83).

**Engine: heightfield geometry + grass texturing built** (`build_walk_heightfield` / `Scene::walk_heightfield` - quad per `0x1000` cell, corner Y from the `+0x4000` LUT; renders as coherent rolling terrain, verified vs disc).

**Ground texturing - per-cell multi-page atlas pinned and shipped:** the walk-view ground is per-cell `POLY_FT4` (cmd `0x2C`) quads, one `32×32` quad per visible cell, emitted in a row-major world-cell sweep. The texture is selected **per cell** from the cell's object-record `+0x14..+0x18` run: `+0x14` = `8×8` atlas tile index (`u=(id%8)×32`, `v=(id/8)×32`), `+0x15` = PSX `tpage` (the terrain VRAM page / type: `0x1A` grass, `0x0C` mountain, `0x1B`/`0x1C` water, `0x0B` forest), `+0x16..+0x18` = PSX `clut` word. Verified by aligning each quad run's UV→tile sequence to the `.MAP`'s `+0x14` grid (`scripts/ghidra-analysis/analyze-walk-ground-tiles.py --verify-rule`): tile/page/clut match the record **100%** across mountain + coast captures.

Engine bakes per-cell UV + `[clut,tpage]` in `build_walk_heightfield` (`WalkHeightfield::uvs` / `::cba_tsb`).

**Falsified:** (a) the "continent is per-cell instanced *meshes*" model - the bulk `0x1000` cells carry `+0x10 == 0`. (b) the earlier **"single `0x1A` grass page, positional `(col%3,row%3)`, `+0x14` unused metadata"** reading - a misread: grass cells use page `0x1A` with `+0x14` landing in the atlas's top-left `3×3` block, so the mod-3 sequence was coincidental; `+0x14` IS the tile selector and `+0x15`/`+0x16` carry the page/palette. (The static-decomp consumer sweep missed the per-cell terrain renderer, which is overlay-resident and aliased at `0x801F76xx`.) (c) A combined walk+overview mesh pool - 0085's and 0093's slot-0 atlases target the *same* VRAM pages, so they are mutually-exclusive sets that clobber each other if co-loaded.

### Field decoration path - does it dispatch the NCC light handlers?

*Status:* **resolved (no field light; depth-cue only)** - cold-boot `town01` field, `dirty_exec_hot`, ~46M interp hits, zero NCC

The per-prim dispatcher `FUN_80043390` owns four `NCCS`/`NCCT` **light** handlers (dispatch kinds 8..11: `FUN_8004409C`/`FUN_8004423C`/`FUN_80044434`/`FUN_800445B0`) - the ROM's *only* hardware-light code. The field object/decoration pass (`FUN_801F7088`, PROT 0900/0901) emits through `FUN_80043390`, so the field *could* dispatch them. A cold-boot capture settles that it does not.

**Deciding capture:** drove the recomp New Game → prologue (`opdeene`→`opstati`→`opurud`→`map01`) → live `town01` field, then `dirty_exec_hot` across idle + attempted walk (~46M interpreted instructions, 7 samples). Every sample's render band lands in the kind-19 bank-1 depth-cue body `FUN_80045584` `[0x80045584,0x800457C4)` (`DPCT`+`DPCS`), with **zero** hits in the kind-8..11 NCC band `[0x800445B0,0x80044798)` - in particular zero at the two light-op sites `NCCT` `0x80044724` and `NCCS` `0x80044750` (disassembled from the handler body). So the field renders through depth cue, not the light path: the "field shading is baked, no runtime light" model in `renderer.md` / `engine-render::psx_light` holds, and holds for the object path too, not just the TMD mesh path.

**The prior counter-signal, resolved:** a lone earlier `town01` capture (~31K interp hits) showed the kind-11 NCC body and the fog bodies hot in roughly equal measure. Against the ~46M-hit sweep's exact-zero NCC, that ~1500×-smaller window does not reproduce and is discounted as a transitional/mislabeled sample.

**Why the two instruments that looked like they'd ruled NCC out actually couldn't** (kept because they bite again): 
- *`gte_ring` is RTP/INTPL-only.* It records `RTPS`/`RTPT` (`gte_rtp_record`, func `0x01`/`0x30`) and `INTPL` (func `0x11`) - never `NCCS`/`NCCT`/`DPCS`/`DPCT` (`gte.cpp` record hooks). A GTE-ring "zero NCC" is vacuous; only `dirty_exec_hot` is a valid liveness probe here.
- *`fntrace` is blind to the handlers.* It only catches dispatcher round-trips; the SCUS render handlers are natively compiled + directly called, so even `FUN_80043390` records 0 fntrace hits while `fntrace_arm all` catches ~300k dispatches/s.
- *`map01` uses a different table.* The `map01`-class world map dispatches through the **replaced** table `0x801F8968` → the 0901 overlay's own leaves (`dirty_exec_hot` hot at `0x801F6E6C`, **not** the SCUS `0x8004xxxx` handlers), so its "no NCC" is a different-renderer fact, not a light-path test.

**Remaining caveat (narrow):** the sweep covered the Mist-era prologue arrival area, where Vahn's movement is script-locked, so it is effectively one viewpoint's worth of decorations; `map02`/`map03` and free-roam multi-screen towns are unreached (a free-roam sweep is blocked by the recomp savestate-load freeze - a saved town state reloads frozen at mode 0). The finding is robust for the sampled scenes but not an absolute proof that no town object anywhere is authored as a lit kind.


### Kingdom slot 4 - per-record semantic

*Status:* resolved - consumer pinned (slot-4 is read in place, no transcode), per-record semantic decoded, `attr` render-unused

The **consumer is fully decoded** ([`world-map-overlay.md`](../formats/world-map-overlay.md#cluster-a-internals)): `FUN_80043390` walks an 8-byte-header **command stream** (`kind` = bits 17–31, `count` = bits 0–15), tail-calling per-`kind` GTE primitive emitters (kinds 8–19 across 4 banks via the `0x8007657C` table; each reads two packed vertex indices per word `& 0x7FF8` into a vertex pool and emits a `POLY_F3/G3/G4/GT3/GT4` GP0 packet - dispatcher + the kind-12 flat-triangle handler spot-verified against `ghidra/scripts/funcs/{80043390,slot4_k12_bank0_80043658}.txt`).

**The handlers read the slot-4 RAM payload in place - there is no transcode.** A Drake warp capture (`scripts/pcsx-redux/autorun_slot4_source_map.lua`; 365 rows) shows 363 reads of the slot-4 window with the cluster-A GTE prim path (`0x80044C70 = lw …,0x10(a1); … andi …,0x7FF8`, the exact packed-vertex-index extraction) holding slot-4 pointers in `a1`/`a2` (`0x8011A608`, `0x80121614`, …), under return addresses `0x801F78D4` (the world-map top-view overlay renderer, 276 reads) and `0x8001BC8C` (SCUS render, 78). The streaming-chunk processor `FUN_8001E54C` fired only twice and on a non-slot-4 buffer (`0x80184BD0`). So the earlier "`FUN_8001E54C` distributes the slot-4 records into a working buffer the handlers walk" reading is **falsified**:
the slot-4 sub-body payloads *are* the command stream + vertex pool, walked directly. (The working-buffer writers the prior hunt saw - `FUN_80028158` at `0x801BA000` - are unrelated procedural meshes, as that hunt already found.)

**Cross-kingdom: confirmed.** The slot-4 resident base is byte-pinned for all three kingdoms (Drake `0x8011A624`, Sebucus `0x80119CE4`, Karisto `0x80108D84` - it varies per kingdom; `locate_slot4_base.py` matches the disc payload against a post-warp RAM dump, all bodies unanimous). Re-read against the correct Sebucus base, 171/177 of the Sebucus `slot4_source_map` reads land inside the verified window - in-place there too.

**Per-record semantic - decoded.** Each 8-byte record is a **GTE vertex**: the per-kind handler `FUN_80044c14` loads a record's two words into the GTE vertex registers (`VXYn = x | y<<16`, `VZn = z`) and `RTPT`-transforms them, so `x/y/z` are model-space coordinates (the parser's field layout is confirmed) and `attr` (the `VZn` word's high half) is **not** a coordinate. Each body is an object-local vertex pool; the triangle topology lives in a separate cluster-A command stream that indexes the pool by byte offset (`& 0x7ff8`). The transcode question is closed (there is none - the pool is read in place).

**`kind` + `attr` - characterized.** `kind` (1/2/4) tags a body's class/scope: hashing bodies across kingdoms shows `kind 1` = the three leading bodies, **byte-identical across all three kingdoms** (a shared universal mesh set); `kind 2` = full-3D kingdom objects (one cluster also globally shared, others shared between kingdom pairs); `kind 4` ⟺ `flag_a = 1` (widest-extent meshes). So slot 4 is a per-kingdom assembly from a shared mesh library + kingdom-specific bodies. `attr` is genuinely per-vertex (not per-group), **not** position-correlated (`corr ≈ 0.1`), varies smoothly across groups, and rides the unused `VZn` high half.

**`attr` has no reader anywhere in the dumped corpus.** Widening the search beyond the render family, the pool base flows only to the cluster-A GTE renderers; all 43 `>> 0x10` sites in that family extract a *command*-word vertex index, and each record's `z|attr` word is loaded whole into GTE `VZn` (high half never masked); `grep puVar[1]>>0x10` = zero hits. (Dump note: `ghidra/scripts/funcs/80059de4.txt` is mislabeled - its entry is `FUN_80059BD4`, a VRAM `LoadImage` DMA, not a slot-4 reader.)
**`kind`/`count` consumer - pinned.** A Read-watchpoint on body 0's header during the Drake
warp catches the cluster-A handler chain reading it **in place**: `ra = 0x801F78D4` (the
world-map renderer), PC `0x8004568C`/`0x800456F4` (`FUN_80045584`), record pointers also in
slot-4. The handler reads `count`/`kind` and `andi 0x40`-tests a header bit. So there is
**no separate command-stream builder** - each slot-4 body is a self-contained render packet
(header + indexed vertex records) walked in place (the `FUN_8001ada4` → `FUN_80058490`
candidate was falsified: `FUN_80058490` is a libgpu `MoveImage`). A full sweep of the
cluster-A handler family (`FUN_80043658`..`FUN_80045988`) confirms every `>> 0x10` is a
vertex-index extraction or output-packet write, none reading the pool `word1` high half.
So `attr` (real per-vertex data) is ignored by the entire world-map render path -
reserved/authoring data with no live consumer.

## Battle / arts / level-up

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| Encounter MAN sub-section layout | resolved (header shape corrected) | `disassembly` | [details ↓](#encounter-man-sub-section-layout) |
| Battle-intro tile shatter - the side-face shade page | resolved (a resident field asset, not a transition upload) | `capture` | [details ↓](#battle-intro-tile-shatter---the-side-face-shade-page) |
| Which stage-dome objects does the battle backdrop draw? | resolved (drop index 1, not "keep index 0") | `disassembly` + `capture` | The registration edits the object list rather than truncating it: each backdrop actor owns a private `0x9c` part table at `+0x44` (allocated at `0x80021184`), and `0x80051ad4..0x80051bac` applies one `count -= 1` plus one `entry[i] = entry[i+1]` shift from index 1 to each. Object **1** is dropped and everything else kept, gated on `_DAT_8007b64b == 0`. Indistinguishable from "draw object 0" on the two-object shells; on the seven four-object domes it keeps sky, mountains and the ground ring. See [battle.md](../subsystems/battle.md#object-1-is-dropped). |
| Does the battle ground grid roll per-cell randomness? | resolved (no - a four-entry table walk) | `disassembly` | No. `func_0x801d02c0` builds sixteen literal UV words into scratchpad `0x1f800034` (`0x801d0304..0x801d03a0`) and the emit loop reads group `n` for quad `n`, advancing `0x10` each time. They decode to four fixed 32x32 sub-tiles of the `(192..=255)^2` window walked in `sub_row * 2 + sub_col` order, copied into the packet verbatim - no roll, no corner mirror. The grid origin also carries an extra `-0x200` bias on `z`, and pass 1's cull is a view-`z` bracket with **no** screen-space term (that is a separate pass-2 test). See [battle.md](../subsystems/battle.md#the-grids-own-constants-read-off-the-emitter). |
| Endless camera orbit (Gaza 2 softlock) - the `0x19` attack-approach park | resolved (caught live; root-caused; disc fix shipped) | `capture` + `disassembly` | [details ↓](#endless-camera-orbit---the-0x19-attack-approach-park) |
| `0x19` fallback approach drive - which anim-driver field does summon staging leave stale? | resolved (pinned + causally reproduced on the parked save) | `capture` + `disassembly` | [details ↓](#the-summon-then-melee-park-trigger---the-stale-field-is-0x1dc-bit-2) |
| Super / Miracle Arts trigger chain | resolved (all 15 Supers live-executed) | `disassembly` + `capture` | [details ↓](#super--miracle-arts-trigger-chain) |
| Xain "Bloody Horns"/"Terio Punch" ignore elemental guards (community mystery) | resolved | `disassembly` + `capture` | Not an element drop - a **resist-ladder bypass**. Capture-class casts (spell byte `+0` = `'c'`) run per-spell modules (PROT 944..966) whose damage calls pass the caster's seat but pick one of two wrappers: `FUN_801DD4B0` (finisher `param_5=0`, resist ladder runs) or `FUN_801DD6B4` (`param_5=1`, the whole party-defender jewel/guard block is skipped). BH (952) / TP (953) use the bypass wrapper for their main hits; enemy ESM (966) uses the respecting one (hence Cort reads as Dark). Element attribution law + live confirmation: [battle-formulas.md](../subsystems/battle-formulas.md); cast classes: [spell-table.md](../formats/spell-table.md#cast-classes-record-byte-0). |
| First boss trigger → Battle | resolved | `disassembly` | The scripted-battle arm is the field-VM op `3E FF <formation_row>` ([battle.md](../subsystems/battle.md#scripted-battle-entry-3e-ff-row)): Zeto = garmel `P2[12]` row 9 (lone `0x4B`), Caruban = rikuroa stager `P1[3]` row 17 (lone `0x49`, `World::run_boss_stager_record`). `DAT_8007b7fc` closed: writer-less across `SCUS_942.54` + every static overlay (validated absolute + gp-relative + address-materialisation sweep); readers pin it as the debug forced-battle formation id - battle init `FUN_80055b6c` → `FUN_8005567c` seeds the formation cells `DAT_8007BD0C+` from it, and `FUN_80046A20` routes a nonzero value to its mode-0 debug-menu exit. Retail never sets it. See [battle.md](../subsystems/battle.md). |
| Enemy-ally charm battle softlock | resolved (both tracks fixed) | `disassembly` | The state-`0x5A` victory arm's party-slot assumption OOB-indexes the win-pose roster `DAT_8007BD10` (via `0x801E6770`) when a living charmed ally is the acting actor at monster-wipe victory - the `FUN_801E7320` reroll theory is falsified ([`re-do-not-re-walk.md`](re-do-not-re-walk.md#battle--arts--level-up)). Fixed on both tracks: engine `victory_pose_fixup`/`charm_widen`, and the disc-side `legaia_patcher::charm_fix` guard - a single-word detour at the `0x801E6690` keep-branch into a SCUS dead-space liveness guard. Full chain + port: [battle.md](../subsystems/battle.md#enemy-ally-charm-at-the-end-of-action-gate-the-charm-battle-softlock). |
| Battle-actor `+0x16E` bit `0x400` applier (guard-disabling status) | resolved - exhaustive negative | `disassembly` | Bit `0x400` has **no retail setter**: a word-level decode of `SCUS_942.54` + every static-overlay image (all stores covering `+0x16C..+0x171`, pointer precomputes, `ori`/`sllv` bit-set shapes, the `+0x6F6` mirror, the `+0x21F` deferral) finds only clears - accessory cure `FUN_8004CE2C`, the per-round RNG waker `FUN_801F45A4`, item cures, the on-hit strip, battle-exit. The appliers (hit leg `FUN_801EC3E4`, cast leg `FUN_801E09F8`) map kinds 3/4/5/6 → `0x1`/`0x2`/random-`0x38`/`0x1000`, kinds 1-2 → the `0x380` deferral; none reaches `0x400`. Latent content. Writer inventory: [battle.md](../subsystems/battle.md#the-0x16e-status-halfword---retail-writer-inventory). |
| Who calls the battle on-screen test `FUN_8005126C`? | resolved - exhaustive negative | `disassembly` | [details ↓](#who-calls-the-battle-on-screen-test-fun_8005126c) |
| What spawns the battle XA voice selector `FUN_8004DA00`? | resolved | `disassembly` | Nothing calls it - it is the `+0x08` tick of the [static actor template](functions/runtime-libs.md#static-actor-templates) at `0x800767F4`, and the battle scene-loader `FUN_800513F0` spawns that record into the system actor pool at `0x80051D3C` as its last act before returning. The selector is therefore a per-frame pass resident for the whole battle. Port `legaia_engine_audio::battle_voice`; the same reading corrects the template's base and tick offset (the `+0x0C`-from-`0x800767F0` frame was skewed 4 bytes low). |
| Effect-VM pass-1 "state token algebra" (`FUN_801E0088`) | resolved + ported | `capture` | [details ↓](#effect-vm-pass-1-state-token-algebra-fun_801e0088) |
| Seru-magic summon visual (e.g. Tail Fire) | resolved (player visual; wired) | `capture` | [details ↓](#seru-magic-summon-visual-eg-tail-fire) |
| `summon.dat` / `readef.DAT` side-band streaming | resolved (entries + format) | `disassembly` | [details ↓](#summondat--readefdat-side-band-streaming) |
| Monster steal item (Evil God Icon) | resolved | `capture` | [details ↓](#monster-steal-item-evil-god-icon) |
| Battle face-stamp issuing site | resolved | `capture` | [details ↓](#battle-face-stamp-issuing-site) |
| Per-spell magic power / multiplier | resolved (mechanism + roll ported) | `disassembly` | [details ↓](#per-spell-magic-power--multiplier) |
| Arts command sequence - independent source | resolved | `capture` | The SCUS arts-name table (`DAT_80075EC4`) glyph string is byte-exact ground truth for every art's directional command; `legaia_art::ArtsOracle` exposes it, and disc-gated contract tests validate both the best-effort PROT `0x05C4` `parse_record` command-decode and the curated gamedata `directions`/`ap` columns against it (one documented walkthrough error: Hyper Elbow). |
| Weapon-specialty arm width (off-class widens the Arms command) | resolved | `capture` | Not a runtime favored-class comparison. The arm command's AP cost is a per-(character, weapon) byte in the player battle file, at the weapon section's swing record (`section[+0x04]`) `+0x74` (favored `0x1E` / off-class `0x2A` / far `0x36`); LZS-decoded and copied verbatim into the runtime gauge (`DAT_801C9360[char][0x0C]+0x74`) at battle load by `FUN_800557B8`, read by gauge builder `FUN_801D388C` case 9. Byte-validated across all three player files; randomized by `legaia_patcher::weapon_specialty`. See [`docs/subsystems/arts-command-gauge.md`](../subsystems/arts-command-gauge.md). |
| Stat growth-rate source | resolved (validated + wired; core + opt-in jitter) | `capture` | [details ↓](#stat-growth-rate-source) |
| Character-record HP/MP/AP pair order (`+0x104..0x110`) is `(max, cur)` | resolved (relabeled throughout) | `disassembly` | [details ↓](#character-record-hpmpap-pair-order) |
| Monster stat-record archive source | resolved | `capture` | [details ↓](#monster-stat-record-archive-source) |
| Monster mesh + texture pool | resolved | `capture` | [details ↓](#monster-mesh--texture-pool) |
| Terra slot-3 / story-flag overlap | resolved | `capture` | [details ↓](#terra-slot-3--story-flag-overlap) |
| Battle party mesh pack `other5` = **PROT 1204** (battle form; Baka Fighter reuses it) | resolved (empirical) | `capture` | [details ↓](#battle-party-meshes--assembled-from-the-player-battle-files-prot-1204--baka-fighter--default-equipment-sibling) |
| MP-cost ability-bit priority (half vs quarter) | resolved (dump-confirmed) | `disassembly` | [details ↓](#mp-cost-ability-bit-priority-half-vs-quarter) |
| Scripted Tetsu encounter → Battle (v0.1 oracle Battle leg) | resolved | `capture` | All three residuals are now derived from disc bytes: the formation-row selection is the standard scripted-battle op `3E FF 04` in `P1[10]` (same case-`0x3E` install arm as Zeto/Caruban; row 4 = lone Tetsu), the sparring-partner reposition is `P1[10]`'s `4C 51 15 0E 07 22` NpcRun→tile `(21,14)` = `RIM_ELM_SPARRING_CARRIER_TUTORIAL_POS` exactly, and the spar Yes/No is a MES-embedded option picker (`0x29` open + N×2 signed relative-jump table, handler `FUN_80038050`; port `legaia_mes::Picker::jump_target` + `InlineDialogueRunner::last_choice`), not a field-VM opcode. [details ↓](#scripted-tetsu-encounter--battle-v01-oracle-battle-leg) |
| Battle stage backdrop: which `scene_tmd_stream` a scene fights in | resolved | `capture` | A scene bundle carries one stage stream per sub-area, and the battle's is not uniformly the block's first - `map01` uses bundle slot 5 (entry 88), Rim Elm `town01` slot 6 (entry **7**). Engine `ProtIndex::battle_stage_entry_for_scene`. [details ↓](../subsystems/battle.md#which-stage-stream-a-scene-fights-in) |
| Battle stage backdrop: is the authored half completed, and how | resolved (two actors; per-stage transform; object 1 dropped) | `capture` + `disassembly` | `FUN_800513F0` registers the shell TMD once and allocates **two** actors from it (`ctx+0x106C` / `+0x1070`), both drawn. Copy B takes a half turn unless the stage is on the `DAT_80078B50` table, which mirrors it in X instead. Confirmed live in 15 battle saves: both pointers non-null and distinct, object lists identical, the split matching the table every time. Retracts "drawn once, so nothing completes it" ([re-do-not-re-walk.md](re-do-not-re-walk.md#the-backdrop-shell-is-drawn-once-so-no-completion-exists)). [details ↓](../subsystems/battle.md#backdrop-shell---two-copies-of-one-mesh). |
| Battle-stage overlay band (`+0x47`) | resolved | `disassembly` | `FUN_800520F0` pages a per-stage slot-B overlay via `FUN_8003EC70(_DAT_8007B64A + 0x47)`, skipped when the id is `0` (which every catalogued battle but the Tetsu tutorial reads). Engine `engine-core::overlay_loader::battle_stage_overlay_entry`. [details ↓](../subsystems/battle.md#stage-overlay-dispatch-the-0x47-loader-band) |
| Battle-intro tutorial boxes (Tetsu sparring fight) | resolved (machine pinned, ported and wired) | `disassembly` (exclusivity `inference`) | The prompts are resident in stage overlay 967, so porting the battle SM alone could never emit them - though "**only** in 967" is corpus-exhaustiveness, not an instruction claim, and is graded separately. `FUN_801F6B70` is a 91-entry jump-table hook on the flow-state byte `ctx[+0x06]` with just **nine** live slots; each switches on `ctx[+0x28A]` - the battle-mode counter, read here as a lesson index - making the script a `(state × lesson)` cross-product. Port `engine-core::battle_tutorial` reads the prompt text off the user's disc. [details ↓](../subsystems/battle.md#the-sparring-tutorial-prompt-machine-overlay-967) |
| Battle command-flow byte `ctx[+0x06]` | resolved | `disassembly` | The *other* battle SM - `FUN_801D0748`, the menu half, distinct from the action SM's `ctx[+0x07]` and overlapping its value space. Its selection band is regular decimal tens `30..120` (turn prompt / category menu / escape / item / magic / arts entry / target / target confirm / commit / attack-mode), which is what identifies it as the tutorial hook table's key: the nine live hook slots are that band minus the magic window. Engine mirror `engine-core::battle_flow`. [details ↓](../subsystems/battle.md#the-command-flow-byte-ctx0x06---what-the-hook-table-indexes) |
| Action-SM state `0xFF` treated as battle end by the port | resolved (path was live-reachable; port fixed) | `disassembly` | [details ↓](#action-sm-state-0xff-treated-as-battle-end-by-the-port) |
| Spine flag `0x142` (Caruban beat / dolk-dolk2 switch) writer | resolved (disc writers + engine port + oracle) | `capture` | [details ↓](#spine-flag-0x142-caruban-beat--dolk-dolk2-switch-writer) |
| Spine flag `0x482` (Drake mist-wall) writer | resolved (writer-less; "direct code path" presumption falsified) | `capture` | [details ↓](#spine-flag-0x482-drake-mist-wall-writer) |
| CDNAME scene-window frame (`raw = extraction + 2`) in `Scene::load` | resolved (engine converts; misattributions corrected) | `capture` | Engine scene windows used raw-TOC defines as extraction indices - two entries late, dropping each block's first two retail entries and bleeding in the next block's. Corrections that fell out: the `.MAP` is the retail block's FIRST entry (not "two below"); "suimon == dolk2 MAN" and "rikuroa MAN = [18,70,20]" were next-block sidecars under the wrong label; "urudre1 tests 0x15E" and "0x63A has no writer" are falsified; "0x1BE = rikuroa Zeto gate" was geremi's arrival one-shot. Head blocks (defines 0/1, inside the TOC header rows) keep legacy windows. See [cdname.md](../formats/cdname.md#numbering-space). |
| Motion-VM (`FUN_80038158`) bytecode carrier + flag census | resolved (carrier pinned; spine flags negative) | `capture` | The second motion VM's bytecode source is **MAN tail-section 1** (installer `FUN_8003A9D4`; parser `legaia_asset::man_motion`; layout + op table in [`motion-vm.md`](../subsystems/motion-vm.md#the-second-motion-vm---fun_80038158)). Disc-wide op-7/op-8 census (`--motion-flag-census`): overworld walking-band choreography + one `town0b` clear; `0x142`/`0x482`/`0x1BE` and `549` appear in NO stream - the "549 set by op-7 bytecode" carrier claim is **falsified**. Anchor test `motion_flag_census_disc.rs`. |
| Debug-menu "STR trigger teleports + sets flags" mechanism | resolved (no per-FMV event table; dev-menu tools explain it) | `disassembly` (two sub-clauses `inference`) | The two direct `_DAT_8007BA78` store sites (op `4C E2` at `0x801E30F4`, title tick at `0x801DDCE8`) are corpus-exhaustive over a raw-byte sweep of 1,248 files: 24 hits, 6 distinct sites, zero in SCUS. Two stated limits - the sweep cannot see code inside an LZS section, and `fmv_dispatch` decodes only 20 of each slot's 32 bytes. The teleport+flag application is the 0897 dev-menu toolset (warp appliers + the EVENT FLAG editor `FUN_801dbd04`); those corpus states came from its register-pointer editing, invisible to static scans. Do not re-walk the "per-FMV event table" shape. See [cutscene.md](../subsystems/cutscene.md). |
| Spawned-record player-channel (`0xF8`) ExecMove/HaltAcquire handshake | resolved (engine completion model) | `inference` | The timeline stepper models the handshake directly: `A2 F8` ExecMove arms an in-flight countdown (`CutsceneTimeline::player_move_frames`) and `C3 F8` HaltAcquire parks at the op until it drains, then steps past by encoded width (`resolve_target` keeps its `None` contract for `0xF8`) - so door-cutscene records reach their trailing `0x3F` and driven hops land (`jou`→`jouina`, and the full castle chain to `jouinc`, in `chapter1_hub_depth_oracle.rs` part J + `chapter1_hub_breadth_oracle.rs` part F). See [cutscene.md](../subsystems/cutscene.md) § player-channel completion. |
| Equipment stat-bonus table - slot model | resolved (slot model + passives) | `disassembly` | The stat-bonus table (`DAT_80074F68`, 8-byte stride) is decoded from `FUN_801CF650`/`FUN_801CF5D0` (`legaia_asset::equip_stats`): `+0`=INT, `+1`=ATK, `+2`=UDF, `+3`=LDF, `+4`=SPD (the earlier AGL/evasion reading is falsified). Five `lbu`/add pairs at `0x801CF6C0..0x801CF72C`; note the asymmetry that rules out a linearised-C reading - `equip+0` lands on the *last* accumulator, out of sequence with `+1..+4`. AGL takes no equipment add at all. The four `+7` categories are Legaia's four weapon/armour slots (body/head/footwear exact by name; none of the 77 accessories appear in this table). Wired: `DiscEquipInfo` gates `EquipSession`'s per-character list. |
| Flag `0x63A` - the vell/vozz `P2[7]` gate with NO script writer | resolved (script writers exist; the "no writer" premise was the CDNAME +2 skew) | `capture` | [details ↓](#flag-0x63a---the-vellvozz-p27-gate-with-no-script-writer) |
| cave01 `P2[16]` (the `0x15D` entry-key setter) - what spawns it | resolved (slot-counted spawn chain) | `capture` | [details ↓](#cave01-p216-spawner---the-slot-counted-interact-chain) |
| Drake Castle deep interiors (`jouinc`/`jouind`) depth decode | resolved (door-choreography families, not story gates) | `capture` | [details ↓](#drake-castle-deep-interiors-jouincjouind-depth-decode) |
| `scene_destinations` P1-table scan misses P2-only door names | resolved (P2 pass folded in) | `capture` | The P2-only class is the town/dungeon **exit door** (a P2 door-choreography record): `town01`→`map01` (Rim Elm's overworld exit; the P1 pass alone sees *zero* town01 destinations), `retockin`→`retona`, `geremi`→`map02`/`tower` - 13 scenes / 14 destinations disc-wide. The suspected `jouinb`→`jouina` exemplar is falsified: it is P1-visible (the over-walk resyncs across that record). Merged kernel `legaia_asset::man_edit::scene_destinations` (P1 pass as prefix + clean-gated P2 pass, `(name, index)` dedupe); the engine delegates to it; disc pins `scene_destinations_p2_disc.rs`. |
| `0x4C 0x51` byte `+3` = `[bit7 special-model \| facing nibble]` vs the glide-speed interim `depth & 7` reading | resolved (facing wins; the two readings were two different ops) | `disassembly` | [details ↓](#0x4c-0x51-byte-3-reconcile---facing-wins-no-motion-bytecode-synthesis) |
| How an NPC's facing changes **after** spawn - snap vs ramp, and which writer wins | resolved (two laws; order-of-execution priority) | `disassembly` | [details ↓](#npc-dynamic-facing---two-laws-and-an-execution-order) |
| dolk2/rikuroa MAN source (the "v12-embedded MAN" was an over-read) | resolved (streaming carrier) | `capture` | Their own `base+3` bundles are the MAN-less count=4 form `[1,2,6,0x14]`; the "embedded MAN at 0x1000" inside their SceneV12Table entries is an over-read onto the next scene's bundle (suimon's / geremi's; [scene-v12-table.md](../formats/scene-v12-table.md) § over-read). Retail sources their partition scripts from the block's standalone `data_field_streaming` entry's type-3 chunk (`dolk2` ext 70 `[29,73,17]`, `rikuroa` ext 157 `[13,29,64]`; live script-heap byte-match at the Caruban beat). Engine: `field_man_payload` streaming fallback (`streaming_man_payloads`) + retail-frame `Scene::load` windows; pins `v12_bundle_man_disc.rs`. |
| kor-family op-0x49 flag window `[0x138..0x13F]` - what the 8 flags gate | resolved (Uru Mais warp-pad destination memory) | `disassembly` | [details ↓](#kor-family-op-0x49-flag-window-0x1380x13f---uru-mais-warp-pad-picker) |

### Battle-intro tile shatter - the side-face shade page

*Status:* resolved - a resident field asset, not a transition upload; the style draws.

The 4bpp page at VRAM `(448, 0)` the shatter's four semi-transparent side
faces stretch over is the top-left `64 x 64` texel corner of
`legaia_asset::field_char_textures` **entry 0** (PROT 0874 §2): a `256 x 256`
4bpp TIM whose declared destination is `(448, 0)`, uploaded at field init and
resident for the whole field session. `clut 0x7641` decodes to `(16, 473)` -
CLUT index 1 of the same entry's 16-CLUT block, landed as a `256 x 1` strip on
row 473: a black-to-bright, STP-set brightness ramp. `tpage 0x0027` carries
ABR mode 1, so the side faces **add** the ramped texels over their opaque
siblings - a glint cut from the resident player-texture page, not a dedicated
transition asset.

Pinned by a scripted mid-transition capture
(`scripts/pcsx-redux/autorun_tile_shatter_page.lua`: walk the
`karisto_sol_pre_encounter` state into a random encounter, exec-break on the
style-2 tick `FUN_801D0D24`, write save states on shatter frames 1 / 8 / 24,
and log every `LoadImage` / `MoveImage` rect): the `(448, 0)` rect and the
row-473 CLUT are byte-identical to the pack entry before the encounter,
mid-shatter, and across two different field scenes - and **no upload touches
them in the transition window**. The earlier "live only during a transition /
sparse in a battle-load state" framing was battle VRAM layout misread as
sparseness.

The same capture pins the emitter's remaining runtime inputs: the per-tile
view matrix at scratch `0x1F8003C8` is identity rotation with **zero**
translation from the second shatter frame on (frame one still holds the field
camera's last value, so every tile projects behind the near plane and retail's
first frame draws no tiles - the `_DAT_8007B6CC` "not the first frame" flag is
that same signal); the FT4 handler's near cutoff `0x1F80037E` reads `0x10`;
and `ZSF4` is `0x400`, so a primitive's OT depth is the plain four-corner SZ
average. Full spec + engine wiring:
[`cutscene.md`](../subsystems/cutscene.md#what-style-2s-emitter-builds).

### Who calls the battle on-screen test `FUN_8005126C`?

*Status:* resolved - **nobody**, and the same holds for two of its neighbours.

The question was framed as "which draw pass consults the verdict, and what
does it do with a `0`", because the port draws every battle body every frame
and a cull could not be wired without knowing. The premise was wrong: there is
no consumer to find.

A five-form sweep of `SCUS_942.54`, all statically based overlay images and
the raw bytes of every extracted `PROT.DAT` entry finds **no reference to
`0x8005126C` at all** - no literal address word (so it sits in no dispatch
table and no actor template), no `jal`, no `j`, no PC-relative branch, and no
`lui`+`addiu` materialisation
([`address-reference-scan.md`](../tooling/address-reference-scan.md)). The same
sweep returns the same nothing for the passive-name draw `FUN_80035274` and
the angle tween `FUN_80050D40`; each follows a clean `jr ra` epilogue whose
delay slot closes the previous frame, so they are entry points rather than
interior labels. Two of the three then open frames of their own - `FUN_80050D40`
is a frameless leaf, which is a shape rather than a counter-example.
`FUN_80025054` is the same finding one level up - it is a template tick, and
the template record `0x80070614` that would install it is what nothing
materialises. Its table was swept whole, because a record reached as
`base + index` would not be a materialisation pair: the head `0x800705FC` is
named once, in the field overlay, and handed straight to the allocator as one
record rather than indexed.

The scan is only worth its negatives if its positives hold, so it was run
against known answers first: the template word for `FUN_8004DA00` at
`0x800767FC`, the 21 `jal` sites of the billboard projector `FUN_800195A8`, an
intra-function branch target inside `FUN_8005126C` itself, and the menu
overlay's documented sub-screen pointer table at `0x801E4F40`. Two limits bound
the claim: an LZS-compressed PROT entry would hide a reference (overlay *code*
is stored raw, so this does not cover the images callers live in), and an
address assembled in more than two instructions is not a `lui`+`addiu` pair.

Consequence for the port: `battle_on_screen` stays inert on purpose, and the
three worklist rows are unreachable retail code rather than pending work - the
write-ups are in
[`battle.md` § Unreferenced SCUS entry points](functions/battle.md#unreferenced-scus-entry-points),
and the rows themselves are settled under the ignore list's `unreferenced`
section rather than by code
([`worklist-classification.md`](../tooling/worklist-classification.md#the-reachability-claim)).
The one row the same sweep *did* settle positively is `FUN_8004DA00`, whose
spawner is the battle scene-loader `FUN_800513F0`.

The same sweep run over every *disclosed inert* port anchor - each one ported,
unreachable in the engine, and disclosed as such - separates the rows waiting
on wiring from the rows waiting on nothing. Almost all are waiting on wiring;
the closed list of those that are not, SCUS and overlay alike, is on
[`address-reference-scan.md`](../tooling/address-reference-scan.md#the-retail-unreachable-set).

### Action-SM state `0xFF` treated as battle end by the port

*Status:* resolved - the retail half was already graded `disassembly`; the
port-side reachability question is settled (the path **was** reachable in a
live battle) and the port is fixed.

Retail `0xFF` is the **round boundary**: its only writer is the non-wipe arm
of the `0x5A` end-of-action gate, and wipes signal through
`DAT_8007BD71 = 0xFE` without writing a state byte
([battle-action.md](../subsystems/battle-action.md#0xff-is-the-round-boundary-not-the-battles-end)).
The engine port mapped `0xFF` to a `battle_end(BattleEndCause::MonsterWipe)`
terminal instead, and the open question was whether a live battle reaches it.

It does, by concrete trace through `engine-vm`'s own accumulation logic:

- `Begin` stamps the acting actor's counter (`actor[+0x1A]`,
  `BattleActor::action_queue_counter`) from `ctx.queued_action`, which the
  engine's arming paths set to `3`;
- the `0x5A` gate's non-wipe arm bumps it and compares against
  `party_alive + monsters_alive`, so `3 + 1 = 4 >= alive_total` in any battle
  with four or fewer living combatants (3 party + 1 monster, or later rounds
  of a larger fight);
- the gate is dispatched whenever a driver leaves the SM parked at
  `EndOfAction` across a tick - which the live loop does after a folded
  monster spell cast and after a Sleep/Stone skipped turn (a repeatedly
  casting monster's never-restamped counter also walks `1, 2, 3, ...` up to
  the same threshold).

Symptom: a spurious victory - loot and XP granted - after one round with both
sides standing. The fix renames the state to `ActionState::RoundEnd`, whose
handler clears every actor's acted counter and hands control back through
`EndOfAction` (the state the arming driver keys the next turn on); the retail
`0xFF` body (`ctx[+0x28A]` round bump, `FUN_801F45A4` settle) already runs
host-side in `engine-core`'s live loop. `battle_end(..)` now fires only from
the paths that raise retail's `0xFE` signal: the `0x5A` wipe arms and the
escape teardown `0x66`. Regressions: engine-vm
`full_round_with_both_sides_alive_does_not_end_the_battle`, engine-core
`round_boundary_state_is_not_a_spurious_victory`.

### Endless camera orbit - the `0x19` attack-approach park

*Status:* resolved - the park was caught live from ordinary play, the walk-skip
condition is named from the disassembly and confirmed against the parked save,
and a one-word disc fix ships as `legaia-patcher --approach-softlock-fix`.

*Evidence:* `capture` (the fingerprinted scenario `battle_gaza2_park_0x19`,
caught by a human playing under the poll-only dynarec-speed hunter
`autorun_gaza2_park_hunter.lua`; interpreter replay
`autorun_gaza2_range_wedge.lua`; RAM-table read of the parked save) +
`disassembly` (`overlay_battle_action_801e295c.txt`, `0x801E31F4..0x801E32DC`).

The community-reported "endless camera orbit" (Gaza rematch; JP exhibit too) is
the battle-action state machine parking while the idle camera azimuth sweep
(`FUN_801D0748`) keeps orbiting - the orbit is pure symptom. The park: state
`0x14` (attack approach setup), finding the target out of range, looks up the
**walk animation** (action tag `0x20`) in the acting monster's action table via
`FUN_80050E2C`; when the table has no such action - bosses generally never
walk; Gaza's 12-action table reads tags `[00 01 02 03 04 05 0B 0E 13 0C 23 23]`
in the parked save; the tag-`1` "Move" float loop exists but is only played
inside the walk chain the `0x20` gate protects - the fallback stages it and drops straight
into state `0x19`, the range re-poll, **whose SM arm has no movement code and
no timeout** (its not-in-range edge only bumps `ctx[+0x6D4]`, whose sole
reader is the arms-resolver roll, not a limit). Position captures of the same
fight show the fallback normally still approaching *during* `0x19` (~19
units/vsync, driven from the staged Move clip's playback, not the SM); in the
caught parks the drive dies ~12 vsyncs in (anim pair back to `0/0`, frozen
beyond reach), so the fight waits forever on an attack that can never
connect. The trigger is reproduced - a summon immediately followed by the
boss's melee (scenario `battle_gaza2_park_0x19_summon_melee`) - and the
anim-driver field the staging round-trip leaves stale is pinned: actor
`+0x1DC` bit 2, the exit-to-idle anim event flag ([details
↓](#the-summon-then-melee-park-trigger---the-stale-field-is-0x1dc-bit-2)).
The fix is indifferent to it. Full anatomy + fix + engine-port note:
[battle-action.md](../subsystems/battle-action.md#the-0x19-attack-approach-park---a-second-distinct-softlock-class).

Sub-answers settled along the way: the wedged-looking `+0x1DD == 8` targets on
the idle party actors are stale all-target sentinels (the round is stuck on the
boss's action alone); the sibling `0x51` HP-settle park class is a fully
decoded mechanism that remains **injection-only** - a three-capture retail
campaign (twelve Lost-Grail revives, no harness HP writes) measured out both of
its candidate generators
([re-do-not-re-walk.md](re-do-not-re-walk.md#battle--arts--level-up)), and the
`0x19` class explains the community exhibits without any HP desync. Stated
limit: whether any retail sequence can still produce a `0x51` park is unproven
either way; nothing observed requires it.

### The summon-then-melee park trigger - the stale field is `+0x1DC` bit 2

*Status:* resolved - the "frame cursor / clip-length latch" hypotheses are both
wrong; the field the summon staging leaves stale is the battle actor's anim
event-flag byte `+0x1DC`, bit 2 (mask `0x4`), the **stage-idle-at-clip-end**
flag.

*Evidence:* `disassembly` (the driver pair `FUN_80047430`/`FUN_8004AD80`
in `ghidra/scripts/funcs/80047430.txt`/`8004ad80.txt`; the damage primitive's
flinch staging at `0x80042124..0x80042170` in `800402f4.txt`; the SM's `0x14`
fallback stores at `0x801E32B0`/`0x801E32D4` in
`overlay_battle_action_801e295c.txt`) + `capture` (causal control/experiment
replay on the parked save `battle_gaza2_park_0x19_summon_melee`, probe
`scripts/pcsx-redux/autorun_gaza2_stale_flag_repro.lua` with write-watchpoints
on `+0x1DA`/`+0x1D9`/`+0x1DC` logging writer PCs).

The chain: the summon's hit stages Gaza's light flinch with `+0x1DC |= 4|1`
(exit-to-idle + commit-now, `FUN_800402F4`); the flag is normally consumed at
the flinch's own natural end. When the boss's melee follows the summon
immediately, state `0x14`'s walk-less fallback stages the Move clip with
`|= 1` before that happens, and the tick's **event-path** commit - which
clears only bits 0-1 (`andi 0xFC`) where the natural-end path clears bits 0-2
(`andi 0xF8`) - installs the Move clip with bit 2 still set. The Move cycle
(5 frames, rate 2, speed scale 8 → ~12 vsyncs) then hits its first natural
end, where the tick sees bit 2 and stages idle over the queued clip
(`sb zero,0x1da` at `0x80047B44`) instead of re-looping - pair `0/0`, the
idle entry's per-tick speed is 0, state `0x19` re-polls forever. The live
replay shows both halves: the control bounce (state → `0x14`, flag clear)
loops the clip across its natural end (pair stays `1/1`) and arrives ~21
vsyncs later; re-arming bit 2 first reproduces the kill write from
`0x80047B44` at exactly the first natural end, 12 vsyncs after engage, with
the position frozen thereafter. The park save reads `+0x1DC == 0` because the
killing commit consumed the flag - it is only visible in flight, which is
why the parked-state reads never caught it. Full mechanism:
[battle-action.md](../subsystems/battle-action.md#the-stale-field-0x1dc-bit-2-the-exit-to-idle-anim-event-flag);
driver + flag-byte reference:
[monster-animation.md](../formats/monster-animation.md#playback).

### Super / Miracle Arts trigger chain

*Status:* resolved - matcher, tables, builder chain and runtime effect all pinned

The full retail chain: the saved chain is preseeded from the char record `+0x76F`/`+0x77F` by
`FUN_801DA34C` (a verbatim `lbu +0x76F → sb +0x1DF` copy - the char-record chain uses the
queue-space encoding directly, `0x0C/0x0D/0x0E/0x0F` = L/R/D/U, `0x1A` starter, `0x1B..0x32` art
constants); the queue-builder **`FUN_801EED1C`** (battle overlay 0898, ActionSeed state `0x0C`)
rewrites arrow runs to art constants, applies the Miracle replacement inline, then delegates the
Super find→tail-replace to **`FUN_801EF9E4`** - table-driven off `(actor slot, char index)`, find
cells `[len][bytes]` at `0x801F6524 + char*65 + row*13`, replace at `0x801F65E8 + char*80 + row*16`,
first-match-wins. The queue proper is exactly 16 bytes (`actor[+0x1DF..+0x1EE]`; `+0x1EF..` is
neighbouring data). Miracle-before-Super is structural.

The resident find/replace tables were captured byte-exact against the modeled
`crates/art/src/{miracle,super_art}.rs`, and **every one of the 15 Supers is live-executed**: an
applier-entry injection probe (`scripts/pcsx-redux/autorun_super_art_queue_inject.lua`) breakpoints
`FUN_801EF9E4`, writes the target Super's `find` bytes into the queue, retargets the char-index
register, and reads the tail-replaced queue back at the return site - 15/15 byte-exact (the two
combos previously driven by hand, Noa's Miracle and Vahn's Tri-Somersault, served as positive
controls). One post-applier library state per character is re-checked by
`crates/pcsxr/tests/super_art_queue_replace.rs`. Full chain + port:
[battle-action.md](../subsystems/battle-action.md#the-retail-queue-builder-fun_801eed1c-and-super-applier-fun_801ef9e4).

### Character-record HP/MP/AP pair order

*Status:* resolved - `+0x104/+0x108/+0x10C` are the effective **maxima**,
`+0x106/+0x10A/+0x10E` the **currents**

The decisive sequence is the stat aggregator's closing clamp triple at
`0x80042CE4`: `lhu v1,0x104(s0)` / `lhu v0,0x106(s0)` / `sltu` / `sh v1,0x106(s0)`,
repeated identically for `0x108`/`0x10a` and `0x10c`/`0x10e`. It clamps the
*second* halfword of each pair to the first, which only makes sense one way round.

The cap ladder immediately above it (`0x80042C0C..0x80042C50`) is **per-field, not
a flat 999** as previously documented: `+0x104` → 9999, `+0x108` → 999, `+0x10C` →
100, `+0x110` → 280, then 999 for five more. A 100-cap on `+0x10C` is unambiguously
the AP maximum, so the ladder independently corroborates the pair order - a wrong
claim was concealing supporting evidence for its neighbour.

Three further sources agree: (1) walk-regen `FUN_801D0B90` bumps `+0x106`, clamping
at `+0x104`; (2) the aggregator rewrites `+0x104` per frame from base stats plus
%-passives, and a per-frame recompute cannot be current HP; (3) GameShark "Infinite
HP" codes write `+0x106` at every character stride - they pin the *current*.

`legaia_save::HpMpSp` and every consumer carry the `(max, cur)` order; the status AP
gauge reads the AP current at `+0x10E`. Fresh-save fixtures masked the original swap,
because `cur == max` at seed.

### Effect-VM pass-1 "state token algebra" (`FUN_801E0088`)

*Status:* resolved + ported

The "state" bytes are 5.3 fixed-point **wait counters**, not opcodes: two countdown-driven cursor walks (master spawn cadence over 14-byte pack1 records; child anim/motion over 6-byte pack0 frames). `Pool::tick_retail` executes the algebra operator-for-operator (pass 2 = `Pool::child_billboards`), disc-verified over all 33 `efect.dat` scripts. Full algebra: [effect-vm.md](../subsystems/effect-vm.md#the-extracted-pass-1-state-algebra). The engine's live path runs it: `engine-core::World::tick_effects` sweeps `tick_retail` per retail frame and `active_effect_sprites` maps `child_billboards` one-for-one (the legacy fixed-lifetime shim is deleted; dev debug spawns live outside the pool).

### Battle face-stamp issuing site

*Status:* resolved

The facial-texel overwrite is the per-frame **facial animator `FUN_8004C7B4`** (called from the render-node update with the clip's frame cursor; Terra skipped): action-entry facial tracks at `+0x8C` (eyes) / `+0x98` (mouth) select frames from static per-character SCUS tables, stamped by `MoveImage` every frame. Pinned live across a battle entry (`karisto_sol_pre_encounter` + the MoveImage trace probe). Sibling-pass residue closed: `FUN_8004CCD4` is **not a stamp** - it is the equipment mesh-variant swap (same caller + guards, re-run per ghost by the arts trail renderer), driven by the entry's third track at `+0xA4`; retail windows Noa-only. See `battle-data-pack.md` § Facial animation tracks + § Equipment-variant track.

### Spine flag `0x482` (Drake mist-wall) writer

*Status:* resolved (writer-less; the "direct code path" presumption falsified)

The named capture ran: byte write-watch on `0x800857E8` (`autorun_flag_writer_watch.lua`) across the whole post-Zeto beat (battle exit, mist-clear FMV, `map01` arrival). The only write to the byte is the SET helper re-latching `0x484` (store `FUN_8003CE08+0x28`, `ra 0x801E3598`); `0x482` never flips. Every catalogued state through the Karisto era holds it clear (neighbours `0x484..0x487` at `0x0F`), and all 37 census sites stay `DESYNCED`. Verdict: **no writer ever fires** - the `map01` P2[34..36] C1 spawn-block never latches; wall despawn is not flag-driven. Residual: only an engine-side C1-latch-on-fire (pad-walk into a wall) could revive this.

### Flag `0x63A` - the vell/vozz `P2[7]` gate with NO script writer

*Status:* resolved (script writers exist)

Under the fixed scene windows the census shows eight **clean** sites: Set/Clear pairs in the rikuroa post-Caruban variant MAN (PROT 0157 P2[29]/[30], op `0x56`/`0x66`), rikuroa2 (PROT 0122 variant), retockin (PROT 0281 P2[7]/[8]) and edretoin (PROT 0800 P2[7]/[8]) - late-game beats, so the vell/vozz `C1=[0x63A, 0x7]` spawn-block passes for the whole first visit. Retail states corroborate: `0x63A` reads clear through the Karisto era while its bank byte already holds `0x0C` (`0x63C`/`0x63D` set). NB the old row's watch target `0x800858C7` was mis-derived; the byte for `0x63A` is `0x80085758 + (0x63A >> 3) = 0x8008581F`.

### Spine flag `0x142` (Caruban beat / dolk-dolk2 switch) writer

*Status:* resolved (disc writers + engine port + oracle)

Spine-writer #2 of 3, closed. The writers are plain field-VM `51 42` SETs in the
rikuroa **streaming variant MAN** (PROT 0157): `P1[10..12]` plus the
post-victory `P2[50]`, whose own C1 gate is `0x142` itself - the self-latching
one-shot shape. dolk2's carrier `P1[0..1]` re-asserts the flag; dolk `P1[26]`
clears it.

Firehose-caught live (`ra 0x801E3598`), and the resident script heap
byte-matches the carrier. The old corpus-negative stood only because no census
had walked the streaming carriers. Census + pins:
`man_variant_carrier_census_disc.rs`.

The engine sets it **organically**: rikuroa `P2[50]` executes from its own
script bytes on the Battle-to-Field edge (`organic_beat_records_disc.rs`),
which retired the earlier `SCRIPTED_SCENE_BOSSES` victory latch.

### Drake Castle deep interiors (`jouinc`/`jouind`) depth decode

*Status:* resolved (door-choreography families, not story gates)

`jouinc`'s 58-record `C1=[0x00F]` P2 family is a **busy-mutex door family**:
each record SETs `0x00F` first and CLEARs it last, so the C1 gate is a
mutual-exclusion lock rather than a story gate, and the bodies are per-door
walk-through choreography.

`jouind` `P2[10..13]`'s `0x4BE..0x4C2` band is **per-visit door/lift state**,
cleared by `jouina P1[0]` on entry - not a later-chapter revisit gate pair.
`jouinb P2[6..8]` is the interior beat band (`0x44E..0x450` latches plus the
jouinb-local `0x461` state flag).

Decoding these exposed - and fixed - whole-nibble width blindness in the
disassembler (`0x4C` nibbles 9/A/C/D/F). Full mechanism:
[script-vm.md](../subsystems/script-vm.md) § door-choreography record families.

### cave01 P2[16] spawner - the slot-counted interact chain

*Status:* resolved

The ungated `0x15D` setter `P2[16]` (global record `0x1E`; `51 5D` at body `+0x22`, MAN `0x3C10`)
is spawned by `44 1E` at **`P2[12]` body `+0x1C`** (MAN `0x35B9`). The spawn is gated by an
op-`0x4E` **sub-5 slot-table compare** at `P2[12]` `+0x15` (`4E 00 50 08 00 06 00`): while slot
`0x801C6460[0]` < 8 the compare skips forward past the spawn (to the `0x166`→`0x167`→`0x168`
progressive counter at `+0x20`); at 8 it falls into the `44 1E`.

`P2[12]` (global `0x1A`) opens with `4C CB 00 01 00` (slot 0 += 1) and is spawned once per
interaction by each of the five creature-interact scripts **`P1[3..7]`** (`44 1A` at the
first-interact branch tail: `P1[3]` `+0x2CC` = MAN `0x1CE4`, siblings at `0x1FCC` / `0x22B6` /
`0x259E` / `0x2888`). The per-NPC talked latches `0x161..0x165` are re-cleared inside the
interact scripts (`P1[3]` `+0x82..+0x8A`), so interactions repeat and the slot count reaches 8.
`P1[2]` - the lead-NPC ladder record that tests `0x15E`/`0x15D`/…/`0x157` - zeroes slot 0
(`4C CA 00 00 00` at `+0x0C`). PROT cites are the extraction frame (cave01 = PROT extraction 38).

Decoding the sub-5 gate exposed the op-`0x4E` sub-op family mis-read - see
[the 0x4E details](re-do-not-re-walk.md#op-0x4e-sub-op-family---every-sub-op-09-is-a-compare).

### NPC dynamic facing - two laws and an execution order

The spawn heading is settled ([above](#0x4c-0x51-byte-3-reconcile---facing-wins-no-motion-bytecode-synthesis)); this row is
everything after it.

**Two laws, chosen by the bytecode.** Walking **snaps**: every walk kernel -
the `0x47` tail in `FUN_8003774C` and the directional / wander steps in
`FUN_80038158` - quantises the frame's step to the eight-entry compass LUT at
`0x80073F04` (`entry[i] = i * 0x200`, `0` = -Z) and writes `+0x26` outright.
A walking actor therefore never holds an in-between angle; retail has no
walk-turn interpolation. The four dedicated rotate ops (`0x38` / `0x4C`,
`0x04` / `0x0D`) **ramp** instead, stepping `arc * speed / frames_remaining`
off the live heading over a budget the op carries, with an exact snap on the
terminal frame.

**Priority is execution order, not a field.** `FUN_8003BC08` runs the dialog
SM, then `FUN_8003774C`, then `FUN_80038158`, then the anim consumer - so an
actor running both a scripted leg and an ambient stream ends the frame facing
wherever the ambient stream put it.

**Corrections this closed.** Op `0x38`'s case body is `0x800379FC`, not
`0x80037DE0` (only `0x4C` lives there); the jump table at `0x80010EE0` settles
all 22 slots. The LUT is eight entries of `0x200`, not sixteen of `0x100` -
the port's synthetic table pointed rotating NPCs 45° wrong and doubled every
index. `0x4C`'s sub-modes `0x85` / `0x8E` / `0x8F` do not "gate which
component is rotated"; all three take one arm and `0x8F` alone forces the
direction. And `+0x16` is the **terrain-conform angle** sampled from the scene
grid by `FUN_80019278`, not a facing - the yaw is `+0x26`, always.

Live corroboration: a cold-boot `town01` sample off the static recompilation
reads every field actor's `+0x26`; all on-field headings are multiples of
`0x200` with all eight points present, the only exceptions being actors parked
on the `(0x7F, 0x7F)` sentinel tile.

Full write-ups:
[field-locomotion.md](../subsystems/field-locomotion.md#npc-dynamic-facing) +
[motion-vm.md](../subsystems/motion-vm.md#how-an-actors-facing-changes).

### 0x4C 0x51 byte +3 reconcile - facing wins; no motion-bytecode synthesis

*Status:* resolved

Raw asm settles both halves of the overlap:

- **`4C 51` case-1** (dispatcher `overlay_0897_801de840.txt`, case 5 sub 1) consumes byte `+3`
  **only** as `[bit7 -> actor render flag 0x1000000 (special model) | low nibble -> +0x26 =
  heading LUT 0x80073F04[b & 0xF]]`. The op carries **no speed operand**: byte `+4` is the
  move-anim id written to `+0x5C` (consumed by the anim-stream stepper `FUN_800204F8`);
  non-player targets also get the `+0x8C/+0x8D` current-tile bookkeeping, and the trailing
  `FUN_801D81E0` is an active-list relink (the unlink/relink pair `FUN_800204A4` /
  `FUN_80020454`), not a bytecode builder.
- The `depth & 7` base-step selector belongs to the **walk-kernel op `0x47`'s own third
  operand**: `FUN_8003774C` case `0x47` computes `4 << (b & 7)` (per-frame step
  `0x80 * dt / that`) with the high nibble an approach-mode selector; ops `0x37`/`0x41` encode
  their base step as `(op0 >> 5 & 4) | (op1 >> 6)` of their own two operand bytes
  (`ghidra/scripts/funcs/8003774c.txt`).
- There is **no motion-bytecode synthesis step**: the field-VM yield-class ops
  `0x37`/`0x41`/`0x47` (and `0x38` with a nonzero duration) park the current instruction
  pointer at actor `+0x94`, zero the progress cursor `+0x54` and set actor flag `0x400`
  (dispatcher cases `0x37/0x41`, `0x38`, `0x47`), and `FUN_8003774C` interprets the record
  bytes **in place** - it even resolves the field VM's `0x80` extended-target convention
  (`0xF8` player / `0xFB` world-map entity / placement id vs actor `+0x50`).

Consequence (rework landed): `placement_glide_speed` derives the base step from the real
`0x37`/`0x41`/`0x47` yield operands (`placement_yield_step`) and the tail-section-1 wander ops
(`placement_wander_step`), demoting the facing-nibble reading to a documented last-resort
heuristic; `4C 51` byte-`+3` sets facing + the special-model flag only (`placement_initial_facing`).
See [field-locomotion.md](../subsystems/field-locomotion.md) § NPC initial facing / § NPC glide speed.

### kor-family op-0x49 flag window [0x138..0x13F] - Uru Mais warp-pad picker

*Status:* resolved

Each flag is one destination row of the Uru Mais dream-shrine **teleport-pad picker**. The pad
records (kor `P2[17..20]`, kor3 `P2[9..12]`, kor4 `P2[4..7]`; extraction PROT 483/492/501)
clear the whole window, pre-set **their own row** (kor pads -> `0x138..0x13B`, kor3 ->
`0x13C`/`0x13D`, kor4 -> `0x13E`/`0x13F`), run the `FUN_801EF014` picker, then dispatch an
8-way `0x71` test ladder in which each arm clears `0x612`, fades, stops the BGM and executes a
**named `0x3F` SceneChange** (kor `P2[17]` body `+0x8D..+0x1C6`):

| rows | destination |
|---|---|
| 0..3 | `KOR` entries `(0x0E,0x35)` / `(0x1E,0x35)` / `(0x2E,0x35)` / `(0x3E,0x35)` |
| 4..5 | `KOR3` entries `(0x70,0x25)` / `(0x0D,0x36)` |
| 6..7 | `KOR4` entries `(0x27,0x27)` / `(0x1E,0x3E)` |

Widget semantics (`ghidra/scripts/funcs/801ef014.txt`): descriptor `+2` `default` = **first
visible row**, `+3` `rows` = visible row count, so the paired
descriptors are the full 8-row menu (selected by state flag `0x136`) vs the rows-4..7
chambers-only menu (`0x137`; kor3/kor4 carry per-pad record pairs, one per variant). Two
softenings worth keeping honest: the **menu pixel height `rows * 16` is unpinned
inference** - the `sll v0,v0,0x4` is present and the geometry is consistent, but no
reader of that store has been traced, so it is not established that the value reaches
the renderer as a height; and the kor pads themselves **never set `0x137`** - the
`P2[17..20]` records set `0x136` exclusively, so neither flag should be read as
reachable from any pad. State 0
cursors to the pre-set bit (= "you are here") and clears the window; confirming a **different**
row sets `base + selection`; picking the current row or cancelling sets nothing, so the test
ladder falls through to the stay-put arm (`+0x1C9`: clear `0x136`/`0x137`, fade back, park).

### Encounter MAN sub-section layout

*Status:* resolved

`FUN_8003AEB0` is fully decoded. **Header shape corrected against the
instructions:** `+0x22`, `+0x24` and `+0x26` are signed-16 **record counts**
of 3-byte records (assembled from `lbu` pairs then `sll 16`/`sra 16` at
`0x8003B04C..0x8003B098`), and `+0x28` is a **u24** (`0x8003B108..0x8003B120`)
- not four signed-16 section *offsets*. Six sections chain, not four. The
detail block in
[`encounter.md`](../formats/encounter.md#man-section-3-the-camera-region-table)
already carried this correctly; only this summary had drifted, which is a
recurring shape worth noticing. Also decoded there:
`legaia_engine_core::encounter_man::scene_encounter_from_man` reads the
encounter section straight from disc bytes, wiring per-scene `EncounterTable`s
for the standalone towns + kingdom-bundle scenes (the `count = 6` MAN form is
now resolved by `find_bundle`). The region-table section is the per-scene
control block `_DAT_801c6ea4 + 0x4` count-prefixed array of 18-byte records:
`byte[0]` kind selector, `bytes[1..4]` tile-space bounding box `[minX, minZ,
maxX, maxZ]` queried by `FUN_801dba20(tileX, tileZ)` (`tile = (player_pos -
0x40) >> 7`), `bytes[5..17]` a per-region **camera preset** -
decoded byte-for-byte (three mode-keyed splits on `byte[5] >> 4` into the `0x8007B607..0x8007B627` camera globals, consumed by the camera-param builder `FUN_801dab90`) in [`formats/encounter.md`](../formats/encounter.md#man-section-3-the-camera-region-table),

consumed by the field camera arrival handler `FUN_801dbec4` + camera-config `FUN_801dbc20`. The query side is ported: `legaia_engine_core::field_regions::zone_query` (`FUN_801dba20`, with the `FUN_80017fbc` `.MAP` region scan + `FUN_800180ec` attribute refresh) drives `World::refresh_field_regions` per tile crossing, and the section-3 body is the table the boot walk installs at `_DAT_801c6ea4 + 0x4`. Residual: the world-overview actor-placement section (consumed by `FUN_8003A1E4`), tracked separately (see world-overview threads); plus one loose end from the camera decode - the mask-kind records' `bytes[1..4]` side-copy to scratchpad `0x1F8003E8..EB` / mirrors `0x801F2778..84` has a confirmed writer but no traced reader.

### Seru-magic summon visual (e.g. Tail Fire)

*Status:* **player visual resolved and wired** - the player summon renders as its **namesake `battle_data` creature** through the ordinary rigid TRS-keyframe battle draw (`monster_archive::battle_render_mesh` + `MonsterAnimPlayer` + `tmd_to_vram_mesh_posed_rot`), spawned off the live cast band (`request_summon_spawn` → `spawn_summon_creature`); the move-VM `SummonScene` is retained only as the on-disc stager-record
parser/driver + a non-battle debug exerciser + the model for the **enemy** "Fire
Tail" boss move, which is now characterized: a single live move-VM part-actor
(SCUS tick `FUN_80021DF4`) over a battle-overlay (0898) record, with PROT 0900's
screen-widget path dormant - see the Fire-Tail note below. (The earlier
"`FUN_801F7088` rotation node source unpinned" framing is superseded - see the
resolved block below.)

The summon visual is a **per-summon code overlay**, not an opcode or `befect_data`: battle SM `FUN_801E295C` state `0x29` resolves spell id `0x81..0x8b` via `PTR_801f6734[id-0x81]` + `FUN_8003EC70(id-0x79)`.

**Two overlays timeshare the shared buffer at link base `0x801F69D8`** (`*DAT_80010390`):

**PROT 0905** is a **spawn stager** (22 `FUN_80021B04` calls within its trimmed TOC-gap
footprint - see the over-read note below) - under the corrected loader index
math (`FUN_8003EC70(param)` → extraction entry `param + 0x37F`, see [`formats/prot.md § In-RAM
TOC`](../formats/prot.md#in-ram-toc)) it is the **spell-`0x83` slot**, while Gimard `0x81`
arithmetics to **extraction 0903** (also a clean stager; the
historical "0905 = Gimard" label was the `+ 0x381` off-by-2, never content-pinned) - and **PROT
0900** is a resident **transform / GTE-render** overlay (`RotMatrixX/Y/Z` ×6 + prim emit) that
animates and draws the spawned parts. PROT 0900 is the one **byte-resident** in a mid-cast save
state (`battle_gimard_tail_fire_a/_b`: `0x801F8000` ↔ PROT 0900 file `0x1628`) - *after* the
stager has run and been overwritten - which is why a "stager head in RAM" search comes up empty.
The stager spawns each part via the SCUS part-stager **`FUN_80021B04`** (`a1` = world pos, `a2`
= a part record, `a3 = 0x1000`); `FUN_80021B04` stages it as an actor (`actor[+0x48]` = record
move-buffer base, `actor[+0x70] = 2` PC) then `jal FUN_80023070` ticks the **move VM** on
`record+4`.

**Records resolved - in-file, parsed.** Each `FUN_80021B04` call passes its record by absolute pointer (`lui 0x8020 / addiu`); under the correct link base `0x801F69D8` those resolve to PROT 0905 **file `0x180C..0x1E00`** (runtime `0x801F81E4..`), a contiguous table of variable-length records `[i16 model_sel][u16 flags][move-VM bytecode @+4]`, `model_sel == -1` = transform/pivot node (dominant; mesh bound by the move-VM anim-bank ops), `>= 0` = `DAT_8007C018[model_sel + gp[0x754]]`. `legaia_asset::summon_overlay::parse` recovers them by scanning the spawn calls (disc-gated `summon_overlay_real`: 22 sites → 17 part records, all transform nodes, within the trimmed footprint; CLI `asset summon-overlay`).

**Generalizes across the player, evolved-Seru, high-summon and enemy boss blocks - and the sentinel question is resolved.** Every overlay in extraction PROT 0903..=0913 (`spell_id 0x81..=0x8b`, `summon_overlay::PLAYER_SUMMON_STAGER_PROT`), the evolved-Seru block 0914..=0923 (`spell_id 0x8c..=0x95`, `EVOLVED_SUMMON_STAGER_PROT` - same `(id - 0x81) + 903` run; 8/10 legs capture-pinned, only `0x90`/`0x91` predicted), the high-summon block 0927..=0934 (`HIGH_SUMMON_STAGER_PROT`), and the six Cort enemy stagers 0938/0940/0944/0961/0962/0966 (`ENEMY_BOSS_STAGER_PROT`) recovers a move-VM scene-graph (disc-gated `summon_overlay_block` + `enemy_stager_real` sweeps), once two facts are applied:
(1) the high/enemy stagers spawn dominantly through the pool wrapper `FUN_80050ED4` (→ `FUN_80021B04`, pool `DAT_801C90F0`), which the parser scans alongside the direct calls;
(2) **stager extraction entries are over-read windows** - each `.BIN` runs past the next entry's start LBA, so it must be trimmed to `(next_start_lba - start_lba) * 0x800` (`unique_content_len`) before parsing, a boundary the Cort mid-cast saves pin byte-exactly against the slot-B resident image.
After trimming, the record first words across the whole stager corpus are only `-1` / small library indices / **`0x4000`** - matching `FUN_80021B04`'s own dispatch (negative → transform path; `0x4000`/`0x4001` → render-mode nodes `+0x5A = 3`/`5`; else library index). The earlier "`0x1000`/`0x8000`-class sentinel" census was over-read contamination: those offsets belong to *neighbouring* stagers' loads and dereference unrelated bytes in the wrong file window. The `0x4000` render-mode records live in **five** stagers: Palma 0928 (4) / Mule 0929 / Jedo 0931, **plus the evolved-Seru casts 0916 (`0x8e`, 4) and 0921 (`0x93`, 6)** - the first such records found outside the Sim-Seru trio (all are *player* casts, so none unblocks the live-exerciser question below).
The model-library base (`gp[0x754]`) is **resolved** (see the summon-render block below): it is **not per-summon** but one per-battle, party-size-derived value (`party_count + 2`). Still open: the draw behaviour of the `0x4000`/`0x4001` render-mode nodes -
**no live exerciser in the catalogued corpus**. The Cort enemy states' live
pooled part-actors all carry `-1` records (`+0x56 = 4` / `+0x5A = 2` after
move-VM rebinding), and the three player Sim-Seru casts that *carry* the
`0x4000` records (Palma 0928 / Mule 0929 / Jedo 0931) hold **no live stager
part at all** at the captured instant - a RAM pointer-scan finds zero
references to any of the stager's records despite the stager being byte-resident
at slot B (the player summon is the creature pipeline by the on-screen phase).
Newly-captured *ordinary*-enemy casts (the Delilas brothers → 0958/0959/0960,
Zeto → 0946; `enemy_stager_binding`) confirm the enemy stager path generalizes
beyond Cort, but none of those stagers carries a `0x4000` record either, so they
don't seat one. A frame-stepped *enemy* stager-spawn capture whose stager
carries a `0x4000` record (an enemy casting a Sim-Seru creature Palma/Mule/Jedo)
would seat one live (`crates/mednafen/tests/summon_render_mode_node.rs`).

**Decoded (no capture required) + classification ported.** The per-part-tick
`FUN_80021DF4` (the SCUS driver `FUN_80021B04` binds at `actor[+0x70] = 2`)
dispatches the render mode `+0x5A` into **six modes**, fully decoded in
[`move-vm.md` § Part render-tail](../subsystems/move-vm.md#part-render-tail-the-0x5a-render-modes-fun_80021df4):
`2`/`6` = parameter/colour tween, `3` (the `0x4000` node) = moving particle
(`FUN_80019D50`), `4` = VRAM-blit beam (`LoadImage`/`MoveImage`/`StoreImage`
`0x8005842C`/`0x80058490`/`0x800583C8`), `5` (the `0x4001` node) = **3D positional
*sound* emitter** (range/volume + SE trigger - *not a visual node*), `7` = matrix
transform + billboard, else = transform pivot. Key result: `0x4001 → +0x5A = 5`
is audio, so the two "render-mode" sentinels are a particle node and a sound
node, not two draws. `FUN_80021DF4` is a host-emission-heavy dispatcher
(GP0/SPU/VRAM + ~30 abstracted part fields), so the renderer-agnostic surface
that is **ported** is the render-mode classification
`engine-core::summon::RenderMode` (`from_model_sel`, `// PORT: FUN_80021B04`),
consumed by `SummonScene::special_render_nodes` / `part_draws` to split the
audio-only node off the mesh draw path
(`render_mode_classifies_only_the_sentinel_nodes` +
`special_render_nodes_are_split_from_the_mesh_draw_list`); the per-mode
integration + emit paths stay documented for a future renderer/audio host. The
move-VM call gate was already ported (`move_vm::actor_tick`). PR #273's **239**
field-resident prescript render-mode nodes remain the non-summon validation
source (a resident-overworld mednafen read, no live probe) if byte-validation of
the integration is ever wanted - but the draw behaviour is no longer unknown.

**This corrects the earlier "records beyond the `0x5800` file / `0x180C` only coincidentally record-shaped / parser reverted" reading - that was the wrong link base (`0x801F0000` instead of `0x801F69D8`), which pushed the runtime record addresses past the file.** **Still pinned:** the CLUT band is byte-identical across the two animation-distinct frames (motion is geometric, not palette cycling); flame texture is **PROT 870** (three 64x256 4bpp TIMs → battle VRAM `(320/384/448,0)`, CLUTs rows 474..476); the bound flame mesh comes from **PROT 871** (`etmd.dat`, 30-TMD pack) at `DAT_8007C018[26]`.

**Engine:** PROT 871 → `World::global_tmd_pool[3..=32]`, flame atlas uploaded on battle entry, static flame renders with the row-478 CLUT (`GIMARD_TAIL_FIRE_MODEL_INDEX = 26`).

**Animation driver landed.** `engine_core::summon::SummonScene` seeds one move-VM `ActorState` per parsed part (PC=2 → `record+4`, mirroring `FUN_80021B04`) and ticks every part through the already-ported move VM each frame (`World::spawn_summon` / `tick_summon` / `active_summon_part_draws`; `play-window` `G` debug-spawns the Gimard summon and renders one textured TMD per mesh part). The per-part animation *computation* is faithful (verified: every Gimard part runs the move VM without an unimplemented opcode; disc-gated `summon_scene_real`).

**Read the mesh-part draw count off the entry's own footprint.** Gimard's stager (PROT 0903) resolves to a pure transform rig - every recovered record is `model_sel == -1` - so its draw list is legitimately empty and any assertion of the form "this stager has a mesh part" is vacuous on it. Mesh-bearing records are rare across the whole stager corpus, and the ones a stager appears to gain from a longer buffer are the *next* stagers' record offsets read against this entry's bytes. `summon_scene_real` therefore drives two legs: Gimard for the tick path, Nighto (`0x85` → PROT 0907, one mesh record) for the draw path.

**Production cast-band trigger wired.** A player Seru-magic cast (`spell_id` in `0x81..=0x8b`)
now requests the summon at the cast point in both engine cast paths - the action-SM
`spell_anim_trigger` (`World::fold_battle_event` on `BattleEvent::SpellAnimTrigger`) and the
live-loop `cast_spell_on_slots` - via `World::request_summon_spawn`. The host drains
`World::take_pending_summon_spawn`, maps the id to its overlay PROT entry
(`summon::summon_stager_prot_entry`: `0x81..=0x8b → 903..=913`, extraction space - retail
`FUN_8003EC70(id-0x79)`), loads + parses it, and seats the scene-graph (`play-window`). So a
real Gimard *Burning Attack* cast spawns the animated summon, no debug key.

**Per-spell stager assignment capture-pinned for the whole block.** One mid-cast save state per
spell (the `gimard_summon_*` + `<seru>_summon_mid_cast` scenarios in `scripts/scenarios.toml`)
holds the battle overlay's loader-B current-id `0x8007BC4C` at exactly `spell_id - 0x79` for all
eleven ids: `0x81` Gimard→903 through `0x8B` Nova→913, every leg on the linear arithmetic.
Entry 0907 (Nighto) heads with the ASCII title `Hell's Music` + a normal MIPS prologue - the
title is the ATTACK's display name (the SCUS spell table carries the same string, `Hell's
Music|Kill or confuse enemy.`; `summon.dat` lists it among the attack-name records, parallel to
Gimard's `Burning Attack`). The earlier "dance-song / dual-use" reading is **refuted**: an
exhaustive static loader scan of the dance overlay (0980 - jal/tail-call/pointer-word/lui+addiu,
all four mechanisms) finds **zero** slot-B loader callsites; the dance minigame's only
loader-reaching call is the SCUS `FUN_80025BA0` wrapper (ids 5/6 → the 0900/0901 move-FX pair),
and its music is sequenced BGM via the sound streaming loader. Single use: summon stager.

**PROT 0900 resolved - the slot-B *screen-effect + top-view-grid* overlay; `FUN_801F811C` is a 2D screen-mask widget, not a part transform.** A full static decode of the file at the link base `0x801F69D8` (function bodies instruction-diffed identical against the dance / baka-fighter dumps; file `0x0640..0x2660` byte-resident at `0x801F7018..0x801F9038` in the fingerprinted `battle_gimard_tail_fire_a` save) closes the long-open "quad-emit / matrix half" question. Two subsystems coexist in the file:

**(1) `FUN_801F811C` = the screen-mask (iris) widget handler.** Its four tweened channels
(`+0x3c/3e/40/42` targets vs `+0x14/16/18/1a` latched current) are the **left/top/right/bottom
edges of a screen rect**, and the "4 render quads" are the **black border bands** framing that
rect (GP0 `0x28` flat quads, OT `+0x1c`; screen X origin / height from render scratch
`0x1F800388`/`0x1F80038E`). It is kind 1 of a **four-kind 2D screen-widget family** (scripted
sprite `FUN_801F7A9C`, mask `FUN_801F811C`, image panel `FUN_801F849C`, letterbox
`FUN_801F8A34`), bound through 0x18-byte handler descriptors at `0x801F8FE4/8FFC/9014/902C`
(allocator SCUS `FUN_80020DE0` stores the handler at `actor+0xc`; finder `FUN_8003CF04`), with
control APIs `FUN_801F8004` / `FUN_801F8D4C` / `FUN_801F88FC`+`FUN_801F8E6C` / `FUN_801F8F28` -
**called by field/event-VM sub-ops** (`jal` sites inside `FUN_801DE840` at `0x801DF918/974`,
`0x801DFA70/ABC/ACC`). Full reference: [`move-vm.md` § screen-effect widget
family](../subsystems/move-vm.md#screen-effect-widget-family-prot-0900); ported as
`engine-core::screen_fx` (mask / sprite / panel / letterbox + the full 4-mode `FUN_801DE4C8`
interpolator; layout pinned on disc bytes by the disc-gated `screen_fx_disc` test).

Two corrections this lands: (a) apparent references to these handlers from the summon stagers 0910..0915 are **VA aliasing** - in-file `FUN_80021B04` part records at coincident addresses under the shared slot-B base; (b) the earlier "summon-part per-frame position update" reading of `FUN_801F811C` is superseded - the engine keeps that tween shape as the *interpreted* `summon::apply_translation_update` glide (documented as such), faithful port = `screen_fx::MaskWidget`. A tween-math detail the old reading missed: mid-tween the latched current values do **not** move - each frame re-interpolates from them (fixed start), latching only at `+0x9C == +0x9E`.

**(2) The genuine matrix code in PROT 0900 is the top-view grid-instance renderer** - `FUN_801F7088` plus a parallel second-cluster sibling (`RotMatrixX/Y/Z` ×6, GTE `MVMVA`). Per grid cell it composes `TR = R_cam · cell_pos + TR_cam` and `R = R_base` (camera Euler `_DAT_8007B790/2/4`, per-axis skipped by record flags `0x80/0x100/0x200`) `· Rx(rec+8) · Ry(rec+0xa) · Rz(rec+0xc)`, binding model `DAT_8007C018[rec+0x10 + base@0x8007B6F8]` into cluster-A `FUN_80043390`. This code is **genuinely part of PROT 0900** (instruction-identical in the file - correcting the earlier "the `FUN_801F7088` dumps are a different overlay aliasing the band" note below), but the live-trace result stands: it does not run during a player summon, so it is not the summon / move-FX path.

  **(2) 3D MESH ROTATION - `FUN_801F7088` is not the player-summon path (live-trace resolved).** The historical hypothesis was that each summon part's mesh orientation is built by `FUN_801F7088` (a GTE view rotation from the camera Euler globals `_DAT_8007B790/2/4` gated per-axis by a node-flags word's bits `0x80/0x100/0x200`, plus a per-part local Euler at the node's `+0x8/0xa/0xc`, via `RotMatrixX/Y/Z`).

**A live PCSX-Redux capture of a player Gimard "Burning Attack" cast (Vahn solo; scenarios `gimard_summon_start` / `gimard_summon_visible` / `gimard_burning_attack`) falsifies that for the player summon.** Exec-breakpoint counts across all three phases: `FUN_801F7088` = **0 calls**, move VM `FUN_80023070` = **2-3** (trace noise, not a per-part driver), part-stager `FUN_80021B04` = 1, and the **battle per-actor draw `FUN_80048A08` = 35-64×/frame**. The summon is an ordinary battle actor (state `gimard_burning_attack`: actor `0x8008350C`, `+0x5a=3`, 13-group mesh-table at `+0x44`, monster-anim archive at `*(actor+0x4C)+0x88`) drawn by `FUN_80048A08` → the per-object rigid-TRS keyframe decoder `FUN_8004998C` → cluster-A `FUN_80043390`, with each object's Euler composed by `RotMatrixX/Y/Z`.

**[Correction - `0x8008350C` is a Gobu Gobu monster, not the summon; see the resolved block at the end of this row. The durable result here is the call-count finding (`FUN_80048A08` is the draw path); the summon's actual creature is `battle_data` id 10 "Gimard", pinned from the fingerprint-verified frame-0 RAM.]** **So the player Gimard summon is posed exactly like an enemy monster body (per-object rigid TRS keyframes), not via a move-VM scene-graph or `FUN_801F7088`.** This agrees with the `effect.md` / `battle-action.md` / `effect-vm.md` finding ("PROT 905 has zero `jal 0x80023070` - there is no move VM here").
[Superseded detail: the `FUN_801F7088` body is in fact instruction-identical **inside PROT 0900
itself** (the slot-B screen-effect + top-view-grid overlay, see the resolved block above) - the
"different overlay aliasing the band" attribution was wrong, while the "not the battle-summon
code path" conclusion stands.]

Scope: this capture is the player "Burning Attack" move only; the enemy Gimard boss move **"Fire Tail"** (the `battle_gimard_tail_fire_a/_b` captures) is a distinct move with a distinct animation and was traced separately (Fire-Tail note below). (Probes: `autorun_summon_rotation.lua` + `autorun_summon_path_reconcile.lua`; RAM dumps under `captures/summon_rotation/`.) The engine's `summon::SummonScene` move-VM model therefore needs reconciliation: for the player summon the faithful path is the battle TRS-keyframe draw, already ported as `FUN_80048A08` / `FUN_8004998C` in `crates/engine-vm/src/anim_vm.rs`.

**Enemy "Fire Tail" - resolved (move-VM part, not the widget path).** A
pure-Rust scan of the two catalogued mid-cast frames
(`battle_gimard_tail_fire_a/_b`; disc + library gated `firetail_movefx_liveness`)
settles the separate question. The slot-B occupant is the move-FX module **PROT
0900** itself (loader-B id `5`; byte-exact at the residency pin file `0x1628` ↔
`0x801F8000`), *not* a per-spell stager. But PROT 0900's screen-widget family
(the iris/sprite/panel/letterbox set the eight ending scenes drive via field-VM
op `0x43`) is **dormant** here - an effect-actor-list walk of both frames finds
**zero** live widgets. The live effect is a single **move-VM part-actor** in the
part pool `DAT_801C90F0`, ticked per frame by the generic SCUS actor tick
`FUN_80021DF4` (→ `FUN_80023070`; this is the live capture that pins that
render-tail driver). Its `[i16 model_sel][u16 flags][bytecode]` record
(`actor[+0x48]`) lives in the **battle overlay (0898)** resident data at
`0x801F5xxx` - below the 0900 slot-B link base `0x801F69D8`, so not a 0900 record
- with `model_sel` reading `-1` (transform node) / `5` (library mesh). So Fire
Tail's render path is the move-VM scene-graph (one live part) sourced from
battle-overlay data; the 0900 widget reading of it is falsified and the widget
family stays ending-scene-exclusive.

**Animated battle-actor rendering is now wired** (the general pipeline this thread's player-summon render rides on). Enemy monsters animate in `play-window`: `legaia_asset::monster_archive::idle_animation` (action 0, the `+0x8c` 9-byte TRS stream) → `legaia_engine_core::battle_anim::MonsterAnimPlayer` (an 8.8 fixed-point loop cursor producing a `legaia_anm::PoseFrame`, the same per-object `(translation, rotation)` shape the field ANM player produces) → the rigid `legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot` deform (`R·v + T`, `Rz·Ry·Rx`, the validated `monsters.html` `_assemble` math).

`enter_battle_render` attaches the clip per actor, `World::tick_battle_animations` advances it each battle frame into `pose_frame`, and the posed-override path deforms the mesh; the field translation-only path is unchanged. The core (decode → player → posed_rot → moving mesh) is proven on real disc data by `battle_anim_real` (monster 1 = 28 frames × 15 parts).

**Player summon source - resolved: the summon reuses the namesake `battle_data` enemy creature.** (Path to the answer, including a corrected wrong turn.) The actor `0x8008350C` the earlier notes called "the summon" is actually a **Gobu Gobu monster** - its `+0x4C` archive `0x800B2694` (`+0x88` self-ptr → `+0x8C`, 13×18) byte-exactly matches `battle_data` id 4 (Gobu Gobu) action 0. The fix was **fingerprint discipline**: the `summon_rotation/state6` RAM *dump* is the probe advanced N frames; analysing the **fingerprint-verified frame-0 RAM** of the `gimard_summon_visible` save (`8aa0…`, sha256-matched to the catalog + the live slot) instead, the battle actor table `DAT_801C9370` shows slot 0 = Vahn (HP 196) casting `spellid 0x81`, slot 3 = a Gobu Gobu enemy (HP 76, 13 parts / ~10 actions),

and a **distinct 11-part / 2-action** entity. That 11-part idle (`0x800BBB20`, 11×40)

**byte-exactly matches `battle_data` id 10 = "Gimard"** action 0. So **the player Gimard summon spawns the namesake "Gimard" creature** (id 10), reusing its monster-archive mesh + per-object TRS animation - exactly the format the now-wired enemy pipeline consumes. Disc-verified spell→creature map (by name; the `"$2"`/`"$3"` higher-level enemy variants are excluded): Gimard `0x81`→10, Theeder `0x82`→25, Vera `0x83`→28, Gizam `0x84`→55, Nighto `0x85`→49, Zenoir `0x86`→64, Viguro `0x87`→74, Swordie `0x88`→86, Orb `0x89`→83, Freed `0x8a`→92, Nova `0x8b`→95 (`legaia_engine_core::summon::summon_creature_id`, disc-gated `summon_creature_map_real`).

**The summon→creature map is now extended through the evolved-Seru block `0x8C..=0x95` and pinned by mesh identity, not name** - matching each `summon.dat` group's actor-record Legaia TMD against the archive (longest-common-prefix) gives a byte-identical hit for all of `0x81..=0x95` (8–17 KB each): Gola Gola `0x8c`→98, Mushura `0x8d`→101, Aluru `0x8e`→80, Barra `0x8f`→141, **Kemaro `0x90`→144, Spoon `0x91`→147** (the two evolved legs no mid-cast state covered, now disc-pinned), Slippery `0x92`→150, Iota `0x93`→153, Puera `0x94`→156, Gilium `0x95`→159. Map: `legaia_asset::summon_creatures::SUMMON_CREATURES`, byte-validated by disc-gated `summon_creature_tmd_map_real`.

The **high block `0x99..=0xA0`** (Juggernaut / Palma / Mule / Horn / Jedo / Meta / Terra / Ozma) does **not** byte-match any archive record - those summons carry a **bespoke mesh** in the `summon.dat` group's raw part-pool slot, not a reused enemy body (the same oracle asserts no archive byte-match).

**This supersedes the old move-VM `SummonScene` model and the PROT-905-overlay reading** for the *visual*: the faithful summon render is the battle creature drawn through `monster_archive::battle_render_mesh` + `MonsterAnimPlayer` + `tmd_to_vram_mesh_posed_rot` (mesh + texture + animation all from PROT 867), not the stager scene-graph. (PROT 905 is still the magnitude/effect stager - see the per-spell-power thread.) The flame-atlas loader site is now pinned:

**`FUN_80020050`** (SCUS `0x80020050`) uploads PROT entry `0x366` into VRAM twice via `FUN_8001fc00` (→ `FUN_8003e8a8`, the PROT-index loader), with the VRAM region set up by `FUN_80017888` / `FUN_8001e54c` (param `0xf000`); it is gated on `_DAT_8007b868 == 0` (the same field-camera / mode gate `FUN_801dbe9c` reads) and is independent of the `FUN_800520F0` battle-bundle path (which pulls `0x367..0x36d`).

### `summon.dat` / `readef.DAT` side-band streaming

*Status:* **resolved (entries + format)** - the two `0x10800`-slot battle streaming files are pinned and decoded; full reference [`formats/summon-readef.md`](../formats/summon-readef.md), parser `legaia_asset::summon_readef`, disc-gated `summon_readef_real`.

- **Entries pinned by arithmetic + bytes.** `FUN_800558FC` in retail ignores its path string (`_DAT_8007B8C2 != 0` verified live) and consumes the 4th argument as a raw-TOC index: `summon.dat` = `0x37F`, `readef.DAT` = `0x380` → **extraction PROT 893 / 894** (the −2 raw-TOC offset, same as the overlay loaders' `param + 0x381`). Both footprints divide into exactly 103 / 78 slots of `0x10800`. Byte-verified in `battle_gimard_tail_fire_a`: the stream buffer at `*0x8007BD74` equals entry 894 slot 1; slot 0's CLUT row / texture page match VRAM `(0,488)` / `(512,0)` byte-for-byte.
- **Format decoded.** Action id → base slot byte (`FUN_801E295C` case `0x32`): `3*(id-1)` for `id < 0x9A`, else `4*id + 0x63`; bit 7 selects the file. The applier `FUN_801F12D0` streams slots `base..base+3` (readef groups stop after `base+1` unless `base == 0x36`) and uploads CLUT rows + texture pages; `FUN_801F19EC` installs the final slot as the summon creature (via `FUN_80055468`). Summon group 0 (spell `0x81`) carries the "Burning Attack" record. Beyond the cast path, `FUN_801DABA4` seeds the group base **per turn** (party `3*(char−1)`; enemy `3 * monster_record[+0x1C]`) and the battle-end arms directly request `3*char+2` - the traced main-vs-base `"ME"`-archive pick (see [battle-data-pack.md § "ME" stream archives](../formats/battle-data-pack.md#me-stream-archives-readefdat)).

Open residue:

- **Low-band `readef.DAT` aux-slot consumer - groups 0..3 resolved.** The eight aux slots of readef groups 0..3 (slots `3c+1`/`3c+2`, c = Vahn/Noa/Gala/Terra) are the player **art-animation `"ME"` stream archives**, consumed by `FUN_8002B28C` out of the `*0x8007BD74` buffer - see [battle-data-pack.md § "ME" stream archives](../formats/battle-data-pack.md#me-stream-archives-readefdat); parser `legaia_asset::me_archive`. The main-vs-base pick is traced (per battle phase - turn staging vs battle-end win-pose staging; same doc section). Higher readef groups' aux slots remain unattributed as content, but the selection is pinned: the monster record's group byte `+0x1C`, staged per enemy turn by `FUN_801DABA4`; an exec-bp sweep over `*0x8007BD74` readers during an enemy special would close it.
- **Readef id ↔ named attack table.** The Tail Fire capture is consistent with action id 1 → readef group 0; the full `actor+0x1DF` id ↔ enemy-special mapping (the `map[actor+0x1df]` 128-byte band) is unenumerated.
- **CDNAME `#define` number space - resolved: raw-TOC space, uniform −2 to extraction.** Quantified by `scripts/asset-investigation/cdname_shift_analysis.py`:
  1. Every byte-pinned loader constant for a dev-named file *equals* the same-named define - `PLAYER1..4` `0x361..0x364` = `battle_data 865..868` (extraction 863..866 start at the traced PROT.DAT offsets), `monster.snd` `0x37D` = `monster_se 893` (extraction 891 = 206-bank multi-VAB), `summon.dat`/`readef.DAT` `0x37F`/`0x380` = `bat_back_dat 895/896`, overlay slots `0x381+` = `xxx_dat 897+`.
  2. Scene block lengths vary, so the per-scene v12 table's slot position is shift-sensitive after all - all 96 scene-region v12 tables sit at slot 1 under −2 vs scattered over slots 4..10 at shift 0 (constancy alone admits −1/−2/−3; the identities pin −2).
  3. Semantic scoring over decidable blocks: 217/225 at −2 vs 209/225 at 0 (`vab_01` → extraction 1070..1192 = 121/121 VAB-headed; `other_game` banners `OTHER2`/`OTHER3` at extraction 973/974; `move_program_no` → extraction 970, a `\DATA\MOV*.STR` table - MOVie program numbers, dissolving the old `move.mdt` mismatch).

  Extractor filenames stay as-is; `legaia_prot::cdname::block_for_extraction_index` gives the retail-space name. Full table + exceptions: [`cdname.md` § numbering space](../formats/cdname.md#numbering-space).

### Monster steal item (Evil God Icon)

*Status:* resolved - static SCUS table `DAT_80077828`

What the player steals with the Evil God Icon equipped comes from a **static
`SCUS_942.54` table at `DAT_80077828`** (file offset `0x68028`), indexed by
**1-based monster id**: entry `id` sits at `DAT_80077828 + id*2`.

Each entry is a 2-byte `[steal_chance_pct, steal_item_id]` pair. Note the field
order - **chance first, item second**, which is the reverse of the `[item,
chance]` drop fields in the monster record. Reading it in drop order silently
swaps every value.

The table is **not** in the PROT 867 monster record at all. It lives in the
executable, which is why every record-only search came up empty. The negative is
disc-measured over the whole archive: for the 185 monster ids that are both
populated in PROT 867 and stealable in the SCUS table, no byte offset carries the
steal pair in either field order - not in the 13,030,964 bytes of LZS-decoded
monster block (every offset, full block length, not just the `0x4C` stat head),
nor in the 15,155,200 raw bytes of the `0x14000` slots that hold them. Best
agreement in any layer is `[chance,item]` 2/185 and `[item,chance]` 2/185.

Two properties of that measurement are worth keeping, because each would mislead
a re-derivation:

- **The one elevated offset is not a near-miss.** Single-byte offset `0x48`
  scores 31/185 - but `0x48` is the `drop_item` field, and steal and drop draw
  from the same 39-item consumable pool, so incidental agreement is expected.
  None of those 31 also agree on chance at `0x49`, and the best non-drop offset
  anywhere is 7/185, the noise floor.
- **Drop-order field order could not have faked this negative.** A scan looking
  for `[item, chance]` (the drop order, the reverse of this table's) still tops
  out at 2/185. The field-order hazard is real for a *positive* reading; it
  cannot manufacture the negative.

Independent of any scan: monster ids `187..190` are stealable in the SCUS table
but have **no archive slot at all** - PROT 867 is 194 slots of `0x14000` with
only 186 populated. The record cannot be the source for those ids under any
reading.

Pinned from a live player-steal RAM capture - Skeleton, id 13, reads `1e 8a` =
30% Incense, matching the on-screen banner - and then verified **byte-exact
against the complete published steal table** (item and chance both) across every
resolvable monster id, with zero mismatches.

Parser `legaia_asset::steal_table`; doc [`steal-table.md`](../formats/steal-table.md); randomizer `legaia_patcher::steal`. `enemies.toml` `steal` stays useful ground-truth but the SCUS table is now authoritative.


### Per-spell magic power / multiplier

*Status:* **mechanism resolved + roll ported** - the calculator + full three-stage modifier chain (`FUN_801dd0ac` roll → `FUN_801dd864` scale → `FUN_801ddb30` finish) is recovered, and the closed-form roll + scale stages are ported as pure kernels in `battle_formulas`; the `0x801F4F5C` arts table is now located + parsed off the disc (`legaia_asset::move_power`); live wiring + the coupled finisher are the residual

**The static re-dump avenue closed the question.** The 7-entry jump table `FUN_801f2d68` reads (`jr *(0x801F69D8 + state*4)`) resolve to PROT **0900** file offset 0 - the **render** overlay (loads at `0x801F69D8`). Those five entries are staggered entry points into one per-frame routine that lerps move-VM anim banks (`FUN_8003ce9c`/`ce64`/`ceb8`) and emits GPU display-list packets into scratchpad `0x1F800314`:

**zero `mult`/`div`, zero `actor+0x14c` write, no power read** → the "magnitude is in this jump table" hypothesis is **falsified**; it is animation/GPU only. The magnitude is instead applied by the paired **stager** overlay (PROT 0903..0915, the file with the `jal FUN_80021B04` part-spawn calls), in the same function that spawns the body parts - each stager has exactly one `actor+0x14c` writer, and they split:

**damage summons** (PROT 0904/0912/0914 + 0915's 2nd arm; `subu`) call the shared battle kernel **`FUN_801dd0ac`** (`a0` = a per-summon move-type const `0x10..0x12`, `a1 = 7`, `a2` = target slot), clamp to current HP, accumulate the popup at `actor+0x10`, then `HP -= amount`; **heal summons** (PROT 0903/0905/0910/0911/0913 + 0915's 1st arm; `addu`) compute `(power_byte << 5) + 0xe0` inline (clamped to `maxHP-curHP`, dead-guarded), `power_byte` from a `0x80084140`-based table searched by the cast spell-id (`actor+0x1df`: ids at `+0x705`, powers at `+0x729`).

`FUN_801dd0ac` (already dumped, `overlay_battle_action_801dd0ac.txt`) takes the **summon path** for `param_2 == 7`: roll = `rand % (INT@+0x168 + 1) + HP@+0x14c + DAT_801C9370[ctx+0x13]_INT * 2`, returns `roll - defender_mitigation` - so **summon "power" is caster/summon battle-state-derived, not a static per-spell scalar** (which is why SCUS spell-table `+5..+8` are zero and gamedata has no power column). `FUN_801dd0ac`'s **non-summon** branch (`param_2 != 7`, arts/physical) reads a real 26-byte-stride per-move power table at **`0x801F4F5C`** (arts power, **not** magic) - now located on disc as static battle-overlay data (PROT 0898, parser `legaia_asset::move_power`),

indexed via a 128-byte id→index map at `0x801F4E63` (`param_1 = map[actor[+0x1df]]`); **the full 26-byte record is now decoded** (`+0` power, `+0x02` strike-Y offset, `+0x04`/`+0x06` move/phase counters, `+0x08`/`+0x09` homing speed + tracking flag, `+0x0a` impact-effect selector, `+0x0b` trail texture page, `+0x0d` sound cue, `+0x0e` list-mode flag, `+0x12`/`+0x16` effect-id lists; `+0x0c` is an unused `C`/`E`/`G` designer tag) - see [`docs/formats/move-power.md`](../formats/move-power.md). The move-id space is the spell-table id space, so the records label cleanly: idx `0x10..=0x2b` = the named monster special-attacks (`0x25..=0x74`), idx `0x01..=0x0f` = the unnamed internal enemy-attack tiers (`0x04..=0x1f`).

The scale stage `FUN_801dd864` (8×8 element-affinity matrix `0x801F53E8` + status bits + the summon magic-power tail `roll += roll*(power-1)>>3`) and the finisher `FUN_801ddb30` (resistance bits, `rand%9+8` floor, 9999 cap, spirit-gauge, MP drain, stat debuffs) are now fully traced - see the `FUN_801dd864` / `FUN_801ddb30` rows in `functions.md` and the three-stage chain in `battle-formulas.md`.

**Ported:** the closed-form roll + scale arithmetic is now pure kernels in `legaia_engine_vm::battle_formulas` (`summon_attacker_roll` / `summon_defender_roll` / `summon_predamage` / the `apply_*` helpers / `heal_summon_amount`), hand-tested against the disassembly.

**Residual:** (1) the arts/physical kernel is now **wired into the live loop for monster special-attacks** - the move-power table loads onto `World::move_power` (`engine-core::move_power::MovePowerCatalog`, PROT 0898) and `cast_spell_on_slots` overrides a damaging monster cast's magnitude with `arts_physical_predamage_lazy` seeded by that move's `+0` power (`World::enemy_move_predamage`: INT from `battle_accuracy`, defense terms from `battle_defense_split`; the attacker ×2 + defender ×1 `rand()` draws are taken up front and the bonus pair is drawn **lazily**, only when the bonus arm fires, so the shared RNG cursor advances by exactly three or five draws matching `FUN_801dd0ac`'s call order; gated on the table being installed so disc-free battles keep the placeholder + RNG stream).
The player-driven **summon** roll is now wired too (`World::player_summon_predamage`): summon-body HP/INT seed from the namesake `battle_data` creature record, caster INT from `battle_accuracy`, the caster magic-power byte from the character record's spell list (`+0x13D` ids / `+0x161` levels, the `FUN_801dd864` search), and the closed-form `FUN_801ddb30` finisher applies - including the per-caster summon power-percent table `0x801F5468` ((char_id-1)*8 + summon_element; PROT 0898 file `0x26C50`, parsed as `ElementAffinity::summon_power`, byte-pinned: own 100, opposed 40, Gala dark 60). Remaining residue: the live slot-7 actor's HP at roll time is modelled as the creature record's spawn HP (a mid-battle summon that has taken damage is not modelled), and status/guard default to none;

(2) the `FUN_801ddb30` finisher's **closed-form finalisation arithmetic is now ported** (`battle_formulas::damage_finish` - equipment elemental-resistance halving / guard halve / `rand%9+8` no-damage floor / summon power-% scale / 9999 cap - plus `spirit_gauge_fill`, both unit-tested); only its state-mutating tail (damage-popup accumulator, AI revenge table, MP drain, per-element stat-debuff switch) stays in the live battle context; (3) the affinity matrix `0x801F53E8` is now located + parsed off the disc (`legaia_asset::element_affinity`, PROT 0898 file `0x26BD0`, same link base as the move-power table) together with the per-character element table (`0x801F5480`: Vahn=fire/Noa=wind/Gala=thunder/Terra=wind), the matrix orientation is corrected (`matrix[attacker][defender]`;
the retail values are a ±4% nudge - diagonal 96 / opposite-pairs 104 / default 100, not a ×0/×2 weakness table), and the enemy element source is **pinned from the `FUN_801dd864` disasm itself**: the scale stage reads it **record-direct** - `lbu …,0x1d(record)` where `record = 0x801C9348[slot-3]` (the per-enemy record-pointer table, not a copied live-actor field) - so the element is `MonsterRecord::element` (`+0x1D`) consumed exactly as the parser exposes it (the same record the victory-spoils path reads `+0x44/+0x46/+0x48` from). This supersedes the earlier "loader copies `+0x1d` into `actor[+0x1d]`, copy not yet pinned" framing; the curated-element correlation (four party-table ids reproduce exactly + byte ∈ `0..=7` across every populated record) now only corroborates the id *labelling*.

**Wired (both directions):** the monster special-attack path scales by `matrix[enemy_element][party_member_element]` (`World::enemy_affinity_pct` → `enemy_move_predamage`), and the **player Seru-magic** path scales by `matrix[summon-creature element][target element]` (`World::cast_affinity_pct` in `cast_spell_on_slots`): the attacker element resolves off the summon **creature** by name (`World::summon_attacker_element`, the engine-side slot-7 `+0x1d`), the defender by slot (`World::battle_slot_element`). The player multiply is post-roll on the deterministic cast output (RNG untouched); the enemy scale is applied *inside* the roll, before the conditional bonus-arm threshold (so a non-neutral value can shift the lazy bonus draw - faithful to retail's scale→bonus order).
Both are gated so an uninstalled / neutral table reproduces the no-affinity baseline bit-identically (magnitude + RNG stream), keeping disc-free battles deterministic. The player-summon **base** magnitude is still the caster-state stand-in (the faithful slot-7 summon roll is open), so the player direction is the ±4% nudge on a placeholder, not yet byte-exact. See [`battle-formulas.md`](../subsystems/battle-formulas.md#element-affinity-matrix-fun_801dd864-0x801f53e8). The `0x801F4F5C` **arts** power table is located + parsed (`legaia_asset::move_power`), the `param_1` → move-id map resolved (`0x801F4E63`),

and **every record field decoded** (power / strike-Y offset / move + phase counters / homing speed + tracking flag / impact-effect selector / trail texture page / sound cue / list-mode flag / on-contact + launch effect-id lists; `+0x0c` is an unused designer tag with no runtime reader) - see [`docs/formats/move-power.md`](../formats/move-power.md). The auxiliary tables the record's selectors index are now parsed too: `EffectAuxTables` for the `+0x12`/`+0x16` effect-id lists' `0x801F6324` prototype-pointer + `0x801F6418` SFX tables, and `parse_impact_effect_table` for the `+0x0a` `0x801F53D4` config words (this corrects an earlier "pointer table" mislabel - the `0x801F53D4` entries are packed `u32` config words, not pointers).

**The `0x801F6324` spawn entries are decoded.** Each is an overlay VA to a *variable-length move-VM scene-graph record* in the **exact summon-part format** (`+0x00 i16 model_sel`, `+0x02 u16 flags`, `+0x04` move-VM bytecode), spawned by `FUN_80050ed4` → the shared stager `FUN_80021B04` → the ported move VM, with `model_sel` indexing `DAT_8007C018` - the same machinery as `legaia_asset::summon_overlay`. The earlier "~0x20-byte struct" reading was a coincidence (packed records, not a fixed stride). The high-bit (`0x80`) list bytes route instead to the 2D `efect.dat` pool (`FUN_801dfdf0` → `EffectCatalog`, ported as `spawn_by_ui_id`).

Render wiring reuses the summon parser + move VM. The `model_sel` additive base `gp[0x754]` (global `0x8007BA6C`) - only *read* in the corpus - is **resolved from the save corpus**: it is `0` whenever no battle effect-model library is resident, and **`party_count + 2`** when a battle has installed it - `3` for the 1-member training party (Vahn alone), `5` for the 3-member party (Vahn / Noa / Gala). A PCSX-Redux exec-bp on `FUN_80021B04` first pinned the value `3` (probe `autorun_summon_model_base`, confirming the full `FUN_801e09f8 → FUN_80050ed4 → FUN_80021B04` chain - `ra = 0x80050F08`, `a3 = 0x1000`, prototype table `0x801F6324` + effect-list id `0x22` live in registers); reading `0x8007BA6C` + the party count `0x80084594` across the whole mednafen corpus generalised it.
So the base **tracks party size** (the two fixed pool slots + the live party-character meshes precede the effect-model library), and `model_sel` is *library-relative* - `DAT_8007C018[model_sel + gp[0x754]]` lands on the same library model regardless of party size; only the library offset shifts. There is **no per-summon base** - one per-battle value drives both move-FX and summon-part spawns. Pinned by `crates/mednafen/tests/summon_model_base.rs`.

The engine **renders the move-FX scene-graph**: `World::spawn_move_fx` parses a move's spawn-entry records (`MoveFx` via `MovePowerCatalog::fx_for_move_id`), stages them as a `SummonScene` at the effect-model library base (the engine registers PROT 0871 at a fixed `DAT_8007C018[3..]` and `model_sel` is library-relative, so this is the retail `party_count + 2 = 3` case for the 1-member slice; the layouts are equivalent), and drives them through the ported move VM (`tick_move_fx` / `active_move_fx_part_draws`; `play-window` `H` debug-spawn) - reusing the summon machinery wholesale, so it shares the same interpreted-transform caveat. A spawn also surfaces the move's two presentation fields: the **trail texpage** (`+0x0b` → `0x7700 + id`) on `World::active_move_fx_trail_texpage()`,
and the **sound cue** (`+0x0d`) as `World::take_pending_move_fx_cue()`, which the host routes through the now-ported `FUN_8004fcc8` dispatch decode (`legaia_engine_audio::classify_cue` → `CueDispatch`; `voice_pitch` for the voice arm). The 2D afterimage *draw* `FUN_801e1ab0` (the streak pass that consumes the trail texpage) is ported as the pure `legaia_engine_render::afterimage::build_afterimage_quad` - jittered semi-transparent `POLY_FT4` (per-corner `rand` wobble, brightness band, UV/CLUT/texpage layout) from four projected corners + the trail id.
The corner projection is ported too: `FUN_800195a8` (the camera-coupled GTE billboard projector - view-space MVMVA center, ±half-size corner fan-out, rotation+translation reset, RTPT×3 + RTPS; see the [`functions.md` detail](functions/renderer.md#800195a8)) is `legaia_engine_render::billboard::project_billboard`, with the exact `FUN_801e1ab0` call shape (`+0x120` Y push, dynamic half-width `state+0x6c6 − 0x200`, half-height `0x100`) as `afterimage::project_streak_corners`; the `RotMatrix*` sin/cos LUT is pinned as `trunc(4096·sin)` by the disc-gated `gte_sin_lut_real` oracle.
What remains: the live note-on wiring of the resolved cue; and the retail draw transform of a move-VM scene-graph part itself (the `FUN_801F811C` / PROT-0900 reading of that transform is **resolved-as-unrelated** - `FUN_801F811C` is the 2D screen-mask widget, see the PROT 0900 resolved block in the summon-visual row - so the part-draw transform question moves to the `FUN_80021DF4`-family render tail, with the engine's anim-bank-derived draw staying an explicit interpretation). `FUN_80021DF4` is now **live-captured as the part render-tail**: in the enemy "Fire Tail" mid-cast frames the single live move-FX part-actor binds it at `actor[+0xC]` (disc + library gated `firetail_movefx_liveness`; see the Fire-Tail note below).
The **SFX program bank is pinned**: the cue's `program`/`tone` (static `DAT_8006F198` table, [`sfx-table.md`](../formats/sfx-table.md)) index the **per-scene music VAB** the BGM sequencer already has open (`FUN_80065034` reads the libsnd current-bank globals; byte-identical to the disc `music_01` VAB for that scene), so firing a cue is `SfxBank::play_one_shot(spu, scene_vab)` - no separate bank.

**`0x801F4F5C` is special-attack-only:** the id→index map covers 44 ids (internal tiers `0x04..=0x07`/`0x12..=0x1F` + named attacks `0x25..=0x74`); the basic-attack / art bands `0x08..=0x11` and `0x16..=0x18` are unmapped (pinned by a live capture - a party member's Tactical Art carries an unmapped id, e.g. Vahn's Somersault `0x0F`, so it would roll against the zero-power record 0). A party member's arts therefore do **not** use this table - they take their damage from the per-strike *art-record* power byte (which `art_strike.rs` already does, faithfully); the only remaining engine stand-in is `apply_basic_attack`'s flat `art_strike_damage_default` for a no-art generic hit.


### Stat growth-rate source

*Status:* resolved + validated + wired (core + opt-in jitter)

The per-character stat-grant source is **static `SCUS_942.54` tables read by the level-up applier `FUN_801E9504`**. Fully decoded: the parameter block at `DAT_80076918` is **per-character (stride `0x3C`), 8 contiguous 6-byte sub-records `{u16 start, u16 max, u8 jitter, u8 row}`** - `start` = base stat (**Gala matches the new-game template on all 8**), `row` selects one of 3 curves at `DAT_800769CC`. Per-level gain = `max(1, (max-start)×curve[row][level-1]/0x24C0 + rand()%(2×jitter+1) − jitter)`, then caps. The divisor `0x24C0` is the **curve normalizer** (each curve sums to `0x24C0`, so growth accumulates to exactly `max-start` by L99).

**Validated** byte-exact against a single-level capture (Noa L2→L3, the `noa_levelup_*` saves): all 8 deltas within the core ± jitter band - the earlier "~4.8x overshoot" was an artifact of the unreliable multi-level corpus observations (`noa/gala_4_level_jump`), not the formula. Parsed by `legaia_asset::level_up_tables::GrowthTables::{char_params,level_gain_core}` (disc-gated test). The "Seru struct `+0x74`" reading stays **falsified**.

**Engine wiring done (deterministic core, all 8 stats):** `StatGain` carries HP/MP + the six battle stats; `LevelUpTracker::with_growth_tables` + `BootSession` install per-character curves from the user's SCUS, replacing the flat 10/5 placeholder, and `apply_to_record` grows the record-side window then mirrors to live (disc-gated boot test pins Noa's L2→L3 core). The per-level `rand()` jitter is also **modeled (opt-in)**: `LevelUpTracker::with_level_up_jitter(seed)` drives a faithful PSX BIOS-rand LCG (`BiosRand`) drawing one `rand()` per stat per level on the unfloored core before the `max(1,…)` floor - off by default so determinism oracles stay bit-identical (bit-exactness still needs the runtime BIOS-rand seed).

**Remaining:** only the slots-1/2 XP correction. See [`subsystems/level-up.md`](../subsystems/level-up.md#stat-gains).

### Monster stat-record archive source

*Status:* resolved

The monster archive is **PROT entry `0867_battle_data`** (extended footprint; the 15.9 MB archive lives in the entry's trailing-gap sectors). `FUN_800542C8` streams per-monster `0x14000` LZS slots at `(id-1)*0x14000`, each `[u32 dec_size][LZS]` decoding to a block whose head is the `FUN_80054CB0` stat record (name `@0x00`, battle-model TMD offset `@0x04` - **not** XP/drop, which are inline at `@0x44..0x49` - HP `@0x0C`, MP `@0x10`, stat u16s `@0x0E/0x12/0x14/0x16/0x18/0x1A`, magic count `@0x4A`, spell-ptr array `@0x4C`).

Pinned by a live-battle PCSX-Redux watchpoint (`autorun_monster_record_source.lua`) - relative seek `(id-1)*40` sectors + `disc_read` CdlLOC → PROT.DAT `0x38AF000` = entry 867; three records match live actor stats byte-for-byte. Retail-semantically the archive **is** the `monster_data` block: the define `monster_data 869` names extraction entry 867 under the raw-TOC −2 correction ([`cdname.md`](../formats/cdname.md#numbering-space)) - the earlier "misleading `monster_data` stub at 869" reading was the filename shift.

Parser `legaia_asset::monster_archive`; bridge `legaia_engine_core::monster_catalog::catalog_from_monster_archive` wired into `enter_field_scene`. The record is now fully decoded: all six stats are named (ATK/UDF/LDF/INT/SPD/AGL), rewards are inline at `+0x44..0x49`, and `+0x04` is the monster's **battle-model TMD** offset (not XP/drop - see the mesh thread below).

### Monster mesh + texture pool

*Status:* resolved

The monster's 3D battle model is a [Legaia TMD](../formats/tmd.md) embedded in each PROT 867 archive block at the offset in stat record `+0x04` (installed at battle-actor `+0x230`; the `0x1C`-stride records `FUN_80049858`/`FUN_800495C8` walk are its object table).

**186/194 slots parse cleanly.** The texture/CLUT pool at record `+0x08` is decoded from the battle loader `FUN_80055468`: a `0x1E0`-byte region of fifteen 16-colour CLUTs followed by a 4bpp page (always 256 rows tall, 128 or 256 texels wide; palette = `cba & 0x3F`). Byte-exact vs pool sizes; renders to recognizable atlases. The on-disc CBA/TSB are nominal defaults the loader relocates per slot, so the raw pool does not appear verbatim in a battle VRAM dump - the loader layout is the ground truth. Parser `legaia_asset::monster_archive::{mesh, MonsterMesh::texture}`; CLI `--obj` + `--texture-png`; WASM `monster_mesh_*` + `monster_texture_*` accessors drive the enemy-table site page's per-row WebGL viewer (textured + directional-lit).


### Terra slot-3 / story-flag overlap

*Status:* resolved

The **header-size constant drifted**: `RETAIL_CHAR_RECORD_HEADER_SIZE` was `0x66F` (the *name* field) but the true record base is `game+0x3C8` (live RAM `0x80084708`), with the display name at internal offset `+0x2A7`. Confirmed across six in-game RAM captures: mid-game stats at `record+0x104`/`+0x11C` read back the expected per-character HP/MP for all four slots. The four-slot array runs into the global region, so slot 3 (Terra)'s tail (record offset ≥ `+0x2BC` = `game+0x12C0`) aliases the story-flag bitmap and inventory; Terra's meaningful fields (name, live stats, RecordStats) sit before that boundary. There is **no special case** - Terra is the New Game template's fourth roster entry (HP 400) but never a savable battle-party member, so the tail aliasing is benign.

The constant is now `0x3C8`, `legaia_save::CharacterRecord` gains a `name()`/`set_name()` accessor at `NAME_OFFSET` (`+0x2A7`), and the off-by-`0x2A7` that made `Party::from_retail_sc_block` read stats from the wrong fields on a populated save is fixed (proven by synthesising an SC block from a live RAM dump and checking the parsed HP).


### Battle party meshes = **assembled from the player battle files** (PROT 1204 = Baka Fighter / default-equipment sibling)

resolved (static chain + byte-verified) - A real main-game battle renders the party from a **per-character merged TMD the engine assembles at battle setup** out of that character's player battle file (`data\battle\PLAYER<n>`, extraction 0863..0866), selecting one section per equipment slot by the **equipped item ids** (char record `+0x196..+0x19A`).

Chain: `FUN_80052770` case 4 (section select) → `FUN_80052FA0` (assembler, blob at `ctx+0x50`) → `FUN_800536BC` ×5 (object splice; `nobj += section_nobj`, bone-id byte per object, surplus objects tagged = equipment visual meshes) → `FUN_80053898` (retag 200/201/100+, attach bones at `blob+nobj`, sort) → `FUN_800513F0` registers `blob+0x18` into `DAT_8007C018[slot]`. Full format + chain: [`formats/battle-data-pack.md`](../formats/battle-data-pack.md) + [`formats/character-mesh.md` § Battle form](../formats/character-mesh.md#battle-form---assembled-from-the-player-files). This also closes the **weapon-mesh / `nobj` 15→17** hunt: the +2 are the weapon + Ra-Seru sections' extra objects (NOT `FUN_8001EBEC`, which only toggles a pose transform).

**This supersedes two earlier conclusions in turn** ("battle reused the field pack 0874 §0", then "battle renders PROT 1204 directly"). The 1204 attribution rested on partial vertex-pool matches (12/17 for Vahn in the full-party Gobu Gobu save): those 12 are the **default-equipment sections' geometry, byte-shared** between the player files and 1204; the 5 equipped-variant objects (Hunter Clothes body ×2, Survival Knife piece + extra, the equipped Ra-Seru piece) match **only** the player-file sections and appear nowhere in 1204. Byte-verified in the full-party save: `DAT_8007C018[0] = ctx+0x50+0x18` exactly, `nobj=17`, bone bytes `[0..14,200,201]`, attach `[5,8]`, and **all 17 vertex pools** found in PLAYER1's sections with equipment-selective matches.

The **Baka Fighter minigame loads PROT 1204** (`overlay_baka_fighter` loads `data\field\other5.lzs` + PROT 1205/1206, debug `"OTHER5 %d %d"`) - its bundled meshes are the same characters with default equipment, which is why earlier captures during Baka Fighter sessions pinned 1204. Field-pack distinctness still stands (`battle_char_pack_real::battle_pack_is_distinct_from_field_pack`); parser for 1204 `legaia_asset::battle_char_pack`.

**Loader - pinned (write-watchpoint).** The captured battle loader `FUN_800520F0` `tmd_register`s PROT `0x36a` into the *effect* window `DAT_8007C018[3..]` (`etmd.dat`), not the party `[0..=2]`. The party-mesh install into `[0..=2]` is **static SCUS**, through the generic registrar `FUN_80026B4C` (store `0x80026BA8`), from two battle state-handlers:

**`FUN_800513F0`** (lead/active actors - `tmd_register(*(actor+0x50)+0x18, 0)` in a `while<3` loop over the active-actor table `0x801C9360`, right after the `FUN_80052FA0` palette decode) and **`FUN_800542C8`** (additional members - per-member loop bounded by `*(rec+0x4a)`, `tmd_register(*(*rec+4), 0)`). Both are reached **indirectly** (state-handler dispatch), so a static cross-reference on `0x8007C018` finds no writer - which is why this was long mis-assumed to live in an overlay.

Pinned by a `DAT_8007C018[0..2]` write-watchpoint across the auto-starting Queen Bee field→battle transition ([`autorun_battle_party_mesh_install.lua`](../../scripts/pcsx-redux/autorun_battle_party_mesh_install.lua)): all three installs fire at `game_mode 0x15`, and the installed pointers byte-match the battle form (Vahn → `0x80165F48`, the value a battle save holds in `DAT_8007C018[0]`). Dumps `funcs/800513f0.txt` / `800542c8.txt`.

**Superseded on the texel source:** the runtime battle bands are uploaded from the **player battle files' per-section texture pools** at the static rect table `0x800775B8` (`FUN_80052FA0` → `FUN_80053B9C` LoadImage front-end; ≥99.6% band reproduction vs clean full-party battles). The 1204 atlases hold the same default-equipment content - which is why they matched 73–98% - but the shortfall was the equipped-variant texels; 1204 is the default-equipment sibling/fallback, not the runtime source. See [`battle-data-pack.md`](../formats/battle-data-pack.md) § "Texture-pool VRAM placement".

**Battle render = load-time TSB/CBA relocation (this supersedes the "nominal CBA / no-relocation / VRAM-residue palette" model below, which is FALSIFIED).** At battle entry the party-setup overlay rewrites every prim's TSB+CBA into a packed per-slot runtime band:

**Vahn** (640,0)/(704,0)·rows490/491 → **(512,256)/(576,256)·row481**; **Noa** (640,256)/(704,256)·492/493 → **(640,256)/(704,256)·row482**; **Gala** (512,0)/(576,0)·494/495 → **(768,256)/(832,256)·row483**. CBA column preserved; both disc rows of a char collapse to one runtime row (one 256-colour palette per char). The disc TSB/CBA are an **authoring layout** the Baka Fighter minigame uses directly; normal battles relocate it. Pinned by dumping the runtime TMD (`flags=1`, abs pointers; convert `p→p−base−12`) from a clean battle save and reading its relocated prims - they render the correct characters from the save's VRAM; the disc mesh walked as-is renders incoherently.

The `0x8007BEC0` table (`FUN_800198E0`) is the **scene** renderer's, not characters - the earlier reading that routed character CLUTs through it, and the "rows 490..497 are scene-residue party palette / dolk→town01→map01 recipe", are **falsified** (rows 490..497 hold *scene environment* palette shared by a scene's field+battle modes).

**Palette - resolved (all three party palettes decode from the disc; see the end of this entry for the solution).** It is a **battle-allocated** resident block DMA'd to rows 481/482/483. In a clean full-party battle save the three blocks are contiguous at **`0x800ebee8`/`0x800ec0c8`/`0x800ec2a8`** (Vahn/Noa/Gala), a fixed **`0x1E0` (480-byte) stride = 15 × 16-colour sub-CLUTs, one per disc mesh object** - matching both the per-object CBA columns read off the runtime TMD and the 15-object disc form.

It is ≠ the field char palette (set test: only 10 of Vahn's 130 battle-novel colours - and **0** of Noa's/Gala's - in any field-pack CLUT) and ≠ the bundled atlas CLUTs = Baka (**146 of Vahn's 256** runtime colours appear in *no* CLUT the 1204 pack ships → a genuinely distinct asset, not a recolour).

**It is character-intrinsic and produced fresh at battle load** (mednafen bracket: name-entry / front-of-Tetsu / load-initiating saves all lack it; it appears as a single copy only once the battle is up, byte-identical between the Tetsu and Drake fights). The work-arena is `memset`-zeroed at load by the `sw $zero` loop at SCUS `0x80055F14` (`base=*(0x8007BD3C)`, `0x1e8d` words), then sparsely filled - the palette sits at `arena_base+0x4048`.

**It is not a stored disc blob - exhaustively:** absent uncompressed (full row + every 32-byte sub-CLUT window across all PROT/`SCUS`/`init_data`), not the CLUT of any of 6372 strict TIMs, 0 hits in the LZS-*container* sections of all entries, AND **not the decompressed output of any LZS stream at any offset** in the battle/scene/character entries (town01 bundle `0003..0011`, `0865`/`0867`/`0871..0876`/`0896`/`0900`/`1204`, output windows to 24 KB - past the `0x4048` depth) nor anywhere in the ≤2 MB corpus (1 KB windows). Brute tool: `lzs-decode find` (validated).

Since it is deterministic yet stored nowhere verbatim, it is **assembled at battle entry.** **Assembler pinned (write-watchpoint, `autorun_battle_palette_writer.lua`, clean Tetsu fight):** `FUN_80053B9C` (per-colour store `sh a0, 0x894(v0)` at `0x80053C6C`) copies a source CLUT struct `[u16 base][u16 count][BGR555]` into the per-char block at `dst = arena + slot*0x1E0 + (base+idx)*2`, **OR-ing `0xFFFF8000` (STP/bit-15) onto every non-zero colour**. So the runtime palette is bit-15-**set** (`0x9D40…`) and the disc source is bit-15-**clear** (`0x1D40…`) - which is why all prior brutes (bit-15-set needle) missed. Source pointer `s0 = *(*(0x801C92F0)+8) + per-char-off` → a transient `0x800Dxxxx` buffer.

**Solved - source = the Vahn player battle file, extraction PROT `0863` (raw TOC `0x361` = `PLAYER1`), LZS-compressed (bit-15-clear).** A write-watchpoint on the source struct header `0x800D6C98` shows it is filled by `FUN_8001A55C` (LZS decoder); the decoder's input buffer byte-matched the extraction `0861` window at a fixed delta (237-window match) - the same data: `0861`/`0862` are 1-sector stubs whose over-read tail begins Vahn's file `0x1000` in, and the TOC pins extraction `0863`'s start at exactly the live-traced `0x36E8000` (see [`cdname.md` § numbering space](../formats/cdname.md#numbering-space)).

**Palette now solved byte-exact (all 3 bands).** Running `FUN_80052FA0`'s decode+assembly *as a unit* (decode `record[0]` + the 5 staged sub-records into one work buffer, read CLUTs at the header offsets) reproduces the live Vahn battle palette **byte-exact, all 3 bands** - `base=0x00` = `record[0]`'s CLUT B, `base=0x40` = sub#0's trailing CLUT, `base=0x70` = sub#4's trailing CLUT. The earlier "29/32, 3 diffs = equipment patches" was a **budget-less scratch decoder**, not a data problem: `FUN_8001A55C`'s first arg is an **output-byte budget** (decremented per literal AND per match-copied byte; loop `while budget>0`); ignoring it runs off the stream into the next record. `legaia_lzs::decompress` already honors this, so the port is one `decompress(stream, budget)` per record.

**Source = extraction PROT `0863`** - `"data\battle\PLAYER1"` is a dev-tree label that resolves (raw TOC index `char+0x360`, `FUN_8003e8a8`) to the per-character battle-file cluster, not an ISO9660 file. The record is self-describing relative to `record[0]` (`+0`=desc-table off, `+4`/`+8`=CLUT A/B *decoded* offsets, `+0xC`=budget; descriptor entries `[id, running_a, size]` run while `a[i+1]==a[i]+size[i]`, `id==0` = section separator). On disc the 5 sub-records are **scattered** (Vahn: `0x1C000/0x28800/0x66000/0x85800/0xA2000`), located by `sec_base=align_up(recbase,0x1000)`; sub0..3 = `sec_base + a[entry after each internal separator]`; sub4 = `rec0 + (a_last+size_last)`.

The `0x2000` stride is only the RAM buffer the loader stages - the parser derives the scattered disc offsets directly, **no capture needed**. Every prior byte-brute missed only because it used the bit-15-**set** runtime needle, not the disc bit-15-**clear** form. Clean-room parser **`legaia_asset::battle_char_palette`** (`find_record0` + `parse_record`; synthetic unit test + disc-gated `battle_char_palette_real` which passes byte-exact against extraction PROT `0863` with `record0` at file offset 0 - the identical digest the historical `0861`-window run produced; STP bit-15 set on upload). Tetsu fight is Vahn-only so Vahn (863) is byte-exact-validated + wired.

**Noa = PROT 0864, Gala = PROT 0865** - pinned by matching each `record0` CLUT (header-read, no derivation) against full-party battle VRAM captures (the mednafen full-party battle captures hold rows 481/482/483 all populated): Noa→row482 98%, Gala→row483 100% (1-2% misses = equipment patches in the late-game captures).

**Noa wired** via `collect_palette` (record0 CLUT A/B + each section separator's id=0 unequipped-default trailing CLUT + the final record, filtered to the columns her mesh samples). The equipment loader (`FUN_80052770` case 4) picks per section an equipment-id-matched entry OR the id=0 separator (unequipped default); the mesh-column filter resolves which variant belongs to the character.

**Gala wired - all three party palettes now decode from disc.** Party order confirmed (a full-party capture's char names ASCII at `0x80084708+n*0x414+0x2A7` = Vahn/Noa/Gala → row 483 = Gala).

**Player-file load traced:** the retail ISO9660 open `FUN_800608f0` is a `trap` stub, so `FUN_800558fc` always takes its debug branch → `FUN_8003e8a8(char+0x360)` reads `toc[idx+2]` (in-RAM PROT TOC `0x801C70F0`) as a **sector offset into PROT.DAT**: Vahn(0x361)=PROT.DAT 0x36E8000, Noa(0x362)=0x3791000, Gala(0x363)=0x3828800 (222 sec=0x6F000), Terra(0x364)=0x3897800 - four contiguous player files = extraction entries **0863/0864/0865/0866**, whose TOC starts equal those offsets exactly (raw index − 2; the historical "Vahn = 0861" matched the same bytes through the preceding 1-sector stubs' over-read window).

**The bug:** `sec_base` is `rec0 + align_up(recbase - rec0, 0x2000)` - the `0x1000` alignment matches Vahn/Noa but lands Gala's subs on a zero-padded `0x7000` block (his data starts at `0x8000`). Fixed → Gala's subs decode, bands @0x00/@0x30/@0x50/@0x80 cover all mesh cols at **100%** vs row 483. Wired (slot 2, PROT 865, rows 494/495); disc-gated `noa_gala_collected_palettes_cover_mesh_columns`. Probe `autorun_clut_decode_capture.lua` captured the 5 sub-record streams that pinned this.

**Retraction (corrects an over-claim):** an interim reading said the palette was "LZS-decompressed from the `town0c` scene bundle at `0x23430`"; that write-watchpoint actually caught the **scene bundle's** LZS decompression into the *shared* work-arena (the captured `0x800ebee8` value `0x7965481F` ≠ the Vahn palette `0x409d…`). The party palette is a separate, later write; the scene-decompress part holds but is not the palette source.

**Remaining:** write-watchpoint the *final* party-palette write in a clean Tetsu/Drake fight (writer PC + source regs) to recover the assembly. (PCSX-Redux capture is flaky - segfaults intermittently - and the user's bracket saves are mednafen, which can't drive live watchpoints.)

**Viewer status:** the falsified residue scaffolding (`battle_char_true_vram_bytes`, `paint_scene_party_cluts`, `BATTLE_CLUT_SCENES`) is removed; the Battle form renders the 1204 geometry+textures with the bundled (authoring) palette - visually ≡ the Baka form, and labelled as the authoring/Baka palette - until the true per-battle palette is pinned by the overlay capture. `battle_char_mesh_cba_tsb` stays **nominal** (disc CBA, matching the bundled CLUT rows), which is correct for that authoring-layout render.

The party-mesh trace is in `funcs/8002541c.txt` / `800198e0.txt` / `800520f0.txt`. <details><summary>Archived: the (mis-premised) battle-CLUT investigation</summary>**The battle character textures + palettes both come from disc, just by different paths.** **Images:** the PROT 1204 atlases ARE the real battle character textures (not placeholder), uploaded to VRAM pages 512..960 @ y=0/256.

**CLUTs:** sourced from the **active field scene's decompressed sec0 TIM_LIST** (LZS-compressed on disc) - every CLUT a played map01 battle uploads (rows 490/495/496/497/498/499) is byte-present in `0086_map01` sec0 decompressed and renders as a character palette (e.g. row 498 → recognizable Noa face).

**Upload path (fully traced):** `FUN_800520F0` (battle loader) → `FUN_800198E0` (per-TIM uploader) → `FUN_800583C8` (PsyQ `LoadImage`) → `FUN_8005A1C0` (GPU-queue enqueue, op-type 8 = `FUN_80059BD4` via handler table `0x80078D0C`) → ring `0x801C9590` → `FUN_8005A4A0` flush → `FUN_80059BD4` (GP0 0xA0 / DMA2).

**The "relocation" is not a per-battle VRAM allocator** - each scene's character TIMs declare their own CLUT rows, the upload puts the CLUT there, and `FUN_800198E0` records `table_0x8007BEC0[texpage & 0x1f] = clut_row`. The battle renderer resolves each primitive's CLUT **row** from this **texpage→CLUT-row table** (`0x8007BEC0`, 32×u16), overriding the TMD2's nominal CBA row (the CBA still supplies the sub-CLUT x). So the party palette band shifts between captures (the reference battle capture 492/494 vs a map01 battle 490/495..499) simply because different scenes declare different rows for the same character.

**Falsified along the way (do not re-walk):** "PROT 1204 atlases are placeholder" (images are real); "bundled PROT 1204 CLUTs are the battle palettes" (they're wrong defaults, 0/256 vs retail); "the band is loaded by a battle disc read" (battle-init reads are party-independent - `FUN_800520F0` pulls only monster/effects/music); "it's LZS-decoded at battle entry" (`FUN_8001A55C` hook = zero palette hits); "it's a transient buffer not on disc" (it IS on disc, in scene sec0, just not as a contiguous raw blob - and the upload source is the resident decompressed scene buffer, freed only on scene change not per-frame).

**Engine implication:** to match retail, the viewer/engine should source the battle character CLUTs from the active scene bundle's sec0 (decompressed) and apply the per-battle row allocation - not from PROT 1204's bundled default CLUTs.

**Viewer-fix limitation (Noa/Gala-present-scene hunt, negative):** only **Vahn's** battle palette is cleanly recoverable - `map01` sec0 row 490 pairs correctly with the 1204 Vahn atlas (world-map Vahn renders in battle-form), but it's just his row 490 (not 491). For Noa/Gala, **no scene's sec0 CLUTs pair with the 1204 battle atlases**: scanning every scene bundle found full-party-ish CLUT rows (0400_doman 488-492, 0061_dolk, PROT 1200 other4 490-494) but rendering the 1204 atlases with any of them yields garbage - those are field-form (PROT 0874) / other-pack palettes, not the battle-form palette the 1204 atlas needs.

So the battle-form Noa/Gala palettes are scene-resident/runtime-composed and not a static disc asset pairing with the atlases; a faithful all-3 viewer fix would need save-state palettes (Sony bytes, disallowed) or a full port of the runtime per-scene character-texture composition. The viewer keeps the bundled CLUTs (the scene-sourced Vahn-only overlay was tried and reverted as net-worse). Tooling: [`autorun_clut_upload_hook.lua`](../../scripts/pcsx-redux/autorun_clut_upload_hook.lua) / [`autorun_clut_upload_watch_live.lua`](../../scripts/pcsx-redux/autorun_clut_upload_watch_live.lua) (live upload `(rect,src)` capture), [`autorun_clut_uploader_pc.lua`](../../scripts/pcsx-redux/autorun_clut_uploader_pc.lua) (read-watchpoint that pinned `FUN_80059BD4`),

[`autorun_find_clut_decode.lua`](../../scripts/pcsx-redux/autorun_find_clut_decode.lua), [`autorun_battle_char_clut_source.lua`](../../scripts/pcsx-redux/autorun_battle_char_clut_source.lua) + [`map_clut_disc_reads.py`](../../scripts/pcsx-redux/map_clut_disc_reads.py); functions in [`reference/functions.md`](functions.md) (`FUN_80059BD4` / `FUN_8005A4A0` / table `0x80078D0C`). <details><summary>Full investigation trail (archived)</summary>The PROT 1204 atlas **images are the real battle character textures** - not placeholder. (2) Each battle TMD samples a clean, self-consistent `(CLUT row, sub-CLUT, tpage)` set (decoded properly via `tmd_to_vram_mesh`, not the earlier garbage byte-window scan):

**Vahn** rows 490/491 (sub-CLUTs 0,1,4,5 / 0,1,7,8) pages (640,0)/(704,0); **Noa** rows 492/493 (sub-CLUTs 0,1,2,5,6,7 / 0,3,4,8) pages (640,256)/(704,256); **Gala** rows 494/495 pages (512,0)/(576,0); **aux1** row 496 page (448,256); **aux2** row 497 page (512,256). So PROT 1204's atlases are uploaded at exactly the positions the TMDs sample. (3)

**But the bundled PROT 1204 CLUTs are the wrong defaults** - direct value comparison of PROT 1204's bundled row-492 CLUT vs a retail battle capture's VRAM row 492 is **0/256** and not any channel swap (the viewer renders Noa's pants green where retail is red, hair orange where retail is dark-red - a uniform per-character palette mismatch, not a shader bug). Rendering Noa's atlas with the **retail** captured row-492 CLUT yields correct brown skin tones; with the bundled CLUT yields wrong purple/gold.

**Where the correct CLUTs live (resolved above: scene-resident/runtime-composed).** Only **Vahn's** row-490 CLUT exists verbatim on disc - LZS-compressed in map01/map02 sec0 as a flag-`0x80000008` 256×1 TIM (the reserved high bit makes `parse_strict` reject it, which is why all TIM tooling + raw greps miss it).

**Noa (492) and Gala (494) palettes are not verbatim anywhere** - not in any raw PROT entry, not in any LZS-decompressed player.lzs/flat-streaming section (1204/1205/1206 are uncompressed copies of the same wrong defaults), not in PROT 0874/0876, not in PROT 0865 (battle_data) records. The **CLUT band (rows 490..497, x=0..255) is byte-identical across seven captured save states - six progressive battle-load frames plus a separate gobu-gobu battle - and absent in non-battle saves** (the boot/opdeene/town captures = 0%): so it is **battle-context-loaded and then persists in VRAM**, not boot-global and not per-battle-recomputed.

It is **never in main RAM** in any captured save (checked every 32-byte sub-CLUT window across all party rows) - a transient **decompress→DMA-to-VRAM→free** upload that completes *before* the "encounter triggered" frame, faster than manual save granularity. The battle scene is **map01** (world map; `*(0x80084540)=0x55`), party Vahn/Noa/Gala, so the non-Vahn CLUTs are pulled by the **battle-entry party-load path**, not the field scene. Per-scene row-49x 16×1 CLUTs (35 scenes incl. town01) are field-actor palettes (0% value match to battle Noa) - a red herring.

**Battle-init disc reads are party-INDEPENDENT** (PCSX-Redux probe, sstate8 Vahn-only vs sstate2 full-party - byte-identical raw-TOC index set; raw → extraction is −2: monster `0x365`→867, conditional stream + `etim` + `etmd` `0x367/8/9`→869/870/871, `efect` `0x36B`→873, `readef` `0x380`→894, overlay `0x384`→898, `0x37A`→888, music raw 1016, field-scene re-read `0x5A`→88).

**No character-CLUT read fires at battle entry** - the party CLUTs are resident in VRAM before the fight. Proper-decode (validated: finds Vahn490 in map01 sec0) of 871/872/873/875 + 0865 battle_data + 1202-1206 + 0874 all empty for Noa/Gala.

**Key state finding:** the mednafen opdeene + town01 full-party captures hold the band absent (0%) - so the band is *cleared* at certain field transitions and *reloaded* entering battle; the sstate2 probe missed the reload because sstate2 was already band-present.

**Decisive - the band is a non-LZS GPU upload** (PCSX-Redux probes on band-absent slot 4 + battle-initiating slot 5): VRAM dumps show row 490 (Vahn) full but rows 492/494 (Noa/Gala)

**Empty at battle-init** - they load later as the battle renders. Hooking the universal LZS decoder `FUN_8001A55C` and scanning every decompressed output for the Noa row-492 signature over 3000 frames of battle (incl. advancing via CROSS) yields **zero hits** - the palettes are never LZS-decoded. Combined with party-independent battle reads + total absence from main RAM (even mid-battle), the band is uploaded by a **LoadImage/GPU-DMA from a source freed within the upload frame** (Vahn's source persists as the field-scene buffer at `0x800e96a0`, the only one ever in RAM).

**Uploader pinned - `FUN_80059BD4`** (LoadImage-equivalent; `a0=RECT{x,y,w,h}`, `a1=src_ptr`; see [`reference/functions.md`](functions.md)), reached via the once-per-frame upload-queue flusher `FUN_8005A4A0`. The [`autorun_clut_upload_hook.lua`](../../scripts/pcsx-redux/autorun_clut_upload_hook.lua) probe hooks its entry and captures every band upload's `(dest rect, source ptr)` + dumps the source.

**Captured (slot 4/5):** rows 488/490/497/498/499 + the row-495/496 effect sub-CLUTs upload from scattered RAM sources (byte-matching the reference battle capture 100%); Vahn's row-490 source is the resident field buffer `0x800E9690`.

**Noa/Gala (rows 492/494) do not upload at battle-init** - they enqueue only when the party characters actually render during combat, which headless input (CROSS hold/pulse) can't reliably drive (it flees or diverges; live `getVRAM`/`takeScreenShot` are nil/GL-gated in this build).

**Interactive capture done** ([`autorun_clut_upload_watch_live.lua`](../../scripts/pcsx-redux/autorun_clut_upload_watch_live.lua), played the slot-5 fight with all chars attacking): the battle character images upload via `FUN_80059BD4` (pages 512/576/640/704/768/832/864/960 @ y=0) and band CLUT rows 488/490/495..499 upload too (256-wide rows match the reference battle capture's same rows 100%).

**But the reference battle capture's Noa(492)/Gala(494) palettes appear in none of those uploads** - so the per-character CLUT **row assignment is battle-context-specific** (this encounter places party palettes at different rows than the reference capture's did). The uploaded CLUT RAM sources are **not verbatim raw on disc** (490/497/498/499 = 0 raw hits) - LZS-compressed or runtime-composed.

**Cleanest deterministic finish (no more emulator runs):** Ghidra-trace the **enqueuer** that pushes character CLUTs into `FUN_8005A4A0`'s ring during battle-actor render (reveals the per-character source + composition rule + disc origin), or match each captured CLUT RAM-source address against the LZS-decompressed scene/befect buffer resident there. Other tooling shipped: [`autorun_battle_char_clut_source.lua`](../../scripts/pcsx-redux/autorun_battle_char_clut_source.lua) (disc-read logger), [`map_clut_disc_reads.py`](../../scripts/pcsx-redux/map_clut_disc_reads.py), [`autorun_find_clut_decode.lua`](../../scripts/pcsx-redux/autorun_find_clut_decode.lua) (LZS-output scanner),

[`autorun_clut_uploader_pc.lua`](../../scripts/pcsx-redux/autorun_clut_uploader_pc.lua) (read-watchpoint that pinned the uploader).</details></details>

### MP-cost ability-bit priority (half vs quarter)

*Status:* resolved (dump-confirmed)

Reading the block in `overlay_battle_action_801e295c.txt` settles **both** open questions. It is inlined twice: `0x801E4568` in state `0x28` (right after that state's capture-archive `jal 0x8003EC70` at `0x801E44EC`) and `0x801E3D0C` in state `0x3C` (right after that state's Pomander `+0x1DF == 0xFE` case at `0x801E3C4C`). The two are byte-identical, so which state a citation names does not change the answer - but the pairing above is the one the dump supports. (1)

**PRIORITY - Half (`0x20`) wins.** The code is `andi 0x20; bne <half>` then `andi 0x10; beq <none>`, i.e. `if (bits & 0x20) {half} else if (bits & 0x10) {quarter}` - the `0x20` test short-circuits the `0x10` test. This matches the docs / `MpCostModifier::from_ability_flags`; the engine SM port + live cast path that applied Quarter first were a guess and are now flipped. (2)

**FORMULA - it subtracts a right-shifted copy, not a floor-divide.** Half = `cost - (cost>>1)` (rounds up on odd costs); "MP-quarter" = `cost - (cost>>2)` = **pay 3/4** (shave 25%), not `cost/4`. The engine's `base_cost/2` / `base_cost/4` were both corrected (`battle_formulas::mp_cost_after_ability_bits`); all three cast paths (two SM blocks + `cast_spell_on_slots`) now route through the shared helper. MP cost consumes no RNG, so determinism oracles are unaffected.


### Scripted Tetsu encounter → Battle (v0.1 oracle Battle leg)

*Status:* mostly

The v0.1 oracle now reaches **Battle** from a new-game cold boot: `BootSession::begin_new_game` seeds the opening party (Vahn, 180 HP) - the Tetsu fight is the game's first battle, so the new-game state *is* retail's pre-fight story state (there is no earlier save to seed from) - the cold boot installs town01's sparring carrier from its MAN, and the field-VM dialogue-accept engages it (`v0_1_playthrough.rs::v0_1_battle_leg_reaches_battle_from_new_game`, converging with the cataloged retail Field/Battle anchors). Earlier framing (below) assumed a save-seed was needed; it is not, for the opening fight. The formation is pinned - a lone monster, archive id `0x4F` (Tetsu), `EncounterRecord::rim_elm_training()` - and reachable end-to-end via the arm API (`training_battle.rs`).

The launch mechanism is pinned (`FUN_801DA51C` decomp + corpus RAM): the encounter carrier is a **dedicated MAN-placed field entity** (not the player ctx) that, on reaching SM state 1, copies its `entity[+0x94]` formation into cell `0x8007BD0C` and via the `case 2/3` fall-through writes `_DAT_8007B83C = 8` (the battle handoff). It is **dialogue-driven, not scene-entry-driven**, and **not a script-borne inline arm op**: an opcode-aware walk of town01's MAN partition-1 scripts finds zero `[1][0x4F]` arm sites,
so the carrier installs **town01 MAN formation index 4** by pointing `actor[+0x94]` at that table row - and the pointing op itself is now pinned as the standard scripted-battle install `3E FF 04` (third bullet below). The carrier is pinned to town01 P1's placement at tile (76, 65) / model `0x6A` (the sparring partner).

**Engine:** the field-carrier SM tick exists (`tick_field_carriers` / `install_field_carriers` / `engage_field_carrier`) and reaches Battle via formation index 4 (`training_battle.rs`); the carrier set is now **derived from the scene MAN** (`man_field_scripts::derive_field_carriers` + `World::install_field_carriers_from_man`), so the sparring carrier's identity and placement come from the real actor-placement partition. The engage is now **driven by the field-VM dialogue-accept**, not a manual API: a field-interact op (`0x3E`, `op0 < 100`) on the carrier's placement arms the engage (`World::field_carrier_slots` → `pending_carrier_engage`) and accepting its prompt (the `0x4C` n5 sub-4 dialog dismiss) engages it.

`training_battle.rs` drives this end-to-end on disc data, reaching Battle with Tetsu without `engage_field_carrier`. The interaction probe is now ported faithfully: `World::tick_field_interaction_probe` (clean-room `FUN_801cf9f4`) runs retail's `DAT_801f2254` facing probe - a radius-64 compass point ahead of the player's facing, box-tested at ±72 against the talkable NPCs' placement positions (`World::field_npc_positions`) - and on the action button talks to the matched NPC and turns the player toward it, so facing the sparring partner and pressing X starts the fight with no script injection (`training_battle.rs::training_reaches_battle_via_interaction_probe`).

This relies on the **runtime actor frame == MAN placement frame** finding: `FUN_8003A1E4` spawns at `tile*128 + 0x40` via `FUN_80024C88` with no anchor, and the player cold-spawn `0xA40` is `tile 20*128 + 0x40` in that same frame (the apparent mismatch in an earlier town capture was a *patrolling* NPC).

**Auto-navigation now closes the emergent path:** `World::nav_step_toward` drives the player along a BFS route over the real collision grid, so the v0.1 oracle's emergent Battle leg (`v0_1_playthrough.rs::v0_1_battle_leg_walk_talk_accept`)

**walks** the player from the cold-boot spawn to the partner, **talks** via the probe, and **accepts** → Battle, with no teleport.

**Carrier-reposition finding:** the carrier's MAN placement tile `(76, 65)` is its *post-tutorial* village spot - in a town01 sub-area not walk-reachable from the spawn (BFS: 2855 reachable sub-cells, carrier not among them; town01's MAN spans several door-connected sub-areas). The opening sequence repositions the partner next to Vahn for the tutorial (`RIM_ELM_SPARRING_CARRIER_TUTORIAL_POS` = world `(2752, 1856)` ≈ tile `(21, 14)`, a ~6-tile reachable hop, pinned from the dialogue-accept capture whose `actor[+0x90]` resolves to the `(76,65)`/`0x6A` record - same carrier). The cold boot skips that reposition, so the emergent test places the carrier at its tutorial position first.

**All three former residuals now derived from disc bytes:**

- *Formation-row selection (the "index 4 selection bytecode"):* the install is
  the standard field-VM scripted-battle op **`3E FF 04`** in `P1[10]` at record
  offset `+0x7F7` (MAN body `0x01B67`) - the same case-`0x3E` direct-install
  arm as garmel's Zeto (`3E FF 09`) and rikuroa's Caruban (`3E FF 11`) -
  sitting in the post-"Come at me!" branch (`WaitFrames 16` + flag sets ahead
  of it; the adjacent `Test 0x227`/`JmpRel` targets land on op boundaries, the
  decode-coherence cross-proof). Row 4 = the lone-Tetsu (`0x4F`) formation.
  Pinned by
  `rim_elm_sparring_carrier.rs::town01_p1_10_carries_the_tetsu_3e_ff_04_install`.

- *Opening reposition (bytecode-derived, no longer a bare constant):* town01
  MAN partition-1 record `P1[10]` (`start 0x01370`) carries, twice, at record
  offsets `+0x1D`/`+0x28` (MAN-body `0x0138D`/`0x01398`), the field-VM op
  `4C 51 15 0E 07 22` = `MenuCtrl` nibble-5
  `NpcRun { x_enc: 21, z_enc: 14, depth: 7, move_id: 0x22 }` (`field_disasm`
  `MenuCtrlKind::Nibble5NpcRun`; the dialog-NPC walk-to-tile-with-run path).
  Tile `(21,14)` → world `(21*128+64, 14*128+64)` = `(2752, 1856)` =
  `RIM_ELM_SPARRING_CARRIER_TUTORIAL_POS` exactly, and `P1[10]` is the unique
  record NpcRun-ing to `(21,14)`. The two consecutive identical ops are the
  standard story-flag two-branch scene-entry prologue that hops the carrier next
  to Vahn's spawn tile 20. (Op `0x23 MOVE_TO` is *not* the mechanism - its only
  hits are false decodes in the desyncing dialog region.)
- *Yes/No selection (not a field-VM opcode):* the spar Yes/No is an MES-embedded option picker inside the NPC's inline `0x1F` dialog segment - a `0x29` menu-open followed by an `N*2`-byte signed relative-jump table (handler `FUN_80038050`, the `FUN_80039B7C` dialog-SM family). The commit branch is computed directly: `new_pc = (open + 1 + index*2) + i16_LE(entry[index])`. Ported as `legaia_mes::Picker::jump_target` + `InlineDialogueRunner::last_choice` (`crates/engine-core/src/inline_dialogue.rs`). There is no separate read-and-compare opcode - which is why these interaction records desync under linear disasm (the picker/text bytes alias opcodes).

## Field / locomotion

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| Town/field free-movement locomotion | resolved | `capture` | [details ↓](#townfield-free-movement-locomotion) |
| Field ambient animation - what makes jou's ground pulse and the water shimmer | resolved | `disassembly` | Three mechanisms: the bundle type-6 CLUT-walk table (12 carriers, 9 of them field scenes), the ambient move-VM tree the MAN P1 placements install at entry, and jou's flesh pulse = the mode-3 CLUT-cell HSV cycler (`FUN_80019D50`, lightning = flag `0x364`). Full chain + two move-VM decode corrections: [`field-ambient-fx.md`](../subsystems/field-ambient-fx.md). |
| Ambient render-mode 4 - what the op-`0x1E` seat animates | resolved | `disassembly` | A **cyclic VRAM-rect scroller**, and it is what makes waterfalls fall. Per fired period (`+0xC6` drained by the frame step alone, no speed scalar) the render tail rotates the seated rect `+0xD0..+0xD6` left by `+0xCC * frame_step` and up by `+0xCE * frame_step`, each axis as StoreImage / MoveImage / LoadImage over a bump-allocated strip (`80021df4.txt` `0x80022CB8..0x80022EE0`). Seventeen scenes carry one at plain entry; sixteen scroll upward over a texture-band rect, `tunnelc`'s second seat scrolls a CLUT row sideways. Ported as `engine-core::world::ambient::vram_scroll`. [details ↓](#ambient-render-mode-4---the-vram-rect-scroller) |
| Master ambient record 0 - what reads the 8-byte rows | resolved (it is not a stager record at all) | `disassembly` | The **per-scene sound-effect descriptor bank** for cue ids `>= 0x200`. Both SFX readers resolve those ids as `*(u32*)0x8007B8D0 + offsets[0] + (id - 0x200)*8` - i.e. record 0 of whatever bundle is installed at `0x8007B8D0`, which in field mode is the scene prescript bundle (`field_asset_loader` `0x8001F850..0x8001F864` stores scene buffer + `0x12800`). `offsets[0]` is the identical word `FUN_800252EC` reads for stager id 0. Rows are the 8-byte descriptor of [`sfx-table.md`](../formats/sfx-table.md), category 3 (a variable VAB slot). [details ↓](#master-ambient-record-0---the-per-scene-sfx-descriptor-bank) |
| town0e's morph-record installer | resolved (the census was shape-blind, not the disc) | `disassembly` | The install is the ordinary `0x34` sub-3 arg 0 (stager record 1) - it just rides **partition-1 placement 29**, a full placed actor with dialogue, as that record's second instruction, ahead of its `SysFlag.Test 0x1A` park/seat branch. A census that discriminates by script *shape* reports nothing here; the rule that finds it is the entry-slice one below. Residual: that the entry pre-run slice reaches it is an inference from the settled pre-run mechanism, not a capture. [details](../subsystems/field-ambient-fx.md#town0es-installer-is-a-placed-actor-not-an-effect-actor) |
| Which op-`0x34` sub-3 installs fire at **scene entry** | resolved | `disassembly` | The ones the placement spawn-prologue slice (`FUN_8003A1E4`) executes - not a distinguished kind of script. The pre-run is gated on the record's first opcode being `0x24`/`0x25`, and the slice breaks after an opcode whose full byte is `0x21`; both nops, only one ends the slice. Ported as `engine-core::man_field_scripts::scene_entry_ambient_installs`. [details ↓](#which-op-0x34-sub-3-installs-fire-at-scene-entry) |
| Scene bundle type-7 slot content (VDF) | resolved | `disassembly` | The scene's vertex-morph delta pack (61 bundles populated; jou = 17 sub-entries), installed at `DAT_8007B7DC` via `FUN_8001FBCC`, consumed by the morph stager `FUN_8001C604`. Parser `legaia_asset::scene_vdf`; format in [`field-ambient-fx.md`](../subsystems/field-ambient-fx.md#mechanism-3---strip-cycling-and-vertex-morphs). |
| VDF morph render substitution - what draws the staged vertices, and what arms the lanes | resolved | `disassembly` | Per drawn group of a part whose flags carry op-`0x0A`'s bit `0x1000`, `FUN_8001ADA4` (`0x8001B424..`) calls `FUN_8001C604` (scratch copy + weighted-delta blend + group vertex-pointer retarget) and restores the pointer after the draw. Arming = op-`0x0A` **mesh** stager parts in the ambient tree (`pack slot = model_sel - 5`; rikuroa 69/70 behind flags `0x281`/`0x282`, town0e 10/11, jagaroom 20/21); weights ramp via `FUN_80020740` steered by op-`0x32` envelope flags. jou arms nothing at entry (cutscene op `0x1F` only). Ported to all three render surfaces; details in [`field-ambient-fx.md`](../subsystems/field-ambient-fx.md#the-vdf-vertex-morph-chain). |
| What opens an inn stay in retail? | resolved (the premise was wrong) | `capture` | There is no inn trigger, because there is no inn *session*: retail composes a stay inline in the scene MAN out of generic ops (dialogue, an MES picker, an op-`0x4E` gold gate, op-`0x3A` `ADD_MONEY`, fades) and then one `4C 82 <slot>` per member. That opcode is the only inn-specific thing in the engine. Charge and restore are decoupled, so free rests are the same tail minus the gate. Ported as `op4c_n8_sub2_restore_party_slot`; the old "party-page mirror" label was wrong. [details](../subsystems/field-menu.md#inn-stay-there-is-no-inn-screen) |
| Field collision-map source | resolved (headline corrected: the `.MAP` supplies the base grid) | `disassembly` | [details ↓](#field-collision-map-source) |
| Tile-board grid mode | resolved | `disassembly` | The `_DAT_8007b450`/`DAT_801f35c0`/`801ef2b0` tile-grid walk is a puzzle / board minigame (procedural `rand`-filled board, per-cell drawn tiles), not town locomotion. It is a field-overlay (`0897`) construct driven from the field/event VM (op `0x49`); the `_DAT_8007b450` refs in the hub minigame overlays are only the shared equip-comparison layout hint `FUN_801e5b4c`, not board use. The `func_0x800467e8` facing remap is a quantized 45° octant rotation. Boards are always procedural; no fixed board exists. **There is no `FUN_801e0b1c`** - a mis-based dump alias of `0x801EF334`, interior to `FUN_801ef2b0`. Instruction detail, corrected tile values and the unverified heap claim: [`tile-board.md`](../subsystems/tile-board.md). |
| game_mode 0x03 = field/town gameplay | resolved | `capture` | [details ↓](#game_mode-0x03--fieldtown-gameplay) |
| Scene prescript: field-VM event scripts vs move-VM stagers (dual consumer) | resolved | `capture` | **Single consumer.** The op-`0x34` sub-3 operand census across every scene MAN shows every prescript record is a **move-VM stager**: partition-1 effect-actor records stage the ambience on entry (record 0 = the master ambient record in 62 scenes), partition-2 cutscene timelines install the per-shot ids. Id space = record index (the RAM `[u16 count][u16 offsets]` relocation at `_DAT_8007b8d0`, live-pinned vs the file bundle). The "field-VM runs a record" premise was the engine's own fallback, not retail behaviour. See [scene-bundles](../formats/scene-bundles.md) § consumer census. |
| Engine VRAM byte-exactness for town01 | resolved (major source); minor residue | `capture` | [details ↓](#engine-vram-byte-exactness-for-town01) |
| CLUT row 510 population (env meshes' `(64,510)` CBA) | resolved (boot-resident system-UI strip band); residue = the exact boot walker call site | `capture` | [details ↓](#clut-row-510-population-boot-resident-system-ui-strip-band) |
| Scene-transition (`0x3F` door) destination indexing | resolved | `capture` | [details ↓](#scene-transition-0x3f-door-destination-indexing) |
| Intra-town (house / interior) door mechanism | resolved | `disassembly` | [details ↓](#intra-town-house--interior-door-mechanism) |
| Field/town environment-geometry placement | resolved (renders) | `capture` | [details ↓](#fieldtown-environment-geometry-placement) |
| Overworld / town entrance story-flag gating | resolved | `capture` | An entrance's unlock is its own partition-2 record's C1/C2 gate (`FUN_8003BDE0`; C1 = one-shot latch, C2 = requires-all) against the system-flag bank `_DAT_80085758`. Ops `0x50/0x60/0x70` (SET/CLEAR/TEST) carry `idx = ((opcode & 0x8F) << 8) \| operand` (raw flag number). Disc-pinned via `man-scripts --system-flag-census`: map01 keikoku portals `C1=[0x193]` (setter `vozz` P1[7], the only `0x193` SET disc-wide, byte-pinned by `chapter1_hub_depth_oracle.rs`), mist walls `P2[34..36] C1=[0x482]`, town01 dinner chain `P2[4]`→550→`P2[5]`→551. The dinner "re-fire" is falsified. Full write-up in [world-map.md](../subsystems/world-map.md) + [field-locomotion.md](../subsystems/field-locomotion.md). |
| Overworld story-conditional destination (`dolk`→`dolk2`) | resolved (mechanism + engine port) | `capture` | Beyond the record-level C1/C2 gate, an entrance record can switch its `0x3F` target by an in-record op-`0x70` `SysFlag.Test`. `map01`'s dungeon entrance (`P2[1]`/`P2[2]`) branches on flag `0x142`: clear → `dolk` (pre-boss), set → `dolk2` (post-boss), same trigger + arrival tile. `overworld_portal_sites` decodes the conditional `0x3F` pair (`ConditionalDest`); the seeder resolves via `World::system_flag_test` (`chapter1_boss_spine_oracle` Part D). **Falsifies** "dolk2 is reached from a dungeon interior". The `0x142` setter is now pinned (rikuroa streaming-carrier script records; see the spine `0x142` row). See [world-map.md](../subsystems/world-map.md). |
| Retail-vs-engine NPC + story-flag state parity across the capture library | resolved (breadth oracle); residuals filed as their own rows | `capture` | The sweep oracle `crates/engine-core/tests/field_npc_state_parity_disc.rs` compares every catalogued field-mode library capture against a cold engine entry with the capture's `DAT_80085758` bank seeded byte-for-byte: park/place visibility, seat position within the patrol-locality bound, heading (diagnostic), post-entry flag neutrality. Divergences are classified in-test (`KNOWN_DIVERGENCES`); the dominant class is capture-mid-beat dynamics - a mid-visit choreography re-arranged NPCs after retail's own entry, while the engine reproduces the FRESH-entry arrangement (cross-pinned by sibling captures, e.g. rikuroa `pre_caruban`). |
| Entry pre-run channel slice ends on a no-mask `4C 70` wall paint | resolved (slice-continue landed) | `disassembly` | All four nibble-7 paints CONTINUE - but **not** by the mechanism first claimed. There is no shared continue label and no label-call idiom: `0x801E3624` is the *epilogue*, all four sub-ops genuinely return, and advances differ (subs 0/1 `+6`, subs 2/3 `+7`). The slice continues because the **caller loops**; breaks come only from an executed `0x21` NOP, a stalled PC, or a next opcode whose `& 0x7F` is `< 0x20`. Detail: [`script-vm.md`](../subsystems/script-vm.md). |
| Writer of the Rim Elm opening flag (`549`) | resolved (self-latching script SET; the census was width-blind) | `capture` | Writer = **town01 `P2[3]` itself**, one site: a plain `52 25` SET at body `+0x3` in the very record its C1 gates (the rikuroa-`P2[50]`/`0x142` self-latch shape). A second site once reported in a `gameover_data` "dev copy" was town01's own MAN seen through a neighbouring block's window. Runtime-pinned first (reader-watch from `s2_rimelm_town01`: SET `ra 0x801E3598`, script-PC `+0xF`), then found statically: the preceding `4C ED` op had no width in the disassembler, so the walk desynced one byte short - the old "capture-only" verdict was **width blindness**. See [script-vm.md](../subsystems/script-vm.md); anchor `flag_549_writer_is_the_rim_elm_p2_3_self_latch`. |
| Field `.MAP` PROT resolution - which entry holds a scene's map | resolved (census-pinned; engine resolver corrected) | `capture` | [details ↓](#field-map-prot-resolution---define--2-universal) |
| World-map CLUT cycling beyond the ocean head | closed (operand table + emitter + cadence pinned) | `capture` | [details ↓](#world-map-clut-cycling-beyond-the-ocean-head---closed-operand-table--emitter--cadence-all-pinned) |
| `init_data` UI-tile page residency; the map03 terrain column | resolved (both premises falsified) | `capture` | [details ↓](#init_data-ui-tile-pages---journey-dependent-residency-resolved-map03-texture-column-resolved---not-uploaded-premise-falsified) |
| What transitions retail into game over? | resolved | `capture` + `disassembly` | Retail has **no** mode-`0x12` transition. A party wipe exits battle to mode 2; MAIN INIT `FUN_8003AEB0`'s back-from-battle arm (gated on the `DAT_8007BD60 & 0x80` survivor latch **and** story-flag idx 0 = the scripted-loss latch, raised by field-VM op `4C EA`) stores `game_mode = 0x16` (CARD INIT) with `_DAT_8007BB00 = 1` at `0x8003B5D4`, landing on the **title screen with CONTINUE preselected** - no GAME OVER art, no dedicated menu. Every store PC captured live (`autorun_gameover_mode_writer.lua`). Mode 18/19 + PROT 0902 confirmed an unreachable dev harness. The port's three-row session stays an engine invention. [details](../subsystems/battle.md#party-wipe--the-game-over-overlay) |
| Mid-visit NPC re-arrangement beats (dolk2 market crowd; garmel pre-Zeto staging) | resolved | `disassembly` + `capture` | dolk2: the swap is `P2[11]`, spawned by the `.MAP` fallback walk-on-trigger rows (C1=[`0x27C`], C2=[`0x142`]) - eight `CC <crowd> E3 <day>` seats (op `4C` nE sub-3, `0x801E3108`) put P1[53..60] on the day cohort's tiles and `A3` parks the day cohort at `(127,127)`. garmel: the Zeto stager `P2[12]` materializes P1[3]/P1[4] beside the player (n3 sub-7 player-coord copy `0x801E0FB0`); post-battle re-entries run `P1[0]`'s flag-consume arms. See [script-vm.md](../subsystems/script-vm.md#mid-visit-npc-re-arrangement-beats-dolk2-market-swap--garmel-boss-staging); pinned by `engine-core/tests/man_midvisit_rearrangement_disc.rs`. |
| Region story-flag gate families (record-header C1/C2 gates) | resolved as structure (play-order residual on the open page) | `capture` | [details ↓](#region-story-flag-gate-families) |
| Extraction-0874 §2 (`player.lzs`) F-variant pixels | resolved - installing event named | `capture` + `disassembly` | [details ↓](#extraction-0874-2-playerlzs-f-variant-pixels---a-one-shot-opening-face-frame-stamp-not-a-menu-writer) |
| Who latches the clip-end bit for a conversation's cross-context clip pokes | resolved (port residual named) | `disassembly` + `capture` | The **poked actor's own anim tick**, on the poked actor's own `+0x62`. `FUN_8003C83C` short-circuits target `0xF8` to the live player object out of `_DAT_8007C364` before its actor-list walk, so an NPC record's `A2 F8 <clip>` / `AC F8 08` / `AD F8 08` reads and writes the *player's* clip words. [details ↓](#clip-end-latch-for-cross-context-clip-pokes) |

### Clip-end latch for cross-context clip pokes

*Status:* resolved from the disassembly of both halves and ported; residual is
port fidelity, not an open question

`ctx[+0x62]` is the clip-control word and bit `8` (`0x0100`) is the end latch
(see
[`script-vm.md`](../subsystems/script-vm.md#0x2b-0x33-flag-manipulation-triplets)).
The question was who writes that bit when the triple carries a `0x80`-prefix
target, because then the word does not belong to the record being dispatched.

**The resolver answers it.** `FUN_8003C83C` tests its argument against `0xF8`
before anything else and returns `_DAT_8007C364` - the live player object -
without walking any list (`li v0,0xf8` / `bne a0,v0,0x8003c858` /
`lw v0,-0x3c9c(v0)` / `jr ra`). `0xFB` walks a second list for the entry whose
`+0xC` handler is `0x801DA51C`; every other id walks `_DAT_8007C354` matching
`*(u16*)(ctx+0x50)`. So a cross-context op runs against a **different actor
record**, and the answer to "who latches" is "that actor's own anim tick" -
the same `FUN_800204F8` a prop reaches, on a different struct.

Its two halves both matter to the spin, and both are in the instructions
(`ghidra/scripts/funcs/800204f8.txt`):

| Half | What it does |
|---|---|
| Binder (`0x80020570..0x800205A8`) | Only when `+0x5C != +0x5E`: remember the id, `sh zero,0x68` (cursor to frame 0), point `+0x4C` at the clip. `+0x62` is **not** touched - hold / clamp / reverse are the script's to set, and clearing the latch is its `AC <t> 08`. |
| Advancer (`0x800205AC..`) | Consume `+0x62` bit `0x200` (restart), clear bit `0x100`, step `+0x68` by `+0x6A` unless bit `0x2` (hold) is set, then wrap or clamp at either end and set bit `0x100` there. |

One detail the port flattens: the advancer scales its step by the scratchpad
byte `_DAT_1F800393` (`mult a0,v0` at `0x80020660` / `0x80020680`), the frame
step the driver writes. `PropAnim::tick` advances one step per call, which is
the same thing whenever that byte is `1`.

The idiom is therefore literally a prop door swing aimed at another actor, and
the retock innkeeper's Yes branch reads exactly that way once its bytes are
disassembled: `AC F8 01` (un-hold), `A2 F8 04` (poke the clip), `AC F8 08` /
`AD F8 08` (clear, spin), `AC F8 03` (un-clamp), `4A 03 00`, `AB F8 03`
(re-clamp), `AC F8 08` / `AD F8 08` again, then `A2 F8 02` handing the player
back to the locomotion move. Every one of those bits is an `ANIM_*` bit; none
of them is a per-record local flag.

**Port.** `engine-core::field_env::PropAnimBank` holds a cross-context cursor
per target byte (`actor_clips`, keyed the way the resolver keys its walk).
`World::step_inline_dialogue` binds the poked actor's `+0x62` into the
executing context around each `2B`/`2C`/`2D`, mirrors it back, and **parks** on
the spin - the same bind / re-sync / mirror-back discipline
`World::step_prop_interaction` runs a prop's whole record under, narrowed to
the one word a cross-context op reaches. The runner writes no latch of its own -
`PropAnim::tick` is the port's only latch writer, everything else that touches
`+0x62` is a script op arriving through the bind - and the cursor advances once per
frame whichever driver reaches it first (the field frame's
`tick_prop_interactions`, or the runner itself for a host that drives only a
conversation). Pinned by `crates/engine-core/tests/inline_clip_latch.rs`,
which asserts the latch appears on the poked actor's cursor and never on the
record's own flag word, and that a stalled cursor never lets the spin through.

*Residual (port, not RE).* An actor's **drawn** clip and its cursor are two
objects in the port: the player's gesture is played by the host's
`FieldPlayerAnim` off `World::field_player_move_cues`, an NPC's by the host's
own clip player, while the latch is timed by the bank's cursor. They are
rate-matched by construction (`ANIM_SPAWN_RATE` = 8 cursor units against
`FieldClipPlayer::DEFAULT_TICKS_PER_FRAME` = 2) and sized from the scene ANM
bundle where it resolves the poked id, but a clip the bundle cannot name falls
back to a stand-in length. Retail has one struct; folding the port's two into
one is engine work. Separately, no capture has confirmed that *nothing else*
sets bit `8` at runtime - a script could set it with a `2B <t> 08` of its own,
and none of the records read so far does.

### Ambient render-mode 4 - the VRAM-rect scroller

*Status:* resolved - decoded from the disassembly and ported

The seat is move-VM op `0x1E`: `+0x5A = 4` then seven operands into `+0xC4`
(period reload), `+0xCC` / `+0xCE` (per-period horizontal / vertical step) and
the rect `+0xD0..+0xD6`, in that order (`80023070.txt`
`0x80023694..0x800236F0`). `+0xC6`, the live countdown, is deliberately *not*
seated, so a freshly spawned part fires on its first tick.

The render tail's arm (`80021df4.txt` `0x80022CB8..0x80022EE0`) drains `+0xC6`
by `DAT_1F800393` **alone** - unlike the mode-3 sibling it does not fold in
the `DAT_1F80037D` speed scalar - and tests the underflow with
`sll v0,0x10; bgez`, so it fires exactly on the tick the stored halfword's
sign bit sets. On that tick it reloads the period and rotates the rect: per
axis, `FUN_8005842C` captures the leading strip into a buffer bump-allocated
off `0x1F8003A0`, `FUN_80058490` slides the remainder over it, `FUN_800583C8`
re-inserts the strip at the far edge - a cyclic rotation, horizontal first
then vertical.

Seventeen scenes put one on screen from their plain scene-entry ambient
tree, resolved by walking the records through the move VM. A *linear* scan for
the op word over-reports badly - the records jump, so a linear pass finds
`0x1E`-shaped bytes inside operand streams. Sixteen of the seventeen scroll
vertically only, upward, over a rect at `x >= 0x200`: falling water and energy
columns. The seventeenth is `tunnelc`, whose second seat is a full-width
one-row rect at `(0, 508)` stepping **right** - a CLUT row, walked by the same
rotate primitive, since `StoreImage` / `MoveImage` / `LoadImage` do not care
what the texels mean. The per-scene rect table is on the mechanism page.

Port: `engine-core::world::ambient::vram_scroll`, applied in tick order by
`World::step_ambient_fx` (the rotate is destructive, unlike the mode-3 write
which is recomputed each frame from a cached capture). Disc-gated coverage
`crates/engine-core/tests/ambient_mode4_scroll_disc.rs`. Mechanism write-up:
[`field-ambient-fx.md`](../subsystems/field-ambient-fx.md#the-vram-rect-scroller-render-mode-4).

### Which op-`0x34` sub-3 installs fire at scene entry

*Status:* resolved - decoded from the disassembly and ported

`FUN_8003A1E4` - the pre-run the placement spawn loop calls per just-spawned
placement - carries its **own** copy of the per-actor script runner's frame
slice rather than calling `FUN_80039B7C`. Two branch facts in that copy settle
which installs are entry installs:

- `0x8003A480`: `lbu` the first opcode, `addiu v0,v1,-0x24`, `sltiu v0,v0,0x2`,
  `beq v0,zero,<skip>` - unless that byte is `0x24` or `0x25` the VM loop is
  skipped entirely and the record's script never runs at load.
- `0x8003A498..0x8003A4F4`: run while `(opcode & 0x7F) >= 0x20`; after
  dispatching, `beq s1,s4` against `li s4,0x21` breaks the slice on the **raw**
  byte `0x21` (so a cross-context `0xA1` does not break), as does an unchanged
  returned PC.

The consequence is the whole mechanism. `0x21` and `0x25` both disassemble as
"nop" and only one of them ends the slice, so a record written
`25 / 34 30 00 / …` fires its install in the load slice whatever follows it -
which is why a dialogue-bearing placed actor installs its ambient tree exactly
like a dedicated effect-actor script, and why an install placed *after* a `0x21`
(`edkorout` P1[15]) does not fire at plain entry at all.

Port: `engine-core::man_field_scripts::scene_entry_ambient_installs`, taking the
**unconditional prefix** of that slice - a deliberate under-approximation, since
a flag-gated install deeper in a record (`nilboa` P1[3], `suimon` P1[4]) depends
on runtime state a static census cannot resolve. Disc-gated coverage
`crates/engine-core/tests/ambient_entry_install_census_disc.rs`. Mechanism
write-up:
[`field-ambient-fx.md`](../subsystems/field-ambient-fx.md#which-installs-fire-at-scene-entry).

### Master ambient record 0 - the per-scene SFX descriptor bank

*Status:* resolved - the premise ("a stager record with unknown rows") was wrong

`0x8007B8D0` is a shared **current-bundle** pointer, not one subsystem's.
The field asset loader points it at the scene prescript bundle
(`0x8001F850..0x8001F864`: `lw v0,0xd8(s3)` with `s3 = 0x1F800314`, plus
`0x12800`), which is why `FUN_800252EC` reaches stager records through it at
all. Both sound-effect descriptor readers use the same slot for cue ids
`>= 0x200`, through the bundle's own offset table:

- `FUN_800250D4` (`0x800250FC..0x8002514C`) - `desc = base + offsets[0] +
  (id - 0x200)*8`, then `SpuKeyOn`s `+3 & 0x1F` consecutive voices.
- `FUN_80016B6C` (`0x80016C24..0x80016CB0`) - the cue-ring drain, same
  address arithmetic, and it hands bytes `+0..+4` to the designers' own
  `"setbl p:%d t:%d l:%d n:%d id:%d"` debug print.

`offsets[0]` is the identical word `FUN_800252EC` reads for stager id 0, so
"record 0" and "the runtime SFX bank" name one address. The disc agrees:
every populated row has category `+4 = 3` (a variable VAB slot), voice count
1-2, a level in the low 60s, and a zero `+5..+7` trailer - the layout of
[`sfx-table.md`](../formats/sfx-table.md)'s static table. Size is per scene
(jou reserves 96 rows and populates 40; `rugi` carries 21). jou's own tree
cues `0x20B` and `0x20E..0x211` - rows 11 and 14..17 of its own record 0.

The boot sound-bank loader `FUN_8001FA88` writes the same slot and then
immediately saves *its* bank's record-0 address at `gp+0x678`, precisely
because the next scene load overwrites the slot - so the two readings are
complementary, not contradictory.

### Town/field free-movement locomotion

*Status:* resolved

The player free-movement controller is `FUN_801d01b0` (field overlay 0897), pinned by a runtime write-watchpoint on `*(0x8007c364) + 0x14/0x18` (`autorun_player_pos_watch.lua`). It camera-remaps the held pad (`func_0x800467e8` + `FUN_80046494` → direction bits `& 0xf000`), computes a per-frame speed (`base_step * player[+0x72] >> 12 * DAT_1f800393`, with terrain-slow + diagonal modifiers), then steps the player position 2 units at a time with per-axis collision via `FUN_801cfe4c`. Sets facing `player[+0x26]`. Full write-up in [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md). The `801db81c..801dbf9c` cluster previously suspected here is the field *camera* system, not movement.

**Collision derivation - resolved (capture-proven; engine realigned).** `FUN_801cfe4c` is fully decoded (overlay `0897` @ `0x801CE818` + the on-disc bias table `DAT_801f2214`): three **leading-edge footprint probes** (~47 units ahead, ±16 lateral), each sub-cell derived as `zc = (z>>6)+2`, `xc = ((x+0x3f)>>6)−1`. Two cheat-free Rim Elm wall-press captures settled the long-open indexing question:
**The `+2` Z bias is authored into the wall bits.** In the down-press capture (`rimelm_wall_press_down`, screen-down = world `Z−`) the player legally rests at a position whose plain floor-indexed cell is an all-quads wall byte (unreachable under floor indexing); the biased read places that wall band one tile north, exactly where the press blocks with a step-exact 47-unit standoff. The left-press capture (`rimelm_wall_press_left`) pins the X side: probe reads the wall column's last sub-cell, one 2-unit step shallower reads clear; retail's `ceil−1` equals the floor except at exact 64-multiples (parity-unreachable). The **floor sampler** (`FUN_80019278`) reads the *same bytes* with plain floor indexing - one byte's two nibbles live under two world→cell mappings.
**Engine realigned with proof in hand:** [`World::field_tile_is_wall`] now uses retail's exact derivation (`sample_field_floor_height` keeps the floor, matching its own retail source). **The three-probe leading-edge footprint is wired too** (`World::field_dir_blocked` over the disc-pinned `DAT_801f2214` table - 48-unit edge in the positive directions, 47 in the negative, ±16 lateral - gated by `World::leading_edge_wall_probes` / `play-window --edge-collision`; the candidate-centre test stays the off-flag default for the oracles + nav drivers): driving the engine stepper over each capture's live grid reproduces both retail rest positions **byte-exactly** - and the full-scene legs reproduce them through a real `enter_field_live` scene entry.
**The actor-collision probe is decoded, modelled, and capture-classed.** `FUN_801cfc40` (bits `1`/`4`) walks the active-actor table `DAT_801c93c8`, box-testing the three `DAT_801f21b4` probe points (disc-pinned: 64/63 ahead, ±32 lateral - wider than the wall edge) against each actor: a static entity anchors at its MAN object record (`tile*128 + sub*16`) with the `0x40+0x10` half-extent; a moving actor uses its live position with caller extents (`±40` from the locomotion). The locomotion gates each 2-unit step on the actor bits and the wall bit together, so NPCs block exactly like walls.
The `rimelm_npc_press_tetsu` capture (player pressed into the sparring partner) pins the class from live RAM: the mutual `+0x98` collision link is active in-frame both ways and the NPC's `flags+0x10 = 0x08020884` carries the `0x20000` bit - **village NPCs take the moving-actor arm (bit `1`, ±40 box)**, not the static prop arm. Engine: `World::field_actor_dir_blocked` ports that arm over `field_npc_positions`, gated by `World::solid_field_npcs` / `play-window --solid-npcs`; disc-gated leg `npc_press_pins_moving_actor_arm`.
**The touch/interact dispatch and the static prop arm are decoded and modelled too.** `FUN_801d5b5c` (decoded from a live overlay image - the static 0897 copy is garbled at that VA) posts the touch event: player engaged flag `0x80000`, actor touched mark `0x100`, counters, facing saved to `+0x5A`, and the `FUN_8003c9ac` NPC-motion pause kick. The dispatch in `FUN_801d01b0` fires it automatically per contact step for static props (bit `4`), and on the just-pressed interact button through the third probe table `DAT_801f2254` (disc-pinned at overlay file `0x23A3C`: a radius-64 compass point per 45° facing sector, extents `0x20` → ±72 NPC box) for NPCs - with a face-the-NPC turn (`func_0x80019b28`).
The static-entity anchor formula (record footprint offset incl. the `+0x52 & 8` correction from record flag bit `0x8`) is live-verified against four captures' spawned static actors; the engine models props via `Scene::field_object_placements` collider centres (`field_prop_colliders_live.rs`) and the interact probe via `World::field_interact_probe_slot`.

**NPC motion and the prop walk-touch event are modelled engine-side.** Field NPCs walk: `man_field_scripts::placement_motion_route` decodes each placement's own pre-text `0x4C 0x51` move-to-tile waypoints and `World::tick_field_npc_motions` drives them through the ported motion VM (`FUN_8003774C`), live positions written back into `field_npc_positions` so the ±40 box and the interact probe follow (autonomous patrol opt-in via `animate_field_npcs` / `--live-npcs`; an interaction prologue's `0x4C 0x51` walks the interacted NPC regardless).
Cutscene-timeline **cross-context walks** are modelled too: a partition-2 record's targeted `0x47` yield (`C7 <id> <tx> <tz> <mode>`) parks the record on `CutsceneTimeline::walk_wait` and glides the target (NPC channel or the `0xF8` player anchor) to the tile at the op's own speed, with the paired `A2 <id> <move_id>` ExecMove surfacing the walk/idle clip cue - the town01 Mei walk-on beat's on-camera walk-in (see [script-vm.md](../subsystems/script-vm.md) § yield family).
The prop walk-touch posts for the decoded script classes: `placement_walk_touch_event` classifies genuine `0x3E` door-warps and cross-context player-channel `0x23` teleports, and `World::check_field_walk_touch` posts once per ±80-box contact through `trigger_field_interact` and applies the effect (disc-gated `field_npc_motion_disc.rs` / `field_walk_touch_disc.rs`).
**Residual (open):** the full `FUN_801d5b5c` post-kernel state (engaged flag, facing save/restore, `+0x2A`/`+0xA` touch counters), per-actor field-VM channel execution (yield-paced patrol scripts - the engine loops the decoded waypoints instead), the exact retail NPC glide speed, and prop scripts beyond the two decoded walk-touch classes. The interaction-end teardown is decoded: the dialog SM `FUN_80039b7c` exit path restores the actor facing from `+0x5A`, drains the `+0x2A`/`+0xA` touch-counter pair, and clears the player's `0x80000` engaged flag + `ctrl+0x60` when no interactions remain.
Disc-gated: `engine-shell/tests/field_collision_discriminator.rs` (probe-model + engine-rest legs); unit equivalence `world.rs::tests::field_tile_is_wall_matches_retail_subcell_derivation` + standoff `leading_edge_wall_probes_rest_at_retail_standoff`. Capture note: both wall-press sessions park in `town0c` holding a grid that byte-matches town01's - **resolved, not an anomaly**: town0c's own `.MAP` (PROT 0019, the universal `define−2` resolution) is byte-identical to town01's; PROT 0028 is `izumi`'s map, not town0c's (see the field `.MAP` resolution row below).

### Field collision-map source

*Status:* resolved

The collision grid at `*(_DAT_1f8003ec) + 0x4000` (1 byte/128-unit tile, high nibble = 4 sub-cell wall bits) is **painted by the field-VM `0x4C` opcode, outer-nibble 7** (`op0` ∈ `0x70..0x7F`, handler `0x801e1c64`): a rectangular wall-paint with inline operands `[4C, 0x7s, col0, row0, col1, row1, mask]`, sub-op = clear-walkable / block-all / clear-mask / set-mask. The op is **6 bytes for subs 0/1 and 7 for subs 2/3** - not a flat 7.

**Headline corrected.** The earlier "collision walls are authored in the scene event script, not a separate disc blob" is falsified by a finding recorded a few rows below it: the live `+0x4000` grid **byte-matches PROT 0109 with zero diffs**. The `.MAP` supplies the base grid; the nibble-7 paints are story-conditional **deltas** applied over it. The "residual `+0x4000` zero-init site" that followed from the old reading is therefore a non-question. Note also that `0x801e1c64` is not a function - it is entry `[7]` of the jump table at `0x801CEE60`, an intra-function label.

The `+0x4000` byte's **low nibble is a floor-elevation tier** - a 4-bit index into a 16-entry `short` height LUT at scratchpad `0x1f80035c`, filled at scene entry by `FUN_8003aeb0` from the MAN header (`_DAT_8007b898+2`, 16 negated values) and consumed by the object spawn iterator `FUN_8003a55c` to offset each placed object's Y. The `+0x8000` region is **not** a terrain-flag grid (corrected) - it is a per-tile `u16` object/attribute map (low 9 bits = object-record index into the `+0x0000` table; bit `0x400` = footprint flag ORed in by `FUN_8003aeb0` from field-pack records). See [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md#where-the-collision-grid-comes-from).

Residual sub-question: the `+0x4000` zero-init site (ruled out `FUN_8001f7c0` / `FUN_8003a024` / `FUN_800513f0`; likely a wholesale memset by the scene-boot allocator). Town01 parity confirmed by game-mode binding (Rim Elm = `town01` runs at mode `0x03`, same as the runtime-pinned field `map03`).

### Field `.MAP` PROT resolution - `define − 2`, universal

*Status:* resolved (census-pinned; engine resolver corrected)

A scene's field `.MAP` is its retail block's **first entry** - extraction index `define − 2`, because CDNAME defines are raw-TOC indices shifted `+2` from the extraction frame ([cdname.md](../formats/cdname.md#numbering-space)) - identified by its `0x12000` extended footprint, for **every** field scene. The per-entry extractor's shifted filename labels attribute it to the *previous* block's tail; in the unshifted engine windows of the era the first in-window `0x12000` entry was the **next** scene's map (the "in-block decoy"), which is what the census discriminated against. `Scene::load` now converts windows to the retail frame, so `Scene::field_map_index` is simply the block's first entry.

Pinned by a save-library census (`crates/engine-shell/examples/field_grid_census.rs`): each save's live field buffer (scratchpad `_DAT_1f8003ec` → `+0x4000` grid) classified against candidate on-disc bases. The `keikoku` sessions match PROT 0109 (`define 111 − 2`) with **zero** diffs while the in-block candidate 0118 differs by 3855 bytes; `koin3` matches 0559 exactly (in-block 0568 differs by 531); town01 sessions match 0010 ≡ 0001 exactly. A corpus sweep confirms the structure corpus-wide: every block's in-block `0x12000` hit is exactly the *next* block's `define − 2` entry.

The **object-index grid** (`+0x8000`, the `Scene::field_object_placements` / `field_terrain_tiles` source) is live-validated the same way: residuals of 0..96 bytes against the resolved entry across town01 / town0c / keikoku / koin3 sessions (story-conditional cell mutations - opened chests, prescript object toggles), thousands against every other candidate. Regression-guarded by the disc + save-library gated `engine-shell/tests/field_map_object_grid_live.rs`, which also re-falsifies the in-block rule against live RAM on the placement region for the discriminating scenes.

Consequences: (a) `Scene::field_map_index` now resolves `define − 2` (it previously picked the in-block entry - the **next scene's map** - for every field scene, masked only on town01 where the adjacent Rim Elm variants byte-copy, the one scene it had been validated against; `walk_field_map_index` is now an alias). (b) The town0c "cold `.MAP`" question **dissolves**: town0c's `.MAP` is PROT 0019, **byte-identical** to town01's (0001/0010) - the wall-press captures' "town01 buffer in a town0c session" is simply town0c's own map. (c) "PROT 0028 = town0c's different `.MAP`" is a misattribution - 0028 is `izumi`'s (`define 30 − 2`). (d) The kingdom "in-block decoy" framing is superseded: the decoy is the next scene's continent.

The footprint is corroboration, never the resolver, and the corrected PROT extents make that sharper: **111** entries are exactly `0x12000` bytes and only **101** are maps. Five of the ten strangers sit *inside* named scene blocks (`dolk+5`, `dolk2+5`, `taiku+9`, `taiku+10`, `rugi+7`) and are `scene_tmd_stream` entries - `[u32 size]` then the `0x80000002` TMD magic. So a footprint scan within a block does not merely risk the neighbouring scene's map; it can land on a mesh stream. See [`field-map.md`](../formats/field-map.md#the-footprint-is-necessary-not-sufficient).

### game_mode 0x03 = field/town gameplay

*Status:* resolved

`_DAT_8007B83C` = 0x03 is the in-town / on-field gameplay mode. Pinned empirically by two independent retail captures: the `v0_1_pre_battle_tetsu` save (Vahn walking in Rim Elm / `town01`, before the Tetsu cutscene) and the runtime-pinned free-movement controller on `map03`, both at 0x03. `engine_core::mode::GameMode::scene_mode()` maps `MainMode (3) → SceneMode::Field` accordingly, and the `mode_trace_e3` + `v0_1_playthrough` oracles drive the engine into the field (`enter_field_live`) so they converge against the retail 0x03 snapshot.

**Handler map recovered.** The index → handler/param/name map is now read straight off the disc by [`legaia_asset::mode_table`](../../crates/asset/src/mode_table.rs) (`asset mode-table`; disc-gated `mode_table_real`), so the dispatch is no longer guessed from the misleading dev names.

It confirms the saves: field/town is modes 2/3 MAIN (`game_mode 0x03`), and `MAPDSIP` (12/13) is the **world-map display** mode, not the field - correcting an earlier `functions.md` label that called mode 12 "the actual gameplay-mode entry". Structural finding: 12 of the 14 per-frame modes share the generic per-frame handler `0x80025EEC`; only Mode 13 (world-map) and Mode 23 (memory card) carry their own. Full map in [`boot.md`](../subsystems/boot.md#full-handler-map-recovered-from-the-disc).

**The in-field pause menu = mode 23 (CARD pair).** All six menu-open library captures (equipment / status / options, field `map01` + town `town01`) hold `_DAT_8007B83C = 0x17` - the pause menu runs under the CARD (menu / memory-card overlay) per-frame mode, not field mode 3 (the manifest's earlier `expected_game_mode = 0x03` rows were stale; corrected). Residue resolved: `BootSession` hosts the field-menu session headlessly (`open_field_menu` / the Start-edge path in `tick`; the windowed host layers its sub-session UI on the same session), `engine_core::mode` maps the CARD pair to `SceneMode::Menu`, and the `mode_trace_e3` oracle drives menu scenarios with a scripted Start press and asserts full menu-mode convergence (scene mode + active scene + the engine-emitted `game_mode = 0x17`).

**Engine model reconciled.** `engine_core::mode` holds `SceneMode::Field` for both modes 2/3 (the init mode holds its successor's scene mode, matching the Mapdisp/Battle/Str pairs), the reference handler that drives the pair is named for the field-entry path it exercises, and the table's name/param/next fields are cross-checked against the disc-recovered map by the disc-gated `mode_table_reconcile` test. The retail `+0x0A` next-mode field is decoded (`ModeEntry::next_mode`): `-1` = self-managed, `0` = fall back to mode 0 - the `0xFFFF0000` word previously read as a sentinel is just `-1` over a zero low half.

### Engine VRAM byte-exactness for town01

*Status:* resolved (major source); minor residue

Single-snapshot byte-exact VRAM is **physically unachievable** - ~40% of the texpage band is dynamic/residual (two town01 captures disagree on ~40%), so the oracle (`vram_oracle_e1`) is reframed to the **static mask** (words stable across same-scene captures), excluding the runtime NPC/character CLUT band. With the field pre-pass doing DMA-every-TIM (`BuildOptions.upload_all_tims`), town01 passes byte-exact on every static pixel it uploads.

The dominant missing static block is the **extraction-0874 section-2 TIMs** (retail `player_data` / `player.lzs` §2, the field-character texture band - historically mislabeled `etim.dat`, which is extraction 0870; 4bpp pages at `fb(320/384,256)` etc.) - field-resident, pixel-matched 256 rows byte-exact; the live engine uploads them at field entry (`scene::upload_effect_textures_into_vram`),

and the gap was an oracle artifact (the lightweight pre-pass skipped that step; now fixed, image pages only, since retail uploads their CLUTs at battle entry).

**Earlier negative finding retracted:** "the menu-glyph atlas (`PROT.DAT[0x11218]`) is menu-time-resident, not boot-resident in field VRAM" is **falsified** - the atlas IS boot-resident (its image page and flat-strip CLUT match the disc bytes in every captured phase, title included).
The "wrong static texel at `(960,400)`" that drove the old verdict is real but differently caused: the `(960,400)` 60×24 rect belongs to the **next bundle TIM** (`PROT.DAT[0x19438]`), which retail uploads *after* the atlas and which therefore overlays that part of the atlas image.
Uploading the atlas alone reproduces the pre-overlay bytes there; uploading the whole system-UI bundle in on-disc order reproduces the retail band. See [CLUT row 510 population](#clut-row-510-population-boot-resident-system-ui-strip-band) below.

**Minor residue (open):** `x=896..1024, y=256` (~12k) splits into (a) the now-explained boot-resident system-UI band (the `(960,256)` atlas page + its overlay TIMs; static disc bytes) and (b) the character/party-texture region uploaded by the battle/character targeted-CLUT pass the field pre-pass excludes by design (the CLUT-scattering thread), plus ~2.5k UI residue.

**Per-scene mask premise refined (map01 false red resolved).** Two capture-pinned failure modes of "stable across same-scene captures = static": (1) the extraction-0874 §2 (`player.lzs`) texture band is **global, history-dependent** state - the pause-menu entry path writes a 3-word F-variant onto row 271 that the first battle effect use overwrites with the disc bytes again (pinned at `(853,271)`: menu-lineage captures hold `0xFFFF` words, the disc TIM and effect-lineage captures hold `0x3333`), so same-lineage captures misclassify them as static; the oracle demands cross-scene staticity inside `scene::effect_texture_image_rects`.

(2) the world-map walk view **palette-cycles** specific columns of the kingdom terrain CLUT rows 506/508/509 in place; `vram_oracle::WORLD_MAP_CLUT_CYCLE_CELLS` excludes exactly those columns for world-map scenes (per-column census below) - row 507 and the static columns of 506/508/509 are asserted.


### World-map CLUT cycling beyond the ocean head - CLOSED (operand table + emitter + cadence all pinned)

*Status:* closed. The head-walk operands are a literal disc table - **kingdom-bundle slot 5** (type byte `0x06`), a 516-byte 8-entry CLUT-walk animation table byte-identical across all three kingdoms; the emitter is the SCUS actor walker, not the script-driven CLUT-cell family; the cadence is the table's own per-frame hold bytes.

The full chain (each link byte-verified against live RAM + the disc): loader `FUN_8001F05C` case 6 sets `DAT_8007B7C8` to the decoded slot-5 table; field-init `FUN_801D6704` spawns one render-mode-`0xB` actor per entry via `FUN_80024CFC` (entry pointer at `actor+0x4C`, accumulator `actor+0x68` seeded `100` so the first copy fires at scene entry); the per-frame emitter is `FUN_8001ADA4` **case `0xB`**, which banks `acc += DAT_1F800393` (the adaptive vsyncs-per-game-tick factor) and on `acc >= frame.hold` issues a 16x1 `MoveImage` from the frame's source cell to the entry's destination cell, **resets `acc = 0`**, and advances the frame index. Format + per-entry contents: [`world-map.md`](../subsystems/world-map.md) "Ocean animation"; parser `legaia_asset::clut_walk`.

Live confirmation (PCSX-Redux `MoveImage` exec-BP traces on all three kingdoms): intervals are strictly constant at `ceil(hold/dt)*dt` vsyncs (hold 8 → 9, hold 10 → 12, hold 20 → 21 at overworld `dt = 3`; the non-multiples falsify subtract-remainder semantics), all eight entries fire their first frame on the same vsync at world-map entry then free-run independent phases with zero drift, and the 18-step head cycle is `A,B,f0..f7,(f6,f7)x2,f8..f11` - two extra wave frames parked before the `OCEAN_ANIM_FRAME0_HEAD` signature in kingdom slot 0, ocean frame 12 never shown.

Findings that supersede earlier readings in this thread:

- The head-walk emitter is NOT the field overlay's script-driven CLUT-cell family (`FUN_801E4C58` / `FUN_801E4794`); that family carries only the **row-498 park one-shots/fades** (map01's eight `4C 61` ops; `scene_clut_cell_fx`, disc-gated `map01_clut_fx_disc`). At overworld idle, row 498 serves as a *source* strip for the `(32,508)` / `(48,500)` walkers - the map01-only row-508 "mirror" is slot-5 entry 6 copying from the script-parked row-498 cells.
- The row-506 cols 32..47 ("ring" + "generated pure-channel tail") are written wholesale by slot-5 entries 3/4 from the row-503/502 strips - parked disc bytes walked in place, not runtime-generated colour math.
- "Dest rows = park rows + 8" (computed-coordinate hypothesis) is falsified; the destination cells are literal u16s in the table.
- The engine consumes the table directly (`WaterAnim::Walk` in `play-window`; `vram_oracle::WORLD_MAP_CLUT_CYCLE_CELLS` = the slot-5 destination fold). The scene pre-pass never uploads the park strips (they are raw CLUT-block records, not TIMs) and map02/map03 bundles ship only rows `{501, 503, 505}` - retail relies on VRAM residency from the map01 upload, which the engine mirrors by parking the byte-identical Drake complement.

### `init_data` UI-tile pages - journey-dependent residency (resolved); map03 texture column (resolved - "not uploaded" premise falsified)

*Status:* the keikoku oracle drift is resolved (residency class pinned); the map03 texture divergence is resolved - the "engine fails to upload PROT 0392" premise is **falsified**, the current pre-pass does write the real terrain

`init_data` (PROT 0) carries two 64-word × 256 UI-tile TIMs at fb `(704, 0)` / `(704, 256)`. The capture corpus proves the rects are **journey-dependent residency**, not stable shared texture: overworld transit leaves kingdom-bundle content over parts of the rect (every Drake-stage capture - keikoku, the field-menu states - holds the *same* kingdom bytes at `(704, 256)` where the boot-fresh town01 states hold the disc tiles). Town scenes mask this only because their own scene TIM overwrites the slot; keikoku carries none, exposing the engine's `init_data` upload against retail's resident kingdom content. The parity oracle pools captures across all scenes against `scene::block_image_rects(index, "init_data")` - the same cross-scene dynamism treatment as the befect band.

**Resolved (Sol-residency falsified; "not uploaded" premise also falsified).**
The terrain rect is map03's own: `asset tim-scan` shows **PROT 0392 uploads 8
real 4bpp TIMs into fb `x=576..640, y=320..448`** (not foreign residency), and
the `fbx=576 fby=320` 96×96 4bpp TIM (PROT 0392, `lzs0_off 0x03BDEC`)
**byte-matches the retail resident VRAM at (576,320) 2304/2304 halfwords =
100%**. The earlier reading - that the engine `map03` pre-pass **fails** to
upload PROT 0392's LZS terrain (the `0x3332`-family column) - is **falsified**:
a direct prepass measurement shows map03 uploads 58 TIMs and the `576..640 ×
320..448` region holds 7945 real terrain texels with only 37 stray `0x3332`
cells (scattered in-tile, not a 2.2k hole) - the current prepass writes real
terrain. The `0x3332` gap belonged to an **old build**. Structurally this also
holds for the WorldMap kingdom path: PROT 0392 slot-0 is **byte-identical** to
0391 slot-0, which the engine already uploads (the kingdom sibling-skip at
`crates/engine-core/src/scene_resources.rs:645-668`), so uploading 0392 would
write identical bytes to identical cells - a no-op. **Residual (low):** the
decisive comparison used a direct prepass measurement, not a full VRAM oracle
(no map03-WorldMap-resident save exists in the corpus); a map03-resident
mednafen capture would close it fully.

### CLUT row 510 population (boot-resident system-UI strip band)

*Status:* resolved (source + upload semantics + retail residency pinned; engine pre-pass uploads the bundle - `legaia_asset::system_ui_bundle`); residue = the exact boot-time walker call site only

**Question.** `town01` env-pack slots 21/26/74 and `rikuroa` slots 50/51/63 are textured prims whose CBA decodes to `(64, 510)` with texpage `(960, 256)` 4bpp, yet no scene TIM uploads CLUT row 510 - so what populates it at runtime, and are those prims validly textured in retail frames?

**Answer.** Row 510 (and 511) is the **flat-strip CLUT band of the boot-resident system-UI TIM bundle** - the `prot::timpack` at **raw PROT TOC entry 0** (LBA words `toc[0]=3` / `toc[1]=55` precede `init_data`'s 121, so the "unindexed head gap" is indexed after all, just below the extraction space; CDNAME's `#define init_data 0` names this block, and a second single-TIM pack sits at raw entry 1).
The retail per-TIM uploader `FUN_800198E0` uploads *every* TIM CLUT block as a `w*h × 1` strip at the declared origin (`see ghidra/scripts/funcs/800198e0.txt`), so the atlas at `PROT.DAT[0x11218]` (declared CLUT `(0,510,16,16)`, image `(960,256)` 64×256) lands as the 256-entry strip on row 510 x=0..255, and the `0x19438` UI-strip TIM adds x=256..319; three more bundle TIMs tile row 511 x=0..319.
Full row layout: [`formats/npc-palette.md`](../formats/npc-palette.md#boot-resident-strip-band-rows-510511).

**Evidence (save-state census).** Across mednafen library states spanning every phase - title (`title_screen_new_game`), opening cutscene (`new_game_cutscene_intro_a`), town field (`v0_1_pre_battle_tetsu`), dungeon (`keikoku_chest_pre`), house interior (`mei_house_inside`), world map (`sebucus_overworld_resident`), battle (`v0_1_battle_start_tetsu`) - the row-510/511 strips are **byte-identical to the on-disc CLUT data** (256/256 + 64/64 + 256/256 + 48/48 + 16/16 halfwords per strip, every state), and the `(960,256)` image page matches the disc TIM on every row not covered by a later bundle member.
Compositing the bundle's TIMs in on-disc order (images at declared rects, CLUTs as strips) reproduces the whole retail `(960, 256..511)` band - the last six 64-word rows at y=456..458/460..462, initially unattributed, turn out to be **bare row-patch members of the same pack** (raw-entry-0 members 10..15 at `PROT.DAT 0x1A018..0x1AA7C`: a `[u32, u32]` preamble + TIM-style `[u32 bnum][u16 x,y,w,h]` block declaring `(960, y, 256, 1)`, byte-exact vs live captures; parsed as `RowPatch` in `legaia_asset::system_ui_bundle`).
So the affected prims ARE validly textured in retail: CBA `(64,510)` = atlas strip entries 64..79, and their UVs (u `0..2`, v `240..242`) sample a constant mid-grey texel patch - a flat-material trick through the textured pipeline.

**Falsified along the way:** (a) "row 510 is scene-loaded / a runtime targeted upload" - it is static boot residue, resident before the title screen; (b) "the viewer's CBA decode misreads the row" - the standard `x=(cba&0x3F)*16, y=(cba>>6)&0x1FF` decode is correct and retail-populated; (c) the earlier "menu-glyph atlas is menu-time-resident, not boot-resident" negative (see the retraction in the town01 VRAM section above).

**What would close the residue:** a cold-boot write-watch on the row-510 VRAM upload (the existing `scripts/pcsx-redux/autorun_town01_vram_upload_census.lua` probe) to pin which boot routine issues the `byindex`-style read of raw TOC entries 0/1 and walks the pack into `FUN_800198E0`.

### Scene-transition (`0x3F` door) destination indexing

*Status:* resolved

A field scene reaches another scene through the field-VM **`0x3F` named-scene-change** op, which carries its destination scene name inline.

**Pinned by a live PCSX-Redux dispatch trace** (`autorun_door_dispatch_trace.lua` on the `drake_castle_to_worldmap` capture): the `0x3F` ops are **partition-2 MAN records** reached through the **partition-2 record-offset table** - the controller sets the VM bytecode base to `man_base + data_region + partition2[slot]` and runs the record by fall-through (decisive: `a0 - man_base == data_region + partition2[0]` exactly). Selection is by stable slot index, so the op's `index` field is only the destination-scene id passed to the warp packet (`FUN_8001FD44`). Corpus census (clean partition walk): 160 dest ops / 48 scenes, 153 in partition 2, **zero absolute-reference ops** at/after any dest op.

This made **variable-length** door editing safe (resizing a destination name is a partition-table + section-offset + intra-record-jump-delta + descriptor-size fixup), implemented in `legaia_asset::man_edit` and shipped as the door randomizer. See [`man-relocation.md`](../formats/man-relocation.md).

**The `0x3E` door-warp (7-id `map_id`) is now also resolved - and the "uncaptured handler" framing was wrong:** the whole chain is **SCUS-resident** (`FUN_80025980` mode-24 OTHER INIT entry, `FUN_80026018` exit). There is **no destination name** - the sub-id selects a minigame overlay (extraction PROT 972..977, 980 via the corrected loader math `param + 0x37F`), and the "name handling" is a backup/restore of the *current* scene name (`0x80084548` ↔ `0x8007BAE8`, plus `_DAT_80084540` ↔ `0x8007BAC4`) so the exit re-enters mode 2 on the original scene. Full decode in [`script-vm.md § 0x3E warp`](../subsystems/script-vm.md#0x3e-warp-mode-24-minigame-door-warp).


### Intra-town (house / interior) door mechanism

*Status:* resolved

Entering a house in a town is **not** a scene change - it's an **intra-scene reposition**: the field VM runs a **`0x23 MOVE_TO`** op that teleports the player to an interior sub-area tile within the *same* loaded scene (the scene-name buffers `0x8007050C`/`0x80084548` stay put across the transition; only the player struct position jumps). Pinned at the instruction level by the new `probe.step.find_writer` Lua primitive (a width-correct range write-watch over the player position block): the writer lands in the field-VM dispatcher `FUN_801de840` **`case 0x23`** (`0x801debc4 sh v0,0x14(s5)`), converting the tile operand to world (`tile*128 + 0x40`).

Earlier write-watchpoints missed it (a width-2 watch at `+0x14` caught only a 2-byte no-op re-store in the ledge-hop `FUN_801d1878`, a red herring). Captures: `door_warp_rim_elm_to_mei_house`/`mei_house_inside` (mednafen), `mei_house_door_pcsx`/`mei_house_inside_pcsx` (PCSX).

**A clean door marker exists after all** (the earlier "shared with NPC/cutscene movement, no marker" reading is superseded): house-door warps use the **cross-context form `0xA3 0xF8 xb zb`** - opcode `0x23 | 0x80` dispatched into the player system channel `0xF8` ("make the *player* MOVE_TO this tile"), while plain `0x23` moves the executing actor (NPC/prop positioning).
The carrying partition-0 records have their own header form (`[u8 n][n×2 SJIS name][u8 attr]`, distinct from partition 1) and an explicit naming convention pairing entries with exits (fullwidth `ＩＮ`/`ＯＵＴ`, `入口`/`出口` gates, `Ａ`/`Ｂ` elevator endpoints; optional digit suffixes).
The captured Mei's-house warp is byte-for-byte the `0xA3 0xF8 0x61 0x36` in town01 partition-0 record 34 (an `ＩＮ` record).
The randomizer (`legaia_patcher::house_door`) shuffles only these classified door warps, class-preserving (ＩＮ among ＩＮ, ＯＵＴ among ＯＵＴ) so every exit still lands outside; see [`randomizer.md`](../tooling/randomizer.md).

**`0xA3 0xF8` is one of three player-move forms, and the ＩＮ/ＯＵＴ pair is one of several door shapes.**
A door record repositions the player through *any* of `A3 F8 <xb> <zb>` (op `0x23`, instant),
`CC F8 51 <xb> <zb> <depth> <mv>` (op `0x4C` nibble-5 sub-1, teleport + move anim) or
`C7 F8 <xb> <zb> <mode>` (op `0x47`, animated walk), and the record is a **branching script** whose arm is
selected by story flags - so a door can also be a `0x44` SPAWN_RECORD of a partition-2 choreography that
does the seating itself.
The bind position is the `.MAP` **object's** contact box, not the trigger tile (which is a lookup key and
usually a wall).

**And the MAN is not the only door carrier.** The `.MAP` trigger block's **kind-0** sub-table is a second,
larger door class: `[tile_x][tile_z][dest_x][dest_z]`, no object and no script - crossing the tile seats the
player at `(dest_x*64 + 64, (dest_z + 1)*64)` (`FUN_801D1EC4`'s kind-0 arm at `0x801d21c0`). **2330 records
across 73 scenes.** Most house *exits* are these. This is what produced the (false) "Vahn's house has an ＩＮ
and no ＯＵＴ, so it is a story-entry warp" reading: there is no ＯＵＴ record because the exit is not a
record at all - it is the kind-0 tile `(97,9)` inside the room, ungated by any story flag. Full mechanism:
[`field-locomotion.md`](../subsystems/field-locomotion.md#intra-scene-doorways---the-walk-touch-teleport-family).


### Field/town environment-geometry placement

*Status:* resolved (renders)

The town's environment meshes (terrain + buildings + props) are object-local Legaia TMDs in the **LZS streams of the scene_asset_table** PROT entry (`town01` = entry 4). Placement is `FUN_8003a55c`: the field-map object-index grid at `+0x8000` (`cell & 0x1FF` = object id) selects a `0x20`-byte record in the `+0x0000` table; placed tiles (record `+0x12` bit `0x4`) give the world transform (`world_y = -floorHeightLUT[nibble] + y_off`, the LUT being 16 `s16` at the MAN header `+0x02`). Mesh per object: the record's `+0x10`, for **every** object id (retail `FUN_80020f88`, `actor+0x64 = record[+0x10] + prefix`).
Ids `1/2/3` are protagonist/NPC meshes from the shared pool; `anim_id` only animates.
Validated against a live `town01` save (Vahn's house id `137` → mesh 36), and against the retail GPU prim pool for the ids an earlier positional "field-actor band" rule (`obj_idx - 5`, ids `93..=118`) mis-resolved: town0c cell `(30, 17)` (id `99`, record `+0x10 = 2`) draws its surface from env mesh **2** - the quad's `cba`/`tsb`/UVs match that mesh's primitive byte-for-byte - not from mesh `94`.
The band rule is **falsified**: it swapped ten town meshes per Rim Elm map, dropping the terrain slab south-east of the spawn and leaving a clear-colour hole in the ground.

Parser `legaia_asset::field_objects`; `Scene::field_object_placements`; `play-window` renders the town via `resolve_field_placement_draws`. Full field decode in [`field-locomotion.md`](../subsystems/field-locomotion.md#object-record-format-0x0000-0x20-byte-stride).

**Open (minor):** of 46 placements, the field render now draws **40** (the 2 untextured props were recovered by the vertex-colour path, see (a) below); the remaining **6** that don't draw are all one missing-CLUT mesh. The historical "**8 of 46** drop" split is pinned by cause, and the earlier "all 8 are fully-untextured props" reading is **corrected**. They split into two unrelated causes across **3 distinct env-pack meshes** (disc-gated `town01_dropped_placements_split_untextured_vs_missing_clut`):

**(a) 2 placements** (meshes pack `31`/obj `315` with 30 untextured prims, pack `109`/obj `114` with 12) are genuinely **untextured (per-vertex-RGB) props** - the textured-only builder `tmd_to_vram_mesh_filtered` skips prims with no UVs (`mesh.rs` ~line 508), so a flat/gouraud-only mesh builds empty and is dropped at `res_to_mesh[res_idx] == None`; **(b) 6 placements** (one mesh, pack `74`/obj `347`) are **textured** but every one of their 4 prims is dropped for **`MissingClut`** - the field VRAM pre-pass didn't upload that CLUT row. Neither is a filter *bug* (a mesh whose textures aren't resident *should* drop rather than draw flat `CLUT[0]`),

and the two need **different** fixes: (a) the **per-vertex-RGB props are now rendered** - the untextured-prim colour block is fully RE'd (the per-mode record layouts F4/G3/G4 + the `00 01 03 02` quad winding remap + the negative "no per-prim normal" result, see [`tmd.md` § Per-prim color / texture block](../formats/tmd.md#per-prim-color--texture-block)),

`legaia_tmd::legaia_prims` decodes the colours into `Prim::colors`, `legaia_tmd::mesh::tmd_to_color_mesh` builds a standalone `ColorMesh` from a TMD's untextured prims, and `engine-render` has a dedicated vertex-colour pipeline (`upload_color_mesh` / `Scene::color_draws`) that play-window draws for the dropped props (so town01 recovers the 2 untextured placements → 40/46; pinned by `field_object_placement_disc::town01_dropped_placements_split_untextured_vs_missing_clut`); (b) wants the **missing CLUT row uploaded** (a VRAM-coverage question, sibling of the town01 static-VRAM residue thread - a per-vertex-RGB fallback would render (b) *wrong*, so it stays dropped).

Mixed meshes (some textured + some untextured prims) now render **both** halves: the colour mesh is built unconditionally and is disjoint from the VRAM mesh (`tmd_to_color_mesh` skips textured groups), so a mesh's textured prims go to the VRAM pipeline and its untextured prims to the colour pipeline at the same placement (previously the colour mesh was built only when the whole textured build was empty, dropping the untextured half of a mixed mesh). Only (b) remains (the missing-CLUT runtime upload); the split + counts are pinned by the test above.

### Region story-flag gate families

*Status:* resolved as structure across the chapter-2/3 regions; the in-game play-order residual for the dungeons the capture corpus never walked is tracked on [`open-rev-eng-threads.md`](open-rev-eng-threads.md#region-story-flag-gate-families)

Every field scene's MAN carries one **partition-2 record** per cutscene or story beat, and each record's *header* holds two flag lists that the spawn evaluator `FUN_8003BDE0` checks before running it: a **C1** one-shot list (the record is suppressed once any listed flag is set) and a **C2** requires-all list (the record spawns only when every listed flag is set). Regional progression is expressed almost entirely through these header gates.

Because they live in the record header rather than as inline `0x50`/`0x60`/`0x70` opcodes, the inline flag census (`man-scripts --system-flag-census`) cannot see them — the recurring cause of several "write-only flag" false alarms. `legaia_engine_core::man_field_scripts::partition2_record_gates` decodes them, and the census-file anchor tests named below pin each region's exact lists.

Two reader-only flags first exposed the pattern. `0x1BE` (Jeremi's arrival at `geremi`) is a self-latch: `geremi P2[0]` both sets it and lists it as its own C1 gate (anchor `geremi_p2_0_is_the_0x1be_self_latch`). `549`/`0x225` (the Rim Elm opening) is read the same way across the Rim Elm variants and turned out to be the same self-latch shape once the `4C 0xE_` op widths were fixed.

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
All three Rim Elm renditions ship a `P2[7]` under the same gate shell (`town01`/`town0c`: C1=`[0x22B]`, C2=`[0x147]`); town0b's copy adds `0x141` to C1 and is the rendition whose arms mint the band. (There is no fourth. `gameover_data` was once counted as one; that block's CDNAME window is a subset of `town01`'s and holds no asset-table bundle, so the MAN it appeared to carry was `town01`'s own, reached by an entry-size over-read - see [script-vm.md](../subsystems/script-vm.md#a-second-script-byte-carrier-the-streaming-variant-man).)
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
`0x50A` is the Sol game-hall minigame result toggle, written **natively by
the mode-24 minigame overlays** (a space the MAN script census is structurally blind to):
the Muscle Dome module (PROT 0977) CLEARs it in the post-match settle (`0x801D0FF8`) and
win-re-SETs it (`0x801D101C`, labeled by the overlay's own `WIn on`/`WIn off` debug
strings), and the dance trio (0978..0980) SETs it at session start (`0x801CF968`) / CLEARs
on a missed goal (`0x801CFF10`); koin1 hosts the Muscle Dome + Baka doors (`3E 69`/`3E
68`), koin3 the dance doors (`3E 6A`), and koin1 `P2[9]` (C2=[`0x50A`]) is the returned-
victorious beat. `0x5D6` is **writer-less** (the `0x482` class): negative
across the script census, a disc-wide operand-classified sweep of every native flag-helper
caller (`scripts/asset-investigation/flag_helper_call_sweep.py`), the move-VM ext flag
sub-ops, the motion-VM census and raw MAN operand scans; only the dev-menu flag editor
(index cell `0x801F2AA0`) reaches it, so koin4's `0x5D6` content is dev residue,
unreachable in retail. See [script-vm.md](../subsystems/script-vm.md) § native flag-bank
writers. The guard `koin_gates_0x50a_0x5d6_remain_script_writer_less` stays correct as
stated (script-writer-less).
(Nivora's `0x370` writer surfaced statically under the pinned widths; see the Nivora Ravine paragraph.)
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

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| Dialog font extraction | done - kept for reference | `capture` | Earlier "blocked on runtime trace" framing was wrong; tile-page lives at VRAM `(896, 0)..(960, 256)`, extracted by `legaia-font::font-extract` from any in-game save state. The **on-disc carrier** (previously "unclassified") is now pinned too: a plain 4bpp TIM at `PROT.DAT` offset `0x7F40` (framebuffer `(896, 0)`, CLUT `(0, 510)`), so the font is decodable **without** a save state (`legaia_font::Font::from_disc_tim_and_scus`; the WASM site's pause menu uses it). Byte-verified vs the save-state extraction. Listed here only so the older "open" framing doesn't get re-opened. |
| Inline dialog-box format (`0x1F`-lead segments) | resolved (init-arm count corrected; session-end semantics open) | `disassembly` | [details ↓](#inline-dialog-box-format-0x1f-lead-segments) |
| Tetsu 4-option spar menu mechanism | resolved | `capture` | The menu is a standard `0x29` 4-option **MES inline picker** in the sparring partner's dialogue (cursor `*(0x801C6EA4)+0x0C`; confirming **index 2** "I want to practice with you." starts the spar - live `0x03->0x09->0x15`, driven by the dialog SM not the field VM). It uses the **immediate-labels** form (labels straight after the N jump entries, no continuation byte) - `parse_picker_at` rejected it, now fixed, so town01 decodes the spar menu + its other pickers. Engine: `World::CarrierMenu` presents the picker and engages the carrier only on the index-2 fight option (was any-accept). Tests: `parses_immediate_labels_picker`, `tetsu_spar_picker_disc`, `carrier_spar_menu_*`, the updated `training_battle` legs. |
| Pause Items/Magic screens: remaining sub-flows | resolved (dim-bit residual closed) | `disassembly` + `capture` | [details ↓](#pause-itemsmagic-screens---remaining-sub-flows) |

### Pause Items/Magic screens - remaining sub-flows

*Status:* resolved - all four sub-flows traced from disassembly and ported; the `0x800` dim-bit residual is closed

All four sub-flows are traced and ported: the **window-14 target panel**
(`FUN_801D0520`; the preview modes are the permanent-stat Water previews, superseding the
"HP-restore" reading), the **PAGE sprite** (UI-icon `0x76`), the **SCUS kind-4 list
kernel** (`FUN_80032A44` + allocator `FUN_80030104`), and the **class-`0x80..0x82` Use
routes** (submenus 0xA..0xD: single-target apply `FUN_801D8308`, Door of Light/Wind
`FUN_801D8A58`/`FUN_801D8B90`, Incense `FUN_801D8D94`). Engine
`engine-ui`/`pause_screens`. See [field-menu.md](../subsystems/field-menu.md#items-screen).

The `0x800` dim-bit residual is closed (grade `disassembly` + `capture`): the bit is set at
**build time** by the SCUS content builder `FUN_80030628`'s content-id-3 case (dispatch on
live window `+0x1C`, copied from descriptor byte `+0x0` at create, `0x80032990`) -
equipment always-dim, Door ids `0x88/0x89` scratchpad-gated, field-usable bit `0x2`, then
the `FUN_8003043C` applicability probe (battle context gates bit `0x4`). No
focus-dependent write exists - the white→grey flip is the kernel mode-4 park override; a
capture shows the row words bit-identical across focus states. See
[field-menu.md](../subsystems/field-menu.md#use-list-row-build-content-id-3-fun_80030628).

### Inline dialog-box format (`0x1F`-lead segments)

*Status:* resolved - prologue + pager-side dispatch + option-list inner format + multi-segment box packing all pinned

Placement-NPC / event dialogue text is **inline** in the field-VM interaction record, **not** the scene MES - the opcode-decoded `text_id` is a box-config id that never resolves through `SceneMes::message_offset` (0/13 town01 placement-NPC ids resolve). The text is a run of `0x1F`-lead / `0x00`-terminated segments of MES glyph bytecode. It is recovered **structurally**, not from the `0x3F` op's `len` field: a text-heavy field interaction record desyncs under linear disassembly (a literal `>` is `0x3E`, the warp/interact opcode; ASCII punctuation hits the `0x37`/`0x41` yield bytes), so the decoded `0x3F` op and its `len` are unreliable on field scenes and the byte-`len` capture returned **empty for every town01 NPC**.

`man_field_scripts::first_inline_dialog_offset` finds the first printable `0x1F` segment (printable-ratio gated), `classify_placement` carries the record bytes from there as `PlacementKind::Npc::dialog_inline`, and `OwnedDialogPanel::from_inline_dialog` types the prompt segment; the native `play-window` renders the box. With this, **36 town01 placements recover renderable dialogue** (the sparring partner, Meta the dog, villagers, leftover "dummy" dev placeholders, and the `0x1F`-segment developer story-flag toggle menu at placement P1[1]).

**Segment-pool structure pinned:** the segments are **not** "prompt + option labels" of one box. `dialog::decode_inline_segments` recovers the full `0x1F`-lead pool, and decoding real town01 placements shows each record holds the NPC's *entire* dialogue line set - every line across every story-state branch, with `"Yes"`/`"No"` option labels interspersed (e.g. the Village Elder decodes to 80 segments, Val to 59, both carrying multiple `Yes`/`No` pairs; disc-gated `field_actor_placements_disc::inline_dialogue_decodes_into_full_segment_pool`). So `0x1F` segments are individual lines, *not* page-break-delimited boxes - multi-page speech is multiple `0x1F` segments, not `0x80..=0x9F` control bytes within one.

**There is NO separate "box-geometry header" format (falsified):** the bytes between the placement's `script_pc0` and the first `0x1F` are normal field-VM bytecode - `CFlag` / `SysFlag.Test` / `JmpRel` / `Nop` / `0x4C 0x51` NPC-move-to-tile / `0x4C 0x52` menu-activation poll - that runs as the NPC's interaction prologue (face the player, set conversation flags, walk to the talk position, branch on story flags).

The retail SM `FUN_80039B7C` state 0 calls the field-VM dispatcher `FUN_801DE840` directly on this stream and transitions into the pager only when the dispatcher leaves the actor's PC on a byte where `& 0x7F < 0x20` (a `0x1F` lead or `0x21` terminator); the "select which segment to start at" mechanism is the prologue's own story-flag-gated `SysFlag.Test` branches - the script `JmpRel`s past unwanted segments to the desired one.

**Post-page dispatch - init-arm count corrected, and a false alarm recorded.**
State `0x19` maps `0x25`→state 0, `0x24`→3, `0x48`→9, `0x4C 0xFF`→6,
`0x2A`→`0x11`, `0x27`/`0x28`/`0x29`→`0x13`/`0x15`/`0x17`, default→9. **Three**
arms run the box-reset tail - states 0, 6 **and** 9, not "both init arms
(`case 6` / `case 9`)"; state 3 has its own prologue and jumps away at
`0x801D916C`. The three arms are byte-identical over their 0x98-byte extent
*except one word* - the `li v0,N` selecting the successor (0→1, 6→7, 9→`0xA`).
That word is the whole behavioural difference: `JT[1] == JT[4] == JT[7] ==
0x801D8708` (teardown, with an early return when the state is 4 so `0x24`
keeps its rows), while `JT[0xA] == 0x801D92A4` is the box-open animation. So
`0x25` and `0x4C 0xFF` are indistinguishable from each other and genuinely
differ from `0x48`, and the port's `End`/`Terminate`-vs-`NewBox` grouping is
**faithful** - an audit that read the arms as merely "byte-identical tails"
briefly flagged it as a live bug, which it is not. What the pager does *not*
decide is whether the **conversation** ends; it clears rows and returns no
status, so session-level end is a caller-side decision and remains open.

Pinned by `field_disasm::LinearWalker` decoding the prologue cleanly across every classified town01 dialog NPC once nibble-5 sub-1/sub-2 are covered (disc-gated `field_actor_placements_disc::dialog_prefix_decodes_as_field_vm_bytecode`); the earlier "candidate decoder among `FUN_8003AB2C` / `FUN_8003BDE0`" framing is falsified - both are known: `FUN_8003AB2C` is the per-frame field-VM driver and `FUN_8003BDE0` is the partition-record dispatcher (both already ported).

**`FUN_8001ebec` is not the renderer** - disassembly shows it's a per-character TMD-pose copier (party slots 0..2, indexed by the slot-4 freeze flag `_DAT_8007B824`, copies 7 u32s of pose data from TMD offsets `+0x124..+0x140` or `+0x140..+0x15C` gated on a record flag at `+0x75E`; both arms load seven words, so the second range ends at `+0x15C`, not `+0x158`); the earlier reference to it as the dialog-box renderer in the engine + this thread is wrong (corrected in [`subsystems/script-vm.md`](../subsystems/script-vm.md) op `0x4C` sub-3 sub-F note). The real per-actor dialog SM is `FUN_80039b7c` (advances `actor[+0x9c]` 0→1→2 through `0x1F`-lead segments, consumes the `0xC?` 2-byte escapes); the pager is `FUN_801D84D0`.

**Pager-side dispatch now decoded:** the box geometry is fixed at `_DAT_801F2740 = 3` lines per box at both init arms (`case 6` / `case 9`), and the post-page state `0x19` reads the **next control byte past the box** to pick the follow-on state - `0x25` -> end, `0x24` -> next-line same-box, `0x48` -> new box, `0x4C 0xFF` -> terminate, `0x2A` -> resize, **`0x27` -> 2-option picker** (state `0x13` -> `0x12`), **`0x28` -> 3-option picker** (`0x15` -> `0x14`), **`0x29` -> 4-option picker** (`0x17` -> `0x16`). The open byte is matched as `byte & 0x7F`, so both `0x27..0x29` and the high-bit `0xA7..0xA9` forms are accepted; the field corpus stores the bare form.

Each picker arm sets the box dimensions from a per-N table and clamps the choice cursor at `*(DAT_801c6ea4 + 0xc)`; on confirm it reads the continuation byte at `pbVar14[N*2 + 1]` (same dispatch table as the post-page) and advances. Captured in [`docs/formats/mes.md` § Dialog window pager](../formats/mes.md#dialog-window-pager---fun_801d84d0).

**Option-list inner format resolved:** the control region is `[open][N * 2-byte i16 LE jump table][continuation?][N * 0x1F label segments]`. The continuation byte is **optional** - either a post-page dispatch (`0x24`/`0x25`/`0x48`/`0x4C`) or absent, with the labels starting immediately (the **immediate-labels** form - Rim Elm's Tetsu spar + town01's pickers; see [`mes.md`](../formats/mes.md#picker-control-region-layout)). The labels are standard `0x1F`-lead glyph segments; "labels = the 2-byte entries" is falsified. Each 2-byte entry is a **signed relative jump** `FUN_80038050` applies on confirm: `new_pc = (open + 1 + index*2) + i16_LE(entry[index])`. Pinned: the four `izumi` re-emissions shift all entries by an identical per-emission delta, and every option jumps in-bounds.

Parser `legaia_mes::picker` (`scan_pickers`/`parse_picker_at`/`Picker::jump_target`); disc-gated `field_dialog_pickers_disc` decodes dozens of real menus (config `On`/`Off`/`Exit`, shop haggling, the Genesis-Tree quiz) and asserts in-bounds jumps.

**Engine consumer (faithful path):** `engine_core::inline_dialogue` / `World::step_inline_dialogue` (PORT `FUN_80039B7C`) drives the whole inline script through the real field VM, so a chosen option's branch handler executes its `SET`/`CLEAR` flag ops + scene changes before the reply (`World::use_vm_dialogue`; `play-window` runs this path by default, `--simple-dialogue` opts out).

**Pre-first-segment prologue now runs (VM-dialogue path):** the field-VM dialogue runner (`World::use_vm_dialogue`) executes the interaction prologue before the first segment. The engine keeps the truncated `field_npc_dialog` buffer for the default renderer and stores the **untruncated** record alongside it (`man_field_scripts::placement_inline_prologue` → `field_npc_dialog_prologue`, body + entry PC + first-segment offset); on interaction the runner is started via `InlineDialogue::with_prologue` from `entry_pc` so the prologue's `SysFlag.Test`/`JmpRel` chain selects which segment the box opens at per story state, falling back to the first segment if the prologue can't reach one (never worse than the truncated path).

Disc-gated `field_interact_dialogue_disc` pins the prologue map's byte-consistency + non-vacuous presence on town01; synthetic `inline_dialogue_prologue_selects_segment_by_story_flag` / `…_falls_back_when_it_cannot_reach_a_segment` pin the selection + fallback.

**Multi-segment box packing resolved:** the SM packs **consecutive** `0x1F` lines into one window of `_DAT_801F2740 = 3` rows - a line's `0x00` terminator immediately followed by another `0x1F` is "same box, next row" - and the box ends after at most three rows at the post-page control byte. `FUN_80039B7C`'s state-`0x2` advance (`for (; 0x1e < *pbVar4; ...)`) masks `(*pbVar4 & 0xF0) == 0xC0` and consumes the escape's data byte, so a `0xC?` escape whose argument lands in `0x00..=0x1E` (e.g. `0xC1 0x00`) doesn't terminate the line early.

Decoded by `legaia_mes::dialog_box` (`pack_box` / `pack_boxes`, `LINES_PER_BOX = 3`, `Dispatch` for the terminating control byte); disc-gated `field_dialog_boxpack_disc` pins it on real town01 bytes (all 561 packed boxes ≤ 3 lines; the Tetsu sparring opening packs as three `0x24`-chained 3-row pages → a 4-option `Picker`; the `Mist appeared, .., but` line survives its `0xC1 0x00`). The contiguous box run stops where the pool hands control back to the field VM (a non-pager control byte → `Dispatch::Unknown`), which the faithful `World::step_inline_dialogue` path runs as bytecode. Nothing further open on this thread.

## Animation

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| Player ANM per-record layout | resolved (byte-4 nibble corrected) | `disassembly` | [details ↓](#player-anm-per-record-layout) |
| Battle anim-id space + record[0] "strike family" | resolved | `capture` | Anim ids are entry indices (commit `FUN_8004AD80`; idle id = `0`; `FUN_801D5854` ids 6..9 = a camera program space). Tags `2/3/4/5/0xB` = the hit-reaction family (`+0x1EF..+0x1F3` map; `FUN_800402F4` stages flinch/knockdown). Swings = the equipment-section splice (slots `0xC..0xF`) + dynamic art slots `0x10`/`0x11` from the `+0x58` art bank. Capture-pinned + disc census. See [monster-animation.md](../formats/monster-animation.md) / [battle-data-pack.md](../formats/battle-data-pack.md). |
| `FUN_80047430` caller | resolved | `capture` | Live-captured (`autorun_anim_node_tick_caller.lua`, mid-battle save): a single dispatch site — `jalr v0` at `0x800252B4` inside `FUN_8002519C`, the per-frame actor-list tick iterator, calling the node's `+0x0C` handler slot with the node pointer in `a0`. The anim-node tick is an ordinary list-node tick handler; no other caller fired. See [functions.md](functions.md). |
| Record[0] `+0x5C` pointer + art-anim bank stream source | resolved (`+0x5C` = vestigial paired-relocation) | `disassembly` (SCUS exhaustive; overlays partial) | Art streams = `"ME"` archives in `readef.DAT` slots `3*char+1`/`3*char+2`. `+0x5C` is a self-relative pointer rebased at load, paired with `+0x58`, by `FUN_80052FA0`. `+0x58` has a reader; **no `+0x5C` reader exists in SCUS** - a word-wise sweep of all 110,080 text words finds one non-`sp` load at that offset, the relocation itself. Coverage stated rather than rounded to "exhaustive": 11 overlay images remain dump-only, and a dump sweep cannot establish a negative ([dump-corpus-integrity.md](../tooling/dump-corpus-integrity.md)). See [battle-data-pack.md](../formats/battle-data-pack.md#me-stream-archives-readefdat). |

### Player ANM per-record layout

*Status:* resolved (container + per-`(bone, frame)` semantic)

The on-disc per-record body decodes byte-exact across **all 296 records** in the 5 pinned scenes (296 record / 5 scene corpus, plus every other scene's bundle the corpus sweep finds): `record_size = 16 + 8 × (a & 0xFF) × b`, where `a & 0xFF` is the **bone count** of the clip and `b` is the **frame count**. Layout: 8-byte `(a, b, marker_1=0x080C, flag)` header + 8-byte per-anim prologue + `b` frames × `bone_count` × 8 bytes per (bone, frame). Pinned by the disc-gated regression `crates/asset/tests/player_anm_real.rs` after the offset-convention fix (offsets in the offset table are **absolute** byte offsets, not relative to `+4` - earlier framing was wrong; size invariant now validates 296/296).

**Per-`(bone, frame)` 8-byte semantic - resolved** (the earlier "4 little-endian `i16`s, semantic open" framing is superseded): the entry is **not** four shorts but a **translation + rotation** pair, decoded exactly as the retail interpreter `FUN_8001BE80` (`ghidra/scripts/funcs/8001be80.txt`) does - bytes 0..4 hold three **nibble-packed signed 12-bit translation** values `(t_x, t_y, t_z)` (byte 2 = `high4(t_y)<<4 | high4(t_x)`, byte 4 **low** nibble = `high4(t_z)` - `andi v0,v0,0xf` at `0x8001BF38`, the high nibble is unused; sign-extend on bit 11), and bytes 5/6/7 are three **`u8` rotation angles** `(r_x, r_y, r_z)` each `<< 4` to a PSX 12-bit angle (`4096` = 360°), composed Z→Y→X via `FUN_8004638C`/`FUN_8004629C`/`FUN_800461A4`.

The piece poses `R·v + T` about its own object origin (no centroid subtraction); frame 0 of an idle clip is the rest pose. Decoder `legaia_asset::player_anm::BoneTransform::decode` mirrors the decompiled C, pinned by the byte-exact unit test `bone_transform_decode_signed_12bit` (town01 record 17). The site characters page applies the same `(t, r)` pipeline.

The port was never wrong here: `player_anm.rs` has always decoded `bytes[4] & 0x0F`, and the disc-gated `bone_transform_decode_signed_12bit` would have failed the moment anyone "corrected" the code to match the prose above. A test containing a doc error is the mechanism working - worth stating rather than quietly fixing the sentence.

**Not modelled by the port:** `FUN_8001BE80` is not a pure per-entry decoder. It **lerps between two frames** on a 4-bit sub-frame fraction (`*(u16*)(actor+0x68) & 0xF`), gated on `*(u8*)(a2+1) & 1`: translations as `a + (((b-a)*frac) >> 4)`, angles through the wraparound-aware interpolator `FUN_8001D088` (not a plain lerp), composing into scratchpad `0x1F8002C0`. `BoneTransform::decode` models only the un-interpolated arm.

**Distinct ANM kind (not this one):** `FUN_80021DF4`'s `+0x5A == 6` block uses a separate 24-byte-per-bone keyframe layout - see [`anm.md`](../formats/anm.md).

## Audio

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| SPU reverb live routing (C7-REVERB) | resolved (wired; Studio C, global) | `capture` | [details ↓](#spu-reverb-live-routing-c7-reverb) |
| XA channel map / STR demux SM | resolved (static decompile of PROT 0970 + SCUS) | `disassembly` | [details ↓](#xa-channel-map--str-demux-sm) |
| `FUN_80018DB0` is a rumble cadence, not an audio one | resolved (libpad, not SsAPI; no cue to pin) | `disassembly` | [details ↓](#fun_80018db0-is-a-rumble-cadence-not-an-audio-one) |
| Key-on pitch: what does retail put in the voice pitch register? | resolved (unity on centre; the port was an octave low) | `disassembly` | [details ↓](#key-on-pitch-unity-on-centre) |
| SFX cue bank routing - the category byte selects the VAB slot | resolved (mechanism + the two pinned banks; ported) | `capture` | [details ↓](#sfx-cue-bank-routing---the-category-byte-selects-the-vab-slot) |
| Which PROT entries fill SFX VAB slots 1 / 3 / 6 / 11 | resolved (slot 6 = 0876, slot 11 = 0889; 1 / 3 are variable banks) | `disassembly` | [details ↓](#which-prot-entries-fill-sfx-vab-slots-1--3--6--11) |
| The `FUN_8006EF18` trio is a BIOS kernel-patch sequence, not an SPU init | resolved-negative | `disassembly` | [details ↓](#the-fun_8006ef18-trio-is-a-bios-kernel-patch-sequence-not-an-spu-init) |
| `_DAT_8007B910` is the live audio level, not screen brightness | resolved (both hosts' labels corrected) | `disassembly` | [details ↓](#_dat_8007b910-is-the-live-audio-level-not-screen-brightness) |
| XA clip-table writer + `(clip_id, chan)` cue census | resolved (writer pinned statically; census in `audio.md`) | `disassembly` | [details ↓](#xa-clip-table-writer--clip_id-chan-cue-census) |
| Hyper Arts fanfare selector - what audio fires when a Hyper executes | resolved (per-(char, art) coin flip over a fixed channel pair of the even-slot fanfare bank) | `disassembly` | [details ↓](#hyper-arts-fanfare-selector) |
| Op-`0x35` sub-op `0xA` - what the "unhalt-pause toggle" waits on | resolved (it is the track-swap commit; both globals pinned; ported) | `disassembly` | [details ↓](#op-0x35-sub-op-0xa-is-the-track-swap-commit) |

### Op-`0x35` sub-op `0xA` is the track-swap commit

*Status:* resolved - the arm's two inputs are pinned by writer census, and the
op is ported.

The arm (`0x801E0264`, field overlay 0897) was read long ago; what was open
was who feeds it. A store-offset writer census over SCUS + every based
overlay image (the `lui`+load/store form the literal-word sweep cannot see;
[`address-reference-scan.md`](../tooling/address-reference-scan.md)) answers
all three questions:

1. **Nothing writes `_DAT_8007B868`.** Its only store anywhere in the static
   corpus is a read-modify-write **clearing** bit 1, at `0x8001E008` in the
   boot mode-init `FUN_8001DCF8`; a raw byte sweep over all 1233 PROT entries
   adds only one incidental data word (`0392_map03.BIN +0x2bc40`, surrounded
   by non-code). So the word can never go non-zero in retail play - it is the
   same dev/dual-mode gate the whole actor-sound family
   (`FUN_800266E0`/`80026520`/`26740`/`26478`/`26410`) checks, and the arm's
   early-return when it is set just mirrors its callees, which would all
   no-op anyway.
2. **`_DAT_8007B750` bit 3 has exactly one setter**: `ori v1,v0,0x8` at
   `0x800246D0` inside `FUN_800243F0` - the BGM resolver/poller's
   load-settle stage, reached only while a track swap is in flight, after
   the settle countdown at `gp+0x768` (armed to `0x1E` frames at load
   start, `0x3C` when master mode is 2) hits zero. Immediately after
   setting it the poller stalls its own install while bit 0 (sub-op 9's
   "script-owned start") is up and bit 4 is not (`0x800246E0..E8`): the
   swap waits for the script's commit.
3. **`FUN_80026520` closes what `FUN_800266E0` only detaches**: `800266E0`
   resets the pan state and rewinds/stops the bound sequence
   (`FUN_80064370`, the `SsSeqRewind` wrapper) leaving the source active;
   `80026520` VSyncs, clears the source's active flag (`+0x8`), rewinds
   **and closes** the handle (`FUN_80061E94`, the `SsSeqClose` shim). The
   pair together is a full slot release, which is why the poller's own
   teardown path calls both.

So sub-op `0xA` is not a toggle: it is the **commit** half of the sub-op
9 / `0xA` swap handshake - wait until the incoming track is staged, release
the slot's paused occupant, ack with bit 4 (which unstalls the poller's
install), clear the pause bit 1. Full protocol + the flag word's bit map:
[`audio.md`](../subsystems/audio.md#the-track-swap-handshake-fun_800243f0--op-0x35-sub-op-0xa);
the arm quoted:
[`script-vm.md`](../subsystems/script-vm.md#sub-op-0xa-is-the-swap-commit).
An incidental yield of the same census: sub-op 2's pause **sets** flag bit 1
where sub-op 3 also sets it (calling the voice-stop `FUN_80026740`) and
sub-op 4 clears it (calling the re-attach `FUN_80026478`) - the legacy
Resume/Stop labels on 3/4 describe each other's arm.

Port: `SceneHost::route_bgm_events` routes sub-op 10 to
`BgmDirector::unhalt_pause` (release the source only while the pause latch
is set, then clear the latch unconditionally), overridden by the native
`AudioBgmDirector` and the browser `WebBgmDirector`; both starts also clear
the pause gate, as retail's sub-op 1 arm does. Pinned disc-side by
`crates/engine-core/tests/bgm_midscene_change_disc.rs` (town01's cutscene
records carry the op). `see ghidra/scripts/funcs/800243f0.txt`,
`800266e0.txt`, `80026520.txt`, `8001dcf8.txt`.

### Hyper Arts fanfare selector

*Status:* resolved - selector pinned in code and capture

A Hyper art fires **no pool shout** (its action constant sits below the shout table's `lo`
bound). Instead the staged-animation materialiser `FUN_8004AD80`, on the Hyper class byte
`0x1A` at `actor+0x1DA`, reads the queued art constant and fires the jingle queue
(`FUN_8004FCC8` → `FUN_8003D53C`) with `jingle_id = rand()%2*3 + base` - a per-(character,
art) coin flip between the fixed channel pair `{base, base+3}` of the character's stereo
fanfare bank (the even clip slots: Vahn `XA1.XA`, Noa `XA3.XA`, Gala `XA5.XA`). Super and
Miracle expansions take a sibling branch to fixed ids `0x101`/`0x111`/`0x121` = the same
bank's generic channel 1; a Miracle's finisher additionally fires its anim cue track
(`FUN_800508DC`, ids `0xC8..0xFF` rebased `+0x38`). No avoid-repeat memory, unlike the
shout pool. All nine per-art Hyper rows are capture-witnessed off the `FUN_8003D53C`
staging globals, and every witnessed duration reproduces the `0x800788B8` table
arithmetic against the real SCUS. Full selector + table:
[`battle-action.md`](../subsystems/battle-action.md); engine table
`legaia_art::hyper_fanfare::CAPTURED_FANFARES`. Residue (guarded, low priority): the
second pair member for seven of nine arts is rule-derived rather than witnessed, and the
Vahn/Noa Miracle finisher cue-track ids plus `XA1` channel 0 are unwitnessed.

### XA clip-table writer + `(clip_id, chan)` cue census

*Status:* resolved - writer pinned; cue census decoded

The `0x801C6ED8` clip-table content is pinned (34 `[CdlLOC][len]` slots = `XA1..XA34`, title-capture byte-exact vs the disc files). The filler is **`FUN_801CFA78`** in PROT 0895 `init.pak` (base `0x801CE818`, recovered from four in-blob string refs): it sprintf-generates `\XA\XA%d.XA;1` per slot and fills `[BCD-MSF][size]` via the ISO9660 lookup `FUN_8005DBB4`, called once from the init boot tick `0x801CF500`. The earlier "filler is an untraceable DMA/computed write" framing was the SCUS-only sweep's blind spot - the two `lui 0x801c` materialisation sites in SCUS (`FUN_8003D53C`/`FUN_8003EAE4`) are the **readers**, and the writer is overlay-resident, so no absolute-form scan of SCUS could see it.

A caller census of `FUN_8003D53C`/`FUN_8003EAE4` names each `(clip_id, chan)` cue. Decoded: menu
voice `FUN_8004FCC8`; the normal-move grunt (`XA30` chan 0/4/6, overlay `0x801EEB44`); the
**arts shout** (`FUN_8004C140` → `XA2`/`XA4`/`XA6` per character, per-art channel pool;
live-battle fires captured frame-tagged off the `FUN_8003D53C` staging globals by
`scripts/recomp/xa_cue_capture.py`, which also pins the live table variant + the packed
second-half spans - [battle-action.md](../subsystems/battle-action.md), witnessed picks in
`legaia_art::arts_voice::CAPTURED_ART_CHANNELS`); SM state-`0x6E` (`XA9` via `0x800787AF`);
slot machine `XA1`. Full deduped one-shot + streamed cue census in [`audio.md`](../subsystems/audio.md). Census note: PROT-entry over-read aliases callsites into neighbouring overlays - dedupe by true entry extent (gameover 0902 / world-map 0901 have zero genuine XA calls).

### SFX cue bank routing - the category byte selects the VAB slot

*Status:* resolved - a cue names its own bank, and both hosts now stage two banks
and route by category. Which PROT entry fills each slot is
[the next entry](#which-prot-entries-fill-sfx-vab-slots-1--3--6--11).

The mechanism. A descriptor's `+4` byte is a category, and it selects the 12-byte
mixer record at `0x80091508 + category*12`. That record's `+8` is a **VAB slot
id**, not a level: `FUN_80065034` hands it to `FUN_80068b98`, which rejects it
unless the per-bank open-state byte `_DAT_801CE368[id] == 1` and then repoints
the current-bank globals at that slot *before* the program / tone lookup. Across
the catalogued save states record `N` holds `+8 == N` and `+0` == slot `N`'s live
`VabHdr`, in every record of every state, so category `N` selects slot `N`. Slot
0 is **PROT 0868** (a live field state's 512-byte slot-0 `VagAtr` program-0 page
occurs verbatim in that entry at VAB offset `+4`, `ps = 5` agreeing); slot 2 is
the class-2 bank **PROT 0869** the battle scene loader `FUN_800520F0` loads with
`a1 = 2`. Histogram over the 100 descriptors: `0`:16, `2`:53, `6`:30, `11`:1.

Why it was worth grading rather than assuming. A port that stages one bank and
fires everything through it does not error and does not go silent, because both
banks carry a one-VAG-per-semitone UI key map at program 0 - so a category-`0`
id resolves to a *sibling* sample. The browser play page sounded its pause menu
out of PROT 0869 that way: genuine retail data, roughly twice as long and a
fifth lower than the field menu's, because 0869's `center` bytes are authored
higher. Peak, duration and "did a voice key on" all pass in that state; the only
observable that separates them is which entry the samples came from, which is
what the disc-gated oracles now assert.

The port. `legaia_asset::sfx_table` carries the law (`slot_for_category`,
`prot_index_for_slot`, `SLOT_BANKS`, `PINNED_SLOT_BANKS`); `engine-shell`'s boot
and the browser play page each stage the two resident banks out of **one** SPU
allocator over their shared reserved region and resolve every cue through its
own slot. Categories `6` and `11` fall back to the class-2 bank - the exact
pre-routing behaviour. Byte-level detail:
[`sfx-table.md`](../formats/sfx-table.md#category-is-a-bank-selector-and-four-banks-are-open-at-once).

### Which PROT entries fill SFX VAB slots 1 / 3 / 6 / 11

*Status:* resolved - slot `6` is **PROT 0876** and slot `11` **PROT 0889**; slots
`1` and `3` hold banks that are re-selected at runtime, so neither has a fixed
entry to name. Grade `disassembly` for the bindings, with a `capture` byte-pin
and a structural cross-check on top.

**The installer names every binding.** A bank reaches a slot through one pair of
calls: `FUN_8001FC00(raw_toc_index, category, buf, append, len)` streams the
entry in, and `FUN_8001E54C(category, buf, len)` installs it - indexing the same
12-byte mixer record the descriptors do, taking the header buffer from `+0` and
the VAB slot from `+8`, and opening the bank via `FUN_8002630C` →
`SsVabOpenHead` (sticky, at the SPU address the per-slot table at `0x800917B0`
holds) → `SsVabTransBody`. Reading `a0` at every call site of `FUN_8001E54C` is
therefore the sweep that closes this, and the earlier framing ("sweep the
loader's `a1`") named the wrong argument: `FUN_8001FC00` ignores its second
argument entirely - it is carried only so the pair reads as one binding.

| Slot | Filler | Call site |
|---|---|---|
| `0` | PROT 0868 | resident system bank |
| `1` | current BGM bank (`music_01`), variable | `FUN_800243F0`, `raw = *(0x8007BC64) + id - 2000` |
| `2` | PROT 0869 (raw `0x367`) / `0875` | `FUN_800520F0`, `FUN_801CF00C` |
| `3` | a `vab_01` side-band bank, variable | `FUN_800243F0`, `raw = *(0x8007BBE4) + id - 2000` from `_DAT_8007BABC` |
| `6` | PROT 0876 (raw `0x36E`) | field init `FUN_801D6704` |
| `7` / `8` | the two `monster.snd` banks | `FUN_8003E104` from `FUN_800520F0` |
| `11` | PROT 0889 (raw `0x37B`) | battle-end reward resolution `FUN_8004E568` |

**Why the two new pins are not just an argument read.** PROT 0889 populates
exactly one `ProgAtr` slot - number **10** - and the one category-`11`
descriptor (`0x50`) names program 10 with 2 voices against that program's 2
tones; the function that loads it is the same one that fires the cue. PROT 0876
holds **30** VAGs for the 30 category-`6` descriptors, and its populated
programs `1..=7` cover 29 of the 30. A catalogued field state's live slot-6 and
slot-1 header buffers match extraction 0876 and 0998 byte for byte - unique hits
across all 218 VABs on the disc, once the runtime-written `ProgAtr +8..0xF`
words are excluded.

**Two laws fell out of the same read.** `FUN_8001D424` writes `+8 = record
index` for all 16 mixer records, so "category *is* the slot" is the
initialiser's own statement rather than a cross-state observation; and it
assigns four pairs of records one shared header buffer, which `FUN_800265E8`
matches with one shared SPU base. Slot 6 and slot 2 are consequently **the same
physical bank in two modes** - which is why retail needs no extra SPU room for
the field cues, and why a host that stages once at boot cannot simply add them.
The save-state catalogue confirms the partition without ambiguity: the open-state
array `_DAT_801CE368` holds slots `0,1,3,6` in every field-family state and
`0,1,2,7` in every battle state, and never 2 and 6 together.
Map, budget arithmetic and the port surface:
[`sfx-table.md`](../formats/sfx-table.md#which-prot-entry-reaches-which-slot).

### The `FUN_8006EF18` trio is a BIOS kernel-patch sequence, not an SPU init

*Status:* resolved-negative - the trio touches no SPU register, voice block or
libspu global. Grade `disassembly`: the veneer bodies and the patch payloads are
both read out of the executable, which is what the open thread asked for.

`FUN_8006EF68` is a bare BIOS stub (`li t2,0xb0; jr t2; li t1,0x4c`) = B0 `0x4C`
`StopCARD`; its immediate neighbours `8006EF48` / `8006EF58` are the same shape
with `0x4A` `InitCARD` and `0x4B` `StartCARD`. The other two callees patch
kernel **code**:

- `FUN_8006F088` calls `GetB0Table`, takes entry `0x5B` (`ChangeClearPAD`) as a
  version-stable anchor, and **swaps** five words between `+0x9C8` off it and
  the static block at `0x8006F058`. The shipped block is a `jalr` trampoline
  back to `0x8006F058`, so after the swap the kernel calls a buffer that holds
  its own displaced instructions, falls through into a `0xC8`-iteration
  busy-wait at `0x8006F070`, and returns - a timing delay spliced into a kernel
  routine. A swap is its own inverse, which is why install and teardown both
  call it.
- `FUN_8006F118` calls `GetC0Table`, takes entry `6` (`ExceptionHandler`) and
  copies three words from `0x8006F180` over `+0x70..+0x78` - blanking the
  immediate pair that its install-side sibling `FUN_8006EFD0` reads to
  reconstruct a kernel address (and then patches at `+0x28` with a jump out into
  SCUS).

Both are bracketed by `EnterCriticalSection` (`syscall(1)`) and `FlushCache`
(A0 `0x44`). The install veneer is `FUN_8006EE8C(pad_enable)` -
`ChangeClearPAD(0)`, `InitCARD`, then `_EFD0` + `_F088` - and `FUN_8006EF18` is
its teardown mirror, which is exactly why the caller `FUN_8002035C` runs it after
closing eight kernel event handles. Table + citations:
[`functions/runtime-libs.md`](functions/runtime-libs.md#the-bios-kernel-patch-cluster-8006ee8c--8006ef18).

### `_DAT_8007B910` is the live audio level, not screen brightness

*Status:* resolved - the cell is a **volume**, `_DAT_8008457C` is its persistent
reference, and the two labels the corpus carried were never in tension: one of
them had no instruction behind it. Grade `disassembly`.

The discriminator the open thread named was `FUN_80062004`'s libsnd entry, and
it settles cleanly: `FUN_80062004(a, b, c)` tail-calls `FUN_80061EDC(a, 0, b,
c)` = `SsSeqSetVol(slot, channel 0, vol, …)`. So the halved cell
(`(v << 15) >> 16`) that `FUN_800267A8` passes lands in the **volume**
argument. The second reader is the same answer from a different direction:
`FUN_80026478` hands `v >> 1` to `FUN_8002657C`, which writes it as *both*
channels of `FUN_80064890(slot, vol_l, vol_r)` - a symmetric level, so not the
directional pan that function was labelled with either.

A full sweep of the dumped corpus finds **26 read sites** of the cell. They
resolve to `SsSeqSetVol` (six), `SpuSetCommonAttr` (`FUN_8006BCB4`, four - each
building an `SpuCommonAttr` on the stack with the cell in the CD-volume pair),
the audio-context volume re-apply `FUN_8002614C`, `FUN_8002657C`, and
arithmetic / tween plumbing. **None reaches a draw primitive.** The cold reset
`FUN_8001FFA4` seeds `0xD7` into both the persistent `_DAT_8008457C` and the
live `_DAT_8007B910` and then calls `FUN_8002614C(0)` - the volume re-apply.
The range agrees too: a `0..255` cell halved is exactly libsnd's `0..0x7F`.

What the ramps become. The battle-action states `0x35` / `0x6F` / `0x70` duck
the mix to 75% of the configured level (50% for spell ids `>= 0x99`) and `0x51`
restores it; the world-map sub-list halves it on open and doubles it on close;
the field VM's `MENU_CTRL` sub-`0xD` sets it to `(input * _DAT_8008457C) >> 12`,
a percentage of the player's setting.

Why the brightness reading looked right anyway: a summon really does dim the
screen, and it ramps in step - but that is a **different scalar**,
`_DAT_8007B440`, ramped by `FUN_801ED308` and drawn by the wipe/curtain emitter
`FUN_8003479C` (clamped `0xF2`). Ports renamed with the fact:
`BattleActionHost::duck_audio_level`, `BattleEvent::DuckAudioLevel`,
`SubListEffect::ScaleAudioLevel`, `PanelActorHost::audio_level` (seeded `0xD7`
like retail). Detail:
[`battle-action.md`](../subsystems/battle-action.md#the-_dat_8007b910-ramps-are-an-audio-duck).

### Key-on pitch: unity on centre

*Status:* resolved - `note == center` keys **`0x1000`**, unity, 44.1 kHz.

Both the SFX/direct key-on path (`FUN_80065034`) and the sequencer note-on path
(`FUN_80066308`) reach the same arithmetic (`FUN_80066e50` / `FUN_80066d8c`) and
hand the result to `FUN_80067550`, which stores it **verbatim** into the shadow
register file at `0x801CE084 + voice*16` (voice `+4` = pitch). Nothing rescales it
afterwards:

```
n     = note + 60 - center + carry        (MIPS div: truncates toward zero)
pitch = PITCH[(n % 12) * 16 + fine] << (n / 12 - 5)
```

`PITCH` is the 192-entry table at `DAT_8007A940` (SCUS file `0x6B140`). Every
entry is exactly `floor(0x1000 * 2^(k/192))` - **192/192 verified against the
disc**, first entry `0x1000`, last `0x1fe2`. So it is a one-octave table at
1/16-semitone resolution starting at unity, and the octave is applied by the
shift. Because the closed form is exact, no disc bytes are needed to reproduce it.

The retail cue arm passes `fine = 0x40` at every traced `FUN_80065034` call site,
so a cue keys half a semitone above the sequencer for the same tone; the two paths
also differ in whether the fine index saturates or carries a whole semitone.

**Why this was worth grading rather than assuming.** A 22.05 kHz VAG body is
authored with `center` twelve semitones high, so the sample rate is *already
encoded in `center`*. Applying a `22050/44100` factor on top double-counts it and
keys every voice exactly one octave low - which is what the port did, for BGM and
SFX alike. Corroborated by capture: 126 of 128 voices holding a non-zero staged
pitch match this law exactly, with the 2 misses being records whose bank was
swapped after key-on.

**The recomp PCM oracle could not have caught it**, because it mirrors retail's
captured pitch into the engine SPU rather than deriving a pitch to compare. An
oracle that copies the answer cannot check the answer. Full law and the port's
two defects: [`audio.md` § key-on pitch law](../subsystems/audio.md).

### `FUN_80018DB0` is a rumble cadence, not an audio one

*Status:* resolved - the surrounding cluster is **libpad**, `DAT_800915DA`/`DB` are port 0's actuator bytes, and the kernel plays no sound at all. Closes "retail's footstep SFX cue id" as a resolved-negative: there is no cue to pin.

The two entries the corpus filed as SsAPI are libpad, and the identification is instruction-level on both sides (the `FUN_8006CE30` and `FUN_8001D230` windows were re-read straight out of `extracted/SCUS_942.54` at `0x800 + va - 0x80010000`, so the dump's printed addresses are not load-bearing).

- **`FUN_8006E2B4(buf0, buf1)` = `PadInitDirect`.** `FUN_8001D230` `bzero`s `0x44` = 2 x `0x22` bytes at `0x800840F8` and calls it with `(0x800840F8, 0x800840F8 + 0x22)` (`addiu a1,a0,0x22`) - the canonical pair of 34-byte direct-mode report buffers. It clears `0x1E0` = 2 x `0xF0` at `0x801CE628` (one context per socket), stores the two buffers at each context `+0x30`, seeds each buffer `[0] = 0xFF` / `[1] = 0`, and fills six bytes at context `+0x5D` with `0xFF` - `PadSetActAlign`'s unassigned default. The pad pump `FUN_8001822C` decodes those very buffers as `[status][type nibble][inverted u16 buttons]`, port 1 at `+0x22`/`+0x23`.
- **`FUN_8006CE30(socket, table, len)` = `PadSetAct`.** Three arguments in the instructions: `a0` passes through untouched into the context resolver `jalr _DAT_801CE564`, `a1`/`a2` are stashed in `s0`/`s1` and forwarded. Ghidra's C drops `param_1` - artifact #1 in [`ghidra.md`](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims), and exactly what made a 3-argument libpad call read as a 2-argument sequencer setter. The tail `FUN_8006D7B4` stores `ctx+0x28 = table`, `ctx+0x34 = (u8)len`.
- **The siblings agree.** `FUN_8006CA7C` = `PadGetState` (report status byte through `ctx+0x30`, then normalises `ctx+0x49`); `FUN_8006CB3C` = `PadInfoMode`, whose `term = 4` branch returns the id-table length `ctx+0xE3` for `offs < 0` and otherwise the bounds-checked `((u16 *)ctx[0])[offs]` - `InfoModeIdTable`'s contract, with no sequencer analogue; `FUN_8006CDB0` = `PadSetActAlign`; `FUN_8006D1E0`/`FUN_8006D2AC` = `PadStartCom`/`PadStopCom` (`ChangeClearRCnt(3, 0)` vs `(3, 1)`). `FUN_8006EE8C`/`FUN_8006EEE0` call `ChangeClearPAD` (B0 `0x5B`) and wrap `InitCARD`/`StartCARD` (B0 `0x4A`/`0x4B`); `FUN_80056618` = `_bu_init`. The eight `OpenEvent`/`EnableEvent` pairs on `0xF4000001`/`0xF0000011` are the memory-card event set.
- **The bytes `FUN_80018DB0` writes are that actuator table.** It stores to `0x800915DA`/`0x800915DB`, and `FUN_80018F94` registers the same block per port with `PadSetAct(socket, block+2, 2)` where `block = 0x800915D8 + (socket>>4)*0x40 + (socket&3)*0x10` - the `0x40` stride matching `FUN_8001D230`'s `s1+2` / `s1+0x42`, and the `0x80`-byte `bzero` matching 2 x `0x40`.
- **`DAT_8007B79C` is not a footstep-active flag.** `FUN_80018F94` sets it from `_DAT_800845A8 == 0 && PadInfoMode(socket, 2, 0) == 0` - the pad reports no extended-mode data. It selects between two actuator payload layouts: set → `act[0] = 0x40` fixed with `act[1]` carrying the pulse; clear → `act[0]` carries the pulse and `act[1]` is loaded with the low byte of `gp+0x618` every frame.
- **There is no audio call in the kernel.** Its other branch counts down ~1200 frames and calls `FUN_8005C034(9, 0)`, the retry wrapper over `CdControl` (`FUN_8005CF80`) issuing `CdlPause` - a CD-drive pause, not a voice stop or rewind.

**What this implies for the two "per-voice trigger bytes".** They are one actuator payload: a per-step on/off pulse and an intensity level, transmitted by libpad every poll. Correspondingly, `gp+0x614`/`gp+0x618` are not a locomotion speed - `+0x618` is written verbatim into an actuator level byte, so they read as vibration-intensity requests, and their writers stay unpinned. That also explains the capture: `_DAT_8007B8A4` pinned at `2` across four field and overworld runs means nothing was requesting vibration while walking, which is what the game's own Vibration options (battles / events / encounters) would predict.

**What the SsAPI label rested on, and why it fails.** Three things, each individually reasonable: the `0x8006C000..0x8006F000` band does hold genuine libspu/libsnd code; a vtable of installed hooks over a stride-`0xF0` record array with an `0xFF` idle fill and a per-record state byte reads exactly like a sequence-worker table; and with `param_1` dropped, `FUN_8006CE30` renders as "set user data on a resolved context".

It fails on three checks: the resolved context's `+0x30` is provably the button report `FUN_8001822C` decodes, `PadInfoMode`'s id-table branch has no sequencer reading, and the record count is 2 - the number of controller sockets, not a sequencer's slot count. The port `engine-audio::footstep` mirrors the arithmetic correctly and keeps its `// PORT:` tag; only its labels were wrong. Corrected cluster: [`audio.md`](../subsystems/audio.md#not-ssapi-the-0x801ce628-cluster-is-libpad).

Provenance: `see ghidra/scripts/funcs/8006e2b4.txt`, `8006ce30.txt`, `8006d7b4.txt`, `8006ca7c.txt`, `8006cb3c.txt`, `8006cdb0.txt`, `8006d1e0.txt`, `8006d2ac.txt`, `8001d230.txt`, `8001822c.txt`, `80018db0.txt`, `80018f94.txt`, `8005c034.txt`.

### XA channel map / STR demux SM

*Status:* resolved - the historically "overlay-blocked" halves are statically decompiled from PROT 0970 at its base + the SCUS St library; three superseded readings worth not re-walking.

- **No XA channel selector exists in the STR overlay.** FMV playback reads with Setmode `0xE0` (`Speed|RT|Size1`, sector filter **off**): the drive hardware-plays every ADPCM sector, and each `MOV/MV*.STR` interleaves exactly one XA track at `(file 1, chan 0)` (raw-subheader-verified across all six movies). The old hypothesis - "the channel selector is driven by the multi-channel `\DATA\MOV.STR` container" - is **falsified**: `MOV.STR` is a dev path in slots 11..=22 of the dispatch table, absent from the disc. The real per-cue channel selector is the SCUS XA-clip sequencer `FUN_8003D764` (`CdlSetfilter {file 1, chan}`, mode `0xC8`), used for the `XA1..XA34` voice/music files, not for movies. See [cutscene.md § XA channel selection](../subsystems/cutscene.md#xa-channel-selection).
- **The FMV dispatch table stride is 32 bytes, not 64.** The selector at `0x801CEC9C` is `sll v0,v0,0x5`; the earlier `sll v0,v0,6` transcription paired wrong slot halves and concluded `MV2`/`MV5` were unreferenced and the `town0d`/`uru`/`jouine` triggers vestigial.
  Under the disc bytes (byte-identical in the RAM capture) all nine retail slots `0..=8` resolve - every movie on the disc plays, `MV3.STR` carries four abutting segments - and the master dispatch `FUN_801CEA3C` hands each mid-game FMV off to a **return scene** (the seven-label table at `0x801CE8AC` + spawn word). Corrected mapping + parser: [str-fmv-table.md](../formats/str-fmv-table.md#authoritative-runtime-mapping), `legaia_asset::fmv_dispatch` (disc-gated `fmv_dispatch_real`); the engine resolver `legaia_engine_core::cutscene::fmv_index_to_str_filename` mirrors the corrected nine-slot map and the `0x801CE8AC` return scenes.
- **The "compact MV table" was libcd's directory cache mis-phased.** The 24-byte records at `0x801CAE08` are `CdlFILE` structs (`[loc][size][name[16]]`); the historical name-first parse paired each name with the next record's location, manufacturing the "MV1 points at disc MV2 / MV6 points at XA15" shift. See [str-fmv-table.md](../formats/str-fmv-table.md#directory-record-cache-0x801cae08-24-b-cdlfile-records).

### SPU reverb live routing (C7-REVERB)

*Status:* resolved - retail runs **`Studio C`, master-enabled, globally**; the "selective per-cue reverb-enable source" the hunt was looking for does not exist.

A pure-Rust read of the save-state corpus (no live probe) settled it. `legaia_mednafen::PsxSpu` reads the SPU register shadow (`Regs` block): `reverb_master_enabled` (`SPUCNT` bit 7), `reverb_registers` (the 32 reverb coefficient/address registers at `0x1F801DC0..0x1F801DFF`), and `voice_reverb_mask` (the per-voice `EON` enable at `0x1F801D98`/`0x9A` - which mednafen also mirrors under its `Reverb_Mode` sub-entry, a byte-for-byte cross-check across every state). CLI: `mednafen-state spu <state>`.

Across all 45 mednafen states (field / town / battle / summon / title / minigames):

- **Master reverb is always enabled** (`SPUCNT` bit 7 set everywhere). No scene toggles it.
- **The preset is `Studio C` everywhere** - the 32-register block is byte-identical in every state and matches the `StudioC` libspu preset exactly (`dAPF1=0x00E3`, `dAPF2=0x00A9`, work area `0x6FE0`). [`engine_audio::ReverbMode::identify`](../../crates/engine-audio/src/spu/reverb.rs) resolves the captured block → `StudioC`.
- **Per-voice reverb-send (`EON`) is broad** - 15–22 of 24 voices in any state, BGM + SFX alike. Reverb is the default routing, not a per-cue effect.

So the blocker (the per-cue enable source) dissolves: there is nothing to trace. **Wired:** the live engine calls `Spu::set_retail_reverb` once at SPU init (`StreamResampler::new`) - `ReverbMode::StudioC` + every voice routed. The PCM oracle's retail-side reverb is also fixed (it previously mis-read the EON mask as a mode byte and ran `Off`). Residual is only the output-depth tuning (`SpuSetReverbDepth`, `vLIN`/`vROUT`; the engine uses a fixed half-scale approximation). Falsifies the earlier "Spirit-Arts / echo cues opt in, everything else dry" reading in [`audio.md`](../subsystems/audio.md#retail-reverb-routing---studio-c-always-on-capture-confirmed).

## Title / boot / overlays

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| `_DAT_8007B8C2` polarity, and its writer | resolved (docs were backwards) | `disassembly` + `capture` | [details ↓](#_dat_8007b8c2-polarity-and-its-writer) |
| `title.pak` PROT entry | resolved | `capture` | [details ↓](#titlepak-prot-entry) |
| Title screen mode-table PROT | resolved (no such entry) | `inference` | [details ↓](#title-screen-mode-table-prot) |
| Load-screen panel 9-slice geometry | resolved (engine renders byte-perfect) | `capture` | Pinned in [`subsystems/save-screen.md`](../subsystems/save-screen.md#pinned-9-slice-tile-rects-system-ui-tim-clut-row-2): retail composes the 81×29 panel at dst `(6, 4)` from 14 textured-sprite primitives (GP0 cmd `0x64`) sampling the system-UI sheet with CLUT `(32, 511)`. The exact per-tile rects are exported as `legaia_asset::title_pak::OVERLAY_SYSTEM_UI_PANEL_*` and emitted by `legaia_engine_render::save_select_chrome_draws_for` (covered by `save_select_chrome_emits_9slice_panel_and_pills` test). No interior fill sprite is drawn - the "marbled blue" look is the dimmed title art bleeding through the empty middle of the frame. |
| Key-item area consumers (`0x800859E8..0x80085A40`) | resolved (narrow negative); reader list incomplete | `disassembly` (enumeration `inference`) | [details ↓](#key-item-area-consumers) |
| XP-table source + reader | resolved + ported | `capture` | [details ↓](#xp-table-source--reader) |
| New-game world-state seed store widths (`FUN_80034A6C`) | resolved (port confirmed, no change) | `disassembly` | [details ↓](#new-game-world-state-seed-store-widths) |
| Overlay identity from the disc (static extraction) | resolved (pipeline landed) | `capture` | [details ↓](#overlay-identity-from-the-disc-static-extraction) |
| SCUS recomp gap - render/GTE + boot/init clusters | resolved (aliases + libgte residue + dev tooling; `main()` documented) | `disassembly` | [details ↓](#scus-recomp-gap---rendergte--bootinit-clusters) |
| Options/menu overlay PROT entry | resolved (RAM-verified; PROT 0899 @ `0x801CE818`) | `capture` | The options/pause/inventory-equipment-status menu overlay is **PROT 0899**, not 0896: `FUN_801CF650`'s signature byte-matches PROT 0899 file `0xe38`, and the `.text`+`.rodata` prefix is byte-identical across six menu-open saves. VA-alias sibling of the field overlay 0897 in slot A - the menu overlay replaces the field overlay at the base. The earlier "0896 = menu" label is falsified. |
| PROT 0896 (`bat_back_dat`) identity | resolved | `capture` | The unique ~`0x9000`-byte head is the **vestigial Japanese-build field-menu / config / status overlay** - the debug-string sibling of the English retail menu overlay PROT 0899 (same `~0x801D0000` window-renderer VA family, a `"FWIN ERR %d"` printf at file `0x3D4`, `0x414`-byte char-record indexing). 0899 ships the English label set with zero `FWIN`; a signature scan finds 0896 resident in **0** of 140 states (control: English "Battle Voices" resident in 10), so the USA build never loads it. [details ↓](#prot-0896-bat_back_dat-identity) |
| Slot-A scene-overlay family beyond field/battle/menu | resolved (in the static map) | `disassembly` | The rest of the slot-A (`0x801CE818`) VA-alias family is pinned from the disc: **0970 cutscene_str** (STR/MDEC FMV, modes 26/27) and the minigame overlays **0972 fishing / 0975 slot_machine / 0976 baka_fighter / 0980 dance** (the mode-24 `0x3E` door-warp sub-id slots 0/3/4/6), each cross-checked by a documented function landing on a prologue at the base. Minigame entries over-read each other (phantom-base risk); the canonical entry recovers `0x801CE818` and is the entry the warp streams (the historical "slot_machine = 0973 @ `0x801CA818`" was the phantom - the image inside 0973's over-read tail). Found via `asset overlay scan` + the leading dev string. |
| "world-map / save / shop" overlay PROT entries | resolved (not separate entries) | `disassembly` | The world-map / overworld controller `FUN_801E76D4` lives in the **field overlay 0897** (base+0x18EBC), and the save-slot dispatcher `FUN_801DC6B4` + the shop/buy session live in the **menu overlay 0899** (save at base+0xDE9C) - each function's instruction signature byte-matches only that one entry (`asset overlay find-sig`). So "world-map", "save", and "shop" are *subsystems* of existing slot-A overlays, not separate PROT entries; recorded in the 0897 / 0899 map notes. |
| PROT 0977 / 0978 extraction + the dump re-key | resolved | `disassembly` | [details ↓](#prot-0977--0978-extraction--the-dump-re-key) |
| Slot-B capture-module band `0935..0966` per-entry identity | resolved (statically derived, capture-corroborated) | `disassembly` | [details ↓](#slot-b-capture-module-band-09350966-per-entry-identity) |
| Phantom-VA sweep of the PROT 0897 imports | resolved | `disassembly` | [details ↓](#phantom-va-sweep-of-the-prot-0897-imports) |
| Debug flag `0x8007B98F` | resolved (the MSB of the debug-mode word `_DAT_8007B98C`) | `disassembly` + `capture` | [details ↓](#_dat_8007b98f-is-byte-3-of-the-debug-mode-word-_dat_8007b98c) |
| New-Game opening chain + narration roller | resolved (chain + caption + roller + prologue gold grade; far-geometry residual resolved-negative) | `capture` + `disassembly` | [details ↓](#new-game-opening-chain--narration-roller) |
| Overlay-loader index off-by-2 - remaining ripple | resolved (slot A reconciled; slot-B per-spell identity capture-pinned) | `capture` + `disassembly` | [details ↓](#overlay-loader-index-off-by-2---remaining-ripple) |
| Slot-B overlay cluster (`0900..0969`) per-entry identity | resolved for every entry | `capture` + `disassembly` | [details ↓](#slot-b-overlay-cluster-09000969-per-entry-identity) |
| PROT 0968 - what it is, who loads it, and how big it really is | resolved (residency capture still owed - on the open page) | `disassembly` | [details ↓](#prot-0968---the-cort-battle-stage-overlay) |
| `0x80010390` - the SCUS word that looked like a lead on 0968 | resolved: it is the slot-B overlay destination pointer, shared by every slot-B entry | `disassembly` | [details ↓](#0x80010390-is-the-slot-b-overlay-destination-pointer) |

### `_DAT_8007B98F` is byte +3 of the debug-mode word `_DAT_8007B98C`

*Status:* resolved - no byte-granular reader exists; the 32-bit word is the consumer surface, statically pinned and runtime-confirmed

Neither `0x8007B98F` nor its sibling `_DAT_8007B8C2` is BIOS-zeroed: the PS-X EXE header carries `b_addr = 0, b_size = 0`, so no BSS is cleared for this executable at all. The earlier "zero-initialised at boot" framing was wrong independently of any polarity question. (The `_DAT_8007B8C2` half of the old thread is settled separately - see [`_DAT_8007B8C2` polarity, and its writer](#_dat_8007b8c2-polarity-and-its-writer).)

**Corpus sweep.** The dump sweep across SCUS + every captured overlay finds zero
references - read or write - to `_DAT_8007B98F`, because it is **not read byte-granularly
at all**: it is byte +3 (the MSB, little-endian) of the 32-bit debug-mode word
`_DAT_8007B98C`, and that word is the real consumer surface. Grep of
`ghidra/scripts/funcs/` for `8007b98f` returns 0 hits; `_DAT_8007B98C` is read as the
debug gate in SCUS (`FUN_8001822c` at `8001822c.txt:500/533`, plus
`80016230`/`80016444`/`800173bc`/`800188c8`/`8003cbf8`/`8004ad80`/`80025cb4`) and across
the field/dialog/world-map overlays (an aligned word-search of the 23 static overlays
finds 14 genuine `lw ...,-0x4674(reg)` reads of `0x8007B98C` in the field overlay 0897,
base reg = `0x80080000`), with the sole `sw` writer in the shared menu/title/save-init
routine (`overlay_menu_801de234`/`overlay_title_801ddccc` internal offset `0x4158`). So
`SELECT+START` / GameShark writing `0x8007B98F = 1` sets the MSB of the word, and every
`_DAT_8007B98C != 0` gate then reads the debug mode active. The earlier "stripped at link
time / inert" AND "consumer in an uncaptured overlay" framings are both superseded: the
consumer is `FUN_8001822c` + the resident field-overlay gates, statically pinned, no
capture required. See
[`subsystems/boot.md` § Debug flags](../subsystems/boot.md#debug-flags) and
[`reference/builds.md` § Debug input bindings](builds.md#debug-input-bindings) for the
combo table.

**Runtime confirmation.** The static model was derived without ever opening the
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

*Status:* resolved - the chain, caption, roller, and prologue gold grade are pinned; the far-geometry-brightness residual closed resolved-negative

**The roller config op's operand decode is re-derived from the field-overlay
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
camera-mover law rest on captures.

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

**Render-fidelity residuals - both closed:**

- **Prologue gold grade = palette-space collapse (grade `capture`).**
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
- **Far-geometry brightness (resolved-negative, grade `disassembly` + `capture`).**
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
  [Field decoration path](#field-decoration-path---does-it-dispatch-the-ncc-light-handlers)),
  made visible only by opdeene's unusually dim ambient plus the port's lack of
  distance culling widening the sampled far region. Reproducing it faithfully would
  mean porting a GTE ambient/light op that contradicts that boundary, so no engine
  change was warranted.

### Overlay-loader index off-by-2 - remaining ripple

*Status:* resolved - slot A reconciled, per-spell summon identity capture-pinned across every block (player, evolved, flutes, enemy), engine mirrors updated

The overlay loaders (`FUN_8003EBE4`/`FUN_8003EC70` → `FUN_8003E8A8(param + 0x381)`) resolve against the in-RAM TOC at `0x801C70F0`, which is **raw `PROT.DAT` from byte 0** (byte-verified vs the `door_warp_town01_to_map01` state); the extraction index space slices entry starts 2 words higher, so the loaded entry is **extraction `param + 0x37F`** - every historical `param + 0x381` PROT attribution is 2 high. Slot A is fully reconciled (field 0897 = mode 2, battle 0898, menu 0899 = mode 22, STR-path 0969, cutscene 0970, debug menu 0971 = mode 0, the seven `0x3E` minigame slots, efect-test 0979 = mode 8 - each content/prologue-anchored; see [`boot.md`](../subsystems/boot.md)). The three sub-threads:

1. **Per-spell summon-stager identity (slot B) - every id capture-pinned.**
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
   evidential. The whole spell block `0x81..=0x8B` is capture-pinned to `903..=913` (one mid-cast
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
   base block. **Eight legs are capture-pinned** by mid-cast states (loader-B id +
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
   casts - so neither seats a live render-mode part.
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
2. **The 0977 sub-id-5 minigame.** `0977` ("Ronginus") is the mode-24 case-5 **door/init** slot: the `0x801CEA6C` init prologue + the arena monster-name roster + `other6` dev paths. The Muscle Dome **match SM `FUN_801D0748` + all its data lives in the battle-action overlay (PROT 0898)**, not in `0977` and not in a separate aliasing overlay - the arena is a *mode of the battle engine* (fighters are battle actors, entered directions resolve through the battle-action path).
   Pinned by `asset overlay find-sig` of the controller prologue (`lui v0,0x8008; lw v0,-0x42dc(v0)` reading the ctx `_DAT_8007bd24`) → 0898 @ base `0x801CE818` file offset `0x1F30`, plus the deck/sub-draw/victory tables resolving in-overlay (`legaia_asset::muscle_dome::verify_resident`; the Duckstation `overlay_muscle_dome.bin` capture was that overlay's slot).
3. **Engine mirrors.** `OVERLAY_PROT_BASE` carries the extraction-space `0x37F` (the engine host chain - `prot_one_shot_load` → `entry_start_lba_retail`, whose `toc` array starts at raw dword 2 - consumes extraction indices, so the raw `+ 0x381` loaded entries 2 high); `summon.rs` maps `0x81..=0x8B → 903..=913` directly. The constant's unit test documents the raw-vs-extraction shift.

### Muscle Dome match shape: an ordinary battle ladder, not a card battle

*Status:* resolved (disassembly) -
[`minigame-muscle-dome.md § Course ladder`](../subsystems/minigame-muscle-dome.md#course-ladder-the-opponent-per-course-round)

The arena's match rules read as a card battle scored on a per-fighter HP
ratio "out of 108". All three parts of that are wrong. The four "cards" are
the four d-pad **direction commands** `0xC..=0xF`, each carrying that
fighter's own AP cost - the same input a normal battle command screen takes,
bounded by AP. The `0x6C` came from consuming only part of the compiler's
`× 100` shift-add chain at `0x801d0f38..0x801d0f4c`.

What the arena *is*: a ladder of ordinary battles. PROT 0977's course
descriptor table (`0x801D1A08`, three `{ i32 rounds; ptr first }` records)
walks 29 `{ u32 label; u32 monster_id }` round records at `0x801D1920`, and
`FUN_801D1510` stores the round's id into formation slot 0 at `0x8007BD0C`.
Courses are 8 / 8 / 13 rounds, matching the populated rows of the score
table at `0x801D1860`, and the 29 ids resolve against PROT 867 to the
curated `casino.toml` line-ups 29 of 29 in order.

Superseded within this entry: "battle type `0xB6` under a four-turn limit".
`0x8007BD0C` is the **formation cell**, not a battle-type byte, so the
strip's gate reads "the first enemy is monster `0xB6`" - Koru, the game's one
four-turn timed boss - and no dome round fields that id. Falsification
trail: [`re-do-not-re-walk.md`](re-do-not-re-walk.md#muscle-dome-was-never-a-card-battle).
Ports: `engine-core::muscle_dome` (`parse_course_ladder` /
`course_score_cell` / `resolve_turn` playing whole strings per actor /
`DomeDamageModel`, the one retail damage kernel both hosts resolve through).

### The dome runs two state machines; the outer one is the contest

*Status:* resolved (disassembly) -
[`minigame-muscle-dome.md § Two state machines`](../subsystems/minigame-muscle-dome.md#two-state-machines-not-one)

The battle round driver `FUN_801D0748` is the *inner* machine and has exactly
one contest-gated arm (`0x801D322C`, the flee path). The **contest** - which
`(course, round)` is staged, whether the run continues, what a cleared leg is
worth and what the run pays - is a second machine living wholly in PROT 0977:
`FUN_801CEA6C` re-entered after every leg, and the hub `FUN_801CF870`
dispatching `DAT_801D1A78` through a 51-entry jump table at `0x801CE990`.

Course and round are packed in the low byte of the mode-24 sub-id word
`_DAT_8007BAC0` (`course = ((w-1) & 0xFF) >> 4`, `round = (w-1) & 0xF`); a
finished leg is `w += 1`. Which course opens is picked by story flags
`0x536`/`0x537`/`0x538`, and only the Master course's length is clamped, by
`0x378`/`0x382`/`0x471`.

Two things this settles that had been open. **"Which arm decides a leg was
survived"** is neither of the two `FUN_801D0CD4` / `FUN_801D0068` arms it was
hunted in: it is `DAT_8007BD60 & 0x80` at `0x801CEDD8`, cleared by the
battle's own `0x5A` party-wipe scan. So `settle_contest`'s `continuing` input
is derived, not prompted. And **what the six tally rows hold** - three of them
are HP recovery (`round*2`, `min(turns,8)`, `[8,12,4,2][outcome]`, each
`× max_hp / 100`) draining into the restore accumulator `DAT_801D1AC8`; only
the `(course, round)` score cell reaches the coin tally.

A cleared course therefore banks its whole score row, which is the join that
corrected the curated Master reward from 13856 to the disc's **13830**. Port:
`engine-core::muscle_dome::DomeContest`, driven by `World::report_muscle_leg`
/ `World::settle_muscle_contest` and the browser's `muscle_contest_*`
bindings.

### Battle arts-input UI decomposition (dome = standard battle input)

*Status:* resolved (capture) - the input screen's full piece decomposition +
flow are packet-pinned in
[`minigame-muscle-dome.md § Arts command input`](../subsystems/minigame-muscle-dome.md#arts-command-input-packet-pinned)

What the arts command input (the `FUN_801D0748` state-`0x50` arm) actually
draws, and from where, was unread - the earlier HUD capture pinned the
command cluster but not the input screen or its Triangle list. A live dome
match in the static recomp (slot-5 savestate + scripted pad), read through
the runtime's `gpu_frame_dump` GP0 ring plus a same-moment full-VRAM dump,
decides it byte-for-byte: the High/Left/Right/Low chips are widget-page
hexagon pieces + baked label strips + diamond ends; the input bar is the
tiled maroon widget bar filling with command pennants at cost-wide pitch;
the AP plate on the right reads the Spirit gauge (the entry budget's only
visible form is the bar); Triangle cycles a 5-row-per-page learned-arts
window (system-UI interior tiles under a `0x40..0x88` gouraud) whose
name/arrows/AP columns are the SCUS arts-name table's own, drawn through
orange sub-palette 15; the green Triangle circle is its own 64x32 gap TIM
at `PROT.DAT 0x7B00`. Behaviour pinned live: per-press `ctx+0x6dc` debit +
`actor+0x1df` append, auto-end on exhaustion (`0x50 -> 0x5a`), the
`0x5a -> 0x6e` Begin|Reselect chain, and Triangle inert at learned-art
constant 0. Ports: `engine-core::muscle_dome`
(`selection_exhausted` / `reset_selection`),
`web-viewer::minigames_muscle` (`arts_input` pieces +
`muscle_arts_list_json`).

### Slot-B overlay cluster (`0900..0969`) per-entry identity

*Status:* resolved for every entry except **0968**, whose identity hunt stays on [`open-rev-eng-threads.md`](open-rev-eng-threads.md#title--boot--overlays)

The slot-B buffer (link base `0x801F69D8`) timeshares the `0900..0969` blobs; static
extraction at the link base is the clean path, each base cross-checked by in-file
self-pointer resolution (`static_overlay::pointer_resolution`, ≥70%). A static shape
census over the whole cluster (per-entry `lui 0x801F/0x8020; addiu` in-file resolution
at the link base, `FUN_80021B04` / `FUN_80050ED4` spawn-call counts, damage-wrapper
`jal` words) corroborates slot-B linkage for every entry except the slot-A 0902; the
CDNAME label is `xxx_dat` (dev placeholder) across the cluster, so labels contribute
nothing here. The full accounting:

- **0900/0901** = the slot-B *default* render pair - `FUN_80025BA0` loads param 5 or 6
  by flag `DAT_8007B6A8` (0900 field scenes, 0901 world-map scenes).
- **0902** = GAME OVER, a **slot-A** row (loader census `FUN_8003EBE4(7)` in the
  mode-18 init; its old slot-B row was the `pointer_resolution` false positive).
- **0903..0913** = the player summon-stager block, spells `0x81..=0x8B`, fully
  capture-pinned per spell id (0907 = Nighto; "Hell's Music" is the attack's display
  name, the dance-song reading is refuted).
- **0914..0923** = the evolved-Seru stager block `0x8C..0x95`, capture-pinned 10/10.
- **0924/0925/0926** = the rare-Seru flute block, capture-pinned: Lippian `0x96`,
  Spikefish `0x97`, and the unused-`0x98` one-sector `jr ra` stub.
- **0927..0934** = Evil Seru Magic Juggernaut + the Sim-Seru quartet + the Ra-Seru
  trio (`0x99..0xA0`), capture-pinned linear.
- **0935..0966** = the **capture-class cast-module band**, per-entry identity a
  **static disc fact**: each capture-class spell record's `+1` sub-id names its module
  (`extraction = 935 + sub_id`), and enumerating the `'c'`-class records out of
  `SCUS_942.54` yields the complete map with zero gaps - every entry in the band is
  some cast's module. Full table:
  [`spell-table.md § capture-class module index`](../formats/spell-table.md#capture-class-module-index-prot-09350966)
  (parser `legaia_asset::spell_names::capture_class_records`; disc-gated test
  `spell_names_real`). It agrees with all six capture-pinned boss stagers, the
  playtest-pinned Delilas/Xain modules, and the damage-wrapper census. Two closures
  that fell out: **0957** = the Death Game / Thunder Storm module (its
  `Dies/Puera/Both/Damage/Recover` head strings are Death Game's roulette outcome
  labels - the "summon-effect descriptor vs debug table" question is closed), and
  **0965** = the Doomsday module (its "shifted sibling of 0967" reading was an
  entry-size over-read artifact: shift `0x5FE8` lies wholly past 0965's real
  `0x2000`-byte extent, and the corrected entries share no content). See also the
  [band's own settled row](#slot-b-capture-module-band-09350966-per-entry-identity).
- **0967** = the battle sparring-tutorial overlay (capture-pinned, s5 needle-sweep);
  battle-stage id `1`.
- **0968** = the **Cort battle's stage overlay**, battle-stage id `2` -
  [details ↓](#prot-0968---the-cort-battle-stage-overlay).
- **0969** = the STR-path table the STR-mode init pages
  (`FUN_8003EC70(0x4A)`; [`boot.md`](../subsystems/boot.md)). An overlay-resident
  callsite loads it too, and it is the **same gate as 0968's**: at `0x801E6D04`
  the battle SM reads `*(u8 *)0x8007BD0C` - the first **formation monster id** -
  compares it to `0xB5`, and pages `0x4A` (`jal 0x8003ec70` at `0x801E6D14`,
  `overlay_battle_action_801e6968`). The guard immediately above it is
  `lhu v0, 0x14C(actor)` on slot 3 of the actor table `0x801C9370`, taking the
  load only when that actor's **HP has reached zero** - so 0969 is Cort's
  form-transition module, paged when a form dies. The earlier reading of that
  `0xB5` as "the Lapis Wave spell id" was an **id-space collision**: spell `0xB5`
  is Lapis Wave, but the byte the branch reads is the formation id, and formation
  `0xB5` is Cort (monster-archive id 181).

### `0x80010390` is the slot-B overlay destination pointer

*Status:* resolved - the one non-confounded lead in the 0968 hunt is a collision

An address-reference sweep for `0x801F69D8` (0968's link base) over all 1234
images produced exactly one literal-word hit outside the overlay band: a
`0x801F69D8` at `SCUS_942.54 +0x390`. Since SCUS is not in the band and the
band's own hits are worthless - slot-B overlays *share* the base, so the VA is
simultaneously live in ~70 sibling images and every `jal`/`j`/branch to it is
that sibling's own code - the SCUS hit read as the one genuine cross-image
reference and the last thing left to follow.

It is a collision. `0x80010390` is a **SCUS-resident global holding the slot-B
overlay load address**, and `0x8001038C` is its slot-A twin. The two loaders
are otherwise identical - `FUN_8003EBE4` reads `*(0x8001038C)` at `0x8003EC24`,
`FUN_8003EC70` reads `*(0x80010390)` at `0x8003ECCC`, both then run
`FUN_8003E8A8(param + 0x381)` and `FUN_8003E800` into that buffer, and they
differ only in which residency tracker they stamp (`gp+0x924` vs `gp+0x934`).
No instruction in `SCUS_942.54` ever *stores* to either word; a sweep for
`lui 0x8001` paired with a memory op at `+0x38C`/`+0x390` finds nine sites and
all nine are `lw`. So the literal is the slot-B base constant itself, shared by
every slot-B entry, and carries zero information about 0968 specifically.

The general lesson is the one the sweep tool already warns about in its own
docstring, arriving from the other direction: **when overlays share a load
base, a reference to that base is a reference to the slot, not to a tenant.**

### PROT 0968 - the Cort battle stage overlay

*Status:* resolved except for a residency capture, which stays on
[`open-rev-eng-threads.md`](open-rev-eng-threads.md#prot-0968-identity---the-one-unidentified-slot-b-cluster-entry)

**Why the callsite hunt kept failing.** The search was for loader param `0x49`
as a *constant*, and no constant produces it. Stage overlays are paged by a
**computed** parameter: sub-states `0x0E`/`0x10` of the battle loader read the
stage-id byte `_DAT_8007B64A` and call `FUN_8003EC70(stage_id + 0x47)`
([`battle.md`](../subsystems/battle.md)). Nothing named `0x49` anywhere,
because nothing ever writes it.

**The selector.** `FUN_80055B6C`, the battle scene initialiser, ends its
formation fix-up with a hardcoded override at `0x80055D2C`:

```
lbu   v1, -0x42f4(v1)   ; v1 = *(u8 *)0x8007BD0C - the first formation monster id
addiu v0, zero, 0xb5
bne   v1, v0, 0x80055d48
addiu v0, zero, 2       ; delay slot
sb    v0, -0x49b6(at)   ; *(u8 *)0x8007B64A = 2 - the battle-stage id
```

Formation id `0xB5` is monster-archive id **181 = Cort**, read straight off
PROT 867 (`asset monster-archive --id 181`). So stage id `2` → param `0x49` →
extraction entry **968**, and the Cort fight is its only gate. The same byte
against the same constant is what pages **0969** mid-battle when a Cort form's
HP reaches zero, which is the corroboration: one boss, two modules, one
formation-id test each.

**Its real extent is 2600 bytes, not 4096.** The entry is 2 sectors, but only
file `0x00..0xA28` is 0968's own content - a 7-entry dispatch table at offset 0
(every target inside that window) and code from `0x1C`. Every `jal`, every `j`
and every LUI+ADDIU materialisation in that window resolves either inside it or
into `SCUS_942.54` / the co-resident slot-A battle overlay; **not one reaches
past `0xA28`.**

The trailing 1496 bytes are byte-identical to PROT 0967 at the *same* file
offsets and cut mid-string at the sector boundary - stale mastering-buffer
content from the neighbouring tutorial overlay, not 0968's. Two independent
proofs it cannot be 0968's own:

- it contains `FUN_801F747C`, a text-box placement routine whose style
  dispatch is `jr *(0x801F6B48 + style*4)`. In 0967 that address is a 10-entry
  jump table sitting at file `0x170`, right after 0967's 91-entry step table;
  in 0968 file `0x170` is live code. The routine cannot run under 0968's
  layout;
- the same window materialises `0x801F7C80`, a string that exists only in
  0967 and lies past 0968's end.

This is why every prior structural read of the entry disagreed with itself:
"pointer-table head, 10 of 11 self-pointers, 2+8 spawn calls" was measured over
all 4096 bytes, mixing two modules. Measured over `0x00..0xA28` the picture is
clean and its first instruction reads the battle-context pointer
`_DAT_8007BD24` and writes `ctx[+0x6D6] = 0x100`.

**What the module does.** A 7-state scripted battle set-piece. Its calls into
`SCUS_942.54` are `FUN_80050ED4` (summon / effect-actor pool allocator) ×8,
`FUN_80021B04` (actor spawn) ×2, `FUN_80024E80` (screen-fade spawn) ×2,
`FUN_8003541C` (text actor), `FUN_8004FCC8` (cue / streamed-voice dispatch),
`FUN_80058490` (`MoveImage` VRAM blit), plus `FUN_80035F04` and `FUN_80050E74`;
it also calls `0x801D829C` in the co-resident slot-A battle overlay four times.
Effect spawns, fades, a VRAM blit, a line of text and a voice cue is the shape
of a boss's scripted sequence, and it is the same external-call family as the
tutorial overlay 0967 minus the tutorial's own prompt helpers.

### PROT 0977 / 0978 extraction + the dump re-key

*Status:* resolved - both entries are in the static overlay map, and every
`overlay_0977_*` / `overlay_0978_*` dump now resolves

The static map ([`static-overlays.toml`](../../crates/asset/data/static-overlays.toml))
carries **0977** (`arena_init`, the Muscle Dome door/init slot-A overlay at
`0x801CE818`, anchor `FUN_801D0F60`) and **0978** (`field_back_read`, slot B
`0x801F69D8`, pinned by the SCUS `FUN_80025358` state-2 call into
`FUN_801F6B24`); `asset overlay verify` reproduces both fingerprints from the
disc. Re-running `check-dump-base-integrity.py` with those images in the index
classifies all 22 dumps in the two families - none is `NOT_FOUND`:

| Dumps | Verdict | Bytes live in |
|---|---|---|
| `801d050c` `801d08ec` `801d1288` `801d1308` `801d14b0`, `slotA_801d0f60` | MATCH | 0977 at the printed VA |
| `other_game_801f6b24` | MATCH | 0978 at the printed VA |
| `0977 801c085c` `801c0f48` `801c2748` | SHIFTED `+0xE818` | 0977 own code (`801C085C→801CF074`, `801C0F48→801CF760`, `801C2748→801D0F60` - the War God Icon settlement) |
| `0977 801c614c` `801c6268` `801c6804` `801c6cf8` | SHIFTED `+0xA018` | 0979 (`801C614C→801D0164`, `801C6268→801D0280`, `801C6804→801D081C`) |
| `0978 801c2b58` `801c3004` `801c39b8` | SHIFTED `+0xD818` | 0979 (`→801D0370` / `801D081C` / `801D11D0`) |
| `0978 801c5c58` `801c7b40` `801c82dc` `801c8b04` `801c8d0c` | SHIFTED `+0x9818` | dance 0980 (`801C5C58→801CF470` - the documented beat-clock SM) |

The deltas decode as **one wrong base each, seen through the pre-correction
over-read footprints imported at `0x801C0000`**. 0977's footprint holds its own
`0x3800` bytes, then 0978 (`0x1000`), then 0979 - so own-content prints re-key
at `+0xE818` (`0x801CE818 − 0x801C0000`) and 0979-stratum prints at
`0xE818 − 0x4800 = +0xA018`. 0978's footprint holds 0979 from `+0x1000`
(`+0xD818`) and the dance overlay from `+0x5000` (`+0x9818`). The two-hit
`801c614c` signature (a duplicated 10-instruction run inside 0979) is
disambiguated by the batch-constant delta: its program siblings resolve
single-hit at `+0xA018`. The old thread's two hints both dissolve: the
"`dance_0980` at `+0x9818`" batch is exactly the 0978 footprint's dance
stratum, and the "`baka_fighter_0976` at `+0x5710`" hit is a cross-overlay
duplicate of a sequence that MATCHes 0977 at its printed VA. The five printed
VAs the thread had written off as unrecoverable (`801c2b58`, `801c3004`,
`801c39b8`, `801c614c`, `801c6804`) all now have owners - four distinct
routines of the field-battle-intro overlay 0979 (two of the dumps are the same
routine `FUN_801D081C` reached through two different wrong bases, which
cross-checks the decode).

### Slot-B capture-module band `0935..0966` per-entry identity

*Status:* resolved - the per-entry map is static spell-table data, readable
out of `SCUS_942.54`

A capture-class spell record (class byte `'c'` at stats `+0`) pages its cast
module through the slot-B loader as `FUN_8003EC70(record[+1] + 0x28)`, and the
loader resolves extraction `param + 0x37F` - so **extraction entry
`935 + record[+1]`**. Enumerating every `'c'`-class record in the SCUS spell
table therefore yields the complete per-entry identity map of the band, with
no capture required; the sub-id space covers `0935..=0966` exactly (no orphan
entries). Full table:
[`spell-table.md § capture-class module index`](../formats/spell-table.md#capture-class-module-index-prot-09350966).
Parser `legaia_asset::spell_names::capture_class_records` /
`capture_module_prot`; the disc-gated `spell_names_real` test asserts the
band coverage and every independently pinned leg (the six capture-pinned boss
stagers 938/940/944/961/962/966, the playtest-pinned 952/953/958/959/960).
A static shape census of the extracted entries corroborates: every band entry
resolves its `lui 0x801F/0x8020; addiu` self-pointers in-file at the slot-B
link base, spawns through `FUN_80021B04` / the `FUN_80050ED4` pool wrapper,
and carries damage-wrapper `jal`s exactly where the
[battle-formulas wrapper census](../subsystems/battle-formulas.md) put them.
Two identities this settled: **0957** = the Death Game / Thunder Storm module
(head strings `Dies/Puera/Both/Damage/Recover` = Death Game's roulette
outcome labels), **0965** = the Doomsday module (the "shifted sibling of the
battle-tutorial overlay 0967" reading was an entry-size over-read artifact -
the claimed shift `0x5FE8` lies wholly past 0965's real `0x2000`-byte extent,
and the corrected entries share no content).

### New-game world-state seed store widths

*Status:* resolved - the port was already right; the evidence under it was not

The widths in `legaia_asset::new_game::new_game_seed_words` rested on Ghidra's
`DAT_` / `_DAT_` naming convention, which is a heuristic over symbol size and
carries no width measurement. The dump behind them reported no instructions and
carried only decompiled C - one of the catalogued artifact shapes in
[`tooling/ghidra.md`](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims).

Re-decoding `FUN_80034A6C` out of `SCUS_942.54` confirms every entry. The routine
holds the save-context base in `$s0` (`lui $s0, 0x8008` / `addiu $s0, $s0, 0x4140`
= `0x80084140`) and issues each seed write as an `sb` or `sw` at `$s0 + off`; the
decoded `(offset, width, value)` set matches the port exactly, so nothing changed
in the table. The full listing is in
[`formats/new-game-table.md`](../formats/new-game-table.md#world-state-seed-code-literals-not-a-table).

Two corrections to the C's rendering, neither affecting the port:

- The absolute globals `DAT_80085958` / `DAT_80085959` are really
  `sb $v0, 0x1818($s0)` / `sb $v0, 0x1819($s0)` - the starting-item pair at
  `INVENTORY_SC_OFFSET`, `SC`-relative and issued *after* the template expander,
  so they were never part of the pre-expander set.
- The story-flag clear is a downward walk from `$s0 + 0x1FF` over
  `sb $zero, 0x1618($v1)`, covering `SC + 0x1618..0x1817` - `0x200` bytes, which
  is what the port's `STORY_FLAGS_LEN` already said.

The reading no longer rests on a dump at all: the disc-gated
`new_game_seed_disc::world_state_seed_matches_the_routines_stores` re-derives the
whole table from the instruction encodings in the user's own executable on every
run, and fails on a wrong offset, value or width.

### `_DAT_8007B8C2` polarity, and its writer

*Status:* resolved - **`!= 0` is retail, `== 0` is dev**, the reverse of what the
docs long carried

Every read is an `lh` of the halfword at `0x8007B8C2` - 43 sites in
`SCUS_942.54`: 40 in the absolute `lui 0x8008` / `lh -0x473e` form, plus **three
gp-relative** `lh v0,0x5aa(gp)` reads at `0x80015FD4` / `0x80016038` /
`0x8001631C` that an absolute-only sweep misses exactly as it missed the store
(the dump corpus including overlays carries 57 sites in total). The two arms split
identically: the `!= 0` arm resolves assets by **PROT-TOC index** (`FUN_8003E8A8` +
`FUN_8003E800`, or `FUN_8003EB98`), while the `== 0` arm opens a path through
`FUN_800608F0` - whose entire body is `break 0x103`, a PsyQ dev-station host trap,
on `h:\` paths that do not exist on a retail disc. Not one site dissents; the
gp-relative read at `0x80016038` is its own witness (`bnez v0` at `0x80016040`
skips the `jal FUN_8003E6BC` dev-path call when the flag is nonzero).

**The flag is not writer-less.** `main()` (`FUN_80015E90`) stores it once at cold
boot: `0x80015F08 sh v0,0x5aa(gp)` with `gp = 0x8007B318`, taking the return of
`FUN_8003F084` - a two-instruction leaf (`jr ra` / `addiu v0,zero,0x1`) returning
the constant `1`, sole caller `0x80015F00`. It is a stubbed-out build-mode
predicate; the dev build presumably returned `0`.

**Why the inversion survived so long** is worth recording, because the failure was
structural rather than a misreading. The store is **gp-relative**, invisible to a
sweep searching only the absolute `lui 0x8008` / `-0x473e` form - as are the
three gp-relative reads above, which the same sweep undercounts to 40. That false
negative produced "zero writers", which produced the inference "BSS zero-init
therefore leaves it `0`, therefore `0` is retail" - and that inference was itself
unfounded twice over, since the PS-X EXE header carries `b_addr = 0, b_size = 0`
and the BIOS clears no BSS for this executable at all. Compounding it, the answer
was **already in the repo**: `boot.md` documented the boot-scene override reading
"the dev flag halfword at `gp+0x5AA` (from `FUN_8003F084`)" some 600 lines above
the section calling the same flag writer-less. Connecting the two required knowing
`gp = 0x8007B318`.

**Capture side:** the halfword reads `1` in **60/60** Mednafen save states - field,
battle, world-map, stock and randomized discs alike.

**Falsified en route:** `FUN_8003E6BC` does no CDNAME name resolution. Its body is
`strcpy` → `break 0x103` → fseek/fread/fclose. The claim that it "resolves
`h:\main\bg\domepack\…` into the appropriate PROT entry through the CDNAME map"
came from reading a Ghidra-supplied `path_opener` label as fact, and it was what
made the backwards polarity look self-consistent - it implied the `== 0` arm was
something retail could service.

See [`ghidra.md`](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims)
for the absolute-only-sweep artifact this produced.

### Key-item area consumers

*Status:* resolved on the narrow negative; the reader enumeration is incomplete

The range is inventory slots `>= 72` of `&DAT_80085958`. Readers mask the slot
`& 0x3ff` and use the id byte as an index into 256-entry, 12-byte-stride item
tables: an `lbu` yields `0..255`, so the maximum offset is 3060 against a 3072-byte
table - bounded by construction, not by a guard.

**The negative holds.** No consumer treats a key-item byte as an unguarded index.
Verified by hunting the one shape that would break it - a **signed** `lb` feeding
an index. Exactly two exist (`0x8004250C`, `0x80042510`, in `FUN_800423E0`); both
are a compaction move immediately re-stored via `sb`, with no index use.

**Two corrections to the surrounding prose.** First, "add/find/consume helpers
bound their scan by the live item count" is true of the *scans* and false of the
id store at `0x800422BC`: when the free-slot loop at `0x80042270` finds no empty
slot, the index exits equal to the window limit and `sb` writes one slot past the
scanned window. The `slt` guard at `0x800422C0` is downstream and gates only the
quantity byte. Second, the reader list is incomplete - an indexed sweep over SCUS
plus all 1,233 PROT entries (156 hits across 11 files) finds an undocumented band
at `0x8004220C..0x800430A0` plus **51 sites in menu overlay 0899**, none named here.

**Bearing on the re-opened ACE/OOB thread:** this locates the mechanism precisely
without strengthening it. The overflow index derives from the window-limit
*global*, not from any attacker-controlled item byte, so it is a bounded one-slot
write rather than an index-OOB amplifier. The row's conclusion - the range
amplifies to game-state corruption, not a native chain step - survives.

The `lb $reg,0x5aXX($zero)` overlay "hits" were mis-decoded data tables: 117
occurrences across 74 files, and SCUS's 7 sit at `0x80010AE4..0x80010AFC` as a
perfect stride-`0x10` progression - a pointer table, not code.

### `title.pak` PROT entry

*Status:* resolved

There is no single `title.pak` bundle entry - the dev-tree `title.pak` content is split across two PROT entries, both confirmed by the init.pak fingerprint method now that a title-phase RAM snapshot exists (`title_screen_new_game` save state): the **title wordmark TIM** is **PROT 888/890** (`sound_data2`; already parsed by `legaia_asset::title_pak`, the big-logo RAM TIM at `0x80170DF8` fingerprint-matches it),

and the **options/config-menu bundle** is **PROT 899** (`xxx_dat`) - its indexed payload opens with the config-menu string pool ("Display Off / Gradual / Immediate / Field HP Display / Encounters / Vibration / Dual Shock / Voices / Battle Camera / Monaural / Stereo …") followed by the small config TIMs (the four RAM TIMs at `0x8010FEF0..0x80110130`, CLUTs byte-matched at 899 offsets `0x169DC` / `0x1F91C`+), with the title-overlay *code* in the trailing unindexed gap after entry 899 (see [[title-overlay-source-pinned]]). Same CDNAME-mislabel pattern as `0895_bat_back_dat` = init.pak.

### Title screen mode-table PROT

*Status:* resolved (no such entry)

**The premise is wrong**: there is no title-screen entry in the 28-entry mode table at `0x8007078C`. Per [`subsystems/boot.md`](../subsystems/boot.md#title-screen-is-not-in-the-mode-table) the title overlay is loaded by a **pre-mode-dispatch boot routine** ahead of the mode table being consulted at all - its tick `FUN_801DD35C` lives in the unindexed 60-sector PROT.DAT gap between TOC entries 899 and 900

**Open sub-question - which overlay owns `FUN_801DD35C`.** That function's
disassembly is identical across the `overlay_menu`, `overlay_title`,
`overlay_save_ui_*` and `overlay_shop_save` dumps - the same
one-resident-function-under-many-scenario-labels shape that settled the `0x2F`
residency thread - and `crates/engine-vm` ports it twice under incompatible
descriptions (`menu.rs` as the menu overlay's dispatcher, `title_overlay.rs` as
the title tick). The residency evidence points at one shared slot-A overlay
generation rather than separate copies, but that is an inference from dumps, not
a capture. Closing it needs the same check the `0x2F` thread used: read the fixed
VA out of each candidate overlay's disc image. See
[vm-inventory.md](../subsystems/vm-inventory.md#one-function-two-ports) ([`legaia_asset::title_pak`](https://github.com/altimit-mii/legend-of-legaia-re/tree/main/crates/asset/src/title_pak.rs) reads the wordmark TIM out of PROT 888/890; PROT 899 carries the options-menu config bundle). NEW GAME is how control crosses from the title overlay into the mode table at mode 2. Row kept so the "title entry is unresolved" framing isn't re-opened.

### XP-table source + reader

*Status:* resolved + ported

The retail XP curve is the static-SCUS per-level delta table `DAT_80076AF4` (u16), read by
the level-up applier `FUN_801E9504` (overlay-resident, called from the reward resolver
`FUN_8004E568` at `0x8004F34C`): the running sum to the current level is scaled
`(sum × 9999999) / 0x140FE` for `level < 0x11` (else `sum × 0x79`) and compared `≤ record
cumulative XP` in a multi-level `do…while` loop.

The earlier `0x8007123C` / `0x80070A3C` framing was doubly wrong (an off-by-`0x800`
file/virtual confusion, then a sin-LUT slice); the sin-LUT slice is additionally
**refuted by retail display** - a New Game Status capture shows "Next Level 121" (the
real L2 threshold), not 50. The delta table is the closed form `delta(n) = ⌊n²/4⌋ + 1`,
so the curve is derivable arithmetic: `legaia_save::RETAIL_XP_CUMULATIVE` /
`retail_xp_table()` ship the derived base curve (`121, 365, 730, …, 9_646_483`), the
boot-time disc parse (`legaia_asset::level_up_tables::xp_thresholds_from_scus` →
`BootSession`) cross-validates byte-identically, and library-wide record sampling
(`+0x0` XP / `+0x4` next threshold / `+0x130` level at `0x80084708 + slot×0x414`)
matches through L37 including the Noa/Gala ± corrections (New Game 121/102/140; L99
carries 0). The Status menu (`FUN_801D33D8`) draws `+0x0`/`+0x4` verbatim.

See [`subsystems/level-up.md`](../subsystems/level-up.md#xp-table).

### Overlay identity from the disc (static extraction)

*Status:* resolved (pipeline landed)

PSX overlays are clean copies of a fixed-VA-linked blob (FlushCache + jump, no per-load relocation), so each runtime overlay can be extracted **statically** from its `PROT.DAT` entry and disassembled at its load base - identity attached from the source entry, not a guessed label. This is the structural fix for the VA-aliasing identity problem (`0x801DD864` = battle-action in one overlay, muscle-dome in another). Proved: the battle overlay (PROT 0898 @ `0x801CE818`) is byte-identical to its resident RAM image over the full `.text`+`.rodata` (`0x28800` of `0x29800` bytes; only the trailing `.bss` diverges). The load base is recovered statically from the overlay's own internal `jal` call graph (`static_overlay::recover_base`); for entries with too sparse a call graph,
the base is cross-checked instead by a documented function landing on a prologue (`anchor_va`,
slot A) or by the fraction of internal absolute self-pointers that resolve in-file
(`static_overlay::pointer_resolution`, slot B). The committed map now spans the whole slot-A
scene family (field/battle/menu + the **cutscene/STR** overlay 0970 + the **minigame** overlays
0972/0973/0976/0980) and the pinned slot-B entries (summon render 0900, the spell-`0x83` summon
stager 0905 - Gimard `0x81` arithmetics to 0903 under the corrected loader index math - GAME
OVER 0902, the Nighto stager 0907 "Hell's Music" + the attack-titled stager-shaped
0924/0927, summon-effect data 0957). Reconnaissance
tooling: `asset overlay scan` (range sweep: base + leading dev string) and `asset overlay
find-sig` (locate a function-head signature → infer the host overlay). Pipeline:
`legaia_asset::static_overlay` + `asset overlay …`;
committed map `crates/asset/data/static-overlays.toml`; see [`tooling/static-overlay-pipeline.md`](../tooling/static-overlay-pipeline.md). It **complements** the dynamic captures - it does not address runtime values (those still need live probes).

### PROT 0896 (`bat_back_dat`) identity

*Status:* **resolved** - the head is the vestigial Japanese-build field-menu /
config / status overlay (the debug-string sibling of the English retail menu
overlay PROT 0899); the "mode-24 OTHER overlay @ `0x801C5818`" hypothesis is
**refuted** and the recovered base was an **alias artifact**.

**Identity (host-capstone decode of the head off the disc entry - extraction
index `896`, verified by locating the `"FWIN ERR"` bytes directly, not by an
index-shift rule).** The head is a self-contained menu/config/status overlay:
a Shift-JIS label pool (config toggles, the Item/Summon/Equip/Status/Config/Save
top menu, the ATK/UDF/LDF/SPD/INT/AGL + EXP status labels), the `"FWIN ERR %d"`
window-manager debug printf at file offset `0x3D4` (`FWIN` = Field WINdow), and
real MIPS at link base `~0x801D0000` - a status/name-draw routine indexing the
`0x414`-byte character records, with head function-pointer tables holding ~61
addresses across `0x801D81C0..0x801DC700` (the window/screen renderers). This is
the same VA family as the live retail menu overlay PROT 0899 (`0x801D33D8`
status renderer, `0x801DC6B4` save SM). **0899 carries the English versions of
the identical label set and zero `FWIN`**, so 0896 is the Japanese,
debug-string-bearing sibling of the same subsystem; the USA localisation dropped
the `FWIN` debug string when it shipped 0899. A distinctive-signature scan across
**140 catalogued RAM states** (37 PCSX `.sstate` + 98 gzipped mednafen states,
all phases) finds 0896 resident in **none**, while the English "Battle Voices"
(live 0899 config) is resident in 10 menu-phase states (the scan's positive
control) - so the `scenarios.toml` `save_select_idle` "overlay 0896 paged in"
note is a mislabel using the extraction-index name; the resident menu code is
the English 0899. 0896 is a vestigial JP-build overlay carried on the USA disc,
never loaded by the USA build (consistent with "no static loader reaches it").

Superseding findings (kept so the reframing isn't re-walked):

1. **The mode-24 entry does not load it.** A live capture of the Baka Fighter
   entry (probe
   [`autorun_minigame_overlay_capture.lua`](../../scripts/pcsx-redux/autorun_minigame_overlay_capture.lua),
   triggered on the `0x8007B83C = 0x18` write; sub-id `0x8007BA34 = 4`,
   live-confirming the `0x3E` operand−100 model) dumped the overlay window at
   +0/+10/+30 vsyncs - spanning the SCUS-resident OTHER INIT handler's
   completion (its `"other init end"` debug print) and the per-minigame
   overlay streaming into slot A. 0896's bytes appear at no offset in any
   dump, nor anywhere in main RAM in the pre-transition save, nor in any of
   the parked library states (45+ checked, all phases).
2. **The `0x801C5818` base (60 jal votes) is an over-read artifact.** 0896's
   file carries the FIELD overlay's bytes from `+0x9000` (consecutive
   entries' footprints over-read), and the field overlay's self-consistent
   code at `0x801CE818` fixes the whole-file recovery to
   `0x801CE818 − 0x9000` by construction. Restricted to the head's own code,
   the jal recovery yields **no landslide** - 0896's true link base is
   unrecovered.
3. **The unique head (~`0x9000` bytes) is a self-contained blob of mixed
   code + data**: real MIPS density (~54 prologues), an `"FWIN ERR %d"`
   printf (the string lives in the blob itself; no `fwin`/`bat_back`
   reference exists in `SCUS_942.54`), and a large byte-map-like data block
   (rows of gradually shifting byte values). The CDNAME label
   `bat_back_dat` (battle background data?) may yet be honest - but no
   captured battle state holds the data either. (Under the raw-TOC index
   shift the CDNAME `#define` covering 0896's *extraction* slot may belong
   to a neighbouring entry anyway - see the index-spaces thread.)
4. **No static loader call can reach it.** A full-image scan of
   `SCUS_942.54` for `jal FUN_8003EBE4`/`FUN_8003EC70` with the `a0` setup
   decoded finds 16 sites; every constant param maps to extraction 897..902,
   969..981, or the spell-/stage-driven bands (`id - 0x79` summon stagers,
   `+0x28` special-attack, `+0x47` battle stage). Extraction 0896 would need
   `param == 1`, which no site produces (the three computed-param sites have
   `+0x74`/`+0x47`/`5-or-6` bases that cannot reach 1). A companion scan for
   the raw indices `0x381`/`0x382` as immediates finds only the two loaders'
   own internal `param + 0x381` adds - no direct `FUN_8003E8A8`/file-open
   path either. The `+0x47` computed site is since fully decoded and can only
   reach extraction 967/968 - see
   [`battle.md` § Stage-overlay dispatch](../subsystems/battle.md#stage-overlay-dispatch-the-0x47-loader-band) -
   so it corroborates rather than weakens the "0896 is unreachable" reading.

What would close it: a consumer - any retail moment where the head bytes are
resident (offline check:
[`overlay_residency.py`](../../scripts/pcsx-redux/overlay_residency.py)
against new captures), or an overlay-resident loader call with a computed
param reaching 1 (the static SCUS census above rules out the constant-param
sites).


### SCUS recomp gap - render/GTE + boot/init clusters

*Status:* resolved (behavior-read + dumped); the general-game band remains the
open remainder

The psxrecomp static recompilation's function inventory surfaced a set of SCUS
entries with no dump / doc / port-tag on our side, clustered by VA band. The
render/GTE and boot/init clusters are now fully attributed, and the attribution
is mostly *negative* - the VA-band labels did not survive a behavior read.
Recorded so the same entries aren't re-flagged:

- **The "COP2 render gap" band (`0x43000..0x47000`) is not render code.** The
  small entries there are recomp block-splits of **inventory/equip predicates**:
  `0x800430D4..0x80043134` = interior of `FUN_800430AC` (party-wide accessory
  unequip-by-id), `0x80043238..0x8004325C` = interior of `FUN_800431FC`
  (knows-spell), `0x80043290/0x800432A8` = interior of `FUN_80043264`
  (accessory-equipped). `0x80043580` / `0x8004361C` are interior blocks of the
  already-documented cluster-A renderer `FUN_80043390` (far-colour / ZSF setup +
  its custom-convention epilogue). `0x80046498` = `FUN_80046494` (+4 entry skew,
  the locomotion collision resolver - the "render→overlay draw seam" reading was
  already falsified) and `0x8004697C` = `FUN_80046978` (+4, palette fade).
- **The 14 `gte_execute` entries are statically-linked libgte per-op wrappers**
  (`MulMatrix0`, `Square12/0`, `AverageZ3/4`, `OuterProduct12/0`, `DCPL`/`DPCT`/
  `INTPL`, the `RotTransPers3`-shaped RTPT projector) with zero static callers
  and zero runtime hot-profile hits - link residue; the render paths issue COP2
  inline. Table: [`functions.md` § libgte primitives](functions/runtime-libs.md#libgte-primitives);
  all ignore-listed.
- **The boot/init cluster is dominated by aliases of documented functions.**
  `0x80016448`→`FUN_80016444`, `0x80016B74`→`FUN_80016B6C`,
  `0x800173C0`→`FUN_800173BC` (dev profiler HUD, ignored),
  `0x80016998`→interior of `FUN_8001698C`, `0x80017914`→`FUN_80017910`,
  `0x80017A04`-family→interior of `FUN_800179C0`, `0x8001A078`→interior of the
  dev printf `FUN_8001A068`, `0x8001A814`→interior of `FUN_8001A78C` (RGB→HSV),
  `0x8001AA14..0x8001AA60` = the six hue-sextant jump-table arms inside
  `FUN_8001A8DC` (HSV→RGB), `0x80019BC0..0x80019D48` = interior of the atan2
  bearing resolver `FUN_80019B28`, `0x8005B2A4`/`0x8005B340` = interior of
  PushMatrix `0x8005B268` / PopMatrix `0x8005B308`.
- **The genuinely-new identifications:** `FUN_80015E90` = **`main()`**
  ([`boot.md` § The main loop](../subsystems/boot.md#the-main-loop-fun_80015e90));
  the dev draw cluster `FUN_8001CE34` (3-D line) / `FUN_8001CAD8` (wireframe
  box, the sole source of `8001CE34`'s in-degree-12 - the "most-called boot
  utility" reading is falsified) / `FUN_8001CCFC` (2-D line) / `FUN_8001C7A0`
  (4x8 digit printer); `FUN_800430AC` (whose Ghidra auto-analysis body was
  degenerate until force-created); and `FUN_8004CE2C`, the largest undumped SCUS
  function - the per-frame battle actor maintenance pass
  ([`battle.md` § Per-frame actor maintenance](../subsystems/battle.md#per-frame-actor-maintenance-fun_8004ce2c)),
  **not** a mode dispatcher.
- **Still open from the same inventory:** the general-game band (never
  per-address catalogued), headed by `0x8002A9F8` (2.2 KB table-driven logic,
  no static caller), `0x8004DC68`, `0x80036D80`, `0x80025DA4`. Next step:
  behavior-read each against its `0x8007xxxx`/`gp` globals the way this
  thread's entries were closed. Three former members are now closed:
  `0x80056208` is **not** a libgpu-band bridge - it is a battle side-band tick
  (three submodes off `DAT_8007B64A`) that merely sits at a PsyQ-adjacent
  address, ported to `engine-render`; and `0x8002149C` / `0x80059E10` both now
  carry full disassembly, so their grade is `disassembly` rather than the
  weaker evidence this line assumed. The PsyQ sound-driver
  cluster is tracked separately under Audio.

### Full-window item-add OOB reachability

*Status:* resolved - the write primitive is real; normal play cannot reach it.
Grade: `disassembly` (full window) + `inference` (the half-window sub-case).

The OOB *write* is confirmed from `FUN_800421D4`'s disassembly: the id store
`sb t0,0x1818(a0)` at `0x800422BC` is unconditional and precedes the `slt`/`beq`
guard (`0x800422C8`/`0x800422CC`) that gates only the count store at
`0x80042300`. When the free-slot scan (`0x80042254..0x8004229C`) exhausts the
window it leaves the index `== end`, so the id lands one slot past the window
(`base + end*2` = `0x80085A58` for `end=128`, `0x80085B58` for `end=256`). The
window is installed only by `FUN_8004313C`, which installs `[0,256)`, `[0,128)`
or `[128,256)` - never the 72-slot span an earlier note recorded.

**Reachability verdict: unreachable through the retail add call sites in normal
play.** No add caller pre-checks room - each loads an item id and `jal`s the
helper directly (shop buy-confirm `0x801C38A4` loads `a0 = rec+8`; battle-loot
`0x8004F380`/`0x8004F608`; plus the menu/save/fishing/world-map/minigame/
equip-refund helpers) - so the helper's own scan is the only backstop, and it
holds:

- **Full window `[0,256)`** (installed for any party of `>= 2`, the normal
  mid/late state; live-verified at 3 members). The merge pass keys on the id
  byte (`andi a3,t0,0xff` @ `0x800421F4`), so each non-zero id occupies at most
  one slot and `0` is the empty sentinel; under the add/consume/normalize
  accessors at most **255** distinct ids occupy the 256 slots, so a hole always
  remains and the scan exits in-window. The OOB store is mathematically
  unreachable here.
- **Half windows `[0,128)` / `[128,256)`** (installed only for a single
  playable member with story flag 20 clear; a transient early/solo phase). 128
  `<= 255` so the id ceiling alone does not forbid a fill, but the real disc item
  population is far below 128, so the scan still terminates on a hole.

A non-add path (debug menu, cheat engine, or a crafted save seeding duplicate
live ids) could still force the exit with an attacker-influenced byte - outside
"normal play", which is what the thread asked. Port + machine-checkable verdict:
`legaia_save::retail_inventory` (`ItemWindow::oob_reachability`,
`MAX_DISTINCT_ITEM_IDS`, `OobReachability`). Provenance:
`ghidra/scripts/funcs/{800421d4,8004313c,8004e568,8003ce64}.txt`,
`overlay_0971_801c36b0.txt`.

### Phantom-VA sweep of the PROT 0897 imports

*Status:* resolved - the three residues the delta arithmetic left open are
byte-decided; standing results in
[`overlay-va-aliases.md § the byte-level sweep`](overlay-va-aliases.md#the-byte-level-sweep)

The two measured deltas (`0xE818` base error, `0x25000` over-read) re-keyed
most of the 0897-import prints but could not decide three residues: the
`0x801E5000` boundary band, the "doubly-aliased" `0x8020D05C`, and whether
PROT 0896's imports obey a law of their own. All three yield to a word-level
comparison
([`resolve-phantom-va.py`](../../scripts/ghidra-analysis/resolve-phantom-va.py)):
compare the dump against each candidate (image, base) reading at the printed
VA, re-encoding Ghidra's data-as-instruction renderings (`nop`,
`<load> rt,imm(zero)`) into exact 32-bit words so that *data* regions - which
defeat any stream match - decide at full strength.

- **Boundary band**: every dump printed in `0x801E4000..0x801E6000` resolves
  to exactly one reading, and the strata switch exactly at `0x801E5000`. The
  two open addresses are 0897 own-content **data** (pointer tables at true
  VAs `0x801F3308` / `0x801F3450`, 14/14 and 13/13 words; the rival 0898
  reading scores 0). `0x801E5134` is printed by two programs with two
  different owners - one print correct, one a phantom of 0898 `0x801CE94C`.
- **`0x8020D05C`**: 0898 rodata at true VA `0x801F6874` (a
  `(pointer, count)` table into 0898's `0x801CF9xx` band). Its words include
  values with no R3000 decoding, matching the dump's zero-instruction
  `halt_baddata`; every rival reading maps the VA to code that would have
  decoded. Not a function under any reading.
- **PROT 0896**: the `overlay_0896_*` prefix covers **two** imports of the
  over-read footprint - untagged at `0x801C0000` (three strata: own content
  `< 0x9000`; field `+ 0x5818`; battle `- 0x1F7E8`) and tagged
  `base=0x801C5818` (the phantom jal-recovered base; prints are 0896's own
  bytes at `printed - 0x801C5818`). Every addressed dump in the family
  resolves under exactly one program, zero exceptions; the header-tag
  partition and the byte partition agree dump-for-dump. Same function
  printed by both programs pins the pair (file `+0x5C90` at `0x801C5C90` /
  `0x801CB4A8`; file `+0xD1C` at `0x801C0D1C` / `0x801C6534`).
- **`0x801FD4C0`** (bonus residue): its dump starts at printed `0x801FD150`
  and is the battle image's `FUN_801E6968`; the printed VA is that body's
  interior at 0898 VA `0x801E6CD8`, not the field image's `FUN_801E6B34`.

Grade `disassembly`: every verdict is a word- or token-exact comparison
against the corrected-extent extracted images, with each rival reading
excluded by the same comparison rather than by arithmetic.

## Rendering / camera

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| Does any retail shot author a non-zero camera roll? | resolved (yes) | `capture` + `disassembly` | Eight scenes stage a reachable, executing op-`0x45` slot-2 roll, from `10` units (0.9 deg) to `-660` (-58 deg). [details ↓](#does-any-retail-shot-author-a-non-zero-camera-roll) |

### Does any retail shot author a non-zero camera roll?

*Status:* resolved - **yes**.

Slot `2` of the op-`0x45` CONFIGURE mask is the roll angle `_DAT_8007B794`,
the argument `FUN_8001CF50` hands to `RotMatrixZ` (`0x8004638C`) as the third
factor of `Rx * Ry * Rz`. The port used to compose pitch and yaw and drop it,
on the stated assumption that retail shots rarely roll.

**Eight scenes roll.** Every one of these beats carries the full nine-slot
mask - pitch, yaw, roll, the eye trio, focus X and Z, `H`, and no focus Y,
matching the `opdeene` reading on [`cutscene.md`](../subsystems/cutscene.md) -
every operand is an in-range 12-bit angle, and the beats of one shot repeat
the same tilt, which is what an authored Dutch angle looks like.

| scene | what it is | PROT entry | record | roll (12-bit) | degrees |
|---|---|---|---|---|---|
| `edstati3` | Ending (station3) | 826 | P2[0] | `10`, `20` | 0.9, 1.8 |
| `station3` | Karisto Station (late) | 616 | P2[0] | `30` | 2.6 |
| `map03` | World map (Karisto) | 392 | P2[10] | `60` | 5.3 |
| `nilboa` | Nivora Ravine | 638 | P2[33] | `60` | 5.3 |
| `taiku` | Muscle Dome | 373 | P2[27] | `-120` | -10.5 |
| `korout` | Field (korout) | 534 | P2[3] | `240` | 21.1 |
| `juui1` | Juggernaut interior 1 | 588 | P2[0], P2[3], P2[4] | `-400` | -35.2 |
| `juui2` | Juggernaut interior 2 | 597 | P2[0] | `-660` | -58.0 |

The two biggest tilts sit inside the Juggernaut, and the smallest opens an
ending cutscene - which is where a canted camera is exactly what an author
would reach for.

Roll stays a **minority** term: of the 371 CONFIGUREs a control-flow walk
reaches, 123 set the slot at all and 15 of those write a non-zero value -
roughly a third, then about one in eight. That is why "rarely" survived as
long as it did. Rare is not never.

**Why it took execution.** A field-VM record's tail is not linearly decodable,
so a *decode* of the corpus has to pick a resynchronisation policy, and the
policy is what it ends up measuring. Three instruments disagreed:

| Instrument | CONFIGUREs reached | Non-zero rolls | What it was really measuring |
|---|---|---|---|
| Strict linear (stop at first decode error) | 21 | 0 | its own blindness - it reaches none of the eight |
| Resuming linear (advance one byte on error) | 2182 | 637 roll operands, 7 of them outside the 12-bit angle space | its resynchronisation into data |
| Raw byte scan (decode at every offset) | 4257 | a "2 %" ratio | its own post-hoc credibility filter |

The resuming sweep's own shortlist of "coherent" candidates - `deroa`,
`chitei2`, `station3`, `town0b`, `retona`, `nilboa`, `edstati3` - scores three
hits out of seven and misses five of the eight real ones. That is the sweep's
signal-to-noise stated as a number: half its picks are data, and it cannot see
most of what is there, because the authored rolls live in **partition 2**
(cutscene-timeline / walk-on beat records) while the linear census walks
partition 1.

The decider is control flow. Stepping the ported field VM
(`legaia_engine_vm::field::step`) from each record's real entry PC, under a
probe host that answers every branch predicate both ways, gives the set of PCs
control flow can arrive at; executing the same records in a real `World`
gives the subset that runs. Both find the same eight scenes, and **neither
reaches a roll operand outside the angle space** - which is what identifies
the resuming sweep's impossible operands (`26708` is not an angle) as data.
Oracle: `crates/engine-core/tests/thread_camera_roll_execution.rs`; the linear
modes are kept as a negative record in
`crates/asset/tests/thread_camera_roll_census.rs`.

**Two corrections fall out.** The parenthetical "zeroed in the field-camera
build path" on the slot-2 row was wrong: none of `FUN_801DAB90`,
`FUN_801DB8EC` or `FUN_801DBE9C` writes `_DAT_8007B794`, and the only zeroing
is the scene-entry reset `FUN_80025C24`. And `edstati3` is a real scene - the
ending cutscene block whose bundle sits at extraction entry 826. Its CDNAME
label inherits forward as far as the battle-data packs at 863/864, and that is
what made it look like a decode artefact rather than a scene.

The port now composes the third factor: `Camera::roll`, the shell's
`psx_camera_mvp` / `cutscene_view`, the cutscene interp's tenth component, and
the browser page's orbit `cam.roll`.

Grade `capture` for the census (a disc-derived executing oracle) and
`disassembly` for the factor itself - `0x8001CFD0..0x8001CFE8` loads
`_DAT_8007B794` through `lui a0,0x8008; lh a0,-0x486c(a0)` and calls
`0x8004638C` unless the render node's `+0x52` bit `0x200` is set.

## Measurement + corpus

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| What is in the `SCUS_942.54` code gap the citation graph never listed | resolved | `disassembly` + `capture` | [details ↓](#what-is-in-the-scus_94254-code-gap) |
| Why three plausible-code blocks in `SCUS_942.54` can never be a function body | resolved | `disassembly` | The BIOS kernel-patch cluster copies `0x8006EF78` and `0x8006F058` into kernel RAM, and `0x8005BBB8` is the stock PSX exception-handler prologue with no reference of any of the five forms anywhere on the disc. Each is real MIPS that executes only after relocation, so nothing calls it at its link address and no function record exists. See [`runtime-libs.md`](functions/runtime-libs.md#the-payload-blocks-are-code-that-is-never-a-function). |
| Is the unattributable dump-extent residue a re-dumping problem | resolved (no) | `capture` | Three shapes remain and only one is dump-shaped: short windows no image reproduces at that VA, bytes in no extracted image at any VA (needs an **extraction**), and extents where two dumps genuinely resolve to different images (an answer). The earlier "repaired by re-dumping" reading is on [`re-do-not-re-walk.md`](re-do-not-re-walk.md#measurement-readings). |
| Can the inner of two nested overlay spans be measured at all | resolved (yes) | `capture` | Address ambiguity is total for it by construction; byte attribution places most of its extents anyway and the row reports. What is structural is that it can never be resolved *by address* - a statement about one method, not about the image. |

### What is in the `SCUS_942.54` code gap

*Status:* resolved

The gap is what the citation-denominated instruments cannot see, and it is
mostly one thing. Working
[the disc-denominated gap list](../tooling/disc-coverage.md) until it stopped
yielding produced ~95 function entries; a five-form reference sweep over each
one, across `SCUS_942.54`, the based overlay images and every PROT entry, splits
them roughly three quarters / one fifth / three:

- **No reference of any form, anywhere.** Ghidra builds functions from the call
  graph, so a routine nothing references gets no function record, no dump, and no
  citation - and is therefore invisible to a worklist derived from citations.
  This is the class only the bytes can find, and it was the majority of what was
  left.
- **Referenced from `SCUS_942.54`.** Mostly reached through the entry-stub / init
  path, or from a body whose own analysis was incomplete.
- **Referenced only from an overlay** (three): live routines whose only caller is
  in an image the SCUS-only analysis never sees. The standing "zero static
  callers is not dead" trap, surfacing as a coverage gap.

Four of the recovered entries are game logic rather than library material and are
written up in the function directory: the inventory count-add primitive carrying
the 99 stack cap (`80042FE8`), the CD-read retry step and subsystem mode toggle
(`8003F210` / `8003EDAC`), and a camera preset (`800260DC`).

Grade `disassembly` for the identifications, `capture` for the reachability
split - the latter rests on
[`find-address-word-refs.py`](../tooling/address-reference-scan.md), whose
negative is a statement about *static* references only: a computed target
assembled some other way, or a caller in an overlay never extracted, would not
appear.

## Related pages

- [`open-rev-eng-threads.md`](open-rev-eng-threads.md) - the live hunts, and the page to move a row back to if new evidence reopens it.
- [`re-do-not-re-walk.md`](re-do-not-re-walk.md) - the falsified hypotheses.
- [`docs/reference/functions.md`](functions.md) - canonical function directory; the place to learn what a `FUN_<addr>` mentioned in a row actually does.
- [`docs/tooling/ghidra.md` § decompiler artifacts](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims) - the grading rubric behind the `decompiled-C` column.
- [`docs/tooling/port-catalog.md`](../tooling/port-catalog.md) - per-function dumped x documented x ported x ignored axes; the function-level companion to this page's question-level index.
