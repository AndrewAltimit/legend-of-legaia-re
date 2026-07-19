# Open reverse-engineering threads

An index of reverse-engineering **questions** about Legaia's runtime, and where each one stands. Rows are questions, not progress markers: each says what is settled, what remains, and what evidence would close it.

## What this page is for

Before starting a hunt, look for it here. A question about this game is usually in one of three states, and this page tells you which:

- The question is **open**, and the row names the concrete next step.
- The question is **answered**, and the row points at the doc, parser, or capture that answers it.
- The question has a tempting wrong answer that was already **falsified**, and the row records why - so you don't spend a week re-deriving the same dead end.

That third case is the point of the page. Most rows here are closed, not open. A falsified row is kept forever, with its reasoning intact, because the reasoning is the deliverable: "the world-map slot-4 bodies are coastline wireframes" is a very plausible reading of those bytes, and knowing *why* it is wrong is worth more than the row it occupies.

Nothing here counts ports, tests, or coverage. Detailed captures and decompiler dumps live in the linked docs and under `ghidra/scripts/funcs/`.

## Status conventions

| Status | Meaning |
|---|---|
| **open** | Active hunt. A concrete next step exists; the row names it. |
| **partial** | The main result is pinned; a residual sub-question remains. |
| **resolved** | Answered. The row (or its details section) cites the evidence and the doc that now owns the fact. |
| **falsified** | Hypothesis disproved. The row is kept so the path is not re-walked. |

Many rows qualify the status in parentheses - `resolved (structural)`, `resolved + ported`, `falsified (VA-aliasing artifact)` - naming *how* it was settled. Read the parenthetical: `resolved (static)` and `resolved (RAM-verified)` are different strengths of evidence.

## How a thread is laid out

Each area below opens with a table of one-line rows. A thread whose write-up outgrows a table cell keeps its one-liner in the table and links to a `###` section immediately after that table via **[details ↓]**; the full analysis - every address, capture, and falsification - lives in that section, under its own *Status:* line.

---

## World map / kingdom bundles

| Thread | Status | What would close it |
|---|---|---|
| Kingdom slot 4 - per-record semantic | partial (transcode closed - read in place; no actionable next step) | [details ↓](#kingdom-slot-4---per-record-semantic) |
| Slot-4 → cluster-A converter site | falsified | There is no slot-4 → cluster-A converter. The cluster-A pool (`DAT_8007C018`) is filled exclusively by `FUN_80026B4C`, reached only from `FUN_8001f05c` **case `0x02`** (TMD pack) and **case `0x09`** (bare TMD). Slot-4's type byte is **`0x05`**, whose `FUN_8001f05c` case merely allocates the MOVE buffer `_DAT_8007B888` and never calls `FUN_80026B4C`. So slot-4 bytes never become cluster-A TMDs; the `DAT_8007C018` kingdom entries are the scene's own type-`0x02` field-file TMD pack(s), installed by the single `FUN_80020224` descriptor-walk. |
| World-map walk-view continent ground render | resolved | [details ↓](#world-map-walk-view-continent-ground-render) |
| `DAT_8007C018[45..53]` mid-load vertex-pool pointers | resolved (structural) | The liveness rule settles it without a snapshot: `DAT_8007C018[i]` is meaningful only for `i <= DAT_8007BB38` (the walker/install counter). Entries above the counter - which includes `45..53` in small field scenes like town01 - are stale carryover from prior game state, never dereferenced; there is no per-index "vertex-pool pointer" semantic. The historical "`[45..53]` = `(-6,-6)` vertex data" reading was a Drake mid-warp snapshot taken *past* the counter. The `field_load_first_town` state the probe would use was never actually captured (no file in the catalogue), so the structural rule supersedes it. |
| World-map outline / coastline reading | falsified | Visual inspection plus the slot-4 record-semantic work refuted the "world-map overlay outlines / coastline wireframe" interpretation. Bodies are most likely small object-local 3D meshes; treat any future "kingdom border lines" claim with suspicion. |


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

## Battle / arts / level-up

| Thread | Status | What would close it |
|---|---|---|
| Encounter MAN sub-section layout | resolved | [details ↓](#encounter-man-sub-section-layout) |
| Effect-VM pass-1 "state token algebra" (`FUN_801E0088`) | resolved + ported | The "state" bytes are 5.3 fixed-point **wait counters**, not opcodes: two countdown-driven cursor walks (master spawn cadence over 14-byte pack1 records; child anim/motion over 6-byte pack0 frames). `Pool::tick_retail` executes the algebra operator-for-operator (pass 2 = `Pool::child_billboards`), disc-verified over all 33 `efect.dat` scripts. Full algebra: [effect-vm.md](../subsystems/effect-vm.md#the-extracted-pass-1-state-algebra). The engine's live path runs it: `engine-core::World::tick_effects` sweeps `tick_retail` per retail frame and `active_effect_sprites` maps `child_billboards` one-for-one (the legacy fixed-lifetime shim is deleted; dev debug spawns live outside the pool). |
| Move-VM op `0x2F` extension dispatcher - per-overlay copies? | falsified (one copy, field overlay 0897 only) | The seven `_801d362c` capture dumps are byte-identical (0897 observed under world-map / dialog / cutscene scenario labels); every other mapped slot-A overlay + the title overlay carries unrelated bytes at the fixed call VA and no JT at `0x801CE868`. So op `0x2F` is executable only while 0897 is resident, and battle-side move records cannot use it. See [move-vm-overlay-ext.md](../subsystems/move-vm-overlay-ext.md#overlay-residency---one-copy-in-the-field-overlay-only). |
| "`FUN_801F3894` spirit/magic damage roll" (state-`0x3D` chain caller) | falsified (VA-aliased dump) | The `overlay_0897_801f3894` dump is `FUN_801DD0AC` byte-for-byte under a double VA shift (0897's extraction over-reads into 0898; the Ghidra program maps the file at `0x801C0000`, not the slot-A base `0x801CE818`), so the already-ported damage kernel surfaces at a fake entry VA. The real state-`0x3D` callee `FUN_801F3990` is a cast **audio-cue dispatcher**; spirit damage is state `0x3E`'s inline formula (`battle_formulas::spirit_damage`). **Corollary: every `overlay_0897_801fxxxx` dump of the `0x801F` region is suspect** — verify vs battle-resident bytes (`0x801F0348`/`0x801F1ED4`/`0x801F45A4` pending). See [battle-formulas.md](../subsystems/battle-formulas.md#whats-still-open). |
| Super / Miracle Arts trigger logic | partial | [details ↓](#super--miracle-arts-trigger-logic) |
| Seru-magic summon visual (e.g. Tail Fire) | resolved (player visual; wired) | [details ↓](#seru-magic-summon-visual-eg-tail-fire) |
| `summon.dat` / `readef.DAT` side-band streaming | resolved (entries + format) | [details ↓](#summondat--readefdat-side-band-streaming) |
| Monster steal item (Evil God Icon) | resolved | [details ↓](#monster-steal-item-evil-god-icon) |
| Battle face-stamp issuing site | resolved | The facial-texel overwrite is the per-frame **facial animator `FUN_8004C7B4`** (called from the render-node update with the clip's frame cursor; Terra skipped): action-entry facial tracks at `+0x8C` (eyes) / `+0x98` (mouth) select frames from static per-character SCUS tables, stamped by `MoveImage` every frame. Pinned live across a battle entry (`karisto_sol_pre_encounter` + the MoveImage trace probe). Sibling-pass residue closed: `FUN_8004CCD4` is **not a stamp** - it is the equipment mesh-variant swap (same caller + guards, re-run per ghost by the arts trail renderer), driven by the entry's third track at `+0xA4`; retail windows Noa-only. See `battle-data-pack.md` § Facial animation tracks + § Equipment-variant track. |
| Per-spell magic power / multiplier | resolved (mechanism + roll ported) | [details ↓](#per-spell-magic-power--multiplier) |
| Arts command sequence - independent source | resolved | The SCUS arts-name table (`DAT_80075EC4`) glyph string is byte-exact ground truth for every art's directional command; `legaia_art::ArtsOracle` exposes it, and disc-gated contract tests validate both the best-effort PROT `0x05C4` `parse_record` command-decode and the curated gamedata `directions`/`ap` columns against it (one documented walkthrough error: Hyper Elbow). |
| Weapon-specialty arm width (off-class widens the Arms command) | resolved | Not a runtime favored-class comparison. The arm command's AP cost is a per-(character, weapon) byte in the player battle file, at the weapon section's swing record (`section[+0x04]`) `+0x74` (favored `0x1E` / off-class `0x2A` / far `0x36`); LZS-decoded and copied verbatim into the runtime gauge (`DAT_801C9360[char][0x0C]+0x74`) at battle load by `FUN_800557B8`, read by gauge builder `FUN_801D388C` case 9. Byte-validated across all three player files; randomized by `legaia_rando::weapon_specialty`. See [`docs/subsystems/arts-command-gauge.md`](../subsystems/arts-command-gauge.md). |
| Stat growth-rate source | resolved (validated + wired; core + opt-in jitter) | [details ↓](#stat-growth-rate-source) |
| Character-record HP/MP/AP pair order (`+0x104..0x110`) is `(max, cur)` | resolved (relabeled throughout) | Three independent sources agree `+0x104/+0x108/+0x10C` = effective maxima, `+0x106/+0x10A/+0x10E` = currents: (1) walk-regen `FUN_801D0B90` bumps `+0x106`, clamping at `+0x104`; (2) aggregator `FUN_80042558` rewrites `+0x104` per frame from base stats + %-passives (a per-frame recompute cannot be current HP); (3) GameShark "Infinite HP" codes write `+0x106` at every character stride - they pin the *current*. `legaia_save::HpMpSp` + every consumer/doc copy now carry the `(max, cur)` order; the status AP gauge reads the AP *current* at `+0x10E`. Fresh-save fixtures masked the original swap (cur == max at seed). |
| Monster stat-record archive source | resolved | [details ↓](#monster-stat-record-archive-source) |
| Monster mesh + texture pool | resolved | [details ↓](#monster-mesh--texture-pool) |
| Terra slot-3 / story-flag overlap | resolved | [details ↓](#terra-slot-3--story-flag-overlap) |
| Navmesh / per-scene navigation data | falsified | `0x80108EA4..0x80109550` is per-scene GPU primitive scratch, not a 24-byte stride navmesh. Pointer hunts find zero RAM cells pointing into the window. Real per-scene region / collision / event-trigger data lives in the field-file preamble (a count + `u16` offset table + records - **not** the field-pack schema slots, which are a global-constant template; see [field-pack](../formats/field-pack.md)); the collision grid is the `+0x4000` MAP region; the encounter-record path lives at `actor[+0x94]`. |
| Battle party mesh pack `other5` = **PROT 1204** (battle form; Baka Fighter reuses it) | resolved (empirical) | [details ↓](#battle-party-meshes--assembled-from-the-player-battle-files-prot-1204--baka-fighter--default-equipment-sibling) |
| MP-cost ability-bit priority (half vs quarter) | resolved (dump-confirmed) | [details ↓](#mp-cost-ability-bit-priority-half-vs-quarter) |
| Scripted Tetsu encounter → Battle (v0.1 oracle Battle leg) | resolved | All three residuals are now derived from disc bytes: the formation-row selection is the standard scripted-battle op `3E FF 04` in `P1[10]` (same case-`0x3E` install arm as Zeto/Caruban; row 4 = lone Tetsu), the sparring-partner reposition is `P1[10]`'s `4C 51 15 0E 07 22` NpcRun→tile `(21,14)` = `RIM_ELM_SPARRING_CARRIER_TUTORIAL_POS` exactly, and the spar Yes/No is a MES-embedded option picker (`0x29` open + N×2 signed relative-jump table, handler `FUN_80038050`; port `legaia_mes::Picker::jump_target` + `InlineDialogueRunner::last_choice`), not a field-VM opcode. [details ↓](#scripted-tetsu-encounter--battle-v01-oracle-battle-leg) |
| First boss trigger -> Battle | mostly resolved (mechanism pinned for both bosses; one vestigial-flag question open) | The scripted-battle arm is the field-VM op `3E FF <formation_row>` ([battle.md](../subsystems/battle.md#scripted-battle-entry-3e-ff-row)). Zeto: garmel `P2[12]` ends `3E FF 09` -> row 9 = lone `0x4B` (firehose-confirmed; `0x8007b7fc` stayed silent) - organic in the engine (`organic_zeto_encounter_disc.rs`). Caruban: rikuroa stager `P1[3]` ends `3E FF 11` -> row 17 = lone `0x49` - same record path; the engine runs `P1[3]` on approach (`World::run_boss_stager_record`; `SCRIPTED_SCENE_BOSSES` deleted). Open: does any retail battle write `DAT_8007b7fc` (reads `0` everywhere; may be vestigial)? |
| Spine flag `0x142` (Caruban beat / dolk-dolk2 switch) writer | resolved (disc writers + engine port + oracle) | Spine-writer #2 of 3, closed: plain field-VM `51 42` SETs in the rikuroa **streaming variant MAN** (PROT 0157: P1[10..12] + post-victory P2[50], C1 = `0x142` itself - the self-latching one-shot), re-asserted by dolk2's carrier P1[0..1], cleared by dolk P1[26]. Firehose-caught live (`ra 0x801E3598`); resident heap byte-matches the carrier. The old corpus-negative stood because no census walked the streaming carriers. Census + pins: `man_variant_carrier_census_disc.rs`; the engine sets it **organically** - rikuroa `P2[50]` executes from its own script bytes on the Battle->Field edge (`organic_beat_records_disc.rs`), retiring the earlier `SCRIPTED_SCENE_BOSSES` victory latch. |
| Spine flag `0x482` (Drake mist-wall) writer | resolved (writer-less; "direct code path" presumption falsified) | The named capture ran: byte write-watch on `0x800857E8` (`autorun_flag_writer_watch.lua`) across the whole post-Zeto beat (battle exit, mist-clear FMV, `map01` arrival). The only write to the byte is the SET helper re-latching `0x484` (store `FUN_8003CE08+0x28`, `ra 0x801E3598`); `0x482` never flips. Every catalogued state through the Karisto era holds it clear (neighbours `0x484..0x487` at `0x0F`), and all 37 census sites stay `DESYNCED`. Verdict: **no writer ever fires** - the `map01` P2[34..36] C1 spawn-block never latches; wall despawn is not flag-driven. Residual: only an engine-side C1-latch-on-fire (pad-walk into a wall) could revive this. |
| CDNAME scene-window frame (`raw = extraction + 2`) in `Scene::load` | resolved (engine converts; misattributions corrected) | Engine scene windows used raw-TOC defines as extraction indices - two entries late, dropping each block's first two retail entries and bleeding in the next block's. Corrections that fell out: the `.MAP` is the retail block's FIRST entry (not "two below"); "suimon == dolk2 MAN" and "rikuroa MAN = [18,70,20]" were next-block sidecars under the wrong label; "urudre1 tests 0x15E" and "0x63A has no writer" are falsified; "0x1BE = rikuroa Zeto gate" was geremi's arrival one-shot. Head blocks (defines 0/1, inside the TOC header rows) keep legacy windows. See [cdname.md](../formats/cdname.md#numbering-space). |
| Motion-VM (`FUN_80038158`) bytecode carrier + flag census | resolved (carrier pinned; spine flags negative) | The second motion VM's bytecode source is **MAN tail-section 1** (installer `FUN_8003A9D4`; parser `legaia_asset::man_motion`; layout + op table in [`motion-vm.md`](../subsystems/motion-vm.md#the-second-motion-vm---fun_80038158)). Disc-wide op-7/op-8 census (`--motion-flag-census`): overworld walking-band choreography + one `town0b` clear; `0x142`/`0x482`/`0x1BE` and `549` appear in NO stream - the "549 set by op-7 bytecode" carrier claim is **falsified**. Anchor test `motion_flag_census_disc.rs`. |
| Debug-menu "STR trigger teleports + sets flags" mechanism | resolved (no per-FMV event table; dev-menu tools explain it) | The FMV dispatch record is playback-only and the two direct `_DAT_8007BA78` store sites (op `4C E2`, title tick) are **corpus-exhaustive**. The observed teleport+flag application is the 0897 dev-menu toolset: warp appliers `FUN_801ee094`/`FUN_801ee328` (MAP CHANGE) + the EVENT FLAG editor (`FUN_801dbd04`, cursor over flags `0..0xFFF`, SET/CLEAR at cursor); the FMV corpus states came from its register-pointer editing (invisible to static scans). Do not re-walk the "per-FMV event table" shape. See [functions.md](functions.md) + [cutscene.md](../subsystems/cutscene.md). |
| Spawned-record player-channel (`0xF8`) ExecMove/HaltAcquire handshake | resolved (engine completion model) | The timeline stepper models the handshake directly: `A2 F8` ExecMove arms an in-flight countdown (`CutsceneTimeline::player_move_frames`) and `C3 F8` HaltAcquire parks at the op until it drains, then steps past by encoded width (`resolve_target` keeps its `None` contract for `0xF8`) - so door-cutscene records reach their trailing `0x3F` and driven hops land (`jou`→`jouina`, and the full castle chain to `jouinc`, in `chapter1_hub_depth_oracle.rs` part J + `chapter1_hub_breadth_oracle.rs` part F). See [cutscene.md](../subsystems/cutscene.md) § player-channel completion. |
| Equipment stat-bonus table - slot model | resolved (slot model + passives) | The stat-bonus table (`DAT_80074F68`, 8-byte stride) is decoded from `FUN_801CF650`/`FUN_801CF5D0` (`legaia_asset::equip_stats`): `+0`=INT, `+1`=ATK, `+2`=UDF, `+3`=LDF, `+4`=SPD (the earlier AGL/evasion reading is falsified). The four `+7` categories are Legaia's four weapon/armour slots (body/head/footwear exact by name; none of the 77 accessories appear in this table). Wired: `DiscEquipInfo` gates `EquipSession`'s per-character list. |
| Enemy-ally charm battle softlock | open (cause unconfirmed) | A charm fight can hard-freeze (user-reported live; once after a Gimard cast, once an arts combo). Cause not pinned. The `FUN_801E7320` "unbounded reroll at `0x801E7370`" theory is unsettled - loop-1 rerolls while `monster[a0].HP == 0` over `a0 = 3 + rand % monster_count`, but the alive charmed ally is itself a slot in that range and can be picked (exit via self-target `0x801E73E0`). Two Lua-forced-battle-end workarounds were tried and reverted: forcing the end mid-action tears the SM down -> game-over, then garbage writes / freeze (the game only writes the end signal at the safe `0x5A` gate). Next: observe with `autorun_charm_win_softlock.lua`, then a disc-side fix. |
| Battle-actor `+0x16E` bit `0x400` applier (guard-disabling status) | open (low value) | Bit `0x400` of the battle-actor flags word `+0x16E` reads as a guard-disabling Sleep/Numb-like status. The sibling AI-delegated bits `0x380` in the same word are pinned (the enemy-ally charm hook sets them via `FUN_801E7320` retarget), but the *setter* that raises `0x400` is not in the decompiled corpus - readers only. Low value. Closes with a before/after RAM capture around a battle turn that inflicts the status, diffing the `+0x16E` word to catch the byte that raises `0x400`. |
| Flag `0x63A` - the vell/vozz `P2[7]` gate with NO script writer | resolved (script writers exist; the "no writer" premise was the CDNAME +2 skew) | Under the fixed scene windows the census shows eight **clean** sites: Set/Clear pairs in the rikuroa post-Caruban variant MAN (PROT 0157 P2[29]/[30], op `0x56`/`0x66`), rikuroa2 (PROT 0122 variant), retockin (PROT 0281 P2[7]/[8]) and edretoin (PROT 0800 P2[7]/[8]) - late-game beats, so the vell/vozz `C1=[0x63A, 0x7]` spawn-block passes for the whole first visit. Retail states corroborate: `0x63A` reads clear through the Karisto era while its bank byte already holds `0x0C` (`0x63C`/`0x63D` set). NB the old row's watch target `0x800858C7` was mis-derived; the byte for `0x63A` is `0x80085758 + (0x63A >> 3) = 0x8008581F`. |
| cave01 `P2[16]` (the `0x15D` entry-key setter) - what spawns it | resolved (slot-counted spawn chain) | [details ↓](#cave01-p216-spawner---the-slot-counted-interact-chain) |
| Op-`0x4E` sub-ops 4..8 "absolute jump" / "rand -> next PC" readings | falsified (all sub-ops 0..9 are the 7-byte compare-and-skip) | [details ↓](#op-0x4e-sub-op-family---every-sub-op-09-is-a-compare) |
| Drake Castle deep interiors (`jouinc`/`jouind`) depth decode | resolved (door-choreography families, not story gates) | `jouinc`'s 58-record `C1=[0x00F]` P2 family is a **busy-mutex door family**: each record SETs `0x00F` first / CLEARs it last, so the C1 gate is a mutual-exclusion lock, and bodies are per-door walk-through choreography. `jouind` P2[10..13]'s `0x4BE..0x4C2` band is **per-visit door/lift state** (cleared by `jouina` P1[0] on entry), not a later-chapter revisit gate pair. Decoding these exposed (and fixed) whole-nibble width blindness (`0x4C` nibbles 9/A/C/D/F). `jouinb P2[6..8]` is the interior beat band (`0x44E..0x450` latches + the jouinb-local `0x461` state flag). Full mechanism: [script-vm.md](../subsystems/script-vm.md) § door-choreography record families. |
| `scene_destinations` P1-table scan misses P2-only door names | resolved (P2 pass folded in) | The P2-only class is the town/dungeon **exit door** (a P2 door-choreography record): `town01`→`map01` (Rim Elm's overworld exit; the P1 pass alone sees *zero* town01 destinations), `retockin`→`retona`, `geremi`→`map02`/`tower` - 13 scenes / 14 destinations disc-wide. The suspected `jouinb`→`jouina` exemplar is falsified: it is P1-visible (the over-walk resyncs across that record). Merged kernel `legaia_asset::man_edit::scene_destinations` (P1 pass as prefix + clean-gated P2 pass, `(name, index)` dedupe); the engine delegates to it; disc pins `scene_destinations_p2_disc.rs`. |
| `0x4C 0x51` byte `+3` = `[bit7 special-model \| facing nibble]` vs the glide-speed interim `depth & 7` reading | resolved (facing wins; the two readings were two different ops) | [details ↓](#0x4c-0x51-byte-3-reconcile---facing-wins-no-motion-bytecode-synthesis) |
| dolk2/rikuroa MAN source (the "v12-embedded MAN" was an over-read) | resolved (streaming carrier) | Their own `base+3` bundles are the MAN-less count=4 form `[1,2,6,0x14]`; the "embedded MAN at 0x1000" inside their SceneV12Table entries is an over-read onto the next scene's bundle (suimon's / geremi's; [scene-v12-table.md](../formats/scene-v12-table.md) § over-read). Retail sources their partition scripts from the block's standalone `data_field_streaming` entry's type-3 chunk (`dolk2` ext 70 `[29,73,17]`, `rikuroa` ext 157 `[13,29,64]`; live script-heap byte-match at the Caruban beat). Engine: `field_man_payload` streaming fallback (`streaming_man_payloads`) + retail-frame `Scene::load` windows; pins `v12_bundle_man_disc.rs`. |
| kor-family op-0x49 flag window `[0x138..0x13F]` - what the 8 flags gate | resolved (Uru Mais warp-pad destination memory) | [details ↓](#kor-family-op-0x49-flag-window-0x1380x13f---uru-mais-warp-pad-picker) |
| `801d58f0` / `801d63b0` as single shared port blockers | falsified (VA-aliasing artifact) | The two addresses host different code in different overlays (byte-verified: 80/228/124/308/1 B and 208/1036 B across 0897/baka/cutscene/debug-menu/fishing/slot/dance) - the port-catalog's bare-VA keying aggregated their refs into phantom top blockers. Tracked per-overlay via `overlay_<label>_<addr>` identities; catalog ignore category `va_aliased_overlay_local`. |


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
[the 0x4E details](#op-0x4e-sub-op-family---every-sub-op-09-is-a-compare).

### Op-0x4E sub-op family - every sub-op 0..9 is a compare

*Status:* falsified ("absolute jump" 5..8 and "rand -> next PC" 4 were Ghidra's collapsed switch)

The raw 12-entry jump table at `0x801CEE30` (field overlay, PROT 0897 file `+0x618`) routes
**every** sub-op 0..9 to a value loader that joins the shared 7-byte compare-and-skip
continuation at `0x801E0B40`:

| sub | loader | state value |
|---|---|---|
| 0 / 1 | `0x801E0A40` / `0x801E0A70` | char-record HP / MP `(cur, max)` pair - the only scaled form (`max * arg >> 8`) |
| 2 | `0x801E0AC0` | char level byte `+0x130` |
| 3 | `0x801E0AEC` | party gold `_DAT_8008459C` |
| 4 | `0x801E0AFC` | **BIOS `Rand() & 0xFF`** - a random-chance branch |
| 5..8 | `0x801E0B0C` | **slot table `0x801C6460[sub - 5]`** (s16; the read side of the `4C CA/CB/CC` slot writes) |
| 9 | `0x801E0B34` | coin bank `_DAT_800845A4` |

Sub-ops 10/11 keep the 9-byte u32 gold/coin form; 12..15 fall through (PC += 7). The decompiled
bare-`break` arms for 2..9 were the collapsed switch - each raw loader ends `j 0x801e0b40` /
`j 0x801e0b3c` with the operand pointer staged in the delay slot (the same class of trap as the
label-call idiom). Disassembler + executing VM corrected: `field_disasm::decode_subops` (single
0..=9 compare arm), `engine-vm` `field/step/flow.rs` + `FieldHost::op4e_char_level` /
`slot_table_read`. cave01's `P2[12]` spawn gate is the live sub-5 exemplar.

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

Consequence: the engine's `placement_glide_speed` heuristic (reading the `4C 51` byte-`+3`
low bits as a base step) actually reads the facing nibble - flagged in
`man_field_scripts::npc_motion` for a rework onto real `0x37`/`0x41`/`0x47` operands. See
[field-locomotion.md](../subsystems/field-locomotion.md) § NPC initial facing / § NPC glide speed.

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
visible row**, `+3` `rows` = visible row count (menu height `rows * 16` px), so the paired
descriptors are the full 8-row menu (selected by state flag `0x136`) vs the rows-4..7
chambers-only menu (`0x137`; kor3/kor4 carry per-pad record pairs, one per variant). State 0
cursors to the pre-set bit (= "you are here") and clears the window; confirming a **different**
row sets `base + selection`; picking the current row or cancelling sets nothing, so the test
ladder falls through to the stay-put arm (`+0x1C9`: clear `0x136`/`0x137`, fade back, park).

### Encounter MAN sub-section layout

*Status:* resolved

`FUN_8003AEB0` is fully decoded: it walks the MAN multi-section header (sections at MAN offsets `+0x22, +0x24, +0x26, +0x28`, signed 16-bit LE) and `legaia_engine_core::encounter_man::scene_encounter_from_man` reads the encounter section straight from disc bytes, wiring per-scene `EncounterTable`s for the standalone towns + kingdom-bundle scenes (the `count = 6` MAN form is now resolved by `find_bundle`). The region-table section is the per-scene control block `_DAT_801c6ea4 + 0x4` count-prefixed array of 18-byte records: `byte[0]` kind selector, `bytes[1..4]` tile-space bounding box `[minX, minZ, maxX, maxZ]` queried by `FUN_801dba20(tileX, tileZ)` (`tile = (player_pos - 0x40) >> 7`), `bytes[5..17]` a per-region **camera preset** -
decoded byte-for-byte (three mode-keyed splits on `byte[5] >> 4` into the `0x8007B607..0x8007B627` camera globals, consumed by the camera-param builder `FUN_801dab90`) in [`formats/encounter.md`](../formats/encounter.md#man-section-3-the-camera-region-table),

consumed by the field camera arrival handler `FUN_801dbec4` + camera-config `FUN_801dbc20`. The query side is ported: `legaia_engine_core::field_regions::zone_query` (`FUN_801dba20`, with the `FUN_80017fbc` `.MAP` region scan + `FUN_800180ec` attribute refresh) drives `World::refresh_field_regions` per tile crossing, and the section-3 body is the table the boot walk installs at `_DAT_801c6ea4 + 0x4`. Residual: the world-overview actor-placement section (consumed by `FUN_8003A1E4`), tracked separately (see world-overview threads); plus one loose end from the camera decode - the mask-kind records' `bytes[1..4]` side-copy to scratchpad `0x1F8003E8..EB` / mirrors `0x801F2778..84` has a confirmed writer but no traced reader.


### Super / Miracle Arts trigger logic

*Status:* partial

The find/replace matcher **is** ported (`legaia_art::{MiracleMatcher,SuperMatcher}`, applied by `legaia_engine_vm::battle_action::resolve_action_queue`).

**Miracle is now wired into the live player-driven Arts submenu**: `battle_arts::miracle_for_chain` flags a saved chain whose directional string is the caster's Miracle Art, and `World::build_battle_arts_rows` resolves the finisher-replacement queue into a per-strike profile (real `ArtRecord` power where staged, synthetic `x12` per component art otherwise).

**Super is now wired into the live submenu, with the queue connectors abstracted.** `legaia_art::recognize_art_sequence` tokenizes a saved chain's flat directional string into its ordered named arts (each identified by its own `ArtRecord::commands`), and `SuperMatcher::trigger_by_art_sequence` tail-matches that ordering against each Super's `SuperArt::art_sequence()` - the `find` pattern projected to art constants only (`[0x27,0x1F,0x27]` for Tri-Somersault), with starters + connectors stripped. `battle_arts::super_for_chain` / `World::build_battle_arts_rows` flag the row (`ArtRow::super_art`) and resolve the `replace`-queue strike profile (shared with the Miracle path).

**What stays open:** the *byte-exact* queue connectors. The connector direction after each art is **combo-specific** (Vahn's `0x27` → `0F` in Tri-Somersault but `0E` in Power Slash), so it can't be derived from each art's commands; the runtime queue-builder that emits them (`ctx[+0x274]`) is **unpinned** (no queue-write watchpoint trace yet). The live match is therefore faithful to *which* combination triggers *which* Super but does not reproduce the literal queue bytes. The byte-exact matcher (`SuperMatcher::try_trigger_at_tail`) is also ported, exercised by `resolve_action_queue`. See `docs/subsystems/battle-action.md` § "Miracle / Super in the live player-driven Arts submenu".

**Queue location pinned; Miracle path validated (capture).**
The action queue is the per-actor **`actor[+0x1DF..+0x1F2]`** action-parameter byte stream (not `ctx[+0x274]` - a capture showed that is the turn-order active-actor index written by `recompute_battle_order` `FUN_801DABA4`).
The directions/connectors encode as `0x0C/0x0D/0x0E/0x0F` = Left/Right/Down/Up, `0x1A` = `SpecialStarter`, `0x1B..0x32` = art constants.
A `battle_noa_miracle_art_combo` capture (probe `autorun_super_art_action_queue.lua`, runbook [`docs/tooling/super-art-queue-capture.md`](../tooling/super-art-queue-capture.md)) read Noa's resident Miracle queue and it matches `crates/art/src/miracle.rs`'s modeled replacement string **byte-exact** - runtime-validating the queue + `ActionConstant` encoding that were previously spreadsheet-sourced.
**Super path also validated:** a `battle_vahn_tri_somersault_super` capture read Vahn's resident Tri-Somersault queue (`…19 27 0F 19 1F 0E 1A 2B 2B 2B`) whose matched/replaced tail is **byte-identical** to `super_art.rs`'s `Tri-Somersault` `replace` - confirming the combo-specific connectors (`Somersault 0x27 → 0F`, `Cyclone 0x1F → 0E`) and the finisher tail. The dequeue site is pc `0x801D89D8`. The only residue is the other 13 Supers' replace strings (each a one-capture check through the same probe).


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

What the player steals (Evil God Icon equipped) is a **static `SCUS_942.54` table at `DAT_80077828` / file offset `0x68028`**, indexed by **1-based monster id** at `DAT_80077828 + id*2`, each entry a 2-byte `[steal_chance_pct, steal_item_id]` pair (chance FIRST, item second - the reverse of the record's `[item, chance]` drop order). It is **not** in the PROT 867 record (the prior exhaustive record scan was correct - the data simply isn't there; it's a separate executable table, which is why every record-only search came up empty). Pinned from a live player-steal RAM capture (Skeleton id 13 → `1e 8a` = 30% Incense, matching the on-screen banner) and verified **byte-exact against the complete published steal table** (item + chance) across every resolvable monster id - zero mismatches.

Parser `legaia_asset::steal_table`; doc [`steal-table.md`](../formats/steal-table.md); randomizer `legaia_rando::steal`. `enemies.toml` `steal` stays useful ground-truth but the SCUS table is now authoritative.


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
The corner projection is ported too: `FUN_800195a8` (the camera-coupled GTE billboard projector - view-space MVMVA center, ±half-size corner fan-out, rotation+translation reset, RTPT×3 + RTPS; see the [`functions.md` detail](functions.md#800195a8)) is `legaia_engine_render::billboard::project_billboard`, with the exact `FUN_801e1ab0` call shape (`+0x120` Y push, dynamic half-width `state+0x6c6 − 0x200`, half-height `0x100`) as `afterimage::project_streak_corners`; the `RotMatrix*` sin/cos LUT is pinned as `trunc(4096·sin)` by the disc-gated `gte_sin_lut_real` oracle.
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

Reading the state-`0x28` block in `overlay_battle_action_801e295c.txt` (`0x801E3D0C`; the same block recurs in state `0x3C` at `0x801E4568`) settles **both** open questions. (1)

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

| Thread | Status | What would close it |
|---|---|---|
| Town/field free-movement locomotion | resolved | [details ↓](#townfield-free-movement-locomotion) |
| "~270 undumped field-overlay functions" (recomp dispatch-entry seed list) | falsified (not a function inventory gap) | [details ↓](#270-undumped-field-overlay-functions-recomp-dispatch-entry-seeds) |
| Field collision-map source | resolved | [details ↓](#field-collision-map-source) |
| Tile-board grid mode | resolved | The `_DAT_8007b450`/`DAT_801f35c0`/`801ef2b0` tile-grid walk is a puzzle / board minigame (procedural `rand`-filled board, per-cell drawn tiles), not town locomotion. It is a field-overlay (`0897`) construct driven from the field/event VM (op `0x49`); the `_DAT_8007b450` refs in the hub minigame overlays are only the shared equip-comparison layout hint `FUN_801e5b4c`, not board use. The `func_0x800467e8` facing remap is a quantized 45° octant rotation. **Sub-questions closed:** boards are always procedural (op `0x49` sub-op-5 = constant 14-byte advance; cells `rand()%6+2` malloc-filled, `FUN_801e0b1c`); no fixed board exists. Header layout + event-cell flags in `docs/subsystems/tile-board.md`. |
| game_mode 0x03 = field/town gameplay | resolved | [details ↓](#game_mode-0x03--fieldtown-gameplay) |
| Scene prescript: field-VM event scripts vs move-VM stagers (dual consumer) | resolved | **Single consumer.** The op-`0x34` sub-3 operand census across every scene MAN shows every prescript record is a **move-VM stager**: partition-1 effect-actor records stage the ambience on entry (record 0 = the master ambient record in 62 scenes), partition-2 cutscene timelines install the per-shot ids. Id space = record index (the RAM `[u16 count][u16 offsets]` relocation at `_DAT_8007b8d0`, live-pinned vs the file bundle). The "field-VM runs a record" premise was the engine's own fallback, not retail behaviour. See [scene-bundles](../formats/scene-bundles.md) § consumer census. |
| Engine VRAM byte-exactness for town01 | resolved (major source); minor residue | [details ↓](#engine-vram-byte-exactness-for-town01) |
| CLUT row 510 population (env meshes' `(64,510)` CBA) | resolved (boot-resident system-UI strip band); residue = the exact boot walker call site | [details ↓](#clut-row-510-population-boot-resident-system-ui-strip-band) |
| Scene-transition (`0x3F` door) destination indexing | resolved | [details ↓](#scene-transition-0x3f-door-destination-indexing) |
| Intra-town (house / interior) door mechanism | resolved | [details ↓](#intra-town-house--interior-door-mechanism) |
| Field/town environment-geometry placement | resolved (renders) | [details ↓](#fieldtown-environment-geometry-placement) |
| Overworld / town entrance story-flag gating | resolved | An entrance's unlock is its own partition-2 record's C1/C2 gate (`FUN_8003BDE0`; C1 = one-shot latch, C2 = requires-all) against the system-flag bank `_DAT_80085758`. Ops `0x50/0x60/0x70` (SET/CLEAR/TEST) carry `idx = ((opcode & 0x8F) << 8) \| operand` (raw flag number). Disc-pinned via `man-scripts --system-flag-census`: map01 keikoku portals `C1=[0x193]` (setter `vozz` P1[7], the only `0x193` SET disc-wide, byte-pinned by `chapter1_hub_depth_oracle.rs`), mist walls `P2[34..36] C1=[0x482]`, town01 dinner chain `P2[4]`→550→`P2[5]`→551. The dinner "re-fire" is falsified. Full write-up in [world-map.md](../subsystems/world-map.md) + [field-locomotion.md](../subsystems/field-locomotion.md). |
| Overworld story-conditional destination (`dolk`→`dolk2`) | resolved (mechanism + engine port) | Beyond the record-level C1/C2 gate, an entrance record can switch its `0x3F` target by an in-record op-`0x70` `SysFlag.Test`. `map01`'s dungeon entrance (`P2[1]`/`P2[2]`) branches on flag `0x142`: clear → `dolk` (pre-boss), set → `dolk2` (post-boss), same trigger + arrival tile. `overworld_portal_sites` decodes the conditional `0x3F` pair (`ConditionalDest`); the seeder resolves via `World::system_flag_test` (`chapter1_boss_spine_oracle` Part D). **Falsifies** "dolk2 is reached from a dungeon interior". The `0x142` setter is now pinned (rikuroa streaming-carrier script records; see the spine `0x142` row). See [world-map.md](../subsystems/world-map.md). |
| Region story-flag gate families (record-header C1/C2 gates) | partial - structure mapped across the chapter-2/3 regions; play order for the dungeons the capture corpus never walked is still owed | [details ↓](#region-story-flag-gate-families) |
| Retail-vs-engine NPC + story-flag state parity across the capture library | resolved (breadth oracle); residuals filed as their own rows | The sweep oracle `crates/engine-core/tests/field_npc_state_parity_disc.rs` compares every catalogued field-mode library capture against a cold engine entry with the capture's `DAT_80085758` bank seeded byte-for-byte: park/place visibility, seat position within the patrol-locality bound, heading (diagnostic), post-entry flag neutrality. Divergences are classified in-test (`KNOWN_DIVERGENCES`); the dominant class is capture-mid-beat dynamics - a mid-visit choreography re-arranged NPCs after retail's own entry, while the engine reproduces the FRESH-entry arrangement (cross-pinned by sibling captures, e.g. rikuroa `pre_caruban`). |
| Entry pre-run channel slice ends on a no-mask `4C 70` wall paint | open (engine gap; retail rule pinned) | Retail's spawn-prologue pre-run (`FUN_8003A1E4`) and the per-frame runner (`FUN_80039B7C`) break a slice only on an executed `0x21` NOP, a stalled PC, or a next-opcode `< 0x20` - never on a nibble-7 paint. The engine's slice also breaks on the `Yield` the **no-mask** nibble-7 forms return (`engine-vm` field `menu_ctrl` nibble-7 sub-0/1). Evidence: `ropeway` P1[30] with `0x1D4` set - both sides take the seat arm, retail reaches the `23 2A 70` seat = `(5440,14400)` (the `octam_to_sebucus_worldmap` node), the engine stops after the paint and leaves the `(127,127)` header park. Closing it: drop the no-mask paint as a slice break, then clear the oracle's `vis:30` row. |
| Mid-visit NPC re-arrangement beats (dolk2 market crowd; garmel pre-Zeto staging) | open | dolk2: a mid-visit record parks the day cohort (prologue-seated at market tiles) and seats the crowd cohort P1[53..60] (bare idle-loop prologues, header parks) - the `dolk2_market_noa` capture holds the post-swap arrangement while its bank still has P1[2]'s `44 72` spawn latch `0x2FE` clear, so the swap runs through a different path than P1[2]'s own `SpawnRecord`. garmel: the pre-Zeto capture stands P1[3]/P1[4] beside the player; cold entry parks them. Closing either: a fresh-entry capture of the same scene (enter, save state before any beat fires), or tracing which record's choreography performs the seats. |
| Writer of the Rim Elm opening flag (`549`) | resolved (self-latching script SET; the census was width-blind) | Writer = **town01 `P2[3]` itself**: a plain `52 25` SET at body `+0x3` in the very record its C1 gates (the rikuroa-`P2[50]`/`0x142` self-latch shape) + the `gameover_data` dev copy. Runtime-pinned first (reader-watch from `s2_rimelm_town01`: SET `ra 0x801E3598`, script-PC offset `+0xF`), then found statically: the preceding `4C ED` op had no width in the disassembler, so the walk desynced one byte short - the old "capture-only" verdict was **width blindness**. Full write-up in [script-vm.md](../subsystems/script-vm.md) (decode-coherence section); anchors `flag_549_reader_is_the_rim_elm_p2_gate` + `flag_549_writer_is_the_rim_elm_p2_3_self_latch`. |


### 270 undumped field-overlay functions (recomp dispatch-entry seeds)

*Status:* falsified - the list is not a function inventory, and the inventory gap it implied does not exist.

A PSXRecomp runtime capture of the slot-A overlay window during a boot-to-town play
session yielded ~312 "call targets" in the `0x801CC000+0x29000` band, ~270 of them
absent from `ghidra/scripts/funcs/` + [`functions.md`](functions.md) - read at the
time as a large undumped-function backlog for PROT 0897. Triaging every address
against the disc overlay images and the captures' own resident bytes falsifies the
premise on three independent axes:

- **They are dispatch entries, not call targets.** The recomp's capture seeds record
  every PC where its dispatcher entered interpretation: indirect-call targets, but
  also **return sites** (the instruction after a `jal`+delay-slot), **interrupt-resume
  PCs** (arbitrary mid-loop addresses, weighted by hot loops), and `jr`-table case
  labels. Against the resident image, only ~1/4 of the entries classify as
  call-shaped at all; the rest sit mid-function or mid-loop.
- **The PC tables span overlay generations; only the byte snapshot is coherent.** The
  capture accumulates PCs across the whole session (title → FMV → menus → field), so
  a "field window" list mixes title-overlay, cutscene-overlay (0970) and menu-era
  PCs with field-era ones. Smoking guns: one source capture's resident bytes match
  the disc 0897 image at only ~16% (title-era, different occupant); dozens of listed
  PCs land inside 0897's **data head** (debug strings + pointer tables - impossible
  as 0897 code); and two entries the list marked as already-known resolve to the
  cutscene overlay's STR dispatch `FUN_801CEA3C` and the actor-VM jump *table*
  `0x801CED70` - a different overlay's function and a data address.
- **No image claims them as functions.** Sweeping all mapped slot-A overlay images +
  the slot-B field library for prologues / static `jal` targets at the listed
  addresses yields only two coincidental hits (both in the never-resident
  slot-machine image) and a handful of `j`-target labels.

The durable lessons: seed lists from a recomp's interpreter dispatcher need
**per-hit resident-image resolution** (e.g. a mode-gated `dirty_exec_hot` window)
before any identity claim, and a "new function" claim needs a prologue or a
static-call witness in the image that was actually resident. The real undumped-code
question for 0897 is better served by the [port-catalog dashboard](../tooling/port-catalog.md)
than by this list.

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
The prop walk-touch posts for the decoded script classes: `placement_walk_touch_event` classifies genuine `0x3E` door-warps and cross-context player-channel `0x23` teleports, and `World::check_field_walk_touch` posts once per ±80-box contact through `trigger_field_interact` and applies the effect (disc-gated `field_npc_motion_disc.rs` / `field_walk_touch_disc.rs`).
**Residual (open):** the full `FUN_801d5b5c` post-kernel state (engaged flag, facing save/restore, `+0x2A`/`+0xA` touch counters), per-actor field-VM channel execution (yield-paced patrol scripts - the engine loops the decoded waypoints instead), the exact retail NPC glide speed, and prop scripts beyond the two decoded walk-touch classes. The interaction-end teardown is decoded: the dialog SM `FUN_80039b7c` exit path restores the actor facing from `+0x5A`, drains the `+0x2A`/`+0xA` touch-counter pair, and clears the player's `0x80000` engaged flag + `ctrl+0x60` when no interactions remain.
Disc-gated: `engine-shell/tests/field_collision_discriminator.rs` (probe-model + engine-rest legs); unit equivalence `world.rs::tests::field_tile_is_wall_matches_retail_subcell_derivation` + standoff `leading_edge_wall_probes_rest_at_retail_standoff`. Capture note: both wall-press sessions park in `town0c` holding a grid that byte-matches town01's - **resolved, not an anomaly**: town0c's own `.MAP` (PROT 0019, the universal `define−2` resolution) is byte-identical to town01's; PROT 0028 is `izumi`'s map, not town0c's (see the field `.MAP` resolution row below).


### Field collision-map source

*Status:* resolved

The collision grid at `*(_DAT_1f8003ec) + 0x4000` (1 byte/128-unit tile, high nibble = 4 sub-cell wall bits) is **painted by the field-VM `0x4C` opcode, outer-nibble 7** (`op0` ∈ `0x70..0x7F`, handler `0x801e1c64`): a rectangular wall-paint with inline operands `[4C, 0x7s, col0, row0, col1, row1, mask]`, sub-op = clear-walkable / block-all / clear-mask / set-mask. So collision walls are authored in the scene event script (not a separate disc blob) - same inline-operand pattern as encounters / tile-board.

The `+0x4000` byte's **low nibble is a floor-elevation tier** - a 4-bit index into a 16-entry `short` height LUT at scratchpad `0x1f80035c`, filled at scene entry by `FUN_8003aeb0` from the MAN header (`_DAT_8007b898+2`, 16 negated values) and consumed by the object spawn iterator `FUN_8003a55c` to offset each placed object's Y. The `+0x8000` region is **not** a terrain-flag grid (corrected) - it is a per-tile `u16` object/attribute map (low 9 bits = object-record index into the `+0x0000` table; bit `0x400` = footprint flag ORed in by `FUN_8003aeb0` from field-pack records). See [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md#where-the-collision-grid-comes-from).

Residual sub-question: the `+0x4000` zero-init site (ruled out `FUN_8001f7c0` / `FUN_8003a024` / `FUN_800513f0`; likely a wholesale memset by the scene-boot allocator). Town01 parity confirmed by game-mode binding (Rim Elm = `town01` runs at mode `0x03`, same as the runtime-pinned field `map03`).


### Field `.MAP` PROT resolution - `define − 2`, universal

*Status:* resolved (census-pinned; engine resolver corrected)

A scene's field `.MAP` is its retail block's **first entry** - extraction index `define − 2`, because CDNAME defines are raw-TOC indices shifted `+2` from the extraction frame ([cdname.md](../formats/cdname.md#numbering-space)) - identified by its `0x12000` extended footprint, for **every** field scene. The per-entry extractor's shifted filename labels attribute it to the *previous* block's tail; in the unshifted engine windows of the era the first in-window `0x12000` entry was the **next** scene's map (the "in-block decoy"), which is what the census discriminated against. `Scene::load` now converts windows to the retail frame, so `Scene::field_map_index` is simply the block's first entry.

Pinned by a save-library census (`crates/engine-shell/examples/field_grid_census.rs`): each save's live field buffer (scratchpad `_DAT_1f8003ec` → `+0x4000` grid) classified against candidate on-disc bases. The `keikoku` sessions match PROT 0109 (`define 111 − 2`) with **zero** diffs while the in-block candidate 0118 differs by 3855 bytes; `koin3` matches 0559 exactly (in-block 0568 differs by 531); town01 sessions match 0010 ≡ 0001 exactly. A corpus sweep confirms the structure corpus-wide: every block's in-block `0x12000` hit is exactly the *next* block's `define − 2` entry.

The **object-index grid** (`+0x8000`, the `Scene::field_object_placements` / `field_terrain_tiles` source) is live-validated the same way: residuals of 0..96 bytes against the resolved entry across town01 / town0c / keikoku / koin3 sessions (story-conditional cell mutations - opened chests, prescript object toggles), thousands against every other candidate. Regression-guarded by the disc + save-library gated `engine-shell/tests/field_map_object_grid_live.rs`, which also re-falsifies the in-block rule against live RAM on the placement region for the discriminating scenes.

Consequences: (a) `Scene::field_map_index` now resolves `define − 2` (it previously picked the in-block entry - the **next scene's map** - for every field scene, masked only on town01 where the adjacent Rim Elm variants byte-copy, the one scene it had been validated against; `walk_field_map_index` is now an alias). (b) The town0c "cold `.MAP`" question **dissolves**: town0c's `.MAP` is PROT 0019, **byte-identical** to town01's (0001/0010) - the wall-press captures' "town01 buffer in a town0c session" is simply town0c's own map. (c) "PROT 0028 = town0c's different `.MAP`" is a misattribution - 0028 is `izumi`'s (`define 30 − 2`). (d) The kingdom "in-block decoy" framing is superseded: the decoy is the next scene's continent.


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


### Extraction-0874 §2 (`player.lzs`) F-variant pixels - pause-menu-lineage, not boot

*Status:* mostly resolved (corpus-pinned; exact writer PC needs a GPU trace)

The earlier "a freshly booted game holds the `0xFFFF` variant" premise is **refuted** by a full save-catalog bisect: the title screen holds the band all-zero (not yet uploaded), and the new-game field-entry load (the mode-2 `FUN_80025B64` → `FUN_801D6704` stage) uploads the **disc** bytes - the whole intro chain through name entry holds `0x3333`. The F-variant is **pause-menu-lineage** instead: every pause-menu capture (CARD-mode init from field, six of six) holds it, the casino prize shop (also mode `0x17`, but hosted via the dialog/door path rather than the pause-menu init) holds the disc bytes, and the first battle **effect use** (not battle entry itself) restores the disc value.

The variant is exactly **3 words** of row 271 (`(853, 271)` `3333→ffff`, `(856, 271)` `3333→fff3`, `(857, 271)` `1e33→1e3f`), each equal to the disc word two rows down at `(x, 273)` - both row variants are consecutive rows of the *same* disc TIM (no sibling copy involved). Remaining residue: the exact pause-menu-path writer (a `LoadImage`/draw trace would pin the PC - low value; the oracle handles the band via the cross-scene shared-band refinement).


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
The randomizer (`legaia_rando::house_door`) shuffles only these classified door warps, class-preserving (ＩＮ among ＩＮ, ＯＵＴ among ＯＵＴ) so every exit still lands outside; see [`randomizer.md`](../tooling/randomizer.md).

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

## Text / fonts / dialog

| Thread | Status | What would close it |
|---|---|---|
| Dialog font extraction | done - kept for reference | Earlier "blocked on runtime trace" framing was wrong; tile-page lives at VRAM `(896, 0)..(960, 256)`, extracted by `legaia-font::font-extract` from any in-game save state. The **on-disc carrier** (previously "unclassified") is now pinned too: a plain 4bpp TIM at `PROT.DAT` offset `0x7F40` (framebuffer `(896, 0)`, CLUT `(0, 510)`), so the font is decodable **without** a save state (`legaia_font::Font::from_disc_tim_and_scus`; the WASM site's pause menu uses it). Byte-verified vs the save-state extraction. Listed here only so the older "open" framing doesn't get re-opened. |
| Inline dialog-box format (`0x1F`-lead segments) | resolved | [details ↓](#inline-dialog-box-format-0x1f-lead-segments) |
| Tetsu 4-option spar menu mechanism | resolved | The menu is a standard `0x29` 4-option **MES inline picker** in the sparring partner's dialogue (cursor `*(0x801C6EA4)+0x0C`; confirming **index 2** "I want to practice with you." starts the spar - live `0x03->0x09->0x15`, driven by the dialog SM not the field VM). It uses the **immediate-labels** form (labels straight after the N jump entries, no continuation byte) - `parse_picker_at` rejected it, now fixed, so town01 decodes the spar menu + its other pickers. Engine: `World::CarrierMenu` presents the picker and engages the carrier only on the index-2 fight option (was any-accept). Tests: `parses_immediate_labels_picker`, `tetsu_spar_picker_disc`, `carrier_spar_menu_*`, the updated `training_battle` legs. |


### Inline dialog-box format (`0x1F`-lead segments)

*Status:* resolved - prologue + pager-side dispatch + option-list inner format + multi-segment box packing all pinned

Placement-NPC / event dialogue text is **inline** in the field-VM interaction record, **not** the scene MES - the opcode-decoded `text_id` is a box-config id that never resolves through `SceneMes::message_offset` (0/13 town01 placement-NPC ids resolve). The text is a run of `0x1F`-lead / `0x00`-terminated segments of MES glyph bytecode. It is recovered **structurally**, not from the `0x3F` op's `len` field: a text-heavy field interaction record desyncs under linear disassembly (a literal `>` is `0x3E`, the warp/interact opcode; ASCII punctuation hits the `0x37`/`0x41` yield bytes), so the decoded `0x3F` op and its `len` are unreliable on field scenes and the byte-`len` capture returned **empty for every town01 NPC**.

`man_field_scripts::first_inline_dialog_offset` finds the first printable `0x1F` segment (printable-ratio gated), `classify_placement` carries the record bytes from there as `PlacementKind::Npc::dialog_inline`, and `OwnedDialogPanel::from_inline_dialog` types the prompt segment; the native `play-window` renders the box. With this, **36 town01 placements recover renderable dialogue** (the sparring partner, Meta the dog, villagers, leftover "dummy" dev placeholders, and the `0x1F`-segment developer story-flag toggle menu at placement P1[1]).

**Segment-pool structure pinned:** the segments are **not** "prompt + option labels" of one box. `dialog::decode_inline_segments` recovers the full `0x1F`-lead pool, and decoding real town01 placements shows each record holds the NPC's *entire* dialogue line set - every line across every story-state branch, with `"Yes"`/`"No"` option labels interspersed (e.g. the Village Elder decodes to 80 segments, Val to 59, both carrying multiple `Yes`/`No` pairs; disc-gated `field_actor_placements_disc::inline_dialogue_decodes_into_full_segment_pool`). So `0x1F` segments are individual lines, *not* page-break-delimited boxes - multi-page speech is multiple `0x1F` segments, not `0x80..=0x9F` control bytes within one.

**There is NO separate "box-geometry header" format (falsified):** the bytes between the placement's `script_pc0` and the first `0x1F` are normal field-VM bytecode - `CFlag` / `SysFlag.Test` / `JmpRel` / `Nop` / `0x4C 0x51` NPC-move-to-tile / `0x4C 0x52` menu-activation poll - that runs as the NPC's interaction prologue (face the player, set conversation flags, walk to the talk position, branch on story flags).

The retail SM `FUN_80039B7C` state 0 calls the field-VM dispatcher `FUN_801DE840` directly on this stream and transitions into the pager only when the dispatcher leaves the actor's PC on a byte where `& 0x7F < 0x20` (a `0x1F` lead or `0x21` terminator); the "select which segment to start at" mechanism is the prologue's own story-flag-gated `SysFlag.Test` branches - the script `JmpRel`s past unwanted segments to the desired one.

Pinned by `field_disasm::LinearWalker` decoding the prologue cleanly across every classified town01 dialog NPC once nibble-5 sub-1/sub-2 are covered (disc-gated `field_actor_placements_disc::dialog_prefix_decodes_as_field_vm_bytecode`); the earlier "candidate decoder among `FUN_8003AB2C` / `FUN_8003BDE0`" framing is falsified - both are known: `FUN_8003AB2C` is the per-frame field-VM driver and `FUN_8003BDE0` is the partition-record dispatcher (both already ported).

**`FUN_8001ebec` is not the renderer** - disassembly shows it's a per-character TMD-pose copier (party slots 0..2, indexed by the slot-4 freeze flag `_DAT_8007B824`, copies 7 u32s of pose data from TMD offsets `+0x124..+0x140` or `+0x140..+0x158` gated on a record flag at `+0x75E`); the earlier reference to it as the dialog-box renderer in the engine + this thread is wrong (corrected in [`subsystems/script-vm.md`](../subsystems/script-vm.md) op `0x4C` sub-3 sub-F note). The real per-actor dialog SM is `FUN_80039b7c` (advances `actor[+0x9c]` 0→1→2 through `0x1F`-lead segments, consumes the `0xC?` 2-byte escapes); the pager is `FUN_801D84D0`.

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

| Thread | Status | What would close it |
|---|---|---|
| Player ANM per-record layout | resolved (container + per-`(bone, frame)` semantic) | [details ↓](#player-anm-per-record-layout) |
| Battle anim-id space + record[0] "strike family" | resolved | Anim ids are entry indices (commit `FUN_8004AD80`; idle id = `0`; `FUN_801D5854` ids 6..9 = a camera program space). Tags `2/3/4/5/0xB` = the hit-reaction family (`+0x1EF..+0x1F3` map; `FUN_800402F4` stages flinch/knockdown). Swings = the equipment-section splice (slots `0xC..0xF`) + dynamic art slots `0x10`/`0x11` from the `+0x58` art bank. Capture-pinned + disc census. See [monster-animation.md](../formats/monster-animation.md) / [battle-data-pack.md](../formats/battle-data-pack.md). |
| `FUN_80047430` caller | resolved | Live-captured (`autorun_anim_node_tick_caller.lua`, mid-battle save): a single dispatch site — `jalr v0` at `0x800252B4` inside `FUN_8002519C`, the per-frame actor-list tick iterator, calling the node's `+0x0C` handler slot with the node pointer in `a0`. The anim-node tick is an ordinary list-node tick handler; no other caller fired. See [functions.md](functions.md). |
| Record[0] `+0x5C` pointer + art-anim bank stream source | resolved (`+0x5C` = vestigial paired-relocation) | Art streams = `"ME"` archives (`me_archive`) in `readef.DAT` slots `3*char+1`/`3*char+2`; staging arm `FUN_80055B4C` driven by streamer `FUN_801F12D0`. **`+0x5C` reframed (was "capture-blocked"):** it is a self-relative pointer rebased to absolute at load, paired with `+0x58`, by `FUN_80052FA0` (`80052fa0.txt:558-561`). `+0x58` has a reader (`FUN_8004AD80:1492`); an exhaustive dump sweep finds NO `+0x5C` reader. So it is a vestigial paired-relocation field - a watchpoint would only prove deadness, not a blocker. See [battle-data-pack.md](../formats/battle-data-pack.md#me-stream-archives-readefdat). |


### Player ANM per-record layout

*Status:* resolved (container + per-`(bone, frame)` semantic)

The on-disc per-record body decodes byte-exact across **all 296 records** in the 5 pinned scenes (296 record / 5 scene corpus, plus every other scene's bundle the corpus sweep finds): `record_size = 16 + 8 × (a & 0xFF) × b`, where `a & 0xFF` is the **bone count** of the clip and `b` is the **frame count**. Layout: 8-byte `(a, b, marker_1=0x080C, flag)` header + 8-byte per-anim prologue + `b` frames × `bone_count` × 8 bytes per (bone, frame). Pinned by the disc-gated regression `crates/asset/tests/player_anm_real.rs` after the offset-convention fix (offsets in the offset table are **absolute** byte offsets, not relative to `+4` - earlier framing was wrong; size invariant now validates 296/296).

**Per-`(bone, frame)` 8-byte semantic - resolved** (the earlier "4 little-endian `i16`s, semantic open" framing is superseded): the entry is **not** four shorts but a **translation + rotation** pair, decoded exactly as the retail interpreter [`FUN_8001BE80`](../../ghidra/scripts/funcs/8001be80.txt) does - bytes 0..4 hold three **nibble-packed signed 12-bit translation** values `(t_x, t_y, t_z)` (byte 2 = `high4(t_y)<<4 | high4(t_x)`, byte 4 high nibble = `high4(t_z)`; sign-extend on bit 11), and bytes 5/6/7 are three **`u8` rotation angles** `(r_x, r_y, r_z)` each `<< 4` to a PSX 12-bit angle (`4096` = 360°), composed Z→Y→X via `FUN_8004638C`/`FUN_8004629C`/`FUN_800461A4`.

The piece poses `R·v + T` about its own object origin (no centroid subtraction); frame 0 of an idle clip is the rest pose. Decoder `legaia_asset::player_anm::BoneTransform::decode` mirrors the decompiled C, pinned by the byte-exact unit test `bone_transform_decode_signed_12bit` (town01 record 17). The site characters page applies the same `(t, r)` pipeline.

**Distinct ANM kind (not this one):** `FUN_80021DF4`'s `+0x5A == 6` block uses a separate 24-byte-per-bone keyframe layout - see [`anm.md`](../formats/anm.md).

## Audio

| Thread | Status | What would close it |
|---|---|---|
| SPU reverb live routing (C7-REVERB) | resolved (wired; Studio C, global) | [details ↓](#spu-reverb-live-routing-c7-reverb) |
| XA channel map / STR demux SM | resolved (static decompile of PROT 0970 + SCUS) | [details ↓](#xa-channel-map--str-demux-sm) |
| XA clip-table writer + `(clip_id, chan)` cue census | open | The `0x801C6ED8` clip-table content is pinned (34 `[CdlLOC][len]` slots = `XA1..XA34`, title-capture byte-exact vs the disc files), but its filler is a DMA/computed write - both `lui 0x801c` materialisation sites in SCUS are the readers (`FUN_8003D53C`/`FUN_8003EAE4`). A write-watchpoint at boot would pin the filler; a caller census of `FUN_8003D53C`/`FUN_8003EAE4` names each `(clip_id, chan)` cue. Decoded: menu voice `FUN_8004FCC8`; the normal-move grunt (`XA30` chan 0/4/6, overlay `0x801EEB44`); the **arts shout** (`FUN_8004C140` → `XA2`/`XA4`/`XA6` per character, per-art channel pool, capture-verified; [battle-action.md](../subsystems/battle-action.md)); SM state-`0x6E` (`XA9` via `0x800787AF`); slot machine `XA1`. |


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

| Thread | Status | What would close it |
|---|---|---|
| `title.pak` PROT entry | resolved | [details ↓](#titlepak-prot-entry) |
| Title screen mode-table PROT | resolved (no such entry) | [details ↓](#title-screen-mode-table-prot) |
| Load-screen panel 9-slice geometry | resolved (engine renders byte-perfect) | Pinned in [`subsystems/save-screen.md`](../subsystems/save-screen.md#pinned-9-slice-tile-rects-system-ui-tim-clut-row-2): retail composes the 81×29 panel at dst `(6, 4)` from 14 textured-sprite primitives (GP0 cmd `0x64`) sampling the system-UI sheet with CLUT `(32, 511)`. The exact per-tile rects are exported as `legaia_asset::title_pak::OVERLAY_SYSTEM_UI_PANEL_*` and emitted by `legaia_engine_render::save_select_chrome_draws_for` (covered by `save_select_chrome_emits_9slice_panel_and_pills` test). No interior fill sprite is drawn - the "marbled blue" look is the dimmed title art bleeding through the empty middle of the frame. |
| Debug flags `0x8007B8C2` / `0x8007B98F` | resolved (static) | `0x8007B98F` has no byte-granular reader: it is byte +3 (MSB, little-endian) of the 32-bit debug-mode word `_DAT_8007B98C`, and *that word* is the consumer surface (grep of `funcs/` for `8007b98f` = 0 hits; `8007b98c` gate-read at `8001822c.txt:500/533` + ~14 field-overlay-0897 gates, sole `sw` writer in the menu/title/save-init routine). Writing `0x8007B98F = 1` sets the MSB so every `_DAT_8007B98C != 0` gate reads active. [details ↓](#debug-flags-0x8007b8c2--0x8007b98f) |
| Key-item area consumers (`0x800859E8..0x80085A40`) | resolved (static; no OOB amplifier) | The range is inventory slots `>= 72` of `&DAT_80085958`; its readers are the indexed item-menu functions (`FUN_8002ff8c`/`800302e4`/`80032a44`/`80030628`/`80034250`), each masking the slot `& 0x3ff` and using the id byte as an index into 256-entry item tables - inherently bounded; add/find/consume helpers bound their scan by the live item count. No consumer treats a key-item byte as an unguarded index, so the range amplifies to game-state corruption (item possession + displayed ids), not a native index-OOB chain step. The `lb $reg,0x5aXX($zero)` overlay "hits" were mis-decoded data tables. Read-BP probe deprioritized. |
| XP-table source + reader | resolved + ported | [details ↓](#xp-table-source--reader) |
| New-Game opening chain + narration roller | partial (chain + caption + roller resolved; two render-fidelity residuals open) | [details ↓](#new-game-opening-chain--narration-roller) |
| Overlay identity from the disc (static extraction) | resolved (pipeline landed) | [details ↓](#overlay-identity-from-the-disc-static-extraction) |
| SCUS recomp gap - render/GTE + boot/init clusters | resolved (aliases + libgte residue + dev tooling; `main()` documented) | [details ↓](#scus-recomp-gap---rendergte--bootinit-clusters) |
| Options/menu overlay PROT entry | resolved (RAM-verified; PROT 0899 @ `0x801CE818`) | The options/pause/inventory-equipment-status menu overlay is **PROT 0899**, not 0896: `FUN_801CF650`'s signature byte-matches PROT 0899 file `0xe38`, and the `.text`+`.rodata` prefix is byte-identical across six menu-open saves. VA-alias sibling of the field overlay 0897 in slot A - the menu overlay replaces the field overlay at the base. The earlier "0896 = menu" label is falsified. |
| PROT 0896 (`bat_back_dat`) identity | resolved | The unique ~`0x9000`-byte head is the **vestigial Japanese-build field-menu / config / status overlay** - the debug-string sibling of the English retail menu overlay PROT 0899 (same `~0x801D0000` window-renderer VA family, a `"FWIN ERR %d"` printf at file `0x3D4`, `0x414`-byte char-record indexing). 0899 ships the English label set with zero `FWIN`; a signature scan finds 0896 resident in **0** of 140 states (control: English "Battle Voices" resident in 10), so the USA build never loads it. [details ↓](#prot-0896-bat_back_dat-identity) |
| Slot-A scene-overlay family beyond field/battle/menu | resolved (in the static map) | The rest of the slot-A (`0x801CE818`) VA-alias family is pinned from the disc: **0970 cutscene_str** (STR/MDEC FMV, modes 26/27) and the minigame overlays **0972 fishing / 0975 slot_machine / 0976 baka_fighter / 0980 dance** (the mode-24 `0x3E` door-warp sub-id slots 0/3/4/6), each cross-checked by a documented function landing on a prologue at the base. Minigame entries over-read each other (phantom-base risk); the canonical entry recovers `0x801CE818` and is the entry the warp streams (the historical "slot_machine = 0973 @ `0x801CA818`" was the phantom - the image inside 0973's over-read tail). Found via `asset overlay scan` + the leading dev string. |
| "world-map / save / shop" overlay PROT entries | resolved (not separate entries) | The world-map / overworld controller `FUN_801E76D4` lives in the **field overlay 0897** (base+0x18EBC), and the save-slot dispatcher `FUN_801DC6B4` + the shop/buy session live in the **menu overlay 0899** (save at base+0xDE9C) - each function's instruction signature byte-matches only that one entry (`asset overlay find-sig`). So "world-map", "save", and "shop" are *subsystems* of existing slot-A overlays, not separate PROT entries; recorded in the 0897 / 0899 map notes. |
| Slot-B overlay cluster (`0900..0969`) per-entry identity | mostly resolved | [details ↓](#slot-b-overlay-cluster-09000969-per-entry-identity) |


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


### `title.pak` PROT entry

*Status:* resolved

There is no single `title.pak` bundle entry - the dev-tree `title.pak` content is split across two PROT entries, both confirmed by the init.pak fingerprint method now that a title-phase RAM snapshot exists (`title_screen_new_game` save state): the **title wordmark TIM** is **PROT 888/890** (`sound_data2`; already parsed by `legaia_asset::title_pak`, the big-logo RAM TIM at `0x80170DF8` fingerprint-matches it),

and the **options/config-menu bundle** is **PROT 899** (`xxx_dat`) - its indexed payload opens with the config-menu string pool ("Display Off / Gradual / Immediate / Field HP Display / Encounters / Vibration / Dual Shock / Voices / Battle Camera / Monaural / Stereo …") followed by the small config TIMs (the four RAM TIMs at `0x8010FEF0..0x80110130`, CLUTs byte-matched at 899 offsets `0x169DC` / `0x1F91C`+), with the title-overlay *code* in the trailing unindexed gap after entry 899 (see [[title-overlay-source-pinned]]). Same CDNAME-mislabel pattern as `0895_bat_back_dat` = init.pak.


### Title screen mode-table PROT

*Status:* resolved (no such entry)

**The premise is wrong**: there is no title-screen entry in the 28-entry mode table at `0x8007078C`. Per [`subsystems/boot.md`](../subsystems/boot.md#title-screen-is-not-in-the-mode-table) the title overlay is loaded by a **pre-mode-dispatch boot routine** ahead of the mode table being consulted at all - its tick `FUN_801DD35C` lives in the unindexed 60-sector PROT.DAT gap between TOC entries 899 and 900 ([`legaia_asset::title_pak`](https://github.com/altimit-mii/legend-of-legaia-re/tree/main/crates/asset/src/title_pak.rs) reads the wordmark TIM out of PROT 888/890; PROT 899 carries the options-menu config bundle). NEW GAME is how control crosses from the title overlay into the mode table at mode 2. Row kept so the "title entry is unresolved" framing isn't re-opened.


### Debug flags `0x8007B8C2` / `0x8007B98F`

*Status:* resolved (static) - `_DAT_8007B8C2` resolved (read-only retail-mode selector); `_DAT_8007B98F` is the MSB of the debug-mode word `_DAT_8007B98C`, whose consumer is statically pinned (`FUN_8001822c` + the resident field-overlay gates), no capture required

Both addresses are in the SBSS/BSS region (zero-initialised at boot). `_DAT_8007B8C2` is the dev/retail asset-load selector: `FUN_8003E360`'s dual-mode loader pattern routes through ISO9660 when `_DAT_8007B8C2 == 0` (retail) and through the PROT-index loader when non-zero (dev); same shape at `FUN_8001FA88` / `FUN_8001FC00` (sound) and `FUN_8001F7C0` (per-scene field-asset loader). Zero code writers across the full corpus - BSS init establishes the retail config.

**Exhaustive corpus sweep (2661 dump files across SCUS + every captured overlay) confirms zero writes to `_DAT_8007B8C2` and zero references - read or write - to `_DAT_8007B98F`.** `_DAT_8007B8C2` is read-only at runtime (10+ `== 0` retail-mode tests, no writers anywhere).

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


### New-Game opening chain + narration roller

*Status:* partial - the chain, caption, and roller are resolved; two render-fidelity residuals below are open

**The opening is a five-scene chain, live-probe + pixel-capture pinned** - `opdeene` → `opstati` → `opurud` → `map01` → `town01`, all master mode 3, zero input; the `FUN_801D1344` `town01` packet is the **intro skip** (its earlier reading as the required hand-off gate is superseded). Each leg's record spawn is pinned (exec-BP on `FUN_8003BDE0`, exactly 5 hits): op `0x44` SPAWN_RECORD in the first three legs' entry scripts (the old op-`0x44` "COUNTER" reading is superseded), the walk-on tile trigger (`FUN_801D1EC4` → `FUN_801D5630`) for `map01`/`town01`. Full mechanics: [`cutscene.md`](../subsystems/cutscene.md#in-engine-3d-opening-the-five-scene-new-game-chain).

**The narration is a bottom-up scrolling crawl** (roller actor `FUN_80037174`, spawned as a **child context** so the parent timeline keeps executing and the between-block camera cuts play under the scroll; per-scene capture-pinned geometry/speed), not a one-caption-at-a-time presenter - the prior one-line model described the separate `4C E1` balloon op (`FUN_8003C764` / `FUN_801DA7F0`) and is superseded. A cold-boot crawl-1 capture (`scripts/pcsx-redux/autorun_crawl1_capture.lua`) confirms the eye cuts through the Genesis-grove foliage to the villager tableau *while* the creation crawl scrolls; the engine ports this as a non-blocking crawl (blocking only the last block of a scene before its terminal SceneChange).
The name-entry auto-open stays pinned: op `0x49` STATE_RESUME sub-op 3 at town01 P2[3] body offset `0x02c6` (`_DAT_8007B450` parks there while name entry is up); the retail town01 order is establishing pan → name entry → Vahn's walk-out.
The op-`0x45` camera param→global map, the GTE rotation build (`FUN_8001CF50`), the per-frame ease (`FUN_801DB510`), and the eye-back depth (the offset-trio slot 5, `0x800840B8` - no separate eye-distance scalar) are all pinned; `play-window` renders through `psx_camera_mvp`.

**Data-source sub-threads - both resolved:**

1. **The *"It was the Seru."* caption's data source - it is not text.** The caption is a **pre-rendered 112×32 4bpp TIM** (two CLUT palettes = the fade steps) baked into the `opdeene` geometry pack **PROT entry 0749** at LZS-decoded offset `0x01EC30` (VRAM `fb=(384,0)`), drawn by the scene renderer as a screen-space textured quad - not a `4C E1` balloon, not a MES id, not any font string. Pinned by cold-boot probes (`autorun_text_census.lua` + `autorun_seru_blit_probe.lua` + a full-RAM dump): every UI text/image draw path fires **zero** times in the caption window and the string is in RAM in **no** encoding. `tim-scan extracted/PROT/0749_opdeene.BIN` renders it. See [`cutscene.md`](../subsystems/cutscene.md#narration-playback---the-crawl-roller-fun_80037174).
2. **The retail roller config op's parameter decode - decoded (Ghidra-traced).** Two sub-ops of field-VM op `0x4C`: the spawner `CC F8 80 N` (`N` = page count) allocates the roller child on `FUN_80037174`, and `CC F8 E8 …` (four signed-16 LE words) seeds the per-scene crawl globals at `_DAT_801C6EA4`: `+0x4C` = window top Y, `+0x4E` = visible line count, `+0x50` = scroll-cadence divisor (`word3` selects seed/pause/resume/kill). The earlier `4C 88`-shaped label was a **mis-attribution** (op0 `0x88` writes `_DAT_80084628/…`, not the crawl geometry; the seed is the nibble-`E` sub-8 `0xE8` form). So `RollerParams::for_scene` is derivable from the scene bytecode, not just the pixel capture. Full decode in [`cutscene.md`](../subsystems/cutscene.md#roller-op-operands-ghidra-traced).

**Render-fidelity residuals (open):**

- **Prologue per-node depth-cue crush.** The engine's sepia grade is the retail modulation multiply (`(1.0, 0.94, 0.43)`, see [`cutscene.md`](../subsystems/cutscene.md#full-scene-sepia-grade-the-gold-prologue-look)); retail additionally pulls each render node toward the gold far colour by its per-node `IR0` (`+0x74`/`+0x78`), so far-field blue crushes to `B/R 0.12..0.18` where the engine holds the uniform `~0.31..0.36`. Closing it means staging the DPCS depth cue per mesh (the engine's `psx_depth_cue` uniform already exists but is scene-global and identity); the next step is a per-mesh `IR0` computed from view depth in the field render loop, gated on the prologue grade.
- **Tableau ground texture chroma.** In the retail `opdeene` villager-tableau framebuffer the ground is warm grey-brown (`G/R 0.88`); the engine draws the same beat's ground vivid green (`G/R ~1.07` after the tint), so the whole-frame ratio diverges. The `0749` pack carries both a green 256×256 page and a neutral-grey one - the engine likely resolves a different page/CLUT for the ground quads than retail's draw list binds. Next step: dump the engine's texture binding for the tableau ground mesh and compare against the retail GP0 texpage/CLUT words for the neutral-modulation `0x2C` quads.


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
   path either.

What would close it: a consumer - any retail moment where the head bytes are
resident (offline check:
[`overlay_residency.py`](../../scripts/pcsx-redux/overlay_residency.py)
against new captures), or an overlay-resident loader call with a computed
param reaching 1 (the static SCUS census above rules out the constant-param
sites).


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
  inline. Table: [`functions.md` § libgte primitives](functions.md#libgte-primitives);
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
  degenerate until force-created); and `FUN_8004CE30`, the largest undumped SCUS
  function - the per-frame battle actor maintenance pass
  ([`battle.md` § Per-frame actor maintenance](../subsystems/battle.md#per-frame-actor-maintenance-fun_8004ce30)),
  **not** a mode dispatcher.
- **Still open from the same inventory:** the general-game band (never
  per-address catalogued), headed by `0x8002A9F8` (2.2 KB table-driven logic,
  no static caller), `0x80056208` (libgpu-band SCUS→overlay bridge into
  `0x801F69xx`), `0x8004DC68`, `0x8002149C`, `0x80036D80`, `0x80059E10`,
  `0x80025DA4`. Next step: behavior-read each against its `0x8007xxxx`/`gp`
  globals the way this thread's entries were closed. The PsyQ sound-driver
  cluster is tracked separately under Audio.

A thread belongs here when:

1. There is something *specific* that would close it - a probe to run, a dump to read, a function to port. "Generally understand X better" is not closable; skip.
2. The next step is non-obvious from the code or git log. If `grep` would surface it, no row needed.
3. The detail lives elsewhere (a memory entry, a docs page, a Ghidra dump). The row is the pointer, not the analysis.

When the thread closes, rewrite the row to a `falsified` or `done - kept for reference` line if the path was instructive enough to warrant a "do not re-walk" marker; otherwise delete the row. Rotating the page is part of using it.

## Related pages

- [`docs/tooling/port-catalog.md`](../tooling/port-catalog.md) - per-function dumped × documented × ported × ignored axes. `port-catalog.py --missing-ports` is the function-level companion to this page's question-level index.
- [`docs/reference/functions.md`](functions.md) - canonical function directory; the place to learn what a `FUN_<addr>` mentioned in a row actually does.
- [`scripts/ci/port-catalog-ignore.toml`](../../scripts/ci/port-catalog-ignore.toml) - addresses explicitly *not* worth investigating (statically-linked PsyQ infra). Disjoint from this page.
