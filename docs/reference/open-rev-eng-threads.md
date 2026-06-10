# Open reverse-engineering threads

An index of still-open hunts and the negative findings worth not re-walking. Rows are *questions*, not progress markers — each entry describes what is settled, what remains, and what would close it. Closing a thread removes or rewrites the row; nothing here counts ports, tests, or coverage.

Use this page to find what's worth digging into next. The detailed write-ups, captures, and decompiler dumps live in the per-topic memory files (`~/.claude/projects/.../memory/project_<slug>.md`) and in the linked docs.

Status conventions:

- **open** — active hunt; concrete next step exists.
- **partial** — main result pinned; a residual sub-question remains.
- **falsified** — hypothesis disproved; row kept so the path isn't re-walked.

Threads whose write-up is too long for a table cell keep a one-line row in the section table with a **[details ↓]** link to a `###` section immediately after that section's table; the full analysis (every address, capture, and falsification) lives there.

---

## World map / kingdom bundles

| Thread | Status | What would close it | Memory |
|---|---|---|---|
| Kingdom slot 4 — per-record semantic | open (next step = the transcode, not the handlers) | [details ↓](#kingdom-slot-4--per-record-semantic) | `project_slot4_is_wireframe_not_terrain.md` |
| Slot-4 → cluster-A converter site | falsified | There is no slot-4 → cluster-A converter. The cluster-A pool (`DAT_8007C018`) is filled exclusively by `FUN_80026B4C`, reached only from `FUN_8001f05c` **case `0x02`** (TMD pack) and **case `0x09`** (bare TMD). Slot-4's type byte is **`0x05`**, whose `FUN_8001f05c` case merely allocates the MOVE buffer `_DAT_8007B888` and never calls `FUN_80026B4C`. So slot-4 bytes never become cluster-A TMDs; the `DAT_8007C018` kingdom entries are the scene's own type-`0x02` field-file TMD pack(s), installed by the single `FUN_80020224` descriptor-walk. | `project_world_map_native_render.md` |
| World-map walk-view continent ground render | resolved | [details ↓](#world-map-walk-view-continent-ground-render) | `project_overworld_walk_pool_pinned.md` |
| `DAT_8007C018[45..53]` mid-load vertex-pool pointers | open | Single Lua write-watchpoint capture on `0x8007C018 + 45*4` during scene load to disambiguate stale-pointer vs. live-data. Steady-state model says reads past `DAT_8007BB38` are stale and never consumed; the mid-load snapshot deserves direct confirmation. | `project_dat_8007c018_global_tmd_table.md` |
| PROT 0874 section-0 outer producer | pinned | The per-scene field initializer `FUN_801D6704` drives the pool fill: `FUN_80020118` (resets `DAT_8007B6F8`=0, loads the party/character meshes via `FUN_8001E890` → `DAT_8007C018[0..4]`) then `FUN_80020224` (walks the scene's main field file — streamed into `_DAT_8007b85c` by `FUN_8001eef0` — dispatching every descriptor through `FUN_8001F05C`; the type-`0x02` packs install into `DAT_8007C018[5..]`). The char-mesh path is `FUN_8001E890` (the PROT 874 §0 producer), the scene-pack path is `FUN_80020224`; both feed `FUN_80026B4C`. | `project_world_map_native_render.md` + `project_global_tmd_pool_source.md` |
| Drake uncapped cluster-A totals | open | Re-run `autorun_slot4_dispatcher_args.lua` with `LEGAIA_PC_CAP=50000` and a `timeout --kill-after=30s 1500s` wrapper. Drake saturated 7 of 9 PCs at PC_CAP=5000; raising the cap closes the cross-kingdom delta table. | `project_open_work_slot4_cluster_a.md` |
| Slot-4 freeze flag `_DAT_8007B824` | open | Write-breakpoint probe on `_DAT_8007B824` during retail play. Either an undumped overlay sets the freeze flag, or the BSS-init zero holds through retail and the "persistent slots" semantic is vestigial. | `project_open_work_slot4_cluster_a.md` |
| World-map outline / coastline reading | falsified | Visual inspection plus the slot-4 record-semantic work refuted the "world-map overlay outlines / coastline wireframe" interpretation. Bodies are most likely small object-local 3D meshes; treat any future "kingdom border lines" claim with suspicion. | `project_slot4_is_wireframe_not_terrain.md` |


### Kingdom slot 4 — per-record semantic

*Status:* **consumer pinned — slot-4 is read in place, no transcode** (Drake capture); residual = the per-record field semantic

The **consumer is fully decoded** ([`world-map-overlay.md`](../formats/world-map-overlay.md#cluster-a-internals)): `FUN_80043390` walks an 8-byte-header **command stream** (`kind` = bits 17–31, `count` = bits 0–15), tail-calling per-`kind` GTE primitive emitters (kinds 8–19 across 4 banks via the `0x8007657C` table; each reads two packed vertex indices per word `& 0x7FF8` into a vertex pool and emits a `POLY_F3/G3/G4/GT3/GT4` GP0 packet — dispatcher + the kind-12 flat-triangle handler spot-verified against `ghidra/scripts/funcs/{80043390,slot4_k12_bank0_80043658}.txt`).

**The handlers read the slot-4 RAM payload IN PLACE — there is no transcode.** A Drake warp capture (`scripts/pcsx-redux/autorun_slot4_source_map.lua`; 365 rows) shows 363 reads of the slot-4 window with the cluster-A GTE prim path (`0x80044C70 = lw …,0x10(a1); … andi …,0x7FF8`, the exact packed-vertex-index extraction) holding slot-4 pointers in `a1`/`a2` (`0x8011A608`, `0x80121614`, …), under return addresses `0x801F78D4` (the world-map top-view overlay renderer, 276 reads) and `0x8001BC8C` (SCUS render, 78). The streaming-chunk processor `FUN_8001E54C` fired only twice and on a non-slot-4 buffer (`0x80184BD0`). So the earlier "`FUN_8001E54C` distributes the slot-4 records into a working buffer the handlers walk" reading is **falsified**:
the slot-4 sub-body payloads *are* the command stream + vertex pool, walked directly. (The working-buffer writers the prior hunt saw — `FUN_80028158` at `0x801BA000` — are unrelated procedural meshes, as that hunt already found.)

**Cross-kingdom: confirmed.** The slot-4 resident base is byte-pinned for all three kingdoms (Drake `0x8011A624`, Sebucus `0x80119CE4`, Karisto `0x80108D84` — it varies per kingdom; `locate_slot4_base.py` matches the disc payload against a post-warp RAM dump, all bodies unanimous). Re-read against the correct Sebucus base, 171/177 of the Sebucus `slot4_source_map` reads land inside the verified window — in-place there too.

**Per-record semantic — decoded.** Each 8-byte record is a **GTE vertex**: the per-kind handler `FUN_80044c14` loads a record's two words into the GTE vertex registers (`VXYn = x | y<<16`, `VZn = z`) and `RTPT`-transforms them, so `x/y/z` are model-space coordinates (the parser's field layout is confirmed) and `attr` (the `VZn` word's high half) is **not** a coordinate. Each body is an object-local vertex pool; the triangle topology lives in a separate cluster-A command stream that indexes the pool by byte offset (`& 0x7ff8`). The transcode question is closed (there is none — the pool is read in place).

**`kind` + `attr` — characterized (consumer is the open tail).** `kind` (1/2/4) tags a body's class/scope: hashing bodies across kingdoms shows `kind 1` = the three leading bodies, **byte-identical across all three kingdoms** (a shared universal mesh set); `kind 2` = full-3D kingdom objects (one cluster also globally shared, others shared between kingdom pairs); `kind 4` ⟺ `flag_a = 1` (widest-extent meshes). So slot 4 is a per-kingdom assembly from a shared mesh library + kingdom-specific bodies. `attr` is genuinely per-vertex (not per-group), **not** position-correlated (`corr ≈ 0.1`), varies smoothly across groups, and rides the unused `VZn` high half — a real per-vertex attribute read by some non-render path.
**`kind`/`count` consumer — pinned.** A Read-watchpoint on body 0's header during the Drake warp catches the cluster-A handler chain reading it **in place**: `ra = 0x801F78D4` (the world-map renderer), PC `0x8004568C`/`0x800456F4` (`FUN_80045584`), record pointers also in slot-4. The handler reads `count`/`kind` and `andi 0x40`-tests a header bit. So there is **no separate command-stream builder** — each slot-4 body is a self-contained render packet (header + indexed vertex records) walked in place (the `FUN_8001ada4` → `FUN_80058490` candidate was falsified: `FUN_80058490` is a libgpu `MoveImage`). **`attr`** is render-unused — a full sweep of the cluster-A handler family (`FUN_80043658`..`FUN_80045988`) confirms every `>> 0x10` is a vertex-index extraction or output-packet write,
none reading the pool `word1` high half. So `attr` (real per-vertex data) is ignored by the entire world-map render path — reserved/authoring data or a non-render-subsystem consumer; nothing in the render family reads it.


### World-map walk-view continent ground render

*Status:* resolved — heightfield geometry + per-cell terrain-type-keyed multi-page texturing (tile=`+0x14`, page=`+0x15`, clut=`+0x16`), shipped in engine

**The continent ground is a procedural heightfield, NOT instanced meshes** — confirmed by **`FUN_80019278`** (SCUS, always-resident, no overlay aliasing): the bilinear ground-height sampler reads an entity's XZ, gates on the object-grid `0x1000` cell bit, and **bilinearly interpolates** the floor height from the 2×2 block of `+0x4000` nibbles (`grid[0],[1],[0x80],[0x81]`, each `& 0xf` → `DAT_1f80035c[nibble]` LUT, weighted by the sub-tile position, `>>0xe`). So the `+0x4000` grid is terrain elevation and the `0x1000` continent is a smooth heightfield surface.

**The slot-1 pack meshes are ONLY the sparse placed landmarks** (`pool = record[+0x10] + prefix`, resolved 14/14 against the live render list via `FUN_8001ADA4` case 5 / `FUN_80024d78` / `FUN_80020f88`; spawned by `FUN_8003A55C`, gated on `flags & 0x4`, ~6 objects → pools 36/34/11/7/19/21). The `0x1000`-gated bulk cells are heightfield ground, not pack-mesh draws.

**`.MAP` source — raw (no compression):** the walk `.MAP` records+grid is a raw `0x10000` region at PROT.DAT `0x655800` (the loader's retail branch resolves it by PROT index `*(0x80084540) = 0x55 = 85` → `toc[87] = 3243` → `0x655800`; the per-entry extractor mis-slices it — its `0085_map01.BIN` count=46 pack at `0x668000` is the field object/script pack, and the real `.MAP` is under the overlapping manifest entry 83).

**Engine: heightfield geometry + grass texturing built** (`build_walk_heightfield` / `Scene::walk_heightfield` — quad per `0x1000` cell, corner Y from the `+0x4000` LUT; renders as coherent rolling terrain, verified vs disc).

**Ground texturing — per-cell multi-page atlas PINNED + shipped:** the walk-view ground is per-cell `POLY_FT4` (cmd `0x2C`) quads, one `32×32` quad per visible cell, emitted in a row-major world-cell sweep. The texture is selected **per cell** from the cell's object-record `+0x14..+0x18` run: `+0x14` = `8×8` atlas tile index (`u=(id%8)×32`, `v=(id/8)×32`), `+0x15` = PSX `tpage` (the terrain VRAM page / type: `0x1A` grass, `0x0C` mountain, `0x1B`/`0x1C` water, `0x0B` forest), `+0x16..+0x18` = PSX `clut` word. Verified by aligning each quad run's UV→tile sequence to the `.MAP`'s `+0x14` grid (`scripts/analyze-walk-ground-tiles.py --verify-rule`): tile/page/clut match the record **100%** across mountain + coast captures.

Engine bakes per-cell UV + `[clut,tpage]` in `build_walk_heightfield` (`WalkHeightfield::uvs` / `::cba_tsb`).

**Falsified:** (a) the "continent is per-cell instanced *meshes*" model — the bulk `0x1000` cells carry `+0x10 == 0`. (b) the earlier **"single `0x1A` grass page, positional `(col%3,row%3)`, `+0x14` unused metadata"** reading — a misread: grass cells use page `0x1A` with `+0x14` landing in the atlas's top-left `3×3` block, so the mod-3 sequence was coincidental; `+0x14` IS the tile selector and `+0x15`/`+0x16` carry the page/palette. (The static-decomp consumer sweep missed the per-cell terrain renderer, which is overlay-resident and aliased at `0x801F76xx`.) (c) A combined walk+overview mesh pool — 0085's and 0093's slot-0 atlases target the *same* VRAM pages, so they are mutually-exclusive sets that clobber each other if co-loaded.

## Battle / arts / level-up

| Thread | Status | What would close it | Memory |
|---|---|---|---|
| Encounter record carrier | resolved (no array) | Decompile of `FUN_801DE840` shows install handlers all use `pbVar43 = param_1 + param_2` (current opcode pointer in the field-VM script bytecode); each scripted encounter is its own dispatcher-op site (cases `0x37`/`0x41`, `0x38`, `0x43`, `0x47`, `0x4C`), with monster count/ids inline as the trailing operand bytes. No separate encounter-record table on disc. See `docs/formats/encounter.md` writer table. | `project_encounter_record_format.md` |
| Random-encounter trigger path | resolved | `FUN_801D9E1C` is the per-step roll function (rate counter `_DAT_8007B5FC`, scaled by config `_DAT_8007B5F8`). On counter underflow it picks a formation from the matching region's RNG range and installs `actor[+0x94] = formation_table_base + 1 + id * stride`, raises bit `0x80000`. Per-scene control block at `_DAT_801C6EA4 + 0x20/0x24/0x28` is populated by `FUN_8003A110` ("Mesworks set encount group table") from the MAN asset (type 0x03) buffered at `_DAT_8007B898`. See `docs/formats/encounter.md` § "Random-encounter trigger path". | `project_random_encounter_trigger_path.md` |
| Encounter MAN sub-section layout | resolved | [details ↓](#encounter-man-sub-section-layout) | `project_man_section_decoded.md` |
| Super / Miracle Arts trigger logic | partial | [details ↓](#super--miracle-arts-trigger-logic) | `project_arts_system.md` |
| Seru-magic summon visual (e.g. Tail Fire) | **player visual RESOLVED + WIRED** | [details ↓](#seru-magic-summon-visual-eg-tail-fire) | `project_effect_pool_draw_bridge.md` |
| Monster steal item (Evil God Icon) | RESOLVED | [details ↓](#monster-steal-item-evil-god-icon) | `project_steal_item_field.md` |
| Per-spell magic power / multiplier | **mechanism RESOLVED + roll PORTED** | [details ↓](#per-spell-magic-power--multiplier) | `project_spell_table_pinned.md`, `project_move_power_special_attack_only.md` |
| Arts command sequence — independent source | resolved | The SCUS arts-name table (`DAT_80075EC4`) glyph string is byte-exact ground truth for every art's directional command; `legaia_art::ArtsOracle` exposes it, and disc-gated contract tests validate both the best-effort PROT `0x05C4` `parse_record` command-decode and the curated gamedata `directions`/`ap` columns against it (one documented walkthrough error: Hyper Elbow). | `project_arts_name_table_pinned.md` |
| Stat growth-rate source | RESOLVED + validated + WIRED (core + opt-in jitter) | [details ↓](#stat-growth-rate-source) | `project_shop_ui_and_levelup.md` |
| Monster stat-record archive source | resolved | [details ↓](#monster-stat-record-archive-source) | `project_monster_stat_archive.md` |
| Monster mesh + texture pool | resolved | [details ↓](#monster-mesh--texture-pool) | `project_monster_mesh_source.md` |
| Terra slot-3 / story-flag overlap | resolved | [details ↓](#terra-slot-3--story-flag-overlap) | `project_char_record_name_offset.md` |
| Navmesh / per-scene navigation data | falsified | `0x80108EA4..0x80109550` is per-scene GPU primitive scratch, not a 24-byte stride navmesh. Pointer hunts find zero RAM cells pointing into the window. Real per-scene region / collision / event-trigger data lives in the field-file preamble (a count + `u16` offset table + records — **not** the field-pack schema slots, which are a global-constant template; see [field-pack](../formats/field-pack.md)); the collision grid is the `+0x4000` MAP region; the encounter-record path lives at `actor[+0x94]`. | `project_navmesh_negative_finding.md` |
| Battle party mesh pack `other5` = **PROT 1204** (battle form; Baka Fighter reuses it) | resolved (empirical) | [details ↓](#battle-party-mesh-pack-other5--prot-1204-battle-form-baka-fighter-reuses-it) | `project_battle_char_pack_is_prot1204.md` (+ archived `project_prot1204_atlases_are_real.md`) |
| MP-cost ability-bit priority (half vs quarter) | resolved (dump-confirmed) | [details ↓](#mp-cost-ability-bit-priority-half-vs-quarter) | `project_re_and_engine_batch_day_branch.md` |
| Scripted Tetsu encounter → Battle (v0.1 oracle Battle leg) | mostly | [details ↓](#scripted-tetsu-encounter--battle-v01-oracle-battle-leg) | `project_v0_1_oracle_phase1.md` |
| Item-effect table — class/targeting + literal amounts | resolved (descriptor table AND amounts are static disc data) | Class/tier/targeting/usability is the static `DAT_800752C0` table; the restore *amounts* are **also static** (apply handler `FUN_800402F4`, tier-indexed HP/MP heal tables, character-relative Seru-heal + `max_hp*0.4 + rand` revive) — not overlay-resident. Parser `legaia_asset::item_effect`; disc-gated `item_effect_real` + `item_catalog_disc` pin it; usability + all-party flags wired (`ItemCatalog::apply_effect_flags`). Full byte-level write-up in [`item-effect-table.md`](../formats/item-effect-table.md). | `project_item_effect_table.md` |
| Equipment stat-bonus table — slot model | **slot model resolved**; accessory/Goods system is the distinct open thread | The stat-bonus table (`DAT_80074F68`, 8-byte stride) is decoded from `FUN_801CF650`/`FUN_801CF5D0` (`legaia_asset::equip_stats`): `+0`=INT, `+1`=ATK, `+2`=UDF, `+3`=LDF, `+4`=SPD (the earlier AGL/evasion reading is falsified). Slot model resolved — the four `+7` categories are Legaia's four weapon/armour slots (body/head/footwear exact by name; none of the 77 accessories appear in this table). WIRED: `DiscEquipInfo` gates `EquipSession`'s per-character list. Still open (distinct thread): where the accessory/Goods records live + how they equip. See [`equipment-table.md`](../formats/equipment-table.md). | `project_equipment_table.md` |


### Encounter MAN sub-section layout

*Status:* resolved

`FUN_8003AEB0` is fully decoded: it walks the MAN multi-section header (sections at MAN offsets `+0x22, +0x24, +0x26, +0x28`, signed 16-bit LE) and `legaia_engine_core::encounter_man::scene_encounter_from_man` reads the encounter section straight from disc bytes, wiring per-scene `EncounterTable`s for the standalone towns + kingdom-bundle scenes (the `count = 6` MAN form is now resolved by `find_bundle`). The region-table section is the per-scene control block `_DAT_801c6ea4 + 0x4` count-prefixed array of 18-byte records: `byte[0]` kind selector, `bytes[1..4]` tile-space bounding box `[minX, minZ, maxX, maxZ]` queried by `FUN_801dba20(tileX, tileZ)` (`tile = (player_pos - 0x40) >> 7`), `bytes[5..17]` payload (sub-split still open),

consumed by the field camera arrival handler `FUN_801dbec4` + camera-config `FUN_801dbc20`. Residual: the world-overview actor-placement section `FUN_8003A1E4` consumes is decoded separately (see world-overview threads).


### Super / Miracle Arts trigger logic

*Status:* partial

The find/replace matcher **is** ported (`legaia_art::{MiracleMatcher,SuperMatcher}`, applied by `legaia_engine_vm::battle_action::resolve_action_queue`).

**Miracle is now wired into the live player-driven Arts submenu**: `battle_arts::miracle_for_chain` flags a saved chain whose directional string is the caster's Miracle Art, and `World::build_battle_arts_rows` resolves the finisher-replacement queue into a per-strike profile (real `ArtRecord` power where staged, synthetic `x12` per component art otherwise).

**Super is now wired into the live submenu, with the queue connectors abstracted.** `legaia_art::recognize_art_sequence` tokenizes a saved chain's flat directional string into its ordered named arts (each identified by its own `ArtRecord::commands`), and `SuperMatcher::trigger_by_art_sequence` tail-matches that ordering against each Super's `SuperArt::art_sequence()` — the `find` pattern projected to art constants only (`[0x27,0x1F,0x27]` for Tri-Somersault), with starters + connectors stripped. `battle_arts::super_for_chain` / `World::build_battle_arts_rows` flag the row (`ArtRow::super_art`) and resolve the `replace`-queue strike profile (shared with the Miracle path).

**What stays open:** the *byte-exact* queue connectors. The connector direction after each art is **combo-specific** (Vahn's `0x27` → `0F` in Tri-Somersault but `0E` in Power Slash), so it can't be derived from each art's commands; the runtime queue-builder that emits them (`ctx[+0x274]`) is **unpinned** (no queue-write watchpoint trace yet). The live match is therefore faithful to *which* combination triggers *which* Super but does not reproduce the literal queue bytes. The byte-exact matcher (`SuperMatcher::try_trigger_at_tail`) is also ported, exercised by `resolve_action_queue`. See `docs/subsystems/battle-action.md` § "Miracle / Super in the live player-driven Arts submenu".

**Queue location PINNED + Miracle path VALIDATED (capture).**
The action queue is the per-actor **`actor[+0x1DF..+0x1F2]`** action-parameter byte stream (not `ctx[+0x274]` — a capture showed that is the turn-order active-actor index written by `recompute_battle_order` `FUN_801DABA4`).
The directions/connectors encode as `0x0C/0x0D/0x0E/0x0F` = Left/Right/Down/Up, `0x1A` = `SpecialStarter`, `0x1B..0x32` = art constants.
A `battle_noa_miracle_art_combo` capture (probe `autorun_super_art_action_queue.lua`, runbook [`docs/tooling/super-art-queue-capture.md`](../tooling/super-art-queue-capture.md)) read Noa's resident Miracle queue and it matches `crates/art/src/miracle.rs`'s modeled replacement string **byte-exact** — runtime-validating the queue + `ActionConstant` encoding that were previously spreadsheet-sourced.
**Super path also validated:** a `battle_vahn_tri_somersault_super` capture read Vahn's resident Tri-Somersault queue (`…19 27 0F 19 1F 0E 1A 2B 2B 2B`) whose matched/replaced tail is **byte-identical** to `super_art.rs`'s `Tri-Somersault` `replace` — confirming the combo-specific connectors (`Somersault 0x27 → 0F`, `Cyclone 0x1F → 0E`) and the finisher tail. The dequeue site is pc `0x801D89D8`. The only residue is the other 13 Supers' replace strings (each a one-capture check through the same probe).


### Seru-magic summon visual (e.g. Tail Fire)

*Status:* **player visual RESOLVED + WIRED** — the player summon renders as its **namesake `battle_data` creature** through the ordinary rigid TRS-keyframe battle draw (`monster_archive::battle_render_mesh` + `MonsterAnimPlayer` + `tmd_to_vram_mesh_posed_rot`), spawned off the live cast band (`request_summon_spawn` → `spawn_summon_creature`); the move-VM `SummonScene` is retained only as the on-disc stager-record parser/driver + a non-battle debug exerciser + the candidate model for the untraced **enemy** "Fire Tail" boss move. (The earlier "`FUN_801F7088` rotation node source unpinned" framing is superseded — see the RESOLVED block below.)

The summon visual is a **per-summon code overlay**, not an opcode or `befect_data`: battle SM `FUN_801E295C` state `0x29` resolves spell id `0x81..0x8b` via `PTR_801f6734[id-0x81]` + `FUN_8003EC70(id-0x79)`.

**Two overlays timeshare the shared buffer at link base `0x801F69D8`** (`*DAT_80010390`):

**PROT 0905** is the Gimard *Tail Fire* **spawn stager** (38 `FUN_80021B04` calls) and **PROT 0900** is a resident **transform / GTE-render** overlay (`RotMatrixX/Y/Z` ×6 + prim emit) that animates and draws the spawned parts. PROT 0900 is the one **byte-resident** in a mid-cast save state (`battle_gimard_tail_fire_a/_b`: `0x801F8000` ↔ PROT 0900 file `0x1628`) — *after* the 0905 stager has run and been overwritten — which is why a "905 head in RAM" search comes up empty. The stager spawns each part via the SCUS part-stager **`FUN_80021B04`** (`a1` = world pos, `a2` = a part record, `a3 = 0x1000`); `FUN_80021B04` stages it as an actor (`actor[+0x48]` = record move-buffer base, `actor[+0x70] = 2` PC) then `jal FUN_80023070` ticks the **move VM** on `record+4`.

**RECORDS RESOLVED — in-file, parsed.** Each `FUN_80021B04` call passes its record by absolute pointer (`lui 0x8020 / addiu`); under the correct link base `0x801F69D8` those resolve to PROT 0905 **file `0x180C..0x1E00`** (runtime `0x801F81E4..`), a contiguous table of variable-length records `[i16 model_sel][u16 flags][move-VM bytecode @+4]`, `model_sel == -1` = transform/pivot node (dominant; mesh bound by the move-VM anim-bank ops), `>= 0` = `DAT_8007C018[model_sel + gp[0x754]]`. `legaia_asset::summon_overlay::parse` recovers them by scanning the spawn calls (disc-gated `summon_overlay_real`: 38 sites → 23 part records, 17 transform nodes; CLI `asset summon-overlay`).

**Generalizes across the whole player-summon block:** every overlay in PROT 0905..=0915 (`spell_id 0x81..=0x8b`, `summon_overlay::PLAYER_SUMMON_STAGER_PROT`) recovers a move-VM scene-graph (disc-gated `summon_overlay_block` sweep — 20..73 spawn sites, 10..43 contiguous in-file records each). Gimard (0905) reads cleanest (transform-node-dominated + small library indices); the larger summons (0906/0911/0915) carry many `SummonPartKind::Sentinel` first-words — node-mode `0x1000`/`0x4000`/`0x8000`-class markers, **not** library indices — so the CLI labels those `sentinel 0xNNNN`. The model-library base (`gp[0x754]`) is **resolved** (see the summon-render block below): it is **not per-summon** but one per-battle, party-size-derived value (`party_count + 2`). Open across the block:
the precise sentinel semantics.

**This CORRECTS the earlier "records beyond the `0x5800` file / `0x180C` only coincidentally record-shaped / parser reverted" reading — that was the wrong link base (`0x801F0000` instead of `0x801F69D8`), which pushed the runtime record addresses past the file.** **Still pinned:** the CLUT band is byte-identical across the two animation-distinct frames (motion is geometric, not palette cycling); flame texture is **PROT 870** (three 64x256 4bpp TIMs → battle VRAM `(320/384/448,0)`, CLUTs rows 474..476); the bound flame mesh comes from **PROT 871** (`etmd.dat`, 30-TMD pack) at `DAT_8007C018[26]`.

**Engine:** PROT 871 → `World::global_tmd_pool[3..=32]`, flame atlas uploaded on battle entry, static flame renders with the row-478 CLUT (`GIMARD_TAIL_FIRE_MODEL_INDEX = 26`).

**Animation driver LANDED.** `engine_core::summon::SummonScene` seeds one move-VM `ActorState` per parsed part (PC=2 → `record+4`, mirroring `FUN_80021B04`) and ticks every part through the already-ported move VM each frame (`World::spawn_summon` / `tick_summon` / `active_summon_part_draws`; `play-window` `G` debug-spawns the Gimard summon and renders one textured TMD per mesh part). The per-part animation *computation* is faithful (verified: every Gimard part runs the move VM without an unimplemented opcode; disc-gated `summon_scene_real`).

**Production cast-band trigger WIRED.** A player Seru-magic cast (`spell_id` in `0x81..=0x8b`) now requests the summon at the cast point in both engine cast paths — the action-SM `spell_anim_trigger` (`World::fold_battle_event` on `BattleEvent::SpellAnimTrigger`) and the live-loop `cast_spell_on_slots` — via `World::request_summon_spawn`. The host drains `World::take_pending_summon_spawn`, maps the id to its overlay PROT entry (`summon::summon_stager_prot_entry`: `0x81..=0x8b → 905..=915`, retail `FUN_8003EC70(id-0x79)`), loads + parses it, and seats the scene-graph (`play-window`). So a real Gimard *Tail Fire* cast spawns the animated summon, no debug key.

**PROT 0900 transform PARTIALLY DECODED.** The resident render overlay (link base `0x801F69D8`) composes each part's transform.

**Translation pinned + ported:** phase A at `0x801F82A0` — when the keyframe gate `*(i16)(actor+0x9C) == *(i16)(actor+0x9E)` holds, the part's world position is **overwritten** by the move-VM anim-bank slots (`anim_3c/3e/40`, op `0x00`, `v << 3`) and `+0x9E` is cleared; the anim banks are summon-local so the engine adds the cast origin (`summon::apply_translation_update`, in `SummonScene::tick`). This is why a part animates with no `WORLD_ADD` op — its motion is in the anim banks.

**Rotation pinned-but-not-sourced:** the overlay builds a per-part render node (rot X/Y/Z at node `+0x8/0xa/0xc`, mesh at `+0x10`, flags at `+0x12`), applies the **camera** angles `_DAT_8007B790/2/4` (the cutscene-camera Euler globals) gated by flags `+0x12` bits `0x80/0x100/0x200`, then the part's local rotation — composed Z·Y·X via `RotMatrixX/Y/Z` (`0x800461A4`/`629C`/`638C`).

**Two distinct render paths in PROT 0900 — separated (correcting two earlier mis-attributions in this thread).**

**(1) POSITION — `FUN_801F811C`, keyframe interpolation, decoded.** This per-part updater takes the actor and advances its world position toward the anim-bank target: when `actor+0x9E` (keyframe **duration**) is non-zero it adds the per-frame delta `_DAT_1F800393` to `actor+0x9C` (keyframe **time**, clamped to the duration), then for each axis interpolates the current world pos (`+0x14/16/18`) toward the anim-bank slots (`anim_3c/3e/40`, `+0x3c/3e/40`) via the lerp helpers `FUN_801DE4C8`/`FUN_801DE648`; when the time reaches the duration it latches `world = anim banks` and clears `+0x9E`.

The engine ports this whole per-frame update as `summon::apply_translation_update`: the keyframe time advances toward its duration and the world position interpolates toward the anim-bank target each frame via the `FUN_801DE4C8` mode-1 lerp + `FUN_801DE648` store, latching exactly to the target on completion (the latch is the terminal case, not the whole behaviour). `FUN_801F811C` *also* emits **2D GP0 sprite packets** (0x18-byte, tag `0x05000000`, colour `0x28808080`) linked into the ordering table by **`FUN_8003D2C4` = the PSX OT-linker `addPrim`** (NOT a mesh renderer).

**Those packet fields `+0x8/0xa/0xc/0x10` are GP0 primitive params (XY/UV/size/clut), NOT Euler rotation** — an earlier note in this row that read them as a "render node" with `mesh = field_74` / scratchpad-staged rotation was a mis-attribution of this 2D sprite layer; disregard it.

  **(2) 3D MESH ROTATION — `FUN_801F7088` is NOT the player-summon path (live-trace resolved).** The historical hypothesis was that each summon part's mesh orientation is built by `FUN_801F7088` (a GTE view rotation from the camera Euler globals `_DAT_8007B790/2/4` gated per-axis by a node-flags word's bits `0x80/0x100/0x200`, plus a per-part local Euler at the node's `+0x8/0xa/0xc`, via `RotMatrixX/Y/Z`).

**A live PCSX-Redux capture of a player Gimard "Burning Attack" cast (Vahn solo; scenarios `gimard_summon_start` / `gimard_summon_visible` / `gimard_burning_attack`) FALSIFIES that for the player summon.** Exec-breakpoint counts across all three phases: `FUN_801F7088` = **0 calls**, move VM `FUN_80023070` = **2-3** (trace noise, not a per-part driver), part-stager `FUN_80021B04` = 1, and the **battle per-actor draw `FUN_80048A08` = 35-64×/frame**. The summon is an ordinary battle actor (state `gimard_burning_attack`: actor `0x8008350C`, `+0x5a=3`, 13-group mesh-table at `+0x44`, monster-anim archive at `*(actor+0x4C)+0x88`) drawn by `FUN_80048A08` → the per-object rigid-TRS keyframe decoder `FUN_8004998C` → cluster-A `FUN_80043390`, with each object's Euler composed by `RotMatrixX/Y/Z`.

**[CORRECTION — `0x8008350C` is a Gobu Gobu monster, NOT the summon; see the RESOLVED block at the end of this row. The durable result here is the call-count finding (`FUN_80048A08` is the draw path); the summon's actual creature is `battle_data` id 10 "Gimard", pinned from the fingerprint-verified frame-0 RAM.]** **So the player Gimard summon is posed exactly like an enemy monster body (per-object rigid TRS keyframes), not via a move-VM scene-graph or `FUN_801F7088`.** This agrees with the `effect.md` / `battle-action.md` / `effect-vm.md` finding ("PROT 905 has zero `jal 0x80023070` — there is no move VM here"). The `FUN_801F7088` dumps are the **world-map top-view tile renderer** aliasing the same `0x801Fxxxx` band — a different overlay, not the battle-summon code.

SCOPE: this capture is the PLAYER "Burning Attack" move only; the ENEMY Gimard boss move **"Fire Tail"** (the `battle_gimard_tail_fire_a/_b` captures) is a DISTINCT move with a distinct animation and was not re-traced — whether it uses the overlay/move-VM path is a separate open question. (Probes: `autorun_summon_rotation.lua` + `autorun_summon_path_reconcile.lua`; RAM dumps under `captures/summon_rotation/`.) The engine's `summon::SummonScene` move-VM model therefore needs reconciliation: for the player summon the faithful path is the battle TRS-keyframe draw, already ported as `FUN_80048A08` / `FUN_8004998C` in `crates/engine-vm/src/anim_vm.rs`.

**Animated battle-actor rendering is now WIRED** (the general pipeline this thread's player-summon render rides on). Enemy monsters animate in `play-window`: `legaia_asset::monster_archive::idle_animation` (action 0, the `+0x8c` 9-byte TRS stream) → `legaia_engine_core::battle_anim::MonsterAnimPlayer` (an 8.8 fixed-point loop cursor producing a `legaia_anm::PoseFrame`, the same per-object `(translation, rotation)` shape the field ANM player produces) → the rigid `legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot` deform (`R·v + T`, `Rz·Ry·Rx`, the validated `monsters.html` `_assemble` math).

`enter_battle_render` attaches the clip per actor, `World::tick_battle_animations` advances it each battle frame into `pose_frame`, and the posed-override path deforms the mesh; the field translation-only path is unchanged. The core (decode → player → posed_rot → moving mesh) is proven on real disc data by `battle_anim_real` (monster 1 = 28 frames × 15 parts).

**Player summon source — RESOLVED: the summon reuses the namesake `battle_data` enemy creature.** (Path to the answer, including a corrected wrong turn.) The actor `0x8008350C` the earlier notes called "the summon" is actually a **Gobu Gobu MONSTER** — its `+0x4C` archive `0x800B2694` (`+0x88` self-ptr → `+0x8C`, 13×18) byte-exactly matches `battle_data` id 4 (Gobu Gobu) action 0. The fix was **fingerprint discipline**: the `summon_rotation/state6` RAM *dump* is the probe advanced N frames; analysing the **fingerprint-verified frame-0 RAM** of the `gimard_summon_visible` save (`8aa0…`, sha256-matched to the catalog + the live slot) instead, the battle actor table `DAT_801C9370` shows slot 0 = Vahn (HP 196) casting `spellid 0x81`, slot 3 = a Gobu Gobu enemy (HP 76, 13 parts / ~10 actions),

and a **distinct 11-part / 2-action** entity. That 11-part idle (`0x800BBB20`, 11×40)

**byte-exactly matches `battle_data` id 10 = "Gimard"** action 0. So **the player Gimard summon spawns the namesake "Gimard" creature** (id 10), reusing its monster-archive mesh + per-object TRS animation — exactly the format the now-wired enemy pipeline consumes. Disc-verified spell→creature map (by name; the `"$2"`/`"$3"` higher-level enemy variants are excluded): Gimard `0x81`→10, Theeder `0x82`→25, Vera `0x83`→28, Gizam `0x84`→55, Nighto `0x85`→49, Zenoir `0x86`→64, Viguro `0x87`→74, Swordie `0x88`→86, Orb `0x89`→83, Freed `0x8a`→92, Nova `0x8b`→95 (`legaia_engine_core::summon::summon_creature_id`, disc-gated `summon_creature_map_real`).

**This supersedes the old move-VM `SummonScene` model and the PROT-905-overlay reading** for the *visual*: the faithful summon render is the battle creature drawn through `monster_archive::battle_render_mesh` + `MonsterAnimPlayer` + `tmd_to_vram_mesh_posed_rot` (mesh + texture + animation all from PROT 867), not the stager scene-graph. (PROT 905 is still the magnitude/effect stager — see the per-spell-power thread.) The flame-atlas loader site is now pinned:

**`FUN_80020050`** (SCUS `0x80020050`) uploads PROT entry `0x366` into VRAM twice via `FUN_8001fc00` (→ `FUN_8003e8a8`, the PROT-index loader), with the VRAM region set up by `FUN_80017888` / `FUN_8001e54c` (param `0xf000`); it is gated on `_DAT_8007b868 == 0` (the same field-camera / mode gate `FUN_801dbe9c` reads) and is independent of the `FUN_800520F0` battle-bundle path (which pulls `0x367..0x36d`).


### Monster steal item (Evil God Icon)

*Status:* RESOLVED — static SCUS table `DAT_80077828`

What the player steals (Evil God Icon equipped) is a **static `SCUS_942.54` table at `DAT_80077828` / file offset `0x68028`**, indexed by **1-based monster id** at `DAT_80077828 + id*2`, each entry a 2-byte `[steal_chance_pct, steal_item_id]` pair (chance FIRST, item second — the reverse of the record's `[item, chance]` drop order). It is **not** in the PROT 867 record (the prior exhaustive record scan was correct — the data simply isn't there; it's a separate executable table, which is why every record-only search came up empty). Pinned from a live player-steal RAM capture (Skeleton id 13 → `1e 8a` = 30% Incense, matching the on-screen banner) and verified **byte-exact against the complete published steal table** (item + chance) across every resolvable monster id — zero mismatches.

Parser `legaia_asset::steal_table`; doc [`steal-table.md`](../formats/steal-table.md); randomizer `legaia_rando::steal`. `enemies.toml` `steal` stays useful ground-truth but the SCUS table is now authoritative.


### Per-spell magic power / multiplier

*Status:* **mechanism RESOLVED + roll PORTED** — the calculator + full three-stage modifier chain (`FUN_801dd0ac` roll → `FUN_801dd864` scale → `FUN_801ddb30` finish) is recovered, and the closed-form roll + scale stages are ported as pure kernels in `battle_formulas`; the `0x801F4F5C` arts table is now located + parsed off the disc (`legaia_asset::move_power`); live wiring + the coupled finisher are the residual

**The static re-dump avenue closed the question.** The 7-entry jump table `FUN_801f2d68` reads (`jr *(0x801F69D8 + state*4)`) resolve to PROT **0900** file offset 0 — the **render** overlay (loads at `0x801F69D8`). Those five entries are staggered entry points into one per-frame routine that lerps move-VM anim banks (`FUN_8003ce9c`/`ce64`/`ceb8`) and emits GPU display-list packets into scratchpad `0x1F800314`:

**zero `mult`/`div`, zero `actor+0x14c` write, no power read** → the "magnitude is in this jump table" hypothesis is **falsified**; it is animation/GPU only. The magnitude is instead applied by the paired **stager** overlay (PROT 0903..0915, the file with the `jal FUN_80021B04` part-spawn calls), in the same function that spawns the body parts — each stager has exactly one `actor+0x14c` writer, and they split:

**damage summons** (PROT 0904/0912/0914 + 0915's 2nd arm; `subu`) call the shared battle kernel **`FUN_801dd0ac`** (`a0` = a per-summon move-type const `0x10..0x12`, `a1 = 7`, `a2` = target slot), clamp to current HP, accumulate the popup at `actor+0x10`, then `HP -= amount`; **heal summons** (PROT 0903/0905/0910/0911/0913 + 0915's 1st arm; `addu`) compute `(power_byte << 5) + 0xe0` inline (clamped to `maxHP-curHP`, dead-guarded), `power_byte` from a `0x80084140`-based table searched by the cast spell-id (`actor+0x1df`: ids at `+0x705`, powers at `+0x729`).

`FUN_801dd0ac` (already dumped, `overlay_battle_action_801dd0ac.txt`) takes the **summon path** for `param_2 == 7`: roll = `rand % (AGL@+0x168 + 1) + HP@+0x14c + DAT_801C9370[ctx+0x13]_AGL * 2`, returns `roll - defender_mitigation` — so **summon "power" is caster/summon battle-state-derived, not a static per-spell scalar** (which is why SCUS spell-table `+5..+8` are zero and gamedata has no power column). `FUN_801dd0ac`'s **non-summon** branch (`param_2 != 7`, arts/physical) reads a real 26-byte-stride per-move power table at **`0x801F4F5C`** (arts power, **not** magic) — now located on disc as static battle-overlay data (PROT 0898, parser `legaia_asset::move_power`),

indexed via a 128-byte id→index map at `0x801F4E63` (`param_1 = map[actor[+0x1df]]`); **the full 26-byte record is now decoded** (`+0` power, `+0x02` strike-Y offset, `+0x04`/`+0x06` move/phase counters, `+0x08`/`+0x09` homing speed + tracking flag, `+0x0a` impact-effect selector, `+0x0b` trail texture page, `+0x0d` sound cue, `+0x0e` list-mode flag, `+0x12`/`+0x16` effect-id lists; `+0x0c` is an unused `C`/`E`/`G` designer tag) — see [`docs/formats/move-power.md`](../formats/move-power.md). The move-id space is the spell-table id space, so the records label cleanly: idx `0x10..=0x2b` = the named monster special-attacks (`0x25..=0x74`), idx `0x01..=0x0f` = the unnamed internal enemy-attack tiers (`0x04..=0x1f`).

The scale stage `FUN_801dd864` (8×8 element-affinity matrix `0x801F53E8` + status bits + the summon magic-power tail `roll += roll*(power-1)>>3`) and the finisher `FUN_801ddb30` (resistance bits, `rand%9+8` floor, 9999 cap, spirit-gauge, MP drain, stat debuffs) are now fully traced — see the `FUN_801dd864` / `FUN_801ddb30` rows in `functions.md` and the three-stage chain in `battle-formulas.md`.

**PORTED:** the closed-form roll + scale arithmetic is now pure kernels in `legaia_engine_vm::battle_formulas` (`summon_attacker_roll` / `summon_defender_roll` / `summon_predamage` / the `apply_*` helpers / `heal_summon_amount`), hand-tested against the disassembly.

**Residual:** (1) the arts/physical kernel is now **wired into the live loop for monster special-attacks** — the move-power table loads onto `World::move_power` (`engine-core::move_power::MovePowerCatalog`, PROT 0898) and `cast_spell_on_slots` overrides a damaging monster cast's magnitude with `arts_physical_predamage_lazy` seeded by that move's `+0` power (`World::enemy_move_predamage`: AGL from `battle_accuracy`, defense terms from `battle_defense_split`; the attacker ×2 + defender ×1 `rand()` draws are taken up front and the bonus pair is drawn **lazily**, only when the bonus arm fires, so the shared RNG cursor advances by exactly three or five draws matching `FUN_801dd0ac`'s call order; gated on the table being installed so disc-free battles keep the placeholder + RNG stream).
The player-driven **summon** roll is now wired too (`World::player_summon_predamage`): summon-body HP/AGL seed from the namesake `battle_data` creature record, caster AGL from `battle_accuracy`, the caster magic-power byte from the character record's spell list (`+0x13D` ids / `+0x161` levels, the `FUN_801dd864` search), and the closed-form `FUN_801ddb30` finisher applies — including the per-caster summon power-percent table `0x801F5468` ((char_id-1)*8 + summon_element; PROT 0898 file `0x26C50`, parsed as `ElementAffinity::summon_power`, byte-pinned: own element 100, opposed 40, Gala dark 60). Remaining residue: the live slot-7 actor's HP at roll time is modelled as the creature record's spawn HP (a mid-battle summon that has taken damage is not modelled), and status/guard default to none;

(2) the `FUN_801ddb30` finisher's **closed-form finalisation arithmetic is now ported** (`battle_formulas::damage_finish` — equipment elemental-resistance halving / guard halve / `rand%9+8` no-damage floor / summon power-% scale / 9999 cap — plus `spirit_gauge_fill`, both unit-tested); only its state-mutating tail (damage-popup accumulator, AI revenge table, MP drain, per-element stat-debuff switch) stays in the live battle context; (3) the affinity matrix `0x801F53E8` is now located + parsed off the disc (`legaia_asset::element_affinity`, PROT 0898 file `0x26BD0`, same link base as the move-power table) together with the per-character element table (`0x801F5480`: Vahn=fire/Noa=wind/Gala=thunder/Terra=wind), the matrix orientation is corrected (`matrix[attacker][defender]`;
the retail values are a ±4% nudge — diagonal 96 / opposite-pairs 104 / default 100, not a ×0/×2 weakness table), and the enemy element source is **pinned from the `FUN_801dd864` disasm itself**: the scale stage reads it **record-direct** — `lbu …,0x1d(record)` where `record = 0x801C9348[slot-3]` (the per-enemy record-pointer table, NOT a copied live-actor field) — so the element is `MonsterRecord::element` (`+0x1D`) consumed exactly as the parser exposes it (the same record the victory-spoils path reads `+0x44/+0x46/+0x48` from). This supersedes the earlier "loader copies `+0x1d` into `actor[+0x1d]`, copy not yet pinned" framing; the curated-element correlation (four party-table ids reproduce exactly + byte ∈ `0..=7` across every populated record) now only corroborates the id *labelling*.

**WIRED (both directions):** the monster special-attack path scales by `matrix[enemy_element][party_member_element]` (`World::enemy_affinity_pct` → `enemy_move_predamage`), and the **player Seru-magic** path scales by `matrix[summon-creature element][target element]` (`World::cast_affinity_pct` in `cast_spell_on_slots`): the attacker element resolves off the summon **creature** by name (`World::summon_attacker_element`, the engine-side slot-7 `+0x1d`), the defender by slot (`World::battle_slot_element`). The player multiply is post-roll on the deterministic cast output (RNG untouched); the enemy scale is applied *inside* the roll, before the conditional bonus-arm threshold (so a non-neutral value can shift the lazy bonus draw — faithful to retail's scale→bonus order).
Both are gated so an uninstalled / neutral table reproduces the no-affinity baseline bit-identically (magnitude + RNG stream), keeping disc-free battles deterministic. The player-summon **base** magnitude is still the caster-state stand-in (the faithful slot-7 summon roll is open), so the player direction is the ±4% nudge on a placeholder, not yet byte-exact. See [`battle-formulas.md`](../subsystems/battle-formulas.md#element-affinity-matrix-fun_801dd864-0x801f53e8). The `0x801F4F5C` **arts** power table is located + parsed (`legaia_asset::move_power`), the `param_1` → move-id map resolved (`0x801F4E63`),

and **every record field decoded** (power / strike-Y offset / move + phase counters / homing speed + tracking flag / impact-effect selector / trail texture page / sound cue / list-mode flag / on-contact + launch effect-id lists; `+0x0c` is an unused designer tag with no runtime reader) — see [`docs/formats/move-power.md`](../formats/move-power.md). The auxiliary tables the record's selectors index are now parsed too: `EffectAuxTables` for the `+0x12`/`+0x16` effect-id lists' `0x801F6324` prototype-pointer + `0x801F6418` SFX tables, and `parse_impact_effect_table` for the `+0x0a` `0x801F53D4` config words (this corrects an earlier "pointer table" mislabel — the `0x801F53D4` entries are packed `u32` config words, not pointers).

**The `0x801F6324` spawn entries are decoded.** Each is an overlay VA to a *variable-length move-VM scene-graph record* in the **exact summon-part format** (`+0x00 i16 model_sel`, `+0x02 u16 flags`, `+0x04` move-VM bytecode), spawned by `FUN_80050ed4` → the shared stager `FUN_80021B04` → the ported move VM, with `model_sel` indexing `DAT_8007C018` — the same machinery as `legaia_asset::summon_overlay`. The earlier "~0x20-byte struct" reading was a coincidence (packed records, not a fixed stride). The high-bit (`0x80`) list bytes route instead to the 2D `efect.dat` pool (`FUN_801dfdf0` → `EffectCatalog`, ported as `spawn_by_ui_id`).

Render wiring reuses the summon parser + move VM. The `model_sel` additive base `gp[0x754]` (global `0x8007BA6C`) — only *read* in the corpus — is **RESOLVED from the save corpus**: it is `0` whenever no battle effect-model library is resident, and **`party_count + 2`** when a battle has installed it — `3` for the 1-member training party (Vahn alone), `5` for the 3-member party (Vahn / Noa / Gala). A PCSX-Redux exec-bp on `FUN_80021B04` first pinned the value `3` (probe `autorun_summon_model_base`, confirming the full `FUN_801e09f8 → FUN_80050ed4 → FUN_80021B04` chain — `ra = 0x80050F08`, `a3 = 0x1000`, prototype table `0x801F6324` + effect-list id `0x22` live in registers); reading `0x8007BA6C` + the party count `0x80084594` across the whole mednafen corpus generalised it.
So the base **tracks party size** (the two fixed pool slots + the live party-character meshes precede the effect-model library), and `model_sel` is *library-relative* — `DAT_8007C018[model_sel + gp[0x754]]` lands on the same library model regardless of party size; only the library offset shifts. There is **no per-summon base** — one per-battle value drives both move-FX and summon-part spawns. Pinned by `crates/mednafen/tests/summon_model_base.rs`.

The engine **renders the move-FX scene-graph**: `World::spawn_move_fx` parses a move's spawn-entry records (`MoveFx` via `MovePowerCatalog::fx_for_move_id`), stages them as a `SummonScene` at the effect-model library base (the engine registers PROT 0871 at a fixed `DAT_8007C018[3..]` and `model_sel` is library-relative, so this is the retail `party_count + 2 = 3` case for the 1-member slice; the layouts are equivalent), and drives them through the ported move VM (`tick_move_fx` / `active_move_fx_part_draws`; `play-window` `H` debug-spawn) — reusing the summon machinery wholesale, so it shares the same interpreted-transform caveat. A spawn also surfaces the move's two presentation fields: the **trail texpage** (`+0x0b` → `0x7700 + id`) on `World::active_move_fx_trail_texpage()`,
and the **sound cue** (`+0x0d`) as `World::take_pending_move_fx_cue()`, which the host routes through the now-ported `FUN_8004fcc8` dispatch decode (`legaia_engine_audio::classify_cue` → `CueDispatch`; `voice_pitch` for the voice arm). The 2D afterimage *draw* `FUN_801e1ab0` (the streak pass that consumes the trail texpage) is ported as the pure `legaia_engine_render::afterimage::build_afterimage_quad` — jittered semi-transparent `POLY_FT4` (per-corner `rand` wobble, brightness band, UV/CLUT/texpage layout) from four projected corners + the trail id. What remains: the per-part transform composition (the still-open `FUN_801F811C` / PROT-0900 piece); the camera-coupled GTE projection of the afterimage corners (`FUN_800195a8`); and the live note-on wiring of the resolved cue.
The **SFX program bank is pinned**: the cue's `program`/`tone` (static `DAT_8006F198` table, [`sfx-table.md`](../formats/sfx-table.md)) index the **per-scene music VAB** the BGM sequencer already has open (`FUN_80065034` reads the libsnd current-bank globals; byte-identical to the disc `music_01` VAB for that scene), so firing a cue is `SfxBank::play_one_shot(spu, scene_vab)` — no separate bank.

**`0x801F4F5C` is special-attack-only:** the id→index map covers 44 ids (internal tiers `0x04..=0x07`/`0x12..=0x1F` + named attacks `0x25..=0x74`); the basic-attack / art bands `0x08..=0x11` and `0x16..=0x18` are unmapped (pinned by a live capture — a party member's Tactical Art carries an unmapped id, e.g. Vahn's Somersault `0x0F`, so it would roll against the zero-power record 0). A party member's arts therefore do **not** use this table — they take their damage from the per-strike *art-record* power byte (which `art_strike.rs` already does, faithfully); the only remaining engine stand-in is `apply_basic_attack`'s flat `art_strike_damage_default` for a no-art generic hit.


### Stat growth-rate source

*Status:* RESOLVED + validated + WIRED (core + opt-in jitter)

The per-character stat-grant source is **static `SCUS_942.54` tables read by the level-up applier `FUN_801E9504`**. Fully decoded: the parameter block at `DAT_80076918` is **per-character (stride `0x3C`), 8 contiguous 6-byte sub-records `{u16 start, u16 max, u8 jitter, u8 row}`** — `start` = base stat (**Gala matches the new-game template on all 8**), `row` selects one of 3 curves at `DAT_800769CC`. Per-level gain = `max(1, (max-start)×curve[row][level-1]/0x24C0 + rand()%(2×jitter+1) − jitter)`, then caps. The divisor `0x24C0` is the **curve normalizer** (each curve sums to `0x24C0`, so growth accumulates to exactly `max-start` by L99).

**VALIDATED** byte-exact against a single-level capture (Noa L2→L3, the `noa_levelup_*` saves): all 8 deltas within the core ± jitter band — the earlier "~4.8x overshoot" was an artifact of the unreliable multi-level corpus observations (`noa/gala_4_level_jump`), not the formula. Parsed by `legaia_asset::level_up_tables::GrowthTables::{char_params,level_gain_core}` (disc-gated test). The "Seru struct `+0x74`" reading stays **falsified**.

**Engine wiring done (deterministic core, all 8 stats):** `StatGain` carries HP/MP + the six battle stats; `LevelUpTracker::with_growth_tables` + `BootSession` install per-character curves from the user's SCUS, replacing the flat 10/5 placeholder, and `apply_to_record` grows the record-side window then mirrors to live (disc-gated boot test pins Noa's L2→L3 core). The per-level `rand()` jitter is also **modeled (opt-in)**: `LevelUpTracker::with_level_up_jitter(seed)` drives a faithful PSX BIOS-rand LCG (`BiosRand`) drawing one `rand()` per stat per level on the unfloored core before the `max(1,…)` floor — off by default so determinism oracles stay bit-identical (bit-exactness still needs the runtime BIOS-rand seed).

**Remaining:** only the slots-1/2 XP correction. See [`subsystems/level-up.md`](../subsystems/level-up.md#stat-gains).


### Monster stat-record archive source

*Status:* resolved

The monster archive is **PROT entry `0867_battle_data`** (extended footprint; the 15.9 MB archive lives in the entry's trailing-gap sectors). `FUN_800542C8` streams per-monster `0x14000` LZS slots at `(id-1)*0x14000`, each `[u32 dec_size][LZS]` decoding to a block whose head is the `FUN_80054CB0` stat record (name `@0x00`, XP/drop `@0x04`, HP `@0x0C`, MP `@0x10`, stat u16s `@0x0E/0x12/0x14/0x16/0x18/0x1A`, magic count `@0x4A`, spell-ptr array `@0x4C`). Pinned by a live-battle PCSX-Redux watchpoint (`autorun_monster_record_source.lua`) — relative seek `(id-1)*40` sectors + `disc_read` CdlLOC → PROT.DAT `0x38AF000` = entry 867; three records match live actor stats byte-for-byte. The `monster_data` label (PROT 869) is a stub.

Parser `legaia_asset::monster_archive`; bridge `legaia_engine_core::monster_catalog::catalog_from_monster_archive` wired into `enter_field_scene`. The record is now fully decoded: all six stats are named (ATK/DEF↑/DEF↓/AGL/SPD/SP), rewards are inline at `+0x44..0x49`, and `+0x04` is the monster's **battle-model TMD** offset (not XP/drop — see the mesh thread below).


### Monster mesh + texture pool

*Status:* resolved

The monster's 3D battle model is a [Legaia TMD](../formats/tmd.md) embedded in each PROT 867 archive block at the offset in stat record `+0x04` (installed at battle-actor `+0x230`; the `0x1C`-stride records `FUN_80049858`/`FUN_800495C8` walk are its object table).

**186/194 slots parse cleanly.** The texture/CLUT pool at record `+0x08` is decoded from the battle loader `FUN_80055468`: a `0x1E0`-byte region of fifteen 16-colour CLUTs followed by a 4bpp page (always 256 rows tall, 128 or 256 texels wide; palette = `cba & 0x3F`). Byte-exact vs pool sizes; renders to recognizable atlases. The on-disc CBA/TSB are nominal defaults the loader relocates per slot, so the raw pool does not appear verbatim in a battle VRAM dump — the loader layout is the ground truth. Parser `legaia_asset::monster_archive::{mesh, MonsterMesh::texture}`; CLI `--obj` + `--texture-png`; WASM `monster_mesh_*` + `monster_texture_*` accessors drive the enemy-table site page's per-row WebGL viewer (textured + directional-lit).


### Terra slot-3 / story-flag overlap

*Status:* resolved

The **header-size constant drifted**: `RETAIL_CHAR_RECORD_HEADER_SIZE` was `0x66F` (the *name* field) but the true record base is `game+0x3C8` (live RAM `0x80084708`), with the display name at internal offset `+0x2A7`. Confirmed across six in-game RAM captures: mid-game stats at `record+0x104`/`+0x11C` read back the expected per-character HP/MP for all four slots. The four-slot array runs into the global region, so slot 3 (Terra)'s tail (record offset ≥ `+0x2BC` = `game+0x12C0`) aliases the story-flag bitmap and inventory; Terra's meaningful fields (name, live stats, RecordStats) sit before that boundary. There is **no special case** — Terra is the New Game template's fourth roster entry (HP 400) but never a savable battle-party member, so the tail aliasing is benign.

The constant is now `0x3C8`, `legaia_save::CharacterRecord` gains a `name()`/`set_name()` accessor at `NAME_OFFSET` (`+0x2A7`), and the off-by-`0x2A7` that made `Party::from_retail_sc_block` read stats from the wrong fields on a populated save is fixed (proven by synthesising an SC block from a live RAM dump and checking the parsed HP).


### Battle party mesh pack `other5` = **PROT 1204** (battle form; Baka Fighter reuses it)

resolved (empirical) — A real main-game battle renders the party from **PROT 1204 (`other5`)**, the higher-detail battle character meshes, installed into `DAT_8007C018[0..=2]`.

**This overturns the earlier conclusion** (that battle reused the field pack 0874 §0 and that 1204 was a Baka-Fighter-only roster). Decisive evidence: reading the live party mesh pointers `DAT_8007C018[0..=2]` out of real-battle save states and byte-matching each runtime TMD's pose-independent vertex pool against the two candidate packs — the party meshes byte-match PROT 1204 and **never** the field pack 0874, across the Tetsu tutorial fight, the **Gimard Seru-boss fight** (an unambiguous turn-based battle, not the minigame), and the full-party Gobu Gobu capture. Runtime `nobj` is +2 over disc (15/16/15 → 17/18/17) via the same `FUN_8001EBEC` equipment-group patch the field form uses.

The **Baka Fighter minigame REUSES this same pack** — it lets you play *as* Vahn/Noa/Gala, so it borrows the battle models (`overlay_baka_fighter` loads `data\field\other5.lzs` + PROT 1205/1206, debug `"OTHER5 %d %d"`); that minigame reuse is why earlier captures pinned the pack during Baka Fighter sessions and read it backwards. Reproduce with `scripts/verify_battle_char_pack.py`; disc-only distinctness pinned by `battle_char_pack_real::battle_pack_is_distinct_from_field_pack`; parser `legaia_asset::battle_char_pack`.

**Loader — PINNED (write-watchpoint).** The captured battle loader `FUN_800520F0` `tmd_register`s PROT `0x36a` into the *effect* window `DAT_8007C018[3..]` (`etmd.dat`), NOT the party `[0..=2]`. The party-mesh install into `[0..=2]` is **static SCUS**, through the generic registrar `FUN_80026B4C` (store `0x80026BA8`), from two battle state-handlers:

**`FUN_800513F0`** (lead/active actors — `tmd_register(*(actor+0x50)+0x18, 0)` in a `while<3` loop over the active-actor table `0x801C9360`, right after the `FUN_80052FA0` palette decode) and **`FUN_800542C8`** (additional members — per-member loop bounded by `*(rec+0x4a)`, `tmd_register(*(*rec+4), 0)`). Both are reached **indirectly** (state-handler dispatch), so a static cross-reference on `0x8007C018` finds no writer — which is why this was long mis-assumed to live in an overlay.

Pinned by a `DAT_8007C018[0..2]` write-watchpoint across the auto-starting Queen Bee field→battle transition ([`autorun_battle_party_mesh_install.lua`](../../scripts/pcsx-redux/autorun_battle_party_mesh_install.lua)): all three installs fire at `game_mode 0x15`, and the installed pointers byte-match the battle form (Vahn → `0x80165F48`, the value a battle save holds in `DAT_8007C018[0]`). Dumps `funcs/800513f0.txt` / `800542c8.txt`.

**Still valid:** the 1204 atlases ARE the real battle character textures (confirmed byte-match 73–98% vs a clean full-party battle, shortfall = equipment overlays).

**BATTLE RENDER = LOAD-TIME TSB/CBA RELOCATION (this supersedes the "nominal CBA / no-relocation / VRAM-residue palette" model below, which is FALSIFIED).** At battle entry the party-setup overlay rewrites every prim's TSB+CBA into a packed per-slot runtime band:

**Vahn** (640,0)/(704,0)·rows490/491 → **(512,256)/(576,256)·row481**; **Noa** (640,256)/(704,256)·492/493 → **(640,256)/(704,256)·row482**; **Gala** (512,0)/(576,0)·494/495 → **(768,256)/(832,256)·row483**. CBA column preserved; both disc rows of a char collapse to one runtime row (one 256-colour palette per char). The disc TSB/CBA are an **authoring layout** the Baka Fighter minigame uses directly; normal battles relocate it. Pinned by dumping the runtime TMD (`flags=1`, abs pointers; convert `p→p−base−12`) from a clean battle save and reading its relocated prims — they render the correct characters from the save's VRAM; the disc mesh walked as-is renders incoherently.

The `0x8007BEC0` table (`FUN_800198E0`) is the **scene** renderer's, NOT characters — the earlier reading that routed character CLUTs through it, and the "rows 490..497 are scene-residue party palette / dolk→town01→map01 recipe", are **falsified** (rows 490..497 hold *scene environment* palette shared by a scene's field+battle modes).

**PALETTE — RESOLVED (all three party palettes decode from the disc; see the end of this entry for the solution).** It is a **battle-allocated** resident block DMA'd to rows 481/482/483. In a clean full-party battle save the three blocks are contiguous at **`0x800ebee8`/`0x800ec0c8`/`0x800ec2a8`** (Vahn/Noa/Gala), a fixed **`0x1E0` (480-byte) stride = 15 × 16-colour sub-CLUTs, one per disc mesh object** — matching both the per-object CBA columns read off the runtime TMD and the 15-object disc form.

It is ≠ the field char palette (set test: only 10 of Vahn's 130 battle-novel colours — and **0** of Noa's/Gala's — in any field-pack CLUT) and ≠ the bundled atlas CLUTs = Baka (**146 of Vahn's 256** runtime colours appear in *no* CLUT the 1204 pack ships → a genuinely distinct asset, not a recolour).

**It is character-intrinsic and produced fresh at battle load** (mednafen bracket: name-entry / front-of-Tetsu / load-initiating saves all lack it; it appears as a single copy only once the battle is up, byte-identical between the Tetsu and Drake fights). The work-arena is `memset`-zeroed at load by the `sw $zero` loop at SCUS `0x80055F14` (`base=*(0x8007BD3C)`, `0x1e8d` words), then sparsely filled — the palette sits at `arena_base+0x4048`.

**It is NOT a stored disc blob — exhaustively:** absent uncompressed (full row + every 32-byte sub-CLUT window across all PROT/`SCUS`/`init_data`), not the CLUT of any of 6372 strict TIMs, 0 hits in the LZS-*container* sections of all entries, AND **not the decompressed output of any LZS stream at any offset** in the battle/scene/character entries (town01 bundle `0003..0011`, `0865`/`0867`/`0871..0876`/`0896`/`0900`/`1204`, output windows to 24 KB — past the `0x4048` depth) nor anywhere in the ≤2 MB corpus (1 KB windows). Brute tool: `lzs-decode find` (validated).

Since it is deterministic yet stored nowhere verbatim, it is **assembled at battle entry.** **ASSEMBLER PINNED (write-watchpoint, `autorun_battle_palette_writer.lua`, clean Tetsu fight):** `FUN_80053B9C` (per-colour store `sh a0, 0x894(v0)` at `0x80053C6C`) copies a source CLUT struct `[u16 base][u16 count][BGR555]` into the per-char block at `dst = arena + slot*0x1E0 + (base+idx)*2`, **OR-ing `0xFFFF8000` (STP/bit-15) onto every non-zero colour**. So the runtime palette is bit-15-**set** (`0x9D40…`) and the disc source is bit-15-**clear** (`0x1D40…`) — which is why all prior brutes (bit-15-set needle) missed. Source pointer `s0 = *(*(0x801C92F0)+8) + per-char-off` → a transient `0x800Dxxxx` buffer.

**SOLVED — source = PROT `0861_edstati3`, LZS-compressed (bit-15-clear).** A write-watchpoint on the source struct header `0x800D6C98` shows it is filled by `FUN_8001A55C` (LZS decoder); the decoder's input buffer byte-matches **PROT `0861`** (237-window match, fixed delta — `0861` loads raw, a stream inside it decompresses).

**PALETTE NOW SOLVED byte-exact (all 3 bands).** Running `FUN_80052FA0`'s decode+assembly *as a unit* (decode `record[0]` + the 5 staged sub-records into one work buffer, read CLUTs at the header offsets) reproduces the live Vahn battle palette **byte-exact, all 3 bands** — `base=0x00` = `record[0]`'s CLUT B, `base=0x40` = sub#0's trailing CLUT, `base=0x70` = sub#4's trailing CLUT. The earlier "29/32, 3 diffs = equipment patches" was a **budget-less scratch decoder**, not a data problem: `FUN_8001A55C`'s first arg is an **output-byte budget** (decremented per literal AND per match-copied byte; loop `while budget>0`); ignoring it runs off the stream into the next record. `legaia_lzs::decompress` already honors this, so the port is one `decompress(stream, budget)` per record.

**Source = PROT `0861` (`edstati3`)** — `"data\battle\PLAYER1"` is a dev-tree label that resolves (disc index `char+0x360`, `FUN_8003e8a8`) to the `edstati3` PROT cluster, NOT an ISO9660 file. The record is self-describing relative to `record[0]` (`+0`=desc-table off, `+4`/`+8`=CLUT A/B *decoded* offsets, `+0xC`=budget; descriptor entries `[id, running_a, size]` run while `a[i+1]==a[i]+size[i]`, `id==0` = section separator). On disc the 5 sub-records are **scattered** (Vahn: `0x1C000/0x28800/0x66000/0x85800/0xA2000`), located by `sec_base=align_up(recbase,0x1000)`; sub0..3 = `sec_base + a[entry after each internal separator]`; sub4 = `rec0 + (a_last+size_last)`.

The `0x2000` stride is only the RAM buffer the loader stages — the parser derives the scattered disc offsets directly, **no capture needed**. Every prior byte-brute missed only because it used the bit-15-**set** runtime needle, not the disc bit-15-**clear** form. Clean-room parser **`legaia_asset::battle_char_palette`** (`find_record0` + `parse_record`; synthetic unit test + disc-gated `battle_char_palette_real` which passes byte-exact against PROT `0861`, pinned by an FNV digest so no palette bytes are committed; STP bit-15 set on upload). Tetsu fight is Vahn-only so Vahn (0861) is byte-exact-validated + wired.

**Noa = PROT 0864, Gala = PROT 0865** — pinned by matching each `record0` CLUT (header-read, no derivation) against a full-party battle VRAM capture (mednafen mc1/mc7/mc9 have rows 481/482/483 all populated): Noa→row482 98%, Gala→row483 100% (1-2% misses = equipment patches in the late-game captures).

****Noa WIRED** via `collect_palette` (record0 CLUT A/B + each section separator's id=0 unequipped-default trailing CLUT + the final record, filtered to the columns her mesh samples). The equipment loader (`FUN_80052770` case 4) picks per section an equipment-id-matched entry OR the id=0 separator (unequipped default); the mesh-column filter resolves which variant belongs to the character.

**Gala WIRED — all three party palettes now decode from disc.** Party order confirmed (mc7 char names ASCII at `0x80084708+n*0x414+0x2A7` = Vahn/Noa/Gala → row 483 = Gala).

**Player-file load traced:** the retail ISO9660 open `FUN_800608f0` is a `trap` stub, so `FUN_800558fc` always takes its debug branch → `FUN_8003e8a8(char+0x360)` reads `toc[idx+2]` (in-RAM PROT TOC `0x801C70F0`) as a **sector offset into PROT.DAT**: Vahn(0x361)=PROT.DAT 0x36E8000, Noa(0x362)=0x3791000, Gala(0x363)=0x3828800 (222 sec=0x6F000), Terra(0x364)=0x3897800 — four contiguous player files; extractor entries 0861/0864/0865 begin at those regions.

**The bug:** `sec_base` is `rec0 + align_up(recbase - rec0, 0x2000)` — the `0x1000` alignment matches Vahn/Noa but lands Gala's subs on a zero-padded `0x7000` block (his data starts at `0x8000`). Fixed → Gala's subs decode, bands @0x00/@0x30/@0x50/@0x80 cover all mesh cols at **100%** vs row 483. Wired (slot 2, PROT 865, rows 494/495); disc-gated `noa_gala_collected_palettes_cover_mesh_columns`. Probe `autorun_clut_decode_capture.lua` captured the 5 sub-record streams that pinned this.

**RETRACTION (corrects an over-claim):** an interim reading said the palette was "LZS-decompressed from the `town0c` scene bundle at `0x23430`"; that write-watchpoint actually caught the **scene bundle's** LZS decompression into the *shared* work-arena (the captured `0x800ebee8` value `0x7965481F` ≠ the Vahn palette `0x409d…`). The party palette is a separate, later write; the scene-decompress part holds but is not the palette source.

**Remaining:** write-watchpoint the *final* party-palette write in a clean Tetsu/Drake fight (writer PC + source regs) to recover the assembly. (PCSX-Redux capture is flaky — segfaults intermittently — and the user's bracket saves are mednafen, which can't drive live watchpoints.)

**Viewer status:** the falsified residue scaffolding (`battle_char_true_vram_bytes`, `paint_scene_party_cluts`, `BATTLE_CLUT_SCENES`) is removed; the Battle form renders the 1204 geometry+textures with the bundled (authoring) palette — visually ≡ the Baka form, and labelled as the authoring/Baka palette — until the true per-battle palette is pinned by the overlay capture. `battle_char_mesh_cba_tsb` stays **nominal** (disc CBA, matching the bundled CLUT rows), which is correct for that authoring-layout render.

The party-mesh trace is in `funcs/8002541c.txt` / `800198e0.txt` / `800520f0.txt`. <details><summary>Archived: the (mis-premised) battle-CLUT investigation</summary>**The battle character textures + palettes both come from disc, just by different paths.** **Images:** the PROT 1204 atlases ARE the real battle character textures (not placeholder), uploaded to VRAM pages 512..960 @ y=0/256.

**CLUTs:** sourced from the **active field scene's decompressed sec0 TIM_LIST** (LZS-compressed on disc) — every CLUT a played map01 battle uploads (rows 490/495/496/497/498/499) is byte-present in `0086_map01` sec0 decompressed and renders as a character palette (e.g. row 498 → recognizable Noa face).

**Upload path (fully traced):** `FUN_800520F0` (battle loader) → `FUN_800198E0` (per-TIM uploader) → `FUN_800583C8` (PsyQ `LoadImage`) → `FUN_8005A1C0` (GPU-queue enqueue, op-type 8 = `FUN_80059BD4` via handler table `0x80078D0C`) → ring `0x801C9590` → `FUN_8005A4A0` flush → `FUN_80059BD4` (GP0 0xA0 / DMA2).

**The "relocation" is NOT a per-battle VRAM allocator** — each scene's character TIMs declare their own CLUT rows, the upload puts the CLUT there, and `FUN_800198E0` records `table_0x8007BEC0[texpage & 0x1f] = clut_row`. The battle renderer resolves each primitive's CLUT **row** from this **texpage→CLUT-row table** (`0x8007BEC0`, 32×u16), overriding the TMD2's nominal CBA row (the CBA still supplies the sub-CLUT x). So the party palette band shifts between captures (mc2 492/494 vs map01-battle 490/495..499) simply because different scenes declare different rows for the same character.

**Falsified along the way (do not re-walk):** "PROT 1204 atlases are placeholder" (images are real); "bundled PROT 1204 CLUTs are the battle palettes" (they're wrong defaults, 0/256 vs retail); "the band is loaded by a battle disc read" (battle-init reads are party-independent — `FUN_800520F0` pulls only monster/effects/music); "it's LZS-decoded at battle entry" (`FUN_8001A55C` hook = zero palette hits); "it's a transient buffer not on disc" (it IS on disc, in scene sec0, just not as a contiguous raw blob — and the upload source is the resident decompressed scene buffer, freed only on scene change not per-frame).

**Engine implication:** to match retail, the viewer/engine should source the battle character CLUTs from the active scene bundle's sec0 (decompressed) and apply the per-battle row allocation — NOT from PROT 1204's bundled default CLUTs.

**Viewer-fix limitation (Noa/Gala-present-scene hunt, negative):** only **Vahn's** battle palette is cleanly recoverable — `map01` sec0 row 490 pairs correctly with the 1204 Vahn atlas (world-map Vahn renders in battle-form), but it's just his row 490 (not 491). For Noa/Gala, **no scene's sec0 CLUTs pair with the 1204 battle atlases**: scanning every scene bundle found full-party-ish CLUT rows (0400_doman 488-492, 0061_dolk, PROT 1200 other4 490-494) but rendering the 1204 atlases with any of them yields garbage — those are field-form (PROT 0874) / other-pack palettes, not the battle-form palette the 1204 atlas needs.

So the battle-form Noa/Gala palettes are scene-resident/runtime-composed and not a static disc asset pairing with the atlases; a faithful all-3 viewer fix would need save-state palettes (Sony bytes, disallowed) or a full port of the runtime per-scene character-texture composition. The viewer keeps the bundled CLUTs (the scene-sourced Vahn-only overlay was tried and reverted as net-worse). Tooling: [`autorun_clut_upload_hook.lua`](../../scripts/pcsx-redux/autorun_clut_upload_hook.lua) / [`autorun_clut_upload_watch_live.lua`](../../scripts/pcsx-redux/autorun_clut_upload_watch_live.lua) (live upload `(rect,src)` capture), [`autorun_clut_uploader_pc.lua`](../../scripts/pcsx-redux/autorun_clut_uploader_pc.lua) (read-watchpoint that pinned `FUN_80059BD4`),

[`autorun_find_clut_decode.lua`](../../scripts/pcsx-redux/autorun_find_clut_decode.lua), [`autorun_battle_char_clut_source.lua`](../../scripts/pcsx-redux/autorun_battle_char_clut_source.lua) + [`map_clut_disc_reads.py`](../../scripts/pcsx-redux/map_clut_disc_reads.py); functions in [`reference/functions.md`](functions.md) (`FUN_80059BD4` / `FUN_8005A4A0` / table `0x80078D0C`). <details><summary>Full investigation trail (archived)</summary>The PROT 1204 atlas **IMAGES are the real battle character textures** — not placeholder. (2) Each battle TMD samples a clean, self-consistent `(CLUT row, sub-CLUT, tpage)` set (decoded properly via `tmd_to_vram_mesh`, not the earlier garbage byte-window scan):

**Vahn** rows 490/491 (sub-CLUTs 0,1,4,5 / 0,1,7,8) pages (640,0)/(704,0); **Noa** rows 492/493 (sub-CLUTs 0,1,2,5,6,7 / 0,3,4,8) pages (640,256)/(704,256); **Gala** rows 494/495 pages (512,0)/(576,0); **aux1** row 496 page (448,256); **aux2** row 497 page (512,256). So PROT 1204's atlases are uploaded at exactly the positions the TMDs sample. (3)

**BUT the bundled PROT 1204 CLUTs are wrong DEFAULTS** — direct value comparison of PROT 1204's bundled row-492 CLUT vs the retail mc1 VRAM row 492 is **0/256** and not any channel swap (the viewer renders Noa's pants green where retail is red, hair orange where retail is dark-red — a uniform per-character palette mismatch, not a shader bug). Rendering Noa's atlas with the **retail** mc1 row-492 CLUT yields correct brown skin tones; with the bundled CLUT yields wrong purple/gold.

**Where the correct CLUTs live (open).** Only **Vahn's** row-490 CLUT exists verbatim on disc — LZS-compressed in map01/map02 sec0 as a flag-`0x80000008` 256×1 TIM (the reserved high bit makes `parse_strict` reject it, which is why all TIM tooling + raw greps miss it).

**Noa (492) and Gala (494) palettes are NOT verbatim anywhere** — not in any raw PROT entry, not in any LZS-decompressed player.lzs/flat-streaming section (1204/1205/1206 are uncompressed copies of the same wrong defaults), not in PROT 0874/0876, not in PROT 0865 (battle_data) records. The **CLUT band (rows 490..497, x=0..255) is byte-identical across seven captured save states — six progressive battle-load frames PLUS a separate gobu-gobu battle — and ABSENT in non-battle saves** (mc0/7/8 = 0%): so it is **battle-context-loaded and then persists in VRAM**, not boot-global and not per-battle-recomputed.

It is **never in main RAM** in any captured save (checked every 32-byte sub-CLUT window across all party rows) — a transient **decompress→DMA-to-VRAM→free** upload that completes *before* the "encounter triggered" frame, faster than manual save granularity. The battle scene is **map01** (world map; `*(0x80084540)=0x55`), party Vahn/Noa/Gala, so the non-Vahn CLUTs are pulled by the **battle-entry party-load path**, not the field scene. Per-scene row-49x 16×1 CLUTs (35 scenes incl. town01) are field-actor palettes (0% value match to battle Noa) — a red herring.

**Battle-init disc reads are party-INDEPENDENT** (PCSX-Redux probe, sstate8 Vahn-only vs sstate2 full-party — byte-identical entry set: monster 0x365→867, befect 0x367/8/9=871/872/873, 0x36B=875, 0x380=896, 0x384=900, 0x37A=890, music 1016, field-scene re-read 0x5A).

**No character-CLUT read fires at battle entry** — the party CLUTs are resident in VRAM before the fight. Proper-decode (validated: finds Vahn490 in map01 sec0) of 871/872/873/875 + 0865 battle_data + 1202-1206 + 0874 all empty for Noa/Gala.

**Key state finding:** mednafen mc7 (opdeene) + mc8 (town01) are full-party with band ABSENT (0%) — so the band is *cleared* at certain field transitions and *reloaded* entering battle; the sstate2 probe missed the reload because sstate2 was already band-present.

**DECISIVE — the band is a NON-LZS GPU upload** (PCSX-Redux probes on band-absent slot 4 + battle-initiating slot 5): VRAM dumps show row 490 (Vahn) full but rows 492/494 (Noa/Gala)

**EMPTY at battle-init** — they load later as the battle renders. Hooking the universal LZS decoder `FUN_8001A55C` and scanning every decompressed output for the Noa row-492 signature over 3000 frames of battle (incl. advancing via CROSS) yields **zero hits** — the palettes are never LZS-decoded. Combined with party-independent battle reads + total absence from main RAM (even mid-battle), the band is uploaded by a **LoadImage/GPU-DMA from a source freed within the upload frame** (Vahn's source persists as the field-scene buffer at `0x800e96a0`, the only one ever in RAM).

**UPLOADER PINNED — `FUN_80059BD4`** (LoadImage-equivalent; `a0=RECT{x,y,w,h}`, `a1=src_ptr`; see [`reference/functions.md`](functions.md)), reached via the once-per-frame upload-queue flusher `FUN_8005A4A0`. The [`autorun_clut_upload_hook.lua`](../../scripts/pcsx-redux/autorun_clut_upload_hook.lua) probe hooks its entry and captures every band upload's `(dest rect, source ptr)` + dumps the source.

**Captured (slot 4/5):** rows 488/490/497/498/499 + the row-495/496 effect sub-CLUTs upload from scattered RAM sources (byte-matching mc2 100%); Vahn's row-490 source is the resident field buffer `0x800E9690`.

**Noa/Gala (rows 492/494) do NOT upload at battle-init** — they enqueue only when the party characters actually render during combat, which headless input (CROSS hold/pulse) can't reliably drive (it flees or diverges; live `getVRAM`/`takeScreenShot` are nil/GL-gated in this build).

**Interactive capture done** ([`autorun_clut_upload_watch_live.lua`](../../scripts/pcsx-redux/autorun_clut_upload_watch_live.lua), played the slot-5 fight with all chars attacking): the battle character IMAGES upload via `FUN_80059BD4` (pages 512/576/640/704/768/832/864/960 @ y=0) and band CLUT rows 488/490/495..499 upload too (256-wide rows match mc2's SAME rows 100%).

**But mc2's Noa(492)/Gala(494) palettes appear in NONE of the slot-5 uploads** — so the per-character CLUT **row assignment is battle-context-specific** (this encounter places party palettes at different rows than mc2's did). The uploaded CLUT RAM sources are **not verbatim raw on disc** (490/497/498/499 = 0 raw hits) — LZS-compressed or runtime-composed.

**Cleanest deterministic finish (no more emulator runs):** Ghidra-trace the **enqueuer** that pushes character CLUTs into `FUN_8005A4A0`'s ring during battle-actor render (reveals the per-character source + composition rule + disc origin), or match each captured CLUT RAM-source address against the LZS-decompressed scene/befect buffer resident there. Other tooling shipped: [`autorun_battle_char_clut_source.lua`](../../scripts/pcsx-redux/autorun_battle_char_clut_source.lua) (disc-read logger), [`map_clut_disc_reads.py`](../../scripts/pcsx-redux/map_clut_disc_reads.py), [`autorun_find_clut_decode.lua`](../../scripts/pcsx-redux/autorun_find_clut_decode.lua) (LZS-output scanner),

[`autorun_clut_uploader_pc.lua`](../../scripts/pcsx-redux/autorun_clut_uploader_pc.lua) (read-watchpoint that pinned the uploader).</details></details>


### MP-cost ability-bit priority (half vs quarter)

*Status:* resolved (dump-confirmed)

Reading the state-`0x28` block in `overlay_battle_action_801e295c.txt` (`0x801E3D0C`; the same block recurs in state `0x3C` at `0x801E4568`) settles **both** open questions. (1)

**PRIORITY — Half (`0x20`) wins.** The code is `andi 0x20; bne <half>` then `andi 0x10; beq <none>`, i.e. `if (bits & 0x20) {half} else if (bits & 0x10) {quarter}` — the `0x20` test short-circuits the `0x10` test. This matches the docs / `MpCostModifier::from_ability_flags`; the engine SM port + live cast path that applied Quarter first were a guess and are now flipped. (2)

**FORMULA — it subtracts a right-shifted copy, not a floor-divide.** Half = `cost - (cost>>1)` (rounds up on odd costs); "MP-quarter" = `cost - (cost>>2)` = **pay 3/4** (shave 25%), NOT `cost/4`. The engine's `base_cost/2` / `base_cost/4` were both corrected (`battle_formulas::mp_cost_after_ability_bits`); all three cast paths (two SM blocks + `cast_spell_on_slots`) now route through the shared helper. MP cost consumes no RNG, so determinism oracles are unaffected.


### Scripted Tetsu encounter → Battle (v0.1 oracle Battle leg)

*Status:* mostly

The v0.1 oracle now reaches **Battle** from a NEW GAME cold boot: `BootSession::begin_new_game` seeds the opening party (Vahn, 180 HP) — the Tetsu fight is the game's first battle, so the new-game state *is* retail's pre-fight story state (there is no earlier save to seed from) — the cold boot installs town01's sparring carrier from its MAN, and the field-VM dialogue-accept engages it (`v0_1_playthrough.rs::v0_1_battle_leg_reaches_battle_from_new_game`, converging with the cataloged retail Field/Battle anchors). Earlier framing (below) assumed a save-seed was needed; it is not, for the opening fight. The formation is pinned — a lone monster, archive id `0x4F` (Tetsu), `EncounterRecord::rim_elm_training()` — and reachable end-to-end via the arm API (`training_battle.rs`).

The launch mechanism is pinned (`FUN_801DA51C` decomp + corpus RAM): the encounter carrier is a **dedicated MAN-placed field entity** (not the player ctx) that, on reaching SM state 1, copies its `entity[+0x94]` formation into cell `0x8007BD0C` and via the `case 2/3` fall-through writes `_DAT_8007B83C = 8` (the battle handoff). It is **dialogue-driven, not scene-entry-driven**, and **not a script-borne inline arm op**: an opcode-aware walk of town01's MAN partition-1 scripts finds zero `[1][0x4F]` arm sites, so the carrier installs **town01 MAN formation index 4** by pointing `actor[+0x94]` at that table row. The carrier is pinned to town01 P1's placement at tile (76, 65) / model `0x6A` (the sparring partner).

**Engine:** the field-carrier SM tick exists (`tick_field_carriers` / `install_field_carriers` / `engage_field_carrier`) and reaches Battle via formation index 4 (`training_battle.rs`); the carrier set is now **derived from the scene MAN** (`man_field_scripts::derive_field_carriers` + `World::install_field_carriers_from_man`), so the sparring carrier's identity and placement come from the real actor-placement partition. The engage is now **driven by the field-VM dialogue-accept**, not a manual API: a field-interact op (`0x3E`, `op0 < 100`) on the carrier's placement arms the engage (`World::field_carrier_slots` → `pending_carrier_engage`) and accepting its prompt (the `0x4C` n5 sub-4 dialog dismiss) engages it.

`training_battle.rs` drives this end-to-end on disc data, reaching Battle with Tetsu without `engage_field_carrier`. The interaction probe is now ported faithfully: `World::tick_field_interaction_probe` (clean-room `FUN_801cf9f4`) runs retail's `DAT_801f2254` facing probe — a radius-64 compass point ahead of the player's facing, box-tested at ±72 against the talkable NPCs' placement positions (`World::field_npc_positions`) — and on the action button talks to the matched NPC and turns the player toward it, so facing the sparring partner and pressing X starts the fight with no script injection (`training_battle.rs::training_reaches_battle_via_interaction_probe`).

This relies on the **runtime actor frame == MAN placement frame** finding: `FUN_8003A1E4` spawns at `tile*128 + 0x40` via `FUN_80024C88` with no anchor, and the player cold-spawn `0xA40` is `tile 20*128 + 0x40` in that same frame (the apparent mismatch in the mc6 capture was a *patrolling* NPC).

**Auto-navigation now closes the emergent path:** `World::nav_step_toward` drives the player along a BFS route over the real collision grid, so the v0.1 oracle's emergent Battle leg (`v0_1_playthrough.rs::v0_1_battle_leg_walk_talk_accept`)

**walks** the player from the cold-boot spawn to the partner, **talks** via the probe, and **accepts** → Battle, with no teleport.

**Carrier-reposition finding:** the carrier's MAN placement tile `(76, 65)` is its *post-tutorial* village spot — in a town01 sub-area NOT walk-reachable from the spawn (BFS: 2855 reachable sub-cells, carrier not among them; town01's MAN spans several door-connected sub-areas). The opening sequence repositions the partner next to Vahn for the tutorial (`RIM_ELM_SPARRING_CARRIER_TUTORIAL_POS` = world `(2752, 1856)` ≈ tile `(21, 14)`, a ~6-tile reachable hop, pinned from the dialogue-accept capture whose `actor[+0x90]` resolves to the `(76,65)`/`0x6A` record — same carrier). The cold boot skips that reposition, so the emergent test places the carrier at its tutorial position first.

**What remains:** deriving that opening reposition from the opening sequence itself (vs the pinned tutorial constant); and the dialogue box's Yes/No selection logic, still undecoded (the engine treats accept as dismiss — faithful for the forced tutorial, which has no decline path).

## Field / locomotion

| Thread | Status | What would close it | Memory |
|---|---|---|---|
| Town/field free-movement locomotion | resolved | [details ↓](#townfield-free-movement-locomotion) | `project_field_locomotion_integrator.md` |
| Field collision-map source | resolved | [details ↓](#field-collision-map-source) | `project_field_locomotion_integrator.md` |
| Tile-board grid mode | resolved (re-scoped) | The `_DAT_8007b450`/`DAT_801f35c0`/`801ef2b0` tile-grid walk is a puzzle / board minigame (procedural `rand`-filled board, per-cell drawn tiles), not town locomotion. Documented in `docs/subsystems/tile-board.md`. Open sub-questions: which minigames use it; whether any board is fixed (inline-script cells) vs. always procedural; the inline cell-array offset. | `project_tile_board_grid.md` |
| game_mode 0x03 = field/town gameplay | resolved | [details ↓](#game_mode-0x03--fieldtown-gameplay) | `project_mode_table_structure.md` |
| Engine VRAM byte-exactness for town01 | resolved (major source); minor residue | [details ↓](#engine-vram-byte-exactness-for-town01) | `project_town01_targeted_upload_fix.md` |

| Scene-transition (`0x3F` door) destination indexing | resolved | [details ↓](#scene-transition-0x3f-door-destination-indexing) | `project_scene_destination_table_indexing.md` |
| Intra-town (house / interior) door mechanism | resolved | [details ↓](#intra-town-house--interior-door-mechanism) | `project_intra_town_door_mechanism.md` |
| Field/town environment-geometry placement | resolved (renders) | [details ↓](#fieldtown-environment-geometry-placement) | `project_town_geometry_render_gap.md` |


### Town/field free-movement locomotion

*Status:* resolved

The player free-movement controller is `FUN_801d01b0` (field overlay 0897), pinned by a runtime write-watchpoint on `*(0x8007c364) + 0x14/0x18` (`autorun_player_pos_watch.lua`). It camera-remaps the held pad (`func_0x800467e8` + `FUN_80046494` → direction bits `& 0xf000`), computes a per-frame speed (`base_step * player[+0x72] >> 12 * DAT_1f800393`, with terrain-slow + diagonal modifiers), then steps the player position 2 units at a time with per-axis collision via `FUN_801cfe4c`. Sets facing `player[+0x26]`. Full write-up in [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md). The `801db81c..801dbf9c` cluster previously suspected here is the field *camera* system, not movement (see the camera notes in `project_field_camera_and_region_table.md`).

**Collision derivation — RESOLVED (capture-proven; engine realigned).** `FUN_801cfe4c` is fully decoded (overlay `0897` @ `0x801CE818` + the on-disc bias table `DAT_801f2214`): three **leading-edge footprint probes** (~47 units ahead, ±16 lateral), each sub-cell derived as `zc = (z>>6)+2`, `xc = ((x+0x3f)>>6)−1`. Two cheat-free Rim Elm wall-press captures settled the long-open indexing question:
**The `+2` Z bias is authored into the wall bits.** In the down-press capture (`rimelm_wall_press_down`, screen-down = world `Z−`) the player legally rests at a position whose plain floor-indexed cell is an all-quads wall byte (unreachable under floor indexing); the biased read places that wall band one tile north, exactly where the press blocks with a step-exact 47-unit standoff. The left-press capture (`rimelm_wall_press_left`) pins the X side: probe reads the wall column's last sub-cell, one 2-unit step shallower reads clear; retail's `ceil−1` equals the floor except at exact 64-multiples (parity-unreachable). The **floor sampler** (`FUN_80019278`) reads the *same bytes* with plain floor indexing — one byte's two nibbles live under two world→cell mappings.
**Engine realigned with proof in hand:** [`World::field_tile_is_wall`] now uses retail's exact derivation (`sample_field_floor_height` keeps the floor, matching its own retail source). **The three-probe leading-edge footprint is wired too** (`World::field_dir_blocked` over the disc-pinned `DAT_801f2214` table — 48-unit edge in the positive directions, 47 in the negative, ±16 lateral — gated by `World::leading_edge_wall_probes` / `play-window --edge-collision`; the candidate-centre test stays the off-flag default for the oracles + nav drivers): driving the engine stepper over each capture's live grid reproduces both retail rest positions **byte-exactly** — and the full-scene legs reproduce them through a real `enter_field_live` scene entry.
**The actor-collision probe is decoded, modelled, and capture-classed.** `FUN_801cfc40` (bits `1`/`4`) walks the active-actor table `DAT_801c93c8`, box-testing the three `DAT_801f21b4` probe points (disc-pinned: 64/63 ahead, ±32 lateral — wider than the wall edge) against each actor: a static entity anchors at its MAN object record (`tile*128 + sub*16`) with the `0x40+0x10` half-extent; a moving actor uses its live position with caller extents (`±40` from the locomotion). The locomotion gates each 2-unit step on the actor bits and the wall bit together, so NPCs block exactly like walls.
The `rimelm_npc_press_tetsu` capture (player pressed into the sparring partner) pins the class from live RAM: the mutual `+0x98` collision link is active in-frame both ways and the NPC's `flags+0x10 = 0x08020884` carries the `0x20000` bit — **village NPCs take the moving-actor arm (bit `1`, ±40 box)**, not the static prop arm. Engine: `World::field_actor_dir_blocked` ports that arm over `field_npc_positions`, gated by `World::solid_field_npcs` / `play-window --solid-npcs`; disc-gated leg `npc_press_pins_moving_actor_arm`.
**The touch/interact dispatch and the static prop arm are decoded and modelled too.** `FUN_801d5b5c` (decoded from a live overlay image — the static 0897 copy is garbled at that VA) posts the touch event: player engaged flag `0x80000`, actor touched mark `0x100`, counters, facing saved to `+0x5A`, and the `FUN_8003c9ac` NPC-motion pause kick. The dispatch in `FUN_801d01b0` fires it automatically per contact step for static props (bit `4`), and on the just-pressed interact button through the third probe table `DAT_801f2254` (disc-pinned at overlay file `0x23A3C`: a radius-64 compass point per 45° facing sector, extents `0x20` → ±72 NPC box) for NPCs — with a face-the-NPC turn (`func_0x80019b28`).
The static-entity anchor formula (record footprint offset incl. the `+0x52 & 8` correction from record flag bit `0x8`) is live-verified against four captures' spawned static actors; the engine models props via `Scene::field_object_placements` collider centres (`field_prop_colliders_live.rs`) and the interact probe via `World::field_interact_probe_slot`.

**Residual (open):** NPC motion (the engine's NPCs stand at their anchors, so live position == anchor) and the prop walk-touch **event** semantics engine-side (props block but the engine has no prop event scripts to fire). The interaction-end teardown is decoded: the dialog SM `FUN_80039b7c` exit path restores the actor facing from `+0x5A`, drains the `+0x2A`/`+0xA` touch-counter pair, and clears the player's `0x80000` engaged flag + `ctrl+0x60` when no interactions remain.
Disc-gated: `engine-shell/tests/field_collision_discriminator.rs` (probe-model + engine-rest legs); unit equivalence `world.rs::tests::field_tile_is_wall_matches_retail_subcell_derivation` + standoff `leading_edge_wall_probes_rest_at_retail_standoff`. Capture note: both wall-press sessions park in `town0c` holding a grid that byte-matches town01's — **resolved, not an anomaly**: town0c's own `.MAP` (PROT 0019, the universal `define−2` resolution) is byte-identical to town01's; PROT 0028 is `izumi`'s map, not town0c's (see the field `.MAP` resolution row below).


### Field collision-map source

*Status:* resolved

The collision grid at `*(_DAT_1f8003ec) + 0x4000` (1 byte/128-unit tile, high nibble = 4 sub-cell wall bits) is **painted by the field-VM `0x4C` opcode, outer-nibble 7** (`op0` ∈ `0x70..0x7F`, handler `0x801e1c64`): a rectangular wall-paint with inline operands `[4C, 0x7s, col0, row0, col1, row1, mask]`, sub-op = clear-walkable / block-all / clear-mask / set-mask. So collision walls are authored in the scene event script (not a separate disc blob) — same inline-operand pattern as encounters / tile-board.

The `+0x4000` byte's **low nibble is a floor-elevation tier** — a 4-bit index into a 16-entry `short` height LUT at scratchpad `0x1f80035c`, filled at scene entry by `FUN_8003aeb0` from the MAN header (`_DAT_8007b898+2`, 16 negated values) and consumed by the object spawn iterator `FUN_8003a55c` to offset each placed object's Y. The `+0x8000` region is **not** a terrain-flag grid (corrected) — it is a per-tile `u16` object/attribute map (low 9 bits = object-record index into the `+0x0000` table; bit `0x400` = footprint flag ORed in by `FUN_8003aeb0` from field-pack records). See [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md#where-the-collision-grid-comes-from).

Residual sub-question: the `+0x4000` zero-init site (ruled out `FUN_8001f7c0` / `FUN_8003a024` / `FUN_800513f0`; likely a wholesale memset by the scene-boot allocator). Town01 parity confirmed by game-mode binding (Rim Elm = `town01` runs at mode `0x03`, same as the runtime-pinned field `map03`).


### Field `.MAP` PROT resolution — `define − 2`, universal

*Status:* resolved (census-pinned; engine resolver corrected)

A scene's field `.MAP` PROT entry is the one **two slots before its CDNAME block start** (`define − 2`), identified by its `0x12000` extended footprint — for **every** field scene, not just the kingdom walk views where the rule was first pinned. The scene PROT clusters overlap by two entries, so the per-entry extractor attributes each scene's `.MAP` to the *previous* block's tail; the first `0x12000` entry **inside** a block is the **next** scene's map.

Pinned by a save-library census (`crates/engine-shell/examples/field_grid_census.rs`): each save's live field buffer (scratchpad `_DAT_1f8003ec` → `+0x4000` grid) classified against candidate on-disc bases. The `keikoku` sessions match PROT 0109 (`define 111 − 2`) with **zero** diffs while the in-block candidate 0118 differs by 3855 bytes; `koin3` matches 0559 exactly (in-block 0568 differs by 531); town01 sessions match 0010 ≡ 0001 exactly. A corpus sweep confirms the structure corpus-wide: every block's in-block `0x12000` hit is exactly the *next* block's `define − 2` entry.

The **object-index grid** (`+0x8000`, the `Scene::field_object_placements` / `field_terrain_tiles` source) is live-validated the same way: residuals of 0..96 bytes against the resolved entry across town01 / town0c / keikoku / koin3 sessions (story-conditional cell mutations — opened chests, prescript object toggles), thousands against every other candidate. Regression-guarded by the disc + save-library gated `engine-shell/tests/field_map_object_grid_live.rs`, which also re-falsifies the in-block rule against live RAM on the placement region for the discriminating scenes.

Consequences: (a) `Scene::field_map_index` now resolves `define − 2` (it previously picked the in-block entry — the **next scene's map** — for every field scene, masked only on town01 where the adjacent Rim Elm variants byte-copy, the one scene it had been validated against; `walk_field_map_index` is now an alias). (b) The town0c "cold `.MAP`" question **dissolves**: town0c's `.MAP` is PROT 0019, **byte-identical** to town01's (0001/0010) — the wall-press captures' "town01 buffer in a town0c session" is simply town0c's own map. (c) "PROT 0028 = town0c's different `.MAP`" is a misattribution — 0028 is `izumi`'s (`define 30 − 2`). (d) The kingdom "in-block decoy" framing is superseded: the decoy is the next scene's continent.


### game_mode 0x03 = field/town gameplay

*Status:* resolved

`_DAT_8007B83C` = 0x03 is the in-town / on-field gameplay mode. Pinned empirically by two independent retail captures: the `v0_1_pre_battle_tetsu` save (Vahn walking in Rim Elm / `town01`, before the Tetsu cutscene) and the runtime-pinned free-movement controller on `map03`, both at 0x03. `engine_core::mode::GameMode::scene_mode()` maps `MainMode (3) → SceneMode::Field` accordingly, and the `mode_trace_e3` + `v0_1_playthrough` oracles drive the engine into the field (`enter_field_live`) so they converge against the retail 0x03 snapshot.

**Handler map RECOVERED.** The index → handler/param/name map is now read straight off the disc by [`legaia_asset::mode_table`](../../crates/asset/src/mode_table.rs) (`asset mode-table`; disc-gated `mode_table_real`), so the dispatch is no longer guessed from the misleading dev names.

It confirms the saves: field/town is modes 2/3 MAIN (`game_mode 0x03`), and `MAPDSIP` (12/13) is the **world-map display** mode, not the field — correcting an earlier `functions.md` label that called mode 12 "the actual gameplay-mode entry". Structural finding: 12 of the 14 per-frame modes share the generic per-frame handler `0x80025EEC`; only Mode 13 (world-map) and Mode 23 (memory card) carry their own. Full map in [`boot.md`](../subsystems/boot.md#full-handler-map-recovered-from-the-disc).

**The in-field pause menu = mode 23 (CARD pair).** All six menu-open library captures (equipment / status / options, field `map01` + town `town01`) hold `_DAT_8007B83C = 0x17` — the pause menu runs under the CARD (menu / memory-card overlay) per-frame mode, not field mode 3 (the manifest's earlier `expected_game_mode = 0x03` rows were stale; corrected). Open residue: `BootSession` doesn't host the field-menu UI stack (it lives in the windowed host), so the `mode_trace_e3` oracle asserts menu scenarios on active-scene convergence only until a menu mode is modelled headlessly.

**Engine model reconciled.** `engine_core::mode` holds `SceneMode::Field` for both modes 2/3 (the init mode holds its successor's scene mode, matching the Mapdisp/Battle/Str pairs), the reference handler that drives the pair is named for the field-entry path it exercises, and the table's name/param/next fields are cross-checked against the disc-recovered map by the disc-gated `mode_table_reconcile` test. The retail `+0x0A` next-mode field is decoded (`ModeEntry::next_mode`): `-1` = self-managed, `0` = fall back to mode 0 — the `0xFFFF0000` word previously read as a sentinel is just `-1` over a zero low half.


### Engine VRAM byte-exactness for town01

*Status:* resolved (major source); minor residue

Single-snapshot byte-exact VRAM is **physically unachievable** — ~40% of the texpage band is dynamic/residual (two town01 captures disagree on ~40%), so the oracle (`vram_oracle_e1`) is reframed to the **static mask** (words stable across same-scene captures), excluding the runtime NPC/character CLUT band. With the field pre-pass doing DMA-every-TIM (`BuildOptions.upload_all_tims`), town01 passes byte-exact on every static pixel it uploads. The dominant missing static block is the **`befect_data` (PROT 0874) section-2 effect-texture TIMs** (`etim.dat`, 4bpp pages at `fb(320/384,256)` etc.) — field-resident, pixel-matched 256 rows byte-exact; the live engine uploads them at field entry (`scene::upload_effect_textures_into_vram`),

and the gap was an oracle artifact (the lightweight pre-pass skipped that step; now fixed, image pages only, since retail uploads their CLUTs at battle entry).

**Negative finding (don't re-walk):** the menu-glyph atlas (`PROT.DAT[0x11218]`) is **menu-time-resident, not boot-resident in field VRAM** — uploading it flags a wrong static texel at `(960,400)`.

**Minor residue (open):** `x=896..1024, y=256` (~12k) is the character/party-texture region uploaded by the battle/character targeted-CLUT pass the field pre-pass excludes by design (the CLUT-scattering thread), plus ~2.5k UI residue.

**Per-scene mask premise refined (map01 false red resolved).** Two capture-pinned failure modes of "stable across same-scene captures = static": (1) the `befect_data` band is **global, history-dependent** state — a few pixels boot with a variant that differs from the disc copy until a battle re-uploads the disc bytes (pinned at `(853,271)`: pre-battle/menu captures hold `0xFFFF` words, the disc TIM and every post-battle capture hold `0x3333`), so same-lineage captures misclassify them as static; the oracle now demands cross-scene staticity inside `scene::effect_texture_image_rects`. (2) the world-map walk view **palette-cycles** the kingdom terrain CLUT rows 506/508/509 in place; `vram_oracle::WORLD_MAP_CLUT_CYCLE_ROWS` excludes them for world-map scenes (see the two open threads below).


### World-map CLUT cycling beyond the ocean head — rows 508/509 + generated row-506 tail

*Status:* open (effects pinned from captures; sources unlocated)

The row-506 **head** (entries 0..15) is the documented 13-frame ocean CLUT animation (`legaia_asset::ocean`, engine-implemented — see [`world-map.md`](../subsystems/world-map.md) "Ocean animation"); a capture holds an arbitrary phase, never the disc base CLUT.
Capture evidence (map01 overworld vs field-menu states) shows the runtime rewrites **more** than that head: rows 508 and 509 each cycle a few entries in place (shoreline shimmer inside the mountain/forest terrain palettes), and row 508's entries 32..47 mirror the live frame of its own 0..15 head.
Row 506's tail (entries ~40..47) additionally holds a **runtime-generated palette** — pure-channel BGR555 combos (B/G/BG/R/BR/GR at intensity `0x11`) present in **no** disc bundle (all 7 kingdom-bundle slots of PROT 0085 + the PROT 0093 overview pack swept).

Open: whether the 508/509 entries + the 32..47 mirror ride the same 13-frame ocean DMA (a wider rect?) or a sibling writer; and the writer + purpose of the generated tail palette (marker/route colours?). VRAM writes aren't RAM-watchable; needs a GPU `LoadImage`-level trace or an overlay sweep for CLUT-row rect constants (`y = 506/508/509`). Engine residue: `play-window` animates the row-506 ocean head only; the exact retail cadence is also still unpinned.


### `befect_data` boot-variant pixels — who uploads the `0xFFFF` row?

*Status:* open

A freshly booted game holds a variant of the `befect_data` effect-texture band whose row-271 pixels (fb_x 852 page) read `0xFFFF` where the disc TIM (PROT 0874 §2) carries `0x3333`; the first battle re-uploads the disc bytes and the disc value then persists (town01 pre- vs post-battle captures discriminate; town0c post-battle-lineage captures all hold the disc value). The disc TIM's row 273 holds the `F`-variant of the same row, so the boot-time source may be a sibling copy or a one-row blit. Open: which boot/new-game path uploads the `F`-variant (a different disc copy? an effect rendering into the page?).


### Scene-transition (`0x3F` door) destination indexing

*Status:* resolved

A field scene reaches another scene through the field-VM **`0x3F` named-scene-change** op, which carries its destination scene name inline.

**Pinned by a live PCSX-Redux dispatch trace** (`autorun_door_dispatch_trace.lua` on the `drake_castle_to_worldmap` capture): the `0x3F` ops are **partition-2 MAN records** reached through the **partition-2 record-offset table** — the controller sets the VM bytecode base to `man_base + data_region + partition2[slot]` and runs the record by fall-through (decisive: `a0 - man_base == data_region + partition2[0]` exactly). Selection is by stable slot index, so the op's `index` field is only the destination-scene id passed to the warp packet (`FUN_8001FD44`). Corpus census (clean partition walk): 160 dest ops / 48 scenes, 153 in partition 2, **zero absolute-reference ops** at/after any dest op.

This made **variable-length** door editing safe (resizing a destination name is a partition-table + section-offset + intra-record-jump-delta + descriptor-size fixup), implemented in `legaia_asset::man_edit` and shipped as the door randomizer. See [`man-relocation.md`](../formats/man-relocation.md).

**Still separate (untouched):** the `0x3E` door-warp (7-id scene-*type* `map_id`) name resolution lives in an uncaptured handler.


### Intra-town (house / interior) door mechanism

*Status:* resolved

Entering a house in a town is **not** a scene change — it's an **intra-scene reposition**: the field VM runs a **`0x23 MOVE_TO`** op that teleports the player to an interior sub-area tile within the *same* loaded scene (the scene-name buffers `0x8007050C`/`0x80084548` stay put across the transition; only the player struct position jumps). Pinned at the instruction level by the new `probe.step.find_writer` Lua primitive (a width-correct range write-watch over the player position block): the writer lands in the field-VM dispatcher `FUN_801de840` **`case 0x23`** (`0x801debc4 sh v0,0x14(s5)`), converting the tile operand to world (`tile*128 + 0x40`).

Earlier write-watchpoints missed it (a width-2 watch at `+0x14` caught only a 2-byte no-op re-store in the ledge-hop `FUN_801d1878`, a red herring). Captures: `door_warp_rim_elm_to_mei_house`/`mei_house_inside` (mednafen), `mei_house_door_pcsx`/`mei_house_inside_pcsx` (PCSX). The `0x23 MOVE_TO` op is shared with NPC/cutscene movement (no clean door marker), so the randomizer (`legaia_rando::house_door`) does a per-scene multiset-preserving shuffle of the non-sentinel target tiles.


### Field/town environment-geometry placement

*Status:* resolved (renders)

The town's environment meshes (terrain + buildings + props) are object-local Legaia TMDs in the **LZS streams of the scene_asset_table** PROT entry (`town01` = entry 4). Placement is `FUN_8003a55c`: the field-map object-index grid at `+0x8000` (`cell & 0x1FF` = object id) selects a `0x20`-byte record in the `+0x0000` table; placed tiles (record `+0x12` bit `0x4`) give the world transform (`world_y = -floorHeightLUT[nibble] + y_off`, the LUT being 16 `s16` at the MAN header `+0x02`). Mesh per object (byte-verified): `pack_index = obj_idx - 5` for the field-actor band `93..=118`, else record `+0x10`; ids `1/2/3` are protagonist/NPC meshes from the shared pool; `anim_id` only animates. Validated against a live `town01` save (Vahn's house id `137` → mesh 36; windmill id `96` → mesh 91).

Parser `legaia_asset::field_objects`; `Scene::field_object_placements`; `play-window` renders the town via `resolve_field_placement_draws`. Full field decode in [`field-locomotion.md`](../subsystems/field-locomotion.md#object-record-format-0x0000-0x20-byte-stride).

**Open (minor):** of 46 placements, the field render now draws **40** (the 2 untextured props were recovered by the vertex-colour path, see (a) below); the remaining **6** that don't draw are all one missing-CLUT mesh. The historical "**8 of 46** drop" split is pinned by cause, and the earlier "all 8 are fully-untextured props" reading is **corrected**. They split into TWO unrelated causes across **3 distinct env-pack meshes** (disc-gated `town01_dropped_placements_split_untextured_vs_missing_clut`):

**(a) 2 placements** (meshes pack `31`/obj `315` with 30 untextured prims, pack `109`/obj `114` with 12) are genuinely **untextured (per-vertex-RGB) props** — the textured-only builder `tmd_to_vram_mesh_filtered` skips prims with no UVs (`mesh.rs` ~line 508), so a flat/gouraud-only mesh builds empty and is dropped at `res_to_mesh[res_idx] == None`; **(b) 6 placements** (one mesh, pack `74`/obj `347`) are **textured** but every one of their 4 prims is dropped for **`MissingClut`** — the field VRAM pre-pass didn't upload that CLUT row. Neither is a filter *bug* (a mesh whose textures aren't resident *should* drop rather than draw flat `CLUT[0]`),

and the two need **different** fixes: (a) the **per-vertex-RGB props are now rendered** — the untextured-prim colour block is fully RE'd (the per-mode record layouts F4/G3/G4 + the `00 01 03 02` quad winding remap + the negative "no per-prim normal" result, see [`tmd.md` § Per-prim color / texture block](../formats/tmd.md#per-prim-color--texture-block)),

`legaia_tmd::legaia_prims` decodes the colours into `Prim::colors`, `legaia_tmd::mesh::tmd_to_color_mesh` builds a standalone `ColorMesh` from a TMD's untextured prims, and `engine-render` has a dedicated vertex-colour pipeline (`upload_color_mesh` / `Scene::color_draws`) that play-window draws for the dropped props (so town01 recovers the 2 untextured placements → 40/46; pinned by `field_object_placement_disc::town01_dropped_placements_split_untextured_vs_missing_clut`); (b) wants the **missing CLUT row uploaded** (a VRAM-coverage question, sibling of the town01 static-VRAM residue thread — a per-vertex-RGB fallback would render (b) *wrong*, so it stays dropped).

Mixed meshes (some textured + some untextured prims) now render **both** halves: the colour mesh is built unconditionally and is disjoint from the VRAM mesh (`tmd_to_color_mesh` skips textured groups), so a mesh's textured prims go to the VRAM pipeline and its untextured prims to the colour pipeline at the same placement (previously the colour mesh was built only when the whole textured build was empty, dropping the untextured half of a mixed mesh). Only (b) remains (the missing-CLUT runtime upload); the split + counts are pinned by the test above.

## Text / fonts / dialog

| Thread | Status | What would close it | Memory |
|---|---|---|---|
| Dialog font extraction | done — kept for reference | Earlier "blocked on runtime trace" framing was wrong; tile-page lives at VRAM `(896, 0)..(960, 256)`, extracted by `legaia-font::font-extract` from any in-game save state. Listed here only so the older "open" framing doesn't get re-opened. | `project_dialog_font_hunt.md` |
| Inline dialog-box format (`0x1F`-lead segments) | resolved | [details ↓](#inline-dialog-box-format-0x1f-lead-segments) | — |


### Inline dialog-box format (`0x1F`-lead segments)

*Status:* resolved — prologue + pager-side dispatch + option-list inner format + multi-segment box packing all pinned

Placement-NPC / event dialogue text is **inline** in the field-VM interaction record, **not** the scene MES — the opcode-decoded `text_id` is a box-config id that never resolves through `SceneMes::message_offset` (0/13 town01 placement-NPC ids resolve). The text is a run of `0x1F`-lead / `0x00`-terminated segments of MES glyph bytecode. It is recovered **structurally**, not from the `0x3F` op's `len` field: a text-heavy field interaction record desyncs under linear disassembly (a literal `>` is `0x3E`, the warp/interact opcode; ASCII punctuation hits the `0x37`/`0x41` yield bytes), so the decoded `0x3F` op and its `len` are unreliable on field scenes and the byte-`len` capture returned **empty for every town01 NPC**.

`man_field_scripts::first_inline_dialog_offset` finds the first printable `0x1F` segment (printable-ratio gated), `classify_placement` carries the record bytes from there as `PlacementKind::Npc::dialog_inline`, and `OwnedDialogPanel::from_inline_dialog` types the prompt segment; the native `play-window` renders the box. With this, **36 town01 placements recover renderable dialogue** (the sparring partner, Meta the dog, villagers, leftover "dummy" dev placeholders, and the `0x1F`-segment developer story-flag toggle menu at placement P1[1]).

**Segment-pool structure pinned:** the segments are **not** "prompt + option labels" of one box. `dialog::decode_inline_segments` recovers the full `0x1F`-lead pool, and decoding real town01 placements shows each record holds the NPC's *entire* dialogue line set — every line across every story-state branch, with `"Yes"`/`"No"` option labels interspersed (e.g. the Village Elder decodes to 80 segments, Val to 59, both carrying multiple `Yes`/`No` pairs; disc-gated `field_actor_placements_disc::inline_dialogue_decodes_into_full_segment_pool`). So `0x1F` segments are individual lines, *not* page-break-delimited boxes — multi-page speech is multiple `0x1F` segments, not `0x80..=0x9F` control bytes within one.

**There is NO separate "box-geometry header" format (falsified):** the bytes between the placement's `script_pc0` and the first `0x1F` are normal field-VM bytecode — `CFlag` / `SysFlag.Test` / `JmpRel` / `Nop` / `0x4C 0x51` NPC-move-to-tile / `0x4C 0x52` menu-activation poll — that runs as the NPC's interaction prologue (face the player, set conversation flags, walk to the talk position, branch on story flags).

The retail SM `FUN_80039B7C` state 0 calls the field-VM dispatcher `FUN_801DE840` directly on this stream and transitions into the pager only when the dispatcher leaves the actor's PC on a byte where `& 0x7F < 0x20` (a `0x1F` lead or `0x21` terminator); the "select which segment to start at" mechanism is the prologue's own story-flag-gated `SysFlag.Test` branches — the script `JmpRel`s past unwanted segments to the desired one.

Pinned by `field_disasm::LinearWalker` decoding the prologue cleanly across every classified town01 dialog NPC once nibble-5 sub-1/sub-2 are covered (disc-gated `field_actor_placements_disc::dialog_prefix_decodes_as_field_vm_bytecode`); the earlier "candidate decoder among `FUN_8003AB2C` / `FUN_8003BDE0`" framing is falsified — both are known: `FUN_8003AB2C` is the per-frame field-VM driver and `FUN_8003BDE0` is the partition-record dispatcher (both already ported).

**`FUN_8001ebec` is NOT the renderer** — disassembly shows it's a per-character TMD-pose copier (party slots 0..2, indexed by the slot-4 freeze flag `_DAT_8007B824`, copies 7 u32s of pose data from TMD offsets `+0x124..+0x140` or `+0x140..+0x158` gated on a record flag at `+0x75E`); the earlier reference to it as the dialog-box renderer in the engine + this thread is wrong (corrected in [`subsystems/script-vm.md`](../subsystems/script-vm.md) op `0x4C` sub-3 sub-F note). The real per-actor dialog SM is `FUN_80039b7c` (advances `actor[+0x9c]` 0→1→2 through `0x1F`-lead segments, consumes the `0xC?` 2-byte escapes); the pager is `FUN_801D84D0`.

**Pager-side dispatch now decoded:** the box geometry is fixed at `_DAT_801F2740 = 3` lines per box at both init arms (`case 6` / `case 9`), and the post-page state `0x19` reads the **next control byte past the box** to pick the follow-on state — `0x25` -> end, `0x24` -> next-line same-box, `0x48` -> new box, `0x4C 0xFF` -> terminate, `0x2A` -> resize, **`0x27` -> 2-option picker** (state `0x13` -> `0x12`), **`0x28` -> 3-option picker** (`0x15` -> `0x14`), **`0x29` -> 4-option picker** (`0x17` -> `0x16`). The open byte is matched as `byte & 0x7F`, so both `0x27..0x29` and the high-bit `0xA7..0xA9` forms are accepted; the field corpus stores the bare form.

Each picker arm sets the box dimensions from a per-N table and clamps the choice cursor at `*(DAT_801c6ea4 + 0xc)`; on confirm it reads the continuation byte at `pbVar14[N*2 + 1]` (same dispatch table as the post-page) and advances. Captured in [`docs/formats/mes.md` § Dialog window pager](../formats/mes.md#dialog-window-pager---fun_801d84d0).

**Option-list inner format RESOLVED:** the control region is `[open][N * 2-byte i16 LE jump table][continuation][N * 0x1F label segments]`. The on-screen **labels are standard `0x1F`-lead glyph segments after the continuation byte** (drawn by the pager render loop via `FUN_8003CA38`/`FUN_80036888`) — the earlier "labels = the 2-byte entries" reading is falsified. Each 2-byte entry is a **signed relative jump** the inline-script control handler `FUN_80038050` applies on confirm: `new_pc = (open + 1 + index*2) + i16_LE(entry[index])`, relative to that option's own entry. Pinned across the corpus: the four `izumi` book-menu re-emissions shift all four entries by an identical per-emission delta (-518/-564/-549), and every decoded option jumps in-bounds.

Parser `legaia_mes::picker` (`scan_pickers`/`parse_picker_at`/`Picker::jump_target`); disc-gated `field_dialog_pickers_disc` decodes dozens of real menus (config `On`/`Off`/`Exit`, shop haggling, the Genesis-Tree quiz) and asserts in-bounds jumps.

**Engine consumer (faithful path):** `engine_core::inline_dialogue` / `World::step_inline_dialogue` (PORT `FUN_80039B7C`) drives the whole inline script through the real field VM, so a chosen option's branch handler executes its `SET`/`CLEAR` flag ops + scene changes before the reply (opt-in `World::use_vm_dialogue` / `play-window --vm-dialogue`).

**Pre-first-segment prologue now RUNS (opt-in path):** the field-VM dialogue runner (`World::use_vm_dialogue`) executes the interaction prologue before the first segment. The engine keeps the truncated `field_npc_dialog` buffer for the default renderer and stores the **untruncated** record alongside it (`man_field_scripts::placement_inline_prologue` → `field_npc_dialog_prologue`, body + entry PC + first-segment offset); on interaction the runner is started via `InlineDialogue::with_prologue` from `entry_pc` so the prologue's `SysFlag.Test`/`JmpRel` chain selects which segment the box opens at per story state, falling back to the first segment if the prologue can't reach one (never worse than the truncated path).

Disc-gated `field_interact_dialogue_disc` pins the prologue map's byte-consistency + non-vacuous presence on town01; synthetic `inline_dialogue_prologue_selects_segment_by_story_flag` / `…_falls_back_when_it_cannot_reach_a_segment` pin the selection + fallback.

**Multi-segment box packing RESOLVED:** the SM packs **consecutive** `0x1F` lines into one window of `_DAT_801F2740 = 3` rows — a line's `0x00` terminator immediately followed by another `0x1F` is "same box, next row" — and the box ends after at most three rows at the post-page control byte. `FUN_80039B7C`'s state-`0x2` advance (`for (; 0x1e < *pbVar4; ...)`) masks `(*pbVar4 & 0xF0) == 0xC0` and consumes the escape's data byte, so a `0xC?` escape whose argument lands in `0x00..=0x1E` (e.g. `0xC1 0x00`) doesn't terminate the line early.

Decoded by `legaia_mes::dialog_box` (`pack_box` / `pack_boxes`, `LINES_PER_BOX = 3`, `Dispatch` for the terminating control byte); disc-gated `field_dialog_boxpack_disc` pins it on real town01 bytes (all 561 packed boxes ≤ 3 lines; the Tetsu sparring opening packs as three `0x24`-chained 3-row pages → a 4-option `Picker`; the `Mist appeared, .., but` line survives its `0xC1 0x00`). The contiguous box run stops where the pool hands control back to the field VM (a non-pager control byte → `Dispatch::Unknown`), which the faithful `World::step_inline_dialogue` path runs as bytecode. Nothing further open on this thread.

## Animation

| Thread | Status | What would close it | Memory |
|---|---|---|---|
| Player ANM per-record layout | resolved (container + per-`(bone, frame)` semantic) | [details ↓](#player-anm-per-record-layout) | `project_player_anm_source_pinned.md` |


### Player ANM per-record layout

*Status:* resolved (container + per-`(bone, frame)` semantic)

The on-disc per-record body decodes byte-exact across **all 296 records** in the 5 pinned scenes (296 record / 5 scene corpus, plus every other scene's bundle the corpus sweep finds): `record_size = 16 + 8 × (a & 0xFF) × b`, where `a & 0xFF` is the **bone count** of the clip and `b` is the **frame count**. Layout: 8-byte `(a, b, marker_1=0x080C, flag)` header + 8-byte per-anim prologue + `b` frames × `bone_count` × 8 bytes per (bone, frame). Pinned by the disc-gated regression `crates/asset/tests/player_anm_real.rs` after the offset-convention fix (offsets in the offset table are **absolute** byte offsets, not relative to `+4` — earlier framing was wrong; size invariant now validates 296/296).

**Per-`(bone, frame)` 8-byte semantic — RESOLVED** (the earlier "4 little-endian `i16`s, semantic open" framing is superseded): the entry is **not** four shorts but a **translation + rotation** pair, decoded exactly as the retail interpreter [`FUN_8001BE80`](../../ghidra/scripts/funcs/8001be80.txt) does — bytes 0..4 hold three **nibble-packed signed 12-bit translation** values `(t_x, t_y, t_z)` (byte 2 = `high4(t_y)<<4 | high4(t_x)`, byte 4 high nibble = `high4(t_z)`; sign-extend on bit 11), and bytes 5/6/7 are three **`u8` rotation angles** `(r_x, r_y, r_z)` each `<< 4` to a PSX 12-bit angle (`4096` = 360°), composed Z→Y→X via `FUN_8004638C`/`FUN_8004629C`/`FUN_800461A4`.

The piece poses `R·v + T` about its own object origin (no centroid subtraction); frame 0 of an idle clip is the rest pose. Decoder `legaia_asset::player_anm::BoneTransform::decode` mirrors the decompiled C, pinned by the byte-exact unit test `bone_transform_decode_signed_12bit` (town01 record 17). The site characters page applies the same `(t, r)` pipeline.

**Distinct ANM kind (not this one):** `FUN_80021DF4`'s `+0x5A == 6` block uses a separate 24-byte-per-bone keyframe layout — see [`anm.md`](../formats/anm.md).

## Audio

| Thread | Status | What would close it | Memory |
|---|---|---|---|
| SPU reverb live routing (C7-REVERB) | resolved + WIRED (Studio C, global) | [details ↓](#spu-reverb-live-routing-c7-reverb) | `project_reverb_studio_c_global.md` |


### SPU reverb live routing (C7-REVERB)

*Status:* resolved — retail runs **`Studio C`, master-enabled, globally**; the "selective per-cue reverb-enable source" the hunt was looking for does not exist.

A pure-Rust read of the save-state corpus (no live probe) settled it. `legaia_mednafen::PsxSpu` reads the SPU register shadow (`Regs` block): `reverb_master_enabled` (`SPUCNT` bit 7), `reverb_registers` (the 32 reverb coefficient/address registers at `0x1F801DC0..0x1F801DFF`), and `voice_reverb_mask` (the per-voice `EON` enable at `0x1F801D98`/`0x9A` — which mednafen also mirrors under its `Reverb_Mode` sub-entry, a byte-for-byte cross-check across every state). CLI: `mednafen-state spu <state>`.

Across all 45 mednafen states (field / town / battle / summon / title / minigames):

- **Master reverb is always enabled** (`SPUCNT` bit 7 set everywhere). No scene toggles it.
- **The preset is `Studio C` everywhere** — the 32-register block is byte-identical in every state and matches the `StudioC` libspu preset exactly (`dAPF1=0x00E3`, `dAPF2=0x00A9`, work area `0x6FE0`). [`engine_audio::ReverbMode::identify`](../../crates/engine-audio/src/spu/reverb.rs) resolves the captured block → `StudioC`.
- **Per-voice reverb-send (`EON`) is broad** — 15–22 of 24 voices in any state, BGM + SFX alike. Reverb is the default routing, not a per-cue effect.

So the blocker (the per-cue enable SOURCE) dissolves: there is nothing to trace. **WIRED:** the live engine calls `Spu::set_retail_reverb` once at SPU init (`StreamResampler::new`) — `ReverbMode::StudioC` + every voice routed. The PCM oracle's retail-side reverb is also fixed (it previously mis-read the EON mask as a mode byte and ran `Off`). Residual is only the output-depth tuning (`SpuSetReverbDepth`, `vLIN`/`vROUT`; the engine uses a fixed half-scale approximation). Falsifies the earlier "Spirit-Arts / echo cues opt in, everything else dry" reading in [`audio.md`](../subsystems/audio.md#retail-reverb-routing--studio-c-always-on-capture-confirmed).

## Title / boot / overlays

| Thread | Status | What would close it | Memory |
|---|---|---|---|
| `title.pak` PROT entry | resolved | [details ↓](#titlepak-prot-entry) | `project_prot_0895_init_pak.md` |
| Title screen mode-table PROT | resolved (no such entry) | [details ↓](#title-screen-mode-table-prot) | `project_mode_table_structure.md` |
| Load-screen panel 9-slice geometry | resolved (engine renders byte-perfect) | Pinned in [`subsystems/save-screen.md`](../subsystems/save-screen.md#pinned-9-slice-tile-rects-system-ui-tim-clut-row-2): retail composes the 81×29 panel at dst `(6, 4)` from 14 textured-sprite primitives (GP0 cmd `0x64`) sampling the system-UI sheet with CLUT `(32, 511)`. The exact per-tile rects are exported as `legaia_asset::title_pak::OVERLAY_SYSTEM_UI_PANEL_*` and emitted by `legaia_engine_render::save_select_chrome_draws_for` (covered by `save_select_chrome_emits_9slice_panel_and_pills` test). No interior fill sprite is drawn — the "marbled blue" look is the dimmed title art bleeding through the empty middle of the frame. | `project_load_screen_panel_source_pinned.md` |
| Debug flags `0x8007B8C2` / `0x8007B98F` | resolved (writer absent **by design** in retail) | [details ↓](#debug-flags-0x8007b8c2--0x8007b98f) | `project_debug_flags.md` |
| XP-table source + reader | resolved + ported | [details ↓](#xp-table-source--reader) | `project_xp_split_static_negative.md` |
| Opening-prologue tail (`opdeene`) | partial | [details ↓](#opening-prologue-tail-opdeene) | `project_cold_boot_prologue.md` |
| Overlay identity from the disc (static extraction) | resolved (pipeline landed) | [details ↓](#overlay-identity-from-the-disc-static-extraction) | `project_static_overlay_pipeline.md` |
| Options/menu overlay PROT entry | resolved + RAM-verified (**PROT 0899** @ `0x801CE818`) | The options/pause/inventory-equipment-status menu overlay is **PROT 0899**, not 0896: `FUN_801CF650`'s signature byte-matches PROT 0899 file `0xe38`, and the `.text`+`.rodata` prefix is byte-identical across six menu-open saves. VA-alias sibling of the field overlay 0897 in slot A — the menu overlay replaces the field overlay at the base. The earlier "0896 = menu" label is falsified. | `project_static_overlay_pipeline.md` |
| PROT 0896 (`bat_back_dat`) identity | open (mode-24-overlay hypothesis refuted; base was an over-read artifact) | [details ↓](#prot-0896-bat_back_dat-identity) | `project_static_overlay_pipeline.md` |
| Slot-A scene-overlay family beyond field/battle/menu | resolved (in the static map) | The rest of the slot-A (`0x801CE818`) VA-alias family is pinned from the disc: **0970 cutscene_str** (STR/MDEC FMV, modes 26/27) and the minigame overlays **0972 fishing / 0973 slot_machine / 0976 baka_fighter / 0980 dance**, each cross-checked by a documented function landing on a prologue at the base. Minigame entries over-read each other (phantom-base risk); the canonical entry recovers `0x801CE818`. Found via `asset overlay scan` + the leading dev string. | `project_static_overlay_pipeline.md` |
| "world-map / save / shop" overlay PROT entries | resolved (they are NOT separate entries) | The world-map / overworld controller `FUN_801E76D4` lives in the **field overlay 0897** (base+0x18EBC), and the save-slot dispatcher `FUN_801DC6B4` + the shop/buy session live in the **menu overlay 0899** (save at base+0xDE9C) — each function's instruction signature byte-matches only that one entry (`asset overlay find-sig`). So "world-map", "save", and "shop" are *subsystems* of existing slot-A overlays, not separate PROT entries; recorded in the 0897 / 0899 map notes. | `project_static_overlay_pipeline.md` |
| Slot-B overlay cluster (`0900..0969`) per-entry identity | mostly resolved | The slot-B buffer (link base `0x801F69D8`) timeshares the `0900..0969` summon/dance/minigame blobs; static extraction at the link base is the clean path, each base cross-checked by in-file self-pointer resolution (`static_overlay::pointer_resolution`, ≥70%). Pinned: 0900 summon render, 0905 Gimard stager, 0902 GAME OVER, 0907/0924/0927 Disco King songs, 0957 summon-effect strings (**NOT** a dance song). The "summon `0905..=0915`" is the loader's arithmetic range, not a stager list (0907 is the dance song "Hell's Music"). **Still open:** the per-summon spell-id → stager-entry assignment for the binary stagers (needs a capture; over-read defeats a static census). | `project_static_overlay_pipeline.md` |


### `title.pak` PROT entry

*Status:* resolved

There is no single `title.pak` bundle entry — the dev-tree `title.pak` content is split across two PROT entries, both confirmed by the init.pak fingerprint method now that a title-phase RAM snapshot exists (`title_screen_new_game` save state): the **title wordmark TIM** is **PROT 888/890** (`sound_data2`; already parsed by `legaia_asset::title_pak`, the big-logo RAM TIM at `0x80170DF8` fingerprint-matches it),

and the **options/config-menu bundle** is **PROT 899** (`xxx_dat`) — its indexed payload opens with the config-menu string pool ("Display Off / Gradual / Immediate / Field HP Display / Encounters / Vibration / Dual Shock / Voices / Battle Camera / Monaural / Stereo …") followed by the small config TIMs (the four RAM TIMs at `0x8010FEF0..0x80110130`, CLUTs byte-matched at 899 offsets `0x169DC` / `0x1F91C`+), with the title-overlay *code* in the trailing unindexed gap after entry 899 (see [[title-overlay-source-pinned]]). Same CDNAME-mislabel pattern as `0895_bat_back_dat` = init.pak.


### Title screen mode-table PROT

*Status:* resolved (no such entry)

**The premise is wrong**: there is no title-screen entry in the 28-entry mode table at `0x8007078C`. Per [`subsystems/boot.md`](../subsystems/boot.md#title-screen-is-not-in-the-mode-table) the title overlay is loaded by a **pre-mode-dispatch boot routine** ahead of the mode table being consulted at all — its tick `FUN_801DD35C` lives in the unindexed 60-sector PROT.DAT gap between TOC entries 899 and 900 ([`legaia_asset::title_pak`](https://github.com/altimit-mii/legend-of-legaia-re/tree/main/crates/asset/src/title_pak.rs) reads the wordmark TIM out of PROT 888/890; PROT 899 carries the options-menu config bundle). NEW GAME is how control crosses from the title overlay into the mode table at mode 2. Row kept so the "title entry is unresolved" framing isn't re-opened.


### Debug flags `0x8007B8C2` / `0x8007B98F`

*Status:* resolved (writer absent **by design** in retail)

Both addresses are in the SBSS/BSS region (zero-initialised at boot). The retail code paths only ever consult them as **dev-vs-retail build-time selectors**: `FUN_8003E360`'s dual-mode loader pattern routes through ISO9660 when `_DAT_8007B8C2 == 0` (the retail path) and through the PROT-index loader when non-zero (the dev path); same shape at `FUN_8001FA88` / `FUN_8001FC00` (sound) and `FUN_8001F7C0` (per-scene field-asset loader, see [`reference/functions.md`](functions.md)). The earlier "at least one writer must exist in an unscanned overlay" framing was wrong: a retail build whose selector lives at zero needs **no writer at all** — BSS init alone establishes the retail config, and the dev branches are never taken because no code path flips the flag.

**Exhaustive corpus sweep (2661 dump files across SCUS + every captured overlay) confirms zero writes to `_DAT_8007B8C2` and zero references — read or write — to `_DAT_8007B98F`.** So `_DAT_8007B8C2` is read-only at runtime (10+ `== 0` retail-mode tests, no writers anywhere), and `_DAT_8007B98F` is effectively inert (the dev branches it would gate were stripped at link time — the byte exists in BSS because GameShark codes can POKE it, but no retail code path consumes it). Row kept so the "writer must exist somewhere" framing isn't re-opened.


### XP-table source + reader

*Status:* resolved + ported

The retail XP curve is the static-SCUS per-level delta table `DAT_80076AF4` (u16), read by the level-up applier `FUN_801E9504` (overlay-resident, called from the reward resolver `FUN_8004E568` at `0x8004F34C`): the running sum to the current level is scaled `(sum × 9999999) / 0x140FE` for `level < 0x11` (else `sum × 0x79`) and compared `≤ record cumulative XP` in a multi-level `do…while` loop. The earlier `0x8007123C` / `0x80070A3C` framing was doubly wrong (an off-by-`0x800` file/virtual confusion, then a sin-LUT slice). The engine now extracts the table at boot (`legaia_asset::level_up_tables::xp_thresholds_from_scus` → `BootSession`); byte-validated L2 = 365 / L3 = 730 against a captured retail level-up. The `retail_xp_table()` sin-LUT slice is the disc-less fallback.

See [`subsystems/level-up.md`](../subsystems/level-up.md#xp-table).


### Opening-prologue tail (`opdeene`)

*Status:* partial

The `opdeene` → `town01` hand-off is **data-driven**: `enter_field_scene` arms it only when partition-2's real `GFLAG_SET 26` write is present (P2 record 18, offset `0xA5E`). The intro cutscene executes in-engine as a spawned field-VM context (`CutsceneTimeline`; `load_cutscene_timeline_from_man` / `step_cutscene_timeline`): op `0x45` Camera Configure + `0x23` MoveTo emit camera/move events, the closing `GFLAG_SET 26` fires the hand-off by execution, and the inline `0x1F`-page narration (parser `legaia_asset::cutscene_text`) plays via the `CutsceneNarration` presenter and gates the hand-off.

The **name-entry auto-open is pinned**: op `0x49` STATE_RESUME sub-op 3 at town01 P2[3] body offset `0x02c6`, pinned by executing P2[3] through the field VM and correlating against the `name_input_ui` save (`_DAT_8007B450` parks at the op while name entry is up); the engine runs P2[3] on the new-game hand-off and op `0x49` opens name entry then resumes (`install_town01_opening_timeline`). The op-`0x45` param→global map is fully pinned (`FUN_801DE084`):

**param 0 = pitch, param 1 = yaw, param 2 = roll** (the three GTE Euler angles), params 3/4/5 = shake/offset trio, params 6/7/8 = camera focus (negated GTE translation), param 9 = GTE H (FOV/zoom). The GTE rotation build is decoded (`FUN_8001CF50` rotates by `RotMatrixX/Y/Z` at `0x800461A4/629C/638C`, sin/cos LUT `0x80070A2C`, 12-bit angles), and the per-frame ease is decoded (`FUN_801DB510` exponential `srav` lerp toward control-block targets). `play-window` wires pitch + yaw + focus + H into `cutscene_camera_mvp` + `CutsceneCameraInterp`.

**What's left:** only the eye **distance** is unmapped - retail has no explicit eye-distance scalar (the eye sits at the GTE translation and projects through H), so the engine still orbits the focus at a scene-sized radius rather than placing the eye at the translation. The snap-vs-interpolate timing is resolved (it eases).


### Overlay identity from the disc (static extraction)

*Status:* resolved (pipeline landed)

PSX overlays are clean copies of a fixed-VA-linked blob (FlushCache + jump, no per-load relocation), so each runtime overlay can be extracted **statically** from its `PROT.DAT` entry and disassembled at its load base — identity attached from the source entry, not a guessed label. This is the structural fix for the VA-aliasing identity problem (`0x801DD864` = battle-action in one overlay, muscle-dome in another). Proved: the battle overlay (PROT 0898 @ `0x801CE818`) is byte-identical to its resident RAM image over the full `.text`+`.rodata` (`0x28800` of `0x29800` bytes; only the trailing `.bss` diverges). The load base is recovered statically from the overlay's own internal `jal` call graph (`static_overlay::recover_base`); for entries with too sparse a call graph,
the base is cross-checked instead by a documented function landing on a prologue (`anchor_va`, slot A) or by the fraction of internal absolute self-pointers that resolve in-file (`static_overlay::pointer_resolution`, slot B). The committed map now spans the whole slot-A scene family (field/battle/menu + the **cutscene/STR** overlay 0970 + the **minigame** overlays 0972/0973/0976/0980) and the pinned slot-B entries (summon render 0900, Gimard stager 0905, GAME OVER 0902, the Disco King dance songs 0907/0924/0927, summon-effect data 0957). Reconnaissance tooling: `asset overlay scan` (range sweep: base + leading dev string) and `asset overlay find-sig` (locate a function-head signature → infer the host overlay). Pipeline: `legaia_asset::static_overlay` + `asset overlay …`;
committed map `crates/asset/data/static-overlays.toml`; see [`tooling/static-overlay-pipeline.md`](../tooling/static-overlay-pipeline.md). It **complements** the dynamic captures — it does not address runtime values (those still need live probes).

### PROT 0896 (`bat_back_dat`) identity

*Status:* open — but the "mode-24 OTHER overlay @ `0x801C5818`" hypothesis is
**refuted**, and the recovered base is an **alias artifact**

PROT 0896 is NOT the menu overlay (that is 0899). Three findings reframe it:

1. **The mode-24 entry does not load it.** A live capture of the Baka Fighter
   entry (probe
   [`autorun_minigame_overlay_capture.lua`](../../scripts/pcsx-redux/autorun_minigame_overlay_capture.lua),
   triggered on the `0x8007B83C = 0x18` write; sub-id `0x8007BA34 = 4`,
   live-confirming the `0x3E` operand−100 model) dumped the overlay window at
   +0/+10/+30 vsyncs — spanning the SCUS-resident OTHER INIT handler's
   completion (its `"other init end"` debug print) and the per-minigame
   overlay streaming into slot A. 0896's bytes appear at no offset in any
   dump, nor anywhere in main RAM in the pre-transition save, nor in any of
   the parked library states (45+ checked, all phases).
2. **The `0x801C5818` base (60 jal votes) is an over-read artifact.** 0896's
   file carries the FIELD overlay's bytes from `+0x9000` (consecutive
   entries' footprints over-read), and the field overlay's self-consistent
   code at `0x801CE818` fixes the whole-file recovery to
   `0x801CE818 − 0x9000` by construction. Restricted to the head's own code,
   the jal recovery yields **no landslide** — 0896's true link base is
   unrecovered.
3. **The unique head (~`0x9000` bytes) is a self-contained blob of mixed
   code + data**: real MIPS density (~54 prologues), an `"FWIN ERR %d"`
   printf (the string lives in the blob itself; no `fwin`/`bat_back`
   reference exists in `SCUS_942.54`), and a large byte-map-like data block
   (rows of gradually shifting byte values). The CDNAME label
   `bat_back_dat` (battle background data?) may yet be honest — but no
   captured battle state holds the data either.

What would close it: a consumer — any retail moment where the head bytes are
resident (offline check:
[`overlay_residency.py`](../../scripts/pcsx-redux/overlay_residency.py)
against new captures), or a static SCUS/overlay loader call resolving PROT
index 896.

---

## When to add a row

A thread belongs here when:

1. There is something *specific* that would close it — a probe to run, a dump to read, a function to port. "Generally understand X better" is not closable; skip.
2. The next step is non-obvious from the code or git log. If `grep` would surface it, no row needed.
3. The detail lives elsewhere (a memory entry, a docs page, a Ghidra dump). The row is the pointer, not the analysis.

When the thread closes, rewrite the row to a `falsified` or `done — kept for reference` line if the path was instructive enough to warrant a "do not re-walk" marker; otherwise delete the row. Rotating the page is part of using it.

## Related pages

- [`docs/tooling/port-catalog.md`](../tooling/port-catalog.md) — per-function dumped × documented × ported × ignored axes. `port-catalog.py --missing-ports` is the function-level companion to this page's question-level index.
- [`docs/reference/functions.md`](functions.md) — canonical function directory; the place to learn what a `FUN_<addr>` mentioned in a row actually does.
- [`scripts/port-catalog-ignore.toml`](../../scripts/port-catalog-ignore.toml) — addresses explicitly *not* worth investigating (statically-linked PsyQ infra). Disjoint from this page.
