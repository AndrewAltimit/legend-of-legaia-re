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
| Which camera does the overworld walk use? | resolved (the field zone camera) | `capture` + `disassembly` | The overworld is a mode-`0x03` field-run scene. On `keikoku_chest_preload`, `sebucus_overworld_resident` and `karisto_overworld_resident` the live pitch, yaw and eye trio `0x800840B8` equal the zone composer `FUN_801DAB90`'s staging descriptor at `0x801F3580`, with `H = 368`; the region ease is `FUN_801DB510` ([`world-map.md`](../subsystems/world-map.md#walk-view-camera-retail-model-ram-pinned)). |
| What does `0x1F800394` bit 0 select? | resolved (the kingdom overworld) | `disassembly` + `capture` | Set on every non-battle kingdom-overworld state in the mednafen library and clear on every other state, overworld battles included. `FUN_801D629C` (PROT 0897, `s4 = 0x1F800314`, `lw 0x80(s4)`) takes an overworld arm on it: a camera-space depth test before the slot pop (`jal 0x8003D344` at `0x801D6460`, `slti 0x4001` at `0x801D6470`) and a further `-0x28` lift of the particle height (`0x801D651C..0x801D653C`); `map01`'s system script raises the pool's gate (`4C 30`) after widening the view window ([`field-ambient-fx.md`](../subsystems/field-ambient-fx.md#the-pool-on-the-kingdom-overworld)). |
| Does the disc carry one `4C EA` occurrence or more than one? | resolved (three - one per kingdom map bundle) | `disassembly` | The field-op census and the op-arm oracle disagreed by one because the census walk ended at each record's first `0x1F` text segment and started partition-0 records three bytes late. With text segments and pickers decoded as strides of the stream, the scripted game-over has one clean site in each of `map01` / `map02` / `map03` ([`field-op-census.md`](../tooling/field-op-census.md#4c-ea---the-scripted-game-over---has-one-carrier-per-kingdom-map)). |
| The ninth routine of PROT 0901's draw band | resolved (it is not a per-prim leaf) | `disassembly` | `FUN_801F89B8` is the **bulk terrain-cell emitter** - a hand-written GTE loop that draws one textured quad per map cell. It reads the four signed bytes of the camera's visible tile window at scratchpad `0x1F8003E8..EB` and takes `max - min` on each axis as its two loop counts (`0x801F89FC..0x801F8A28`), which is what makes that window a draw extent rather than a fog hint, and it indexes the floor-height ladder at `0x1F80035C` (`0x801F8A7C`). `jal`'d from `0x801F733C` in its own image. Row + detail on [`functions/world-map.md`](functions/world-map.md#801f89b8-draws-terrain-cells). |
| The world-map camera's sub-9 arm table, and where its horizon gates store | resolved (three stores were credited to the wrong instruction) | `disassembly` | Field-VM op `0x4C` nibble-4 sub-9 dispatches an arm table spanning `0x801E1480..0x801E162C`. Subs `0xA` / `0xB` / `0xC` write the horizon gate in the **delay slot** of the `j` that leaves the arm (`0x801E1648` / `0x801E1688` / `0x801E16C8`), so a scan that stops at the branch names the instruction before; sub `0xD` scales `_DAT_8008457C >> 12` into `0x8007B910`. The bit-24 arm of sub 9 writes **two** words - `ctrl[+0x4A]` and `_DAT_8007BCAC` - not one. See [`script-vm-menuctrl.md`](../subsystems/script-vm-menuctrl.md). |
| World-map walk-view continent ground render | resolved | `capture` | [details ↓](#world-map-walk-view-continent-ground-render) |
| Walk-view decoration draw gate - which grid bit stamps the `.MAP` record meshes on the overworld? | resolved (`0x2000` alone; no flag or mesh-0 test) | `disassembly` + `capture` | The resident slot-B kernel `FUN_801F69D8` (PROT 0901, byte-matched against a `map01` walk capture) tests only `cell & 0x2000` at `0x801F6ECC`, skips placed records, and reads `+0x10` with no zero test and no flag-`0x2` test; Y is the 2x2 corner average. Mode 3 reaches it through `FUN_80026CE4`, which picks it over the field sibling `FUN_801F7088` on `_DAT_8007BA90`. The big enterable mountains are `0x2000`-only cells, so the port's `0x1000` gate dropped exactly them - addresses and the cell census in [world-map.md](../subsystems/world-map.md#placing-the-continent-terrain-engine-port). |
| Walk-view untextured landmark prims - does retail draw the `F*`/`G*` colour prims of the slot-1 pack meshes (Rim Elm's hut roofs)? | resolved (yes, same per-prim dispatch as the textured prims) | `disassembly` | `FUN_80043390` selects the renderer by the group header's `flags >> 1` (`0x80043614`) and skips a group only on a null table entry. Slots 12..=15 (F3 / F4 / G3 / G4) are populated in the SCUS row `0x8007657C` and in the world-map overlay row `0x801F8968` (`0x801F7644 / 0x801F7838 / 0x801F7F78 / 0x801F8198`, cued like the textured leaves). Rim Elm (Drake slot 29) is 24 `G3` roof triangles over textured walls; 34 slots across the three packs carry colour prims - [world-map.md](../subsystems/world-map.md#top-view-bulk-terrain-render-path-overlay-replaced-per-prim-renderers). |
| `DAT_8007C018[45..53]` mid-load vertex-pool pointers | resolved (structural) | `disassembly` | [details ↓](#dat_8007c018-liveness-rule) |
| Field decoration path - does it dispatch the NCC light handlers? | resolved (no field light; depth-cue only) | `capture` | [details ↓](#field-decoration-path---does-it-dispatch-the-ncc-light-handlers) |
| Kingdom slot 4 - per-record semantic + consumer | resolved (the world-map scene's type-`0x05` **ANM animation bank**; the "vertex pool + cluster-A stream" reading is falsified) | `disassembly` + `capture` | [details ↓](#kingdom-slot-4---per-record-semantic) |
| The placed-actor mesh resolver on the world map | resolved (`FUN_80020F88`, at spawn time - not in the draw loop) | `disassembly` | Called from the allocator `FUN_80020DE0` at `0x80020F18` and from `FUN_80024E08` at `0x80024E60`: `actor+0x64 = .MAP_record[actor+0x60][+0x10] + DAT_8007B6F8`, render mode from `rec[+0x12] & 3`, then `FUN_80024D78` fills the `0x9C`-byte chain at `actor+0x44` from `DAT_8007C018[actor+0x64]`. The port's `field_objects::pack_mesh_index` + `FIELD_ACTOR_PACK_BIAS` already implement it. See [`world-map.md`](../subsystems/world-map.md#placed-actors-and-the-mesh-resolver). |
| MAN sections 2 and 5 - what do `_DAT_801C6EA0` / `DAT_80073EE0` carry? | resolved (both are place-name carriers) | `disassembly` + `capture` | Section 2's body is the **scene display name** the on-entry banner draws (and the save screen's location row); section 5, the universal zero-length terminator, leaves its pointer on the **world-map location table** the kingdom MANs trail - 29 records of `region + map x/y + discovery flag + 24-byte name`, walked by the label pass that draws each place's name at its map position. Together with the SCUS quick-travel cells that makes **three** independent carriers of one place name, which is why a rename has to edit all three. Layout + provenance: [place-names.md](../formats/place-names.md). |
| Where does the model-pool id space split at `0xF0`? | resolved (in the **callers**, not in the resolver) | `disassembly` | `FUN_80024E08` resolves a model id against the pool and does no splitting at all. The `0xF0` fork lives at `0x800393B8` and at `0x8003A2DC` - the latter inside `FUN_8003A1E4`'s placement installer (`0x8003A2CC..0x8003A328`) and its op-`0x0E` sibling - each choosing between `DAT_8007B824` and the scene base `DAT_8007B6F8` before it calls. A port that documents the split on the resolver names a routine that cannot perform it. |
| Is `FUN_80026B4C` the only writer of the TMD pointer table? | resolved (**no** - PROT 0976 zeroes a slot directly) | `disassembly` | The registrar is the only routine that *installs* an entry, but `0x801CF1E0` inside PROT 0976 stores zero into the table without going through it. A liveness rule derived from the registrar alone therefore has one hole; `DAT_8007B6F8` is stored with `sw` and read back with `lhu`, which is the other thing a pool walk has to respect. |
| What is PROT 0981? | resolved (the world-map **top-view debug image**) | `disassembly` | A `0x1000`-byte slot-A image at `0x801CE818`. Its `monster_test` label is CDNAME inheritance from the block opening at extraction 0978; the image's own operands are world-map ones throughout - `DAT_80073EE0`, the kingdom filter `uRam8007b970`, the camera pair `_DAT_80089118` / `_DAT_80089120`, the eye trio `0x800840B8`. One framed function and three leaves: the top-view tick `0x801CE850` (six-arm dispatch, `sltiu a0, 6` on `0x801CF76C`, table `0x801CE838`), the enter / reset `0x801CF4AC`, the record stepper `0x801CF5E8`, the camera clamp `0x801CF678`. [details](../subsystems/world-map.md#the-top-view-image-on-the-disc-is-prot-0981) |
| Which image holds the dev-menu row strings at `0x801CF344`? | resolved (PROT 0897, the field overlay) | `disassembly` + `capture` | File offset `0xB2C` at base `0x801CE818`. The argument is formed by `lui`/`addiu` pairs inside the renderer's own body - `0x801EAE44` / `0x801EAE48` and `0x801EB320` / `0x801EB324` - and a `map03` state's `0x801CE818..0x801D0000` RAM window is byte-identical to 0897's first `0x17E8` bytes. PROT 0981 aliases the same VA as the other slot-A occupant, and the two are never co-resident, which is the whole of why the address read as unattributable. |

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

*Status:* resolved - slot 4 is the world-map scene's asset-type-`0x05` **ANM animation bank**, structurally identical to every field scene's type-`0x05` section. Grade `disassembly` + `capture`.

**What the bytes are.** The container is `[u32 count][u32 byte_offsets]`;
each body is one clip: an 8-byte header (`marker 0x080C`, `part_count`, a
`u16` frame count, an interpolation flag and the sub-frame divisor `1 / 2 /
4`) followed by `parts * frames` entries of 8 bytes, frame-major
(`entry(f, p) = body + 8 + (f * part_count + p) * 8`, `0x8001BAC0..0x8001BAEC`).
One entry packs three **12-bit signed translations** in bytes `0..4`
(sign-extended at `0x8001BF44..0x8001BF6C`, pushed through GTE `MVMVA` at
`0x8001C0E0`) and three **8-bit rotation angles** in bytes `5..7`
(`angle = byte << 4`, read at `0x8001C0CC` / `0x8001C0E4` / `0x8001C0E8`).
The repo's generic detector already said so:
`asset player-anm extracted/PROT/0086_map01.BIN --desc-count 7` reports one
player-ANM bundle with `record0 marker_1 = 0x080C`.

**Who reads it.** `FUN_8001F05C` case 5 stores the buffer at `_DAT_8007B888`
(`0x8001F3A8`); the clip selector `FUN_800204F8` picks one of three banks
(`0x80020534..0x80020598`: party flag `actor[+0x10] & 0x01000000` →
`_DAT_8007B75C`; else `actor[+0x5C] < 0x400` → `_DAT_8007B888`, slot 4; else
`_DAT_8007B840`, the type-`0x0B` "MOVE2" bank) with a **1-based** lookup
`rec = bank + *(u32*)(bank + (id & 0x3FF) * 4)`, stores it at `actor[+0x4C]`
and clocks the 1/16-frame cursor `actor[+0x68]`; the animated renderer
`FUN_8001B964` (`FUN_8001ADA4` render mode 1, table `0x8001042C`) walks the
frame's parts, refusing unless `chain[0] == part_count` (`0x8001BAF0`), and
per part calls the pose decoder `FUN_8001BE80` then `FUN_80043390`
(`0x8001BC84`). `_DAT_8007B888` has six references across SCUS and all 32
extracted overlay images - the store, a reset in `FUN_8002541C`, one read in
`FUN_800204F8` and three in the Baka Fighter overlay - none in a render path.

**Live confirmation.** In a `map01` field-run state `_DAT_8007B888 =
0x8011A624` (the address the old reading called "the Drake slot-4 resident
base"), the 32,304 bytes there are byte-identical to the disc payload, and
four animated actors carry `+0x5C` = 5 / 11 / 14 / 15 with `+0x4C = base +
offsets[id - 1]` and clip `part_count` = 2 / 12 / 14 / 2, equal to their pool
TMD's `nobj`. The archived capture's own return addresses agree:
`ra 0x8001BB28` is the return of `jal FUN_8001BE80` at `0x8001BB20`, and
`0x8001BC8C` the return of `jal FUN_80043390` at `0x8001BC84`, both inside
`FUN_8001B964`.

**What was wrong, and why.** The old reading took each 8-byte record for a
GTE vertex `(i16 x, y, z, attr)` walked by an unpinned "cluster-A command
stream", because `FUN_80044C14` - a per-kind prim handler that genuinely
works that way - was assumed to own the pool. The entry's field boundaries
fall on nibbles, not halfwords; `attr` is the Y / Z rotation pair, read every
frame; the only unread field is byte 4's high nibble, zero in all 22,228
entries. One archived value stays a precise negative: `ra = 0x801F78D4`
cannot be a return address in either slot-B image (0900 / 0901), since
neither holds a `jal` at `0x801F78D0` under base `0x801F69D8`. Residual, not
format work: which actor plays which clip - the ids are scene-script literals
in `actor[+0x5C]`. Full layout: [`world-map-overlay.md`](../formats/world-map-overlay.md).
### DAT_8007C018 liveness rule

*Status:* resolved (structural) - grade `disassembly`

The liveness rule is in the instructions. The registrar `FUN_80026B4C` writes
`DAT_8007C018[cursor]` (`sw a0,0x0(v1)` at `0x80026BA8`, `v1 = 0x8007C018 +
cursor*4`) and mirrors the **pre-increment** cursor into `gp+0x820` =
`_DAT_8007BB38` (`0x80026BBC`); the pool walker `FUN_801D8280` loads that
counter (`0x801D8284`) and runs `i = 0 ..= counter`, stepping the base by 4.
Entries above the counter are never visited, so there is no per-index semantic
and `45..53` in a small field scene is stale carryover. An all-forms sweep finds
exactly **one** store of the counter corpus-wide and it is `gp`-relative - which
is why an absolute-only scan found no writer. Per-stage reset: `FUN_8001E1B4` at
`0x8001E3AC`.

## Battle / arts / level-up

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| What does `FUN_801F0450`'s tail do? | resolved (the Auto command's art insertion) | `disassembly` | `0x801F0B4C..0x801F1274` walks the character's art-animation bank (`0xD0` stride) from record `rand() % 5 + 0xB` and splices learned arts' arrow strings over the end of the queue the spend loop wrote, paying out of a local copy of the Spirit gauge `actor[+0x170]` - nothing is stored back. The per-arrow cost is halved under record `+0xF8 & 0x800` (`0x801F0D00`), the routine's only test of that bit. The pool arm and its tail run only for a slot whose `ctx[+0x266 + slot]` Auto flag is set (`0x801F0704`) ([`battle-action.md`](../subsystems/battle-action.md#the-art-insertion-tail-0x801f0b4c0x801f1274)). |
| Where does the target cursor's name plaque sit? | resolved (placement record `0x29`, centred on `x = 0xE8`) | `disassembly` + `capture` | `FUN_801D5854`'s target arm (`0x801D5B08..0x801D5BAC`) measures the target's name (`jal 0x80035F04`) and seats the box at `x = 0xE8 - w/2`, pulled back to `0x130 - w` past `0x130`, row `162`; the live seat starts at `max(x + 0x80, 0x148)` and slides in. On `party_basic_attack_vs_gobu_gobu` "Gobu Gobu" measures `55` and rests at `(205, 162)` ([`battle-action.md`](../subsystems/battle-action.md#the-target-select-plaque-record-0x29)). |
| Which battle limbs draw dark, and how? | resolved (a party seat's Rot limbs, per mesh object) | `disassembly` | `FUN_80048A08` reloads the colour word `+0x74` and blend `+0x78` for each object (`0x80048BEC..0x80048C00`) and, for a party seat (`+0x5A < 3`), overrides both when the seated actor's `+0x16E` carries a Rot bit and the object falls in that bit's range from the five-byte row at `0x80077998 + (0x8007BD10[seat] - 1) * 5`: bit `0x08` objects `row[0]..=row[1]`, `0x10` `row[2]..=row[3]`, `0x20` from `row[4]` up ([`renderer.md`](../subsystems/renderer.md#rotted-limbs-draw-dark)). |
| Does the tag-`0x67` streak ribbon have a caller outside the move-FX dispatcher? | resolved (yes, the per-clip pass of `FUN_8004CE2C`) | `disassembly` | Gala's tag-`0x67` arm (`0x8004D1E8..0x8004D248`) calls `FUN_801E1D98` with the target's seat vector `+0x3C` (`addiu a0,s1,0x3c` at `0x8004D220`) and the literal trail id `0xC` on every frame the clip cursor sits in `0xB0..=0xF0` ([`battle-action.md`](../subsystems/battle-action.md)). |
| What is `ctx[+0x25]`? | resolved (the round's skip count) | `disassembly` | Actors whose turn the round passes over without acting: `0x801DAB84` clears it (`sb zero,0x25` in the delay slot of `jal FUN_801DABA4`) and the dead-slot sweep bumps it (`0x801DAC2C`). Every other `sb ...,0x25(...)` in PROT 0898 stores a GPU-packet `u` / `v` byte; the round counter is `+0x28A`. The action SM's round-end bound subtracts it ([`battle-action.md`](../subsystems/battle-action.md)). |
| What does `0x801F696C` gate? | resolved (the strike loop's per-frame swing drift) | `disassembly` | `FUN_801EED1C` clears it at its head (`0x801EED88`); the Miracle arm (`sw` at `0x801EF5B8`, a `j` delay slot), the Super tail match (`0x801EFBD4`) and the head of `FUN_801F0450`'s auto-fill arm (`0x801F0518`, not its art tail) set it. Its one reader, `lw` at `0x801E3840`, arms the drift that moves the actor and its target along their facings each frame when the committed clip carries a header byte and the latched clip is outside `0x10..=0x1A` ([`battle-action.md`](../subsystems/battle-action.md)). |
| How is the command-phase commit log laid out? | resolved (records `0x2B + 3n`, seated off the name's width) | `disassembly` + `capture` | `FUN_801D388C`'s commit arms land name, command and target through `FUN_801D5718`, seat them at `x = 16`, `name_w + 0x20`, `name_w + 0x60`, and scroll rows `170` / `146, 170` / `146, 170, 194`; an all-target commit logs record `0x3D` / `0x3E`. `party_basic_attack_vs_gobu_gobu` holds the one-row case ([`battle.md`](../subsystems/battle.md#the-commit-log)). |
| What does the `0x6E` commit-confirm screen gate, and where does `Reselect` land? | resolved (party-wide, after the last able member; one step back onto the last able member) | `disassembly` + `capture` | Every commit site stores `0x6E` instead of `0x28` once `FUN_801DB81C` equals the party count, for every party size and behind no option; with nobody able, the prompt's `Begin` stores it directly (`0x801D10A0`). `Reselect` is `FUN_801D388C(0x21)` -> `FUN_801D32BC(1)` (`0x801D4750`), and the dispatcher refunds the landing member's Item (`0x801D30BC`); the `party_basic_attack_vs_gobu_gobu` display list holds the two `64 x 20` plates at `(84, 82)` / `(172, 82)`. Both hosts stage it ([`battle.md`](../subsystems/battle.md#the-commit-confirm-screen-0x6e)). |
| What does `FUN_801DABA4`'s first loop do to a dead actor? | resolved (zero the key, clamp Spirit, count the skip, refund the Item) | `disassembly` | For a slot with `+0x14C == 0` and an unspent key: key zeroed (`0x801DABF8`), Spirit `+0x170` clamped to 100, `ctx[+0x25]` bumped, and a staged Item (`+0x1DE == 1`) refunded through `FUN_800421D4(+0x1DF, 1)` (`0x801DAC44..0x801DAC5C`). Its tie list is biased: a seat that raises the maximum is entered twice, so the first seat above 0 to reach the top key wins `2 / (ties + 2)` (`0x801DAC7C..0x801DAD60`). |
| What ends Koru's four turns, and how long is the strip up? | resolved (Koru's own AI arm; the command phase) | `disassembly` | No code compares `ctx[+0x28A]` to a bound. The AI switch at `0x801CF1CC` sends entry 178 to `0x801EB52C`, whose round switch (`sltiu v0,v1,5` at `0x801EB540`, table `0x801CF49C`) casts `0xA2..0xA5` on rounds 0-3 and the finisher `0xA1` on round 4. The strip is text actor 1 on the `gp+0x148` list, drained by `FUN_800355F0` at `0x801D0EB4` and through `FUN_801D99BC` (`0x801D9A24`) when a round plays out. |
| What does battle state `0x5A` do? | resolved (the arts target cursor) | `disassembly` | Left / Right walk `+0x1DD` through `FUN_801D8D00` (`0x801D21D4..0x801D2268`) after the arts entry; retail does not pre-pick the target ([`arts-command-gauge.md`](../subsystems/arts-command-gauge.md)). |
| Does the arts-shout selector remember a last pick per character? | resolved (no - one byte party-wide) | `disassembly` | `FUN_8004C140` re-rolls while the draw equals `gp+0xA4A` = `0x8007BD62` (`beq` back to the `rand` call at `0x8004C3F8`), stores the pick there, then forces channel `0xC` when formation id `gp+0x9F4` is `0x4F` (`0x8004C400..0x8004C414`). A one-entry pool whose channel is the last pick would spin. |
| What does the dome hub's first visit draw, and in what order? | resolved (course card over title art; last emitted paints first) | `disassembly` | Arms 4 / 5 call the course card `FUN_801D042C` (six corner draws from records `5 + course` and 8), arm `0x14` raises nothing on a first visit, and `FUN_801D1610` is a subtractive `POLY_G4` (tpage `0x46`, ABR 2). Every hub emitter links at OT slot 3 through the LIFO `FUN_8003D2C4`, so the last packet emitted is drawn first, and retail clears record 4's semi byte before each face (`sb zero,0x176b` at `0x801CFAF8`) ([`minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md)). |
| What triggers retail's in-battle steal? | resolved (the killing blow, once per strike chain) | `disassembly` + `capture` | Not a command and not a hit: `FUN_8004AD80`'s death-spoils arm (`0x8004B29C..0x8004B65C`) runs when a monster's knockdown clip ends at HP 0, and sets the latch `ctx[+0x27]` at `0x8004B3E4` **before** testing the killer, so an action's first kill spends its attempt. The action SM re-arms it at every strike chain's exit (`sb zero, 0x16(s5)` at `0x801E3A84`, `s5 = ctx + 0x11`). The gates, the roll (doubled by Items Up), the caption record and the table's two readers are on [`steal-table.md`](../formats/steal-table.md). |
| Does the same arm do anything else? | resolved (it returns a thief's loot) | `disassembly` | When a monster that stole dies, the item goes back to the bag with a "took" (`0x80077A38`) or "recovered" (`0x80077A4C`) caption, or "recovered all" (`0x80077A70`) when `ctx[+0x18]` already reads `0x5B`. PROT 0941 fills the stolen band at `0x801C8FE0`. |
| How does the command ring step back to an earlier member? | resolved (cancel with a non-zero step counter) | `disassembly` | The ring's cancel tests `ctx[+0x1F]` at `0x801D11B4` before the direction arms: zero returns to Begin / Run, otherwise `FUN_801D388C(0x10)` calls `FUN_801D32BC(1)` at `0x801D4010` and refunds an Item commit (`0x801D12AC`). Case `0x21` is the Reselect on the `0x6E` commit-confirm screen (`0x801D3040..0x801D3088`). |
| What do the first-visit dome hub arms draw? | resolved (course card, title art, backdrop ramp) | `disassembly` + `capture` | Arms 4 / 5 draw the course card `FUN_801D042C` over the title art; arm 2 raises the backdrop level by `4 * dt` while the intro card fades; arm 6 drains it and kicks the battle load (`0x801CF9E4`, `0x801CFC48`). The ROUND banner `FUN_801D02F0` is drawn by arm `0x15` only. |
| How many writers does `ctx[+0x28B]` have? | resolved (five) | `disassembly` | The four SCUS raises plus the tick's own clear, `sb zero, 0x28B(v0)` at `0x801E263C` in `FUN_801E2524` (PROT 0898). Only the MIRACLE raise (`0x8004B7D0`) and the side-array raise (`0x8004B840..0x8004B868`) fire the fixed cue; the HYPER default matches the art byte `actor[+0x1DF + ctx[+0x15]]` against `0x1C..0x1E` and coin-flips through `jal 0x80056798` at `0x8004B91C`. |
| Which stat does each damage wrapper read? | resolved (`801DD4B0` INT, `801DD6B4` ATK) | `disassembly` | `FUN_801DD4B0` reads `+0x168` and `FUN_801DD6B4` reads `+0x158`; the port had the physical and spell names the other way round. `FUN_801E7320`'s `+3` is a constant, and the port's real defect there was taking the modulus over the actor table's length instead of the seated monster count. |
| What actually breaks a rebuilt PROT 0874 container at battle load? | resolved (a truncated pack) | `disassembly` + `capture` | `FUN_8001E890` does check integrity in its own frame, on one arm of three: with `gp+0x6AC` at `2` it reads the raw container back out of VRAM (`0x8005842C`, four rects from `(0x180, 0)`), re-sums every word at `0x8001E9C4..0x8001E9F4` against the boot sum at `gp+0x6B8`, and reloads on a mismatch. That guard is on the decompress **source**; the register-only arm (`bne v1, v0` at `0x8001E974`) skips it, so the gate's writers stay the reason nothing walks a clobbered pack. The open leg was unbuildable: the "header size word" and the "decoded length" are one descriptor field. [`character-mesh.md`](../formats/character-mesh.md). |
| What is `FUN_801F44A0`? | resolved (the floating damage-number ring push) | `disassembly` | `(value_i16, seat_u8)`: value at `ctx[+0x83C + cursor*4]`, seat at `ctx[+0x318 + cursor*2]`, `ctx[+0x85C + cursor*4] = 0`, then the cursor `ctx[+0x262]` bumps and folds `mod 8`, `ctx[+0x273]` counting pushes. `FUN_801E09F8` inlines the block at `0x801E1898..0x801E1918`; PROT 0955's Kiss of Death calls it to show its literal `1`. **Not** the Point Card clamp an earlier citation gave it as. |
| Which entry does the battle loader's VDF registration walk? | resolved (raw TOC 874 = extraction 872, `vdf`) | `disassembly` | It reads raw entries `873`+`874` (`etmd`+`vdf`) in one contiguous transfer, keeps the **second** half's base in `_DAT_8007B878`, and walks a flat `[u32 count][u32 offsets[count]]` pack there, handing each `base + offsets[i]` to `FUN_8001FBCC` at `0x80052584`. Its four constants `0x368..0x36B` are raw TOC `872..875` = extraction `870..873`, the whole `befect_data` block. The player-character pack is extraction 874 = raw `0x36C`, a *different* entry. Body in [`battle.md`](../subsystems/battle.md#battle-scene-loader-fun_800520f0). |
| What makes a Ra-Seru chip render? | resolved | `disassembly` | Three gates in order, and only the third selects a mark: the member's own chip byte `ctx[+0x25F+member]`, then `+0x16E & 0x1000`, then the special-battle word `0x8007BAC0 & 0x200`. Three emitters draw the marks - `FUN_801DBC30` (`(0, 96)`, 64x16, CLUT `0x7704`, the red X), `FUN_801DBD04` (`(80, 96)`, 32x24, CLUT `0x770B`, Attack, gated `+0x16E & 0x38 == 0x38`) and `FUN_801DBEC4` (`(120, 96)`, 64x16, CLUT `0x7700`, Ra-Seru sealed) - all `POLY_FT4`, tag `0x09000000`, code `0x2C808080`, tpage 7, each early-out on `ctx+0x6CE`. See [`minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md#what-makes-a-ra-seru-chip-render). |
| What restricts a special battle's commands (`0x8007BAC0`)? | resolved | `disassembly` | Bit `0x100` bars Item, `0x200` bars magic, course = `((word - 1) & 0xFF) >> 4`. **Two** writer families - an earlier reading checked only the first. SCUS battle init keys on the first enemy monster id (`0x800519DC`; `0x8005200C` under mode `0xC`/`0x15`), and the **arena seeds the word itself**: `FUN_801CEA6C` stores `0x101` / `0x111` / `0x321` over a zero at `0x801CEBA0` / `0x801CEBB4` / `0x801CEBC8` on story flags `0x536` / `0x537` / `0x538`, last match winning - so a seeded visit bars Item on every course and magic on the top one. 13 stores over 84 images, 5 clearing. Five readers gate on non-zero; `0x100` also gates the award arms `0x801E7978` / `0x801E7B40`. |
| What does a cast cost? | resolved (MP, not AP) | `disassembly` | The cast path `0x801D1408..0x801D1528` writes the action queue `+0x1DF[0]`, sets `+0x1DE = 2` and `+0x1E7 = 9` and enters phase `0x46`; it spends no AP. The cost is the spell table's `DAT_800754C8 + id*12 + 3`, discounted by the character record's `+0xF4` ability bits `0x20` / `0x10`. See [`battle-formulas.md`](../subsystems/battle-formulas.md). |
| `0x801E6218` - the multi-cast sweep the port was said to be missing | resolved (already ported; the doc sentence was stale) | `disassembly` | The address is a **latched** arm the port already implements as `battle_action::done`, and a five-form reference sweep over 84 images finds nothing that reaches it. The "unported multi-cast sweep" sentence described work that does not exist. See [`battle-action.md`](../subsystems/battle-action.md). |
| Which routine sets battle status bit `0x400`? | resolved | `disassembly` | PROT 0955's Kiss of Death **miss** arm, `ori v0,v0,0x400` at `0x801F8CFC` followed by `sh v0,0x16e(s0)` - the last unattributed status applier in [`battle-formulas.md`](../subsystems/battle-formulas.md). The hit arm sets the victim to exactly 1 HP and clears `0x0F80` with no wrapper. |
| PROT 0955's six no-damage tick bodies | resolved | `disassembly` | White Shield multiplies the defence pair by 3/2 idempotently from `0x801C9348[seat-3]`; Power Charge adds `x >> 2` to ATK capped at 999 (`sltiu 0x3E8` at `0x801F74D4`); Melt Spray takes 20% off ten halfwords and **underflows** `x` in `{0, 1}` to `0xFFFF` (only 2 of its 10 floor tests read a live 32-bit register - `0x801F83E0`, `0x801F8504` - the other eight re-`lhu` the store, same outcome); Void Accessories rolls one of three Goods slots off `record[+0x19B+slot]`, refunds through `FUN_800421D4` and rebuilds the bitfield with `FUN_80042558`; Kiss of Death is the row above; Terror Scream steals only the turn. Table in [`cast-module.md`](../subsystems/cast-module.md#the-twelve-bodies-the-trampoline-map-names). |
| Which slot-B images write the actor stat block? | resolved (**eight**, not 0955's four) | `disassembly` | Eight of the 64 images write `+0x150..+0x16D`: 0940 (`0x801F78B8`), 0942 (`0x801F7D34`), 0943 (`0x801F6A04` - `0x801F69D8` there is the `0xB5` body's head table, not the writer), 0945 (`0x801F69F8`), 0954 (`0x801F6A58`), 0955's own cells, and 0925 / 0956 which touch `+0x16C` only. The "0955's four bodies are the band's only stat writers" reading missed the other four images entirely. See [`cast-module.md`](../subsystems/cast-module.md#the-band-has-eight-stat-block-writers-not-one). |
| How many capture trampolines are there, and what keys a tick body? | resolved (21 trampolines / 48 arms; the key is a **pair**) | `disassembly` | The band holds 21 trampolines carrying 48 `(action id -> body)` arms over 32 distinct bodies. A body VA is **not** a key: `0x801F69D8` is a tick body in six different modules, so the seam is keyed `(entry, body)`. The earlier map covered 6 of the 21. See [`cast-module.md`](../subsystems/cast-module.md#the-trampolines-are-their-own-port-and-one-cell-holds-six-spells). |
| What does the AI companion pick read? | resolved (it watches the wrong actor by design) | `disassembly` | `FUN_801EED1C`'s character-id-4 arm reads `actor_table[0]` and writes `actor_table[1]`. Its physical leg redraws `rand() % ctx[+1] + 3`; a monster record with `+0x1E == 2` yields a single `0x0E`, otherwise two draws of `rand() % 2 + 0x0C`. The docs' table watched the acting actor. See [`battle-action.md`](../subsystems/battle-action.md#the-retail-queue-builder-fun_801eed1c-and-super-applier-fun_801ef9e4). |
| What are the battle applier's 132 selectors? | resolved (116 of them are the epilogue) | `disassembly` | Jump table `0x80014FA0` holds 132 slots over 15 distinct targets; 116 point at `0x800421A8`, the shared epilogue, so there is no "stat-up / status-clear / queue-end / item slot" family above `0x0E`. The one body above `0x0E` is slot `0x82` at `0x800421A0`, a brightness ramp already ported as `advance_gauge`. Decoded arms: `0x08`, `0x0A`, `0x0B..0x0D`, `0x0E`. See [`battle-formulas.md`](../subsystems/battle-formulas.md). |
| How does a learned skill enter the displayed list? | resolved (ordered insert, not head insert) | `disassembly` | Both list writers insert in **ascending** order: the party-slot learned-Arts list at `+0x74D` (selector `0x0B`) and the displayed-skill list at `+0x186` (arm `0x80041FB4`). A head insert would have reversed both displays. |
| `FUN_80043264` - how many equipment slots does it scan? | resolved (three, not eight) | `disassembly` | The counter starts at `li v1,0x5` (`0x80043284`) and runs while `slti v1,0x8`, so it walks `char +0x19B..+0x19D` - the three accessory ("Goods") slots of the `+0x196..0x19D` block. Row on [`functions/battle.md`](functions/battle.md). |
| What does `FUN_801E09F8`'s hit arm write? | resolved | `disassembly` | The `+0x1DC` writes are **ORs**, not stores (`|= 4` at `0x801E19D8`, `|= 1` at `0x801E1A18`); the face store carries no `+0x800`, so the victim turns **toward** the attacker; and the reaction pick has three legs. Ported as `effect_child_hit`. |
| Which routine applies a player-Seru module's magnitude? | resolved (the tick body, not the stager) | `disassembly` | The `0x801CF4EC` arms are `ctx+0x279` phase machines of 3396..7260 bytes, and the `actor+0x14C` write lives in them - in PROT 0910's case one call deeper still, in the applier `FUN_801F81DC` its tick reaches three times. The per-entry "data stager" verdicts describe a different routine in the same image, which is why a per-image verdict taken inside the stager's frame reads several modules backwards. Bodies in [`cast-module.md`](../subsystems/cast-module.md#the-player-seru-bands-tick-bodies-are-code-not-data). |
| Which player-Seru modules heal, and by what formula? | resolved (**two**, each with its own closed form over the magic level) | `disassembly` | PROT 0905 (Vera) restores `record[+0x729 + slot] * 0x20 + 0xE0`, clamped to the `+0x14C`/`+0x14E` pair, skipped at `HP == 0` or `+0x16E & 4`, and pushed to the popup accumulator as a **negated** value at `0x801F7D0C`. PROT 0911 (Orb) restores `(magic_level << 6) + 0x1C0` over the party row `actor_table[0..ctx[+0]]`. Both unlock a status cleanse at level `>= 3` (tier chosen through `0x801F6960`). The input is the per-magic **level** byte, matched on `actor[+0x1DF]` in the 32-slot spell-id list - ids `+0x705`, levels `+0x729` off `0x80084140`. Everything else in the band subtracts, PROT 0903 / 0910 / 0913 included. |
| How many phase stores does a player-Seru body make? | resolved (3..10, counting both store forms) | `disassembly` | Per module: 0903 4, 0904 6, 0905 4, 0906 8, 0907 10, 0908 6, 0909 6, 0910 5, 0911 3, 0912 6, 0913 5; eight of the eleven write the terminal `0xFF` through a pre-formed pointer. `0x801F69D8` is the tick arm for **six** of the eleven (0903 / 0904 / 0905 / 0908 / 0911 / 0912). A census keyed on the literal `0x279` displacement misses every store through a saved register and reads PROT 0908 as zero ([falsified](re-do-not-re-walk.md#battle--arts--level-up)). |
| What is actor `+0x1DC` at the cast band's reaction sites? | resolved (a bitfield) | `disassembly` | PROT 0903 and 0904 `ori` bits `4` and `1` into it; PROT 0906 and 0908 store `1` and `5`. Nothing increments it, so the "reaction counter" reading describes an operation the band does not perform. |
| What does PROT 0907 (Nighto) roll? | resolved (a kill roll and a resist roll, and the boss case is forced) | `disassembly` + `capture` | `0x801F8534` is `rand() % 8` - zero kills, anything else confuses - and `0x801F853C` is `rand() % (0x13 - level) >= 9` for the resist. The resist is **forced**, not rolled, when `ctx[+0x287]` is non-zero *and* the victim's monster record `0x801C9348[seat-3][+0x20]` is set (`0x801F6BF0..0x801F6C24`), with an extra forced resist on `rand() % 3 == 0` for character index 3 alone. Gaza 2 reproduces it live: `ctx[+0x287] = 4`, record `+0x20 = 1`, boss untouched. |
| Which band routine allocates a battle seat? | resolved (PROT 0940's `0x50` / `0xAE`, and only that one) | `disassembly` | It claims `actor_table[ctx[+1] + 3]`, copies the monster-record pointer into `0x801C9348[seat]`, and seeds `+0x16C = 0`, `+0x1DE = 2`, `+0x1DF = 0x50`, `+0x1DD = 9` plus an HP copy. The `0xAE` arm then coin-flips one of `HP 1` / `MP 0` / `ATK 1` / `AGL 0x20` onto either the clone or the caster - the "nine stores in two passes" shape is two mutually exclusive branches, not one pass. |
| What does PROT 0941's `0x51` (enemy Steal) take? | resolved (a bag slot or an item off the steal table; no damage) | `disassembly` | Against a party victim it rejection-samples the 256-slot bag at `0x80085958` (`rand() % 0x100`, up to `0x400` draws), flooring the slot against `*(0x8007B5EA)` while `DAT_8007BD10[1] == 4`, then calls `FUN_80042310(id, 1)`. That gate is `lbu v1, 1($s5)` at `0x801F77E8`, `$s5` formed as `0x8007BD10` at `0x801F77B0`: **seat 1 holding roster character 4**, the split-bag condition - not a context field, since PROT 0941 makes no `+0x11` access at all. Against a monster victim it rolls `rand() % 100` against `0x80077828 + id*2`, the same table the player's Steal reads. The stolen id lands at `0x801C8FE0 + (seat-3)*4`, the message pointer at `0x800774AC`. |
| What does `ctx[+0]` count in a cast module? | resolved (the **party**, not the whole actor table) | `disassembly` | `0x8004B3F0` walks `DAT_8007BD10`'s party ids to form it, and `ctx[+1]` is the monster count. The `+0x1DD` group codes read against it: below 7 is one seat, `8` sweeps `0..ctx[+0]`, anything else sweeps `3..3+ctx[+1]` (PROT 0956 folds codes below 3 into the `8` case and skips the caster). |
| What escapes PROT 0906's phase-5 dispatch hole? | resolved (the stager's arm 2) | `disassembly` | The tick's arm 4 advances the phase byte into a slot its own table does not serve, and retail parks there busy until the **stager** runs: `0x801F783C..0x801F7858` is `ctx[+0x279] += 1`, reached as arm 2 of the seven-word head table at `0x801F69D8` (`7788`, `780C`, `783C`, `785C`, `7920`, `7964`, `79A8`). |
| What is PROT 0904's arm 12? | resolved (the swept quantity is an **angle**, not a radius) | `disassembly` | `ctx[+0x6D8]` accumulates `frame_delta * 8` and the arm advances when it passes `0x1000` - a full 12-bit turn - so what moves per tick is the direction of a `±0x30` cone, not the radius of a ring. Seats `3..=6` are tested against it by bearing (`FUN_80019B28`), with the unsigned form `(|d| - 0x30) < 0xFB1` admitting both ends of the circle, and `+0x1D9` is what stops a seat being hit twice as the cone comes round. The port's own comment still leads with the "expanding ring" label. |
| What is PROT 0962's arrival predicate? | resolved (it arrives when the test reads **zero**) | `disassembly` | `bne v0,zero,<hold>` at `0x801F7C0C` and `0x801F7D08` holds the approach while `FUN_8004E2F0` returns non-zero, so a non-zero return is *not yet there*. `FUN_80050BB8` on that path is the separation nudge, not the approach step. The port's doc had the sense inverted. |
| Is `ctx[+0x287]` the counter-attack byte? | resolved (**no** - it is the scripted-fight flag, derived at battle init) | `disassembly` + `capture` | `FUN_800513F0` computes it as `(DAT_8007BD60 >> 5) & 4`, i.e. bit `0x80` of the per-battle flags that `FUN_801DA51C` raises for a formation row with a non-zero `record[+0]`; the counter-attack byte is `+0x288`. Its value space is `{0, 4}` (`lbu`, `srl 5`, `andi 4`), and 9 of 96 battle states read `4`. All three `FUN_801E295C` reads gate on it, so two audio-duck arms and the attack-return arm are unreachable in an unscripted fight. |
| What gates each player-Seru arm's dwell? | resolved (measured per arm - and it is not one constant) | `capture` | Driving casts from pre-cast states gives per-arm dwell in module ticks for PROT 0903 / 0904 / 0905 / 0907 / 0908 / 0910 / 0911 and 0959. PROT 0907 holds 8 of its 16 arms identical across two fights and moves the other 7, so a single per-arm frame constant is the wrong model. `ctx[+0x6D8]` seeds 120 and drains one per tick on the player half and holds a constant 20 on the capture half. Tables in [`cast-module.md`](../subsystems/cast-module.md#frame-gating-measured). |
| Do the Seru side-effect debuffs and the encounter boost profiles reproduce live? | resolved (yes, both, at measured magnitudes) | `capture` | A level-9 Seru debuff takes a monster's ATK from 17 to 14 on both halves - the documented 20%. Both boost profiles reproduce off a live fight: a random encounter's `[84, 17, 15, 14, 10, 30]` becomes `17 / 25 / 24 / 12`, and Gaza 2's `288 / 222 / 220` becomes `360 / 444 / 247`. Vera at level 3 restores 320 and Orb 640, matching their closed forms exactly; PROT 0908's splash reproduces `522 -> 130` (`/4`) and `541 -> 405` (`*3/4`). |
| What does the dome tally screen draw? | resolved (six rows, and the HP accumulator sits between lanes 2 and 3) | `disassembly` | The rows are `[lane0 pending, lane1 pending, lane2 pending, the HP accumulator 0x801D1AC8, lane3 pending, the running tally 0x80084440]`, at per-lane brightnesses `[0, 1, 2, 0, 3, 3]` - four steps, not six. The six rows draw through `FUN_801D1308` -> `FUN_801D050C` with the brightness argument **unclamped**; the `0x100 -> 0xFF` clamp at `0x801D08FC` belongs to `FUN_801D08EC`, the corner-anchored sibling, which no tally row calls. `FUN_801D1184` re-forms the `0x801D` base into a different register between the product and the store (`0x801D11E0` / `0x801D11F0`), so `0x801D1ACC` really is the round lane the docs named. |
| What does the dome arena seed `0x8007BAC0` with before any story flag matches? | resolved (`1`) | `disassembly` | `sw $s2` at `0x801CEB8C`, with `$s2` loaded `1` forty-three instructions and three `jal`s earlier. That is course 0 with no bans - a seeded word, not an untouched one. The `0x101` / `0x111` / `0x321` values, and therefore the claim that every seed carries `0x100`, belong to the three flagged arms only. |
| Is `0x801F69D8` a routine in PROT 0943? | resolved (**no** - it is a head table) | `disassembly` | The five-word table at the image base is dispatched by the body at `0x801F6A04`: `sltiu v0,v1,5` at `0x801F6A70` bounds the index, `addiu v0,v0,0x69d8` at `0x801F6A80` forms the entry, `jr v0` at `0x801F6A94` takes it. The body runs `0x801F6A04..0x801F6EF4` and the MP-pair stores at `0x801F6D08` / `0x801F6D1C` are inside it, so citing `0x801F69D8` as the writer names the table that points at the writer. |
| Is the monster record's `+0x20` a per-monster death-immunity byte? | resolved (**no** - a double-width texture-page flag) | `disassembly` | `lbu a2,0x20(v0)` at `0x801F1D0C` hands it to `FUN_80055468`, which widens the model's VRAM rect from `0x20` to `0x40` halfwords at `0x800554E0..0x800554F4`; 37 of 186 records set it. PROT 0907 / 0908 / 0916 read the same byte as a "big model" resist proxy under the scripted-fight flag, which is what made an upload flag read as an immunity list ([falsified](re-do-not-re-walk.md#battle--arts--level-up)). |
| Where does a slot-B module image's **highest** spawn record end? | resolved (its own move-VM program bounds it) | `disassembly` | Walk the opcode widths to a terminator and round the end up to 4, because the records are word-aligned - that alignment step is what the old "within 4 bytes" figure was missing. `HALT` outranks an armed `0x19` / `0x1B` idle loop, the walk chains `[header][program]` rather than assuming one record per pointer, and where it dies with no terminator the last maximal `0x09 0x0FFF` WAIT it stepped over bounds the record. Every image that has a highest record bounds it; the residue misses **below** or lands on the end and never terminates above it. See [`slot-b-module-layout.md`](../formats/slot-b-module-layout.md#bounding-the-highest-record). |
| What are Enemy Steal's acceptance tests, and what does the consume touch? | resolved (a **third** test on shop price, and a window-bounded consume) | `disassembly` + `capture` | Beyond the roll and the occupied-slot test, the sampled item is accepted only when its `+2` **shop price** is non-zero, so the quest/found-only ids - 96 of the 256 - are unstealable. The consume `FUN_80042310` reads `gp+0x2D2` / `gp+0x2D4` as its first two instructions and scans only `[start, end)`, returning the `0x100` sentinel when it finds nothing; a steal outside the active window banners a success and removes nothing. `gp` is `0x8007B318` live, so `gp[+0x2D2]` is `0x8007B5EA`. |
| How large is the item bag, and what bounds a reader? | resolved (256 slots behind an **active window**) | `disassembly` | One 256-slot array at `0x80085958`, reached through five SCUS helpers that each check the window pair `gp[+0x2D2]` (start) / `gp[+0x2D4]` (end), with `gp[+0x2D6]` the count. `FUN_8004313C` is the sole writer of that pair: a page flag at `0x80084594`, a fourth-bank flag test and the party-id byte at `0x80084598` choose start `0` or `0x80` against end `0x100`, which is the "split bag" a lone character sees. A real three-member memory-card block carries items up to index 159, so a 72-slot bound drops 88 of them - that figure is a cheat page's display page, not a capacity. |
| Do the two move-`0x36` / `0x37` damage wrappers differ from the shared kernel? | resolved (the respect wrapper does not, the bypass one does) | `capture` | Driving both against the same seat, `FUN_801DD4B0` (move `0x36`, the defence-respecting wrapper) reproduces the shared kernel exactly - 251 against 251 - so nothing in a capture distinguishes it; `FUN_801DD6B4` (move `0x37`, the bypass) returns 972 where the kernel returns 311. Exactly one capture-class id per wrapper class carries a move-power record. |
| Does the spell record's `+0x00` class byte reach the action seed? | resolved (yes - it moves the fold by 21 frames) | `capture` | The action seeder's Magic arm compares the byte against `0x14`, and flipping it moves the damage fold 21 frames in a driven cast. The byte had been recorded as undecoded. |
| What gates each **capture-class** arm's dwell? | resolved for all fourteen trampoline arms; the drain is per arm | `capture` + `disassembly` | The countdown is drawn down by a **per-arm** multiplier of the scratchpad frame byte at `0x1F800393` - its product with `0x1F80037D`, twice it, or once it - so the single-product model held for one family only, and two arms measured as constants 4 and 8 were `1x` and `2x` a byte reading 4. All fourteen are measured, one fight each (PROT 0943's `0xAB` twice), reading the single `jal 0x801F2160` site in PROT 0898 as one module tick; the last two needed a caster with enough spell entries. Three arms park rather than gate and PROT 0950's arm 6 has no gate. Tables in [`cast-module.md`](../subsystems/cast-module.md#frame-gating-measured). |
| Is the battle loader's model-pack pointer `*(gp+0x6BC)` sane at battle time? | resolved (**no** - the battle load reuses the block) | `capture` | Across the catalogued state population the pointer names the same heap block in 97 of 98 states; it is sane in 37 of 37 field states and garbage in 61 of 61 battle states. The unclamped registrar inside `FUN_8001E890` - `lw a0,0x6BC(gp)` at `0x8001EAFC`, count off `+0x00` at `0x8001EB10`, `jal 0x80026B4C` per entry at `0x8001EB4C` - is reached on all three arms of the `gp+0x6AC` fork, so nothing in its **own frame** keeps it away from such a block. What does is the gate's writers: over six measured routes it is never entered over one (row above). |
| How does the character-pack loader size its section buffers? | resolved (from the container header, so a rebuild cannot change the decoded size) | `disassembly` | `FUN_8001ED60` takes the section-0 and section-1 buffer sizes from the container header words held at `gp+0x69C` and `gp+0x6C8`, and the LZS decode is length-driven by the descriptor. A header-byte-exact rebuild whose decoded size differs therefore gets a **truncated** pack rather than a larger one. The battle loader itself reads neither: `*0x8007B878` is the `vdf` pack and `gp+0xA8C` the `etmd` pack. |
| Is `FUN_801F6B24` one walk or two? | resolved (**two** dispatchers picked by one word) | `disassembly` | `beqz _DAT_8007BAC0` at `0x801F6BA8` selects between a 19-arm field-restore table at `0x801F6AD8` (`sltiu 0x13`) and a 12-arm `int.tim` panel-still table at `0x801F6AA8` (`sltiu 0xC`). Both start at phase 2 because SCUS's `FUN_80025358` owns states 0 and 1. An ordinary battle teardown takes the field-restore table - four 64x256 uploads at `x = 384 / 448 / 512 / 576`, one hit each - and the panel-still table records zero, which is the branch working rather than a missing consumer. Residency gating is load-bearing here: an ungated breakpoint at the same slot-B VA counts hundreds of thousands of hits from whatever else occupies the slot. |
| What are PROT 1221 and 1222? | resolved (the party's ringside **reaction pair**) | `capture` + `disassembly` | Two headerless BGR555 stills, `0x28000` bytes each, uploaded as four 320x64 bands to VRAM `(384, 0)`; one is the cheering party and the other the dejected one, which is what the loader's index rule encodes - the variant is chosen from the lead character's live HP (`u16 0x8008480E < u16 0x80084824 >> 1`). See [`ringside-still.md`](../formats/ringside-still.md). |
| Where does a retail value-readout quad put its far corner? | resolved (**inclusively**) | `capture` | Retail's own readout quads name the last texel, not one past it: 31 against 31, 47 against 47, 55 against 55 over the sampled frames. A port emitting the exclusive corner is one texel wide everywhere, and the digit gap has to drop to 0 in the same change. |
| Is the model-pack registrar ever entered while `*(gp+0x6BC)` holds a reused block? | resolved (**no** - six routes, five entries, count 5 every time) | `capture` | Breakpointing `FUN_8001E890`'s entry, its gate, the registrar at `0x8001EAFC` and every `jal 0x80026B4C`, and write-watching both words, over a door warp, a boss fight resolving to the field, a field walk into an encounter, a cold boot into NEW GAME and a cold boot through CONTINUE into a card load: the routine is entered five times, always over `0x8014D53C`, and the registrar reads count 5 every time. The field-to-battle route enters it zero times. State `1` (register only) occurs solely with the field pack intact; states `0` and `2` always have the file read and the decompress between the gate and the registrar. |
| What is `FUN_8001E890`'s gate? | resolved (the load-state word `gp+0x6AC`, not the game mode) | `disassembly` | `lw v1,0x6ac(gp)` at `0x8001E900` forks three ways: `0` read the entry, decompress and register; `2` re-sum, decompress and register; `1` register only. Twelve writers keep the register-only arm away from a reused buffer - `FUN_80016230` zeroes the word on every mode step outside 2/3 (`0x800163B4`), PROT 0978's post-battle restore writes `0` then `2`, and the core reset, the minigame warp and the field overlay write `0`. The earlier probe's `0x8007B83C` "`== 2` gate" was `gp+0x524`, the game mode ([falsified](re-do-not-re-walk.md#battle--arts--level-up)). |
| Why do PROT 0943's `0x40` and PROT 0944's `0x53` fault before their first tick? | resolved (they do not - the fault is SCUS walking the caster) | `capture` + `disassembly` | Both stage clip `0x0B` on the caster (`sb 0x0B,0x1DA(s1)` at `0x801F6FBC` / `0x801F758C`), and the anim commit `FUN_8004AD80` resolves a staged clip by indexing the monster record's spell-entry offset array with it (`0x8004AF08..0x8004AF18`). Gobu Gobu's array holds ten entries, so index `0x0B` reads the record's name text as a pointer - which is the unmapped read the earlier runs paused on. Forced on a twelve-entry caster both casts complete with zero unmapped accesses, walking arms 0..4 in 1, 9, 40, 8 and 32 ticks. No monster record's magic slots name either id, so both are casts retail never performs. |
| What is battle context `+0x276`? | resolved (the side-band applier's **stage** byte) | `disassembly` | Its writers are `FUN_801DABA4` (`0x801DAD6C`), `FUN_801E295C` (`0x801E49F4`) and the applier SM `FUN_801F12D0`; no tutorial routine writes it. Both CD-XA cue arms test it, and the summon modules poll it before raising their own head cue, so it is open by construction at the moment a cast wants it. A port that fed it from a tutorial flag silenced the melee sting in exactly the battles the flag was set in. |
| What does `FUN_8003DE7C(1)` count? | resolved (vsyncs left in the CD read span) | `disassembly` | `gp+0x91C` is an `i32` the routine drains by `DAT_1F800393` per call and floors at zero (`0x8003DF24..0x8003DF34`), answering "busy" while it is positive. Every XA clip therefore leaves the drive nominally busy for its own duration, which is what a cue arm polling for an idle read is waiting out. Engine mirror `AudioState::battle_xa_busy_frames`. |
| What does the dance count-in banner draw? | resolved (a sprite record, 160 x 32) | `disassembly` | Record 0 of the 20-byte HUD table at `0x801D46CC` - texel seat `(0x48, 0x90)`, texel cell `0xA0` x `0x20`, CBA `0x7D0A` - seated by `FUN_801D2F38`, which halves the cell at the caller's `0x1000` unit scale (`(w * scale) >> 13` at `0x801D3180..0x801D319C`) and centres the result on the seat. Reading `+0x0A` as half-extents doubles it to 320 x 64. It is not text, and its animator samples once per three vsyncs. |
| Does any item reach the Point Card strike (effect class `14`)? | resolved (no - reachable code over unreachable data) | `disassembly` | Arm `14` at `0x8004209C` takes `min(_DAT_800845B4, 9999)` off the Point Card bank and applies it as battle damage, and decoding all 256 item records against the effect table finds no item carrying class `14`. Unused content rather than a missing item; only a forced bank reaches it. [`item-effect-table.md`](../formats/item-effect-table.md) |
| What does an arts book write, and to whom? | resolved (the **class** picks the character, the **tier** is the art id) | `disassembly` | `FUN_800402F4`'s arm at `0x80041FB4` turns the class into a roster slot with `addiu v1,v1,-0xb` at `0x80041FC0`, so classes `11`/`12`/`13` are Vahn / Noa / Gala and the ally the player picked is never read; the tier byte is stored verbatim as the learned id by `sb s6,0x74e(a0)` at `0x80042064`, into the sorted position the loop above it opens. The nine book records carry Fire and Thunder `I`/`II`/`III` = `3`/`2`/`1` and Wind = `5`/`4`/`1`, which is a per-character art-id space and not a book level. [`item-effect-table.md`](../formats/item-effect-table.md#arts-books-class-111213-the-tier-is-an-art-id) |
| Does retail bound the sprite-stack append its fog sheets use? | resolved (no bounds check) | `disassembly` | `FUN_8001FA68` is an eight-instruction append whose store sits in the `jr ra` delay slot; its caller loads a capacity into `a2` (`lh a2,2(a0)` at `0x8003F7FC`) that the callee never reads. The port's `cutscene::sprite_stack_push` is that append; `list_append_u16` was the same address ported twice. |
| Which battle-action state owns the only arm that reaches `FUN_801F3990`? | resolved (state `0x3D`, and an **item** is the door) | `disassembly` + `capture` | One reference disc-wide, the `jal` at `0x801E3E04`, inside action-SM table slot `0x3D` (base `0x801CED44`, the arm's word at `0x801CEE38`) - the Spirit / Item wait state, entered only from `0x3C`, whose `sb v0, 7(v1)` at `0x801E3B60` is unconditional: the arm's one branch, `sltiu v0, v0, 3` at `0x801E3B28`, rejoins at `0x801E3B40`, above it. The Magic arm `0x801E2EB0` reaches `0x3C` only for a class byte `< 0x14` **and** a spell id `< 0x65` (`sltiu` at `0x801E2EF4`), which the player Seru block cannot satisfy. [details](../subsystems/battle-action.md#the-one-caller-is-state-0x3d-and-it-is-an-item--spirit-state) |
| What writes the slot-B stage selector `_DAT_8007B64A`? | resolved (the field entity tick, off system flag `0x19`) | `disassembly` | A `gp`-relative sweep finds 14 accesses - 7 stores, 5 loads - where an absolute-address scan finds none. The value writer is the field entity tick `FUN_801DA51C`: it clears the byte at `0x801DA69C` (in the delay slot of a `jal 0x8003CE64`) and raises `1` at `0x801DA6A8` when system flag `0x19` is set, which the consumer then reads back. `1` pages extraction 967, the battle tutorial; `0` skips the load (`beqz` at `0x80052688`). Battle latches `3` at `0x801E6D2C`, right after `FUN_8003EC70(0x4A, 0)` pages 969 directly. Zeroing sites: SCUS `0x80046E74` / `0x800557AC` / `0x80056114`, field `0x801DA69C`, and both stage images at `0x801F7120`. |
| Which extraction entry does the slot-B pager's argument name? | resolved (`selector + 966`) | `disassembly` + `capture` | `FUN_8003EC70` computes `a0 + 0x381` in **raw TOC** space, and raw is extraction `+ 2` (measured across every TOC entry), so the pager's own index is extraction `a0 + 895`. The one site that hands it the stage selector, `0x8005269C`, passes `selector + 0x47`, so selector `n` pages extraction `n + 966`: `1 -> 967`, `2 -> 968`, `3 -> 969`, and `0` skips the load. The same algebra reproduces the band's other two call sites, `+0x28 -> 935 + i` and `-0x79 -> 903..913`. |
| What do PROT 0968 and 0969 do? | resolved (the two boss-stage modules, one function each) | `disassembly` | Neither is a cast module: neither is named by any of the three PROT 0898 entry tables, and the pager is the only thing that brings them in. Each is one function over its whole image, a phase machine on `ctx[+0x289]` driving the first monster seat. 0968 (`FUN_801F69F4`, seven phases behind the table at `0x801F69D8`) is the arrival staging - camera walk, cue `0x20A`, banner, then a hand-back clearing the stage id at `0x801F7120`. 0969 (`FUN_801F69D8`, four phases, no head table) is the form transition - cue `0x20B`, the seat's HP `+0x14C = 1`, a two-position camera shake, then `FUN_8003ED04(0)`. [details](../subsystems/battle.md#what-the-two-boss-stage-modules-do-overlays-968--969) |
| What is the `!= 4` term in the battle selectable scans? | resolved (a **seat**, not an action state) | `disassembly` | `DAT_8007BD10` is the per-slot roster character id: `FUN_801DA34C` indexes it and subtracts one to reach a character record (`0x801DA37C..0x801DA3B0`). `4` is the AI-companion seat, so the scans are excluding a seat the player does not command. Reading the term as an action-state byte where `4` meant removed or done put a phantom state into the action-SM documentation ([falsified](re-do-not-re-walk.md#battle--arts--level-up)). |
| What is the auto-command string pair at record `+0x1A7` / `+0x1B7`? | resolved (a staged queue and its backup) | `disassembly` | Gated on `DAT_8007BD04`; the band is chosen by `sltu(+0x156, +0x154)`. The primary leg falls back to `+0x77F` when its head is empty; the secondary zero-fills with **no** fallback (`beq` at `0x801DA4CC` into `0x801DA51C`). The write-back overwrites exactly one band and is guarded on `+0x14C != 0` and `+0x1DE == 3`. The `0x5C8` save-window delta corroborates the pair: `0x76F - 0x5C8 = 0x1A7`, and the patcher's performed-Super mask `+0x75D` lands at record `+0x195`. |
| Is `FUN_801D0748` the Muscle Dome's match SM? | resolved (**no** - it is the round SM every battle runs) | `disassembly` + `capture` | It has exactly **one** `jal` across `SCUS_942.54`, every based overlay image and every raw PROT entry: `0x80047014`, inside the SCUS battle frame driver `FUN_80046A20`, with no test in front of it and the fall-through rejoining at `0x8004701C`. Three non-dome battle states enter it 350 / 313 / 255 times over 700 vsyncs each, every entry with `ra = 0x8004701C`. The dome is one of its callers' contexts; see [`minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md). |
| What draws the dome panel still at VRAM `(384, 0)`? | resolved (`FUN_801D00F8` in the contest hub PROT 0977) | `disassembly` + `capture` | Two `POLY_FT4` quads, tpage `0x106` over screen `(0,-20)-(192,220)` and `0x109` over `(192,-20)-(320,220)`, sampling `(384, 0)..(704, 240)`; `*(0x801D1A7C)` is broadcast into all three colour lanes, so it is a fade level and the call is skipped at zero. The emitter never materialises `384` - a textured primitive addresses VRAM through the packed `tpage` page index, `384 / 64 = 6` - which is why a sweep for `0x180` could not find it. Geometry in [`ringside-still.md`](../formats/ringside-still.md#what-draws-it). |
| What raises the still's fork `_DAT_801D1AE0`? | resolved (the arena init, on **re-entry**) | `disassembly` + `capture` | Not the match teardown: `FUN_801CEA6C` forks on the arena word `_DAT_8007BAC0` and stores `0` to the latch at `0x801CEB54` on a first entry (op `0x3E` leaves the word clear) and `1` at `0x801CEC04` on every later one. A 3600-vsync walk into the arena enters the emitter 79 times and takes the six-tile arm every time. The natural arm is now captured too: the checkpoint RAM of a three-visit dome run reads latch `1`, hub arm `0x0A` and level `8` at the second and third visits, with both still packets (`0x2C080808`) in the primitive pool. [`ringside-still.md`](../formats/ringside-still.md#on-a-natural-re-entry) |
| Who raises the Arts banner selector `ctx[+0x28B]`? | resolved (SCUS `FUN_8004AD80`, in sequence, not as alternatives) | `disassembly` | Every `sb ...,0x28b` on the disc is in `FUN_8004AD80`: the raises at `0x8004B774` (`3`), `0x8004B80C` and `0x8004B87C` (`2`), all in the `actor[+0x1DA] == 0x1A` SpecialStarter arm - and `0x8004ADDC` adds `4` to a live one for the matching seat. `0x8004B774` falls through to `0x8004B7D8`, whose read of the side array `0x801F6990` (`0x8004B804`) is stored over the `3`. The four positions are `1` NEW (build-starter mark), `2` HYPER (default), `3` MIRACLE (`ctx[+0x28D + slot]`), `4` SUPER (super-starter mark). [`battle-action.md`](../subsystems/battle-action.md#the-raiser-and-why-its-three-writes-are-not-alternatives) |
| Can a RAM clobber of the resident PROT 0874 container trip `FUN_8001E890`'s checksum? | resolved (**no** - the sum is over VRAM) | `capture` | Forcing `gp+0x6AC = 2` reaches the sum arm; a genuine mismatch enters `0x8001EA08` and self-heals in about 45 vsyncs (clear the gate, `j 0x8001E900`, CD re-read, `gp+0x6AC := 1` at `0x8001EB0C`). Four XOR'd words of the resident container left the sum byte-identical (`0x7BF74962`), because it is taken over a `StoreImage` (`0x8005842C`) read-back of VRAM. Only VRAM at `(0x180 + 0x40i, 0)` or the boot sum at `gp+0x6B8` can break it. Probe `scripts/pcsx-redux/autorun_w5a_0874_clobber.lua`. |
| What does `FUN_801D84C0` build? | resolved (the four **battle-result messages**, not party panel labels) | `disassembly` | Its two arms copy (`FUN_8003CA78`) and append (`FUN_8003CAC4`) pool strings at `0x801F4C38..0x801F4CC4` in PROT 0898 into `ctx+0xA9` / `+0x129` / `+0x159` / `+0x189`: a victory line with its spoils sentence, a defeat line, and the two escape outcomes. `FUN_8003CBF8(buf, 0xC1, 1)` (`0x801D86A4` and siblings) locates the `0xC1` **name escape** - it is not a width measure - and every patch writes the **first** seat's id, so a lone lead is named and a party is the lead's team. [`live-audit-triage.md`](../tooling/live-audit-triage.md#panel_labels-read-the-wrong-thing-out-of-the-right-bytes) |
| Where are the steal-result captions composed? | resolved (in SCUS `FUN_8004AD80`) | `disassembly` | The three `jal FUN_8003CB54` sites on the disc are all in `FUN_8004AD80` (`0x8004B2F8`, `0x8004B338`, `0x8004B60C`), each after a `FUN_8003CA78` copy of a template formed at `0x80077A38` / `0x80077A4C` / `0x80077A64`; `FUN_8003CB54` appends the `0xC2` escape pair. The caption is shown through `FUN_801D8DE8(0x5B)`. Whether the port runs the steal at all is [open](open-rev-eng-threads.md#battle--rendering). |
| What is `FUN_801D32BC`? | resolved (the command window's **member cursor**, not a turn order) | `disassembly` | Six `jal` sites in PROT 0898: the round reset `0x801D8910`, `FUN_801D388C`'s case `0x10` (back, `0x801D4010`), `0x11` (forward, `0x801D4128`), `0x21` (back, `0x801D4750`) and its tail pair `0x801D5690` / `0x801D56A0`. Initiative is the execution order and the port's command order is already retail's slot scan; what the port lacks is the backward step ([open](open-rev-eng-threads.md#battle--rendering)). |
| At what level does a re-entered hub draw the ringside still? | resolved (`*(0x801D1A7C)` across six hub arms) | `disassembly` | The re-entry init seeds hub arm `0x0A` at `0x801CEE2C` and zeroes the level at `0x801CECD0`. Arm `0x0A` raises it `4 dt` to `0x80`; `0x0B` lowers it `2 dt` to `0x40` while tally lane 0 sits at its clamp; `0x0C` holds; `0x14` raises it `4 dt` to `0x80` on the latch-`1` arm only; `0x15` holds under the ROUND banner; `0x16` drains it `2 dt` to `0`. Dispatch is the hub's 51-entry table at `0x801CE990`. [`ringside-still.md`](../formats/ringside-still.md#on-a-natural-re-entry) |


### The battle hit tint - what a landing hit writes and how it reaches the pixel

*Status:* resolved - a landing hit stamps a three-word tint triple on the
struck actor from the acting record's `+0x7A` (party melee / arts) or the
move-power `+0x0A` (monster specials); the presentation SM eases it back;
the draw stages the words as GTE far colour + `IR0` so the texel is
modulated, not replaced. Grade: **capture** (the triple, its ease rate and
the pixel law read off two consecutive save states; every write and gate
also in disassembly).

- **The stamp.** `FUN_801EC3E4` `0x801EE3D4..0x801EE43C`: `+0x04 =
  0x801F53D4[sel - 1]`, `+0x21F = sel`, `+0x0C = 0x1000`, with `sel =
  record[+0x7A]`, `beq zero` past the arm and `sltiu v0,v0,0x6` bounding
  it; no exit precedes the arm, so every connecting swing (a Stone-absorbed
  one included) reaches it. The bound is a route, not a table guard: the
  disc carries `6` on six archive entries and `6` is the tint-less Curse
  arm (`0x801EE690`, a 1-in-4 `+0x16E |= 0x1000` roll) - a census that
  asserted "never past the table" failed on the data and was rewritten
  (`battle_afterimage_gate_real.rs`). `FUN_801E09F8` `0x801E15AC..0x801E15EC` is the
  monster-special twin off `move_power[+0x0A]`, at each arm's impact phase
  (`ctx[+0x24E + i] == 3`). The clip-`0x18` arms of `FUN_8004CE2C` stamp
  the same three words (`0x8004D1D4..0x8004D1E4`, `0x8004D28C..0x8004D29C`).
- **The ease.** `FUN_80050120` arm 0 (`0x800501A4..0x80050210`): the word
  eases to `0x20080200` at `1 * dt * 8` lane units per frame, then `+0x0C`
  drains by `dt << 5`, then `+0x21F` clears; the jump table at
  `0x8001532C` routes states `1`/`3`/`4`/`6..=10` to fixed colours with
  `+0x0C = 0x1000`, `2` to the capture fade, `5` to a hold. Two consecutive
  captures (`battle_gimard_tail_fire_b` -> `_a`) hold Vahn at `+0x21F = 1`,
  `+0x0C = 0x1000` with the red lane at `0x35F` then `0x31F` - eight frames
  of the ease. The converse pin: `battle_melee_hit_spark` (Vahn's
  Somersault landing on Gimard) holds Gimard at the neutral word, `+0x0C =
  0`, `+0x21F = 0` - a selector-0 record tints nothing, and on the disc
  every player-file **basic** entry and every Somersault-class art is
  selector `0`; only five Vahn art records (selector `1`) and two Gala
  records (selector `2`) tint their target.
- **The pixel.** `FUN_8004A908` packs the lanes into the render node's
  `+0x74` and copies `+0x0C` into `+0x78`; `FUN_80048A08` stages them as far
  colour + `IR0` (`gp[0x9D8]` / `gp[0x9DC]`). The struck Vahn in that
  capture reads red `160..248` over green / blue `8..80` across his texture
  - modulation, not a flat fill.

Port: `engine-vm::battle_formulas::tint_sm_step` (the SM),
`World::arm_impact_tint` at the three hit seats, `MonsterAnimation::
impact_class` carrying `+0x7A`, and both hosts staging `IR0 = blend /
0x1000` toward the unpacked lanes (`battle_impact_fx::tint_ir0`). The
native window's former white "hit flash" and the hosts' fixed `0.6`
strength were engine inventions and are gone. Details in
[battle.md](../subsystems/battle.md#how-the-tint-words-reach-the-pixel).
### Battle end - the results sequencer's timeline, and who strikes the pose

*Status:* resolved - `FUN_8004E568` runs every frame the signal is `0xFE`, the exit is `ctx[+0x6CE] >= 0x43`, and the **leader** poses; shipped as `world::battle::victory`

`FUN_80046A20` stops stepping the action SM once `DAT_8007BD71 == 0xFE` (`0x80047040`) and calls
`FUN_8004E568` every frame (`0x800470D0..0x800470E8`); it exits the battle at `slti v0,v0,0x43`
on `ctx[+0x6CE]` (`0x80046DAC`). `_DAT_8007BD2C` doubles as the sequencer's phase word (jump
table `0x800152FC`: `0 -> 2 -> 4 -> 5` through two CD loads for a victory; a wipe's `5` lands on
the annihilated arm directly). The results frame stages the pose into `+0x1DA` of the seat
`ctx[+0x13]` names, and no store in the battle overlay writes a seat there. Measured on
`rim_elm_gimard_victory` (PCSX-Redux poll, N=1): signal v322, results v402, exit fade v657, exit
v723. The three-member `noa_levelup_banner` state reads `ctx[+0x13] == 0` with seat 0 carrying
pose `0x14` while Noa levelled. Evidence: `disassembly` + `capture`. Details:
[battle.md](../subsystems/battle.md#battle-end-retails-way---the-results-sequencer).

### What a normal party attack sounds like

*Status:* resolved - an ordinary swing emits **neither** cue (four captured party swings all fail the `s7` gate); the `XA30` grunt is the arm for a strike that commits the defender's `+0x1F3` reaction; the `0x10C` sting only when the effect handle `_DAT_8007BD84` is non-null

`FUN_801EC3E4` picks one of two emissions on `_DAT_8007BD84`: zero takes `FUN_8003D53C(0x1D,
chan, dur)` at `0x801EEB44` (per-character `(0,0x26)` / `(4,0x2E)` / `(6,0x1A)` off
`DAT_8007BD10[seat]`) and the re-read at `0x801EEB60` skips the cue; non-zero branches over the
grunt (`0x801EEAC8`) into `FUN_8004FE5C(0x10C, seat)` at `0x801EEBE8`, whose voice leg is
further gated on `FUN_8003DE7C(1) == 0` (`0x8004FE9C`). The word is the same cell the damage
finisher reads as the enemy-defender halve; its only dumped stores are zeros (battle start,
round reset). `XA27` is an eight-channel stereo sting bank, `XA30` a ten-channel mono grunt bank
(disc demux). Evidence: `disassembly` throughout. `_DAT_8007BD84` is an
**effect-instance handle**, not a mode word: an all-forms sweep (the `gp` form
`0xA6C(gp)` yields zero) over SCUS, all 1233 PROT entries and the overlay images
finds exactly three stores - `0x8004D658` and `0x80056080`, both `sw zero`, and
one non-zero at PROT 0940 file `+0xCA0` = `0x801F7678`, the Cort "Mystic Shield"
stager saving the `FUN_80021B04` handle it spawned. `FUN_8004CE2C` dereferences
it (`0x8004D548`; `+0x56` / `+0x72` written, `+0x10` read) and consume-releases
it at `0x8004D658` alongside cue `0x10D`; an ordinary swing pages in no capture
module, so the cell reads null. The earlier "grunt latch always passing" is
**falsified**: `0x801EEA88` / `0x801EEAA0` require `s7 != 0` and `s7 ==
actor[s4][+0x1F3]`, `s7` being the staged pose byte committed to `+0x1DA` at
`0x801EEC6C`, and only one of the fourteen definitions reaching the compare
loads `+0x1F3`. The grunt's seat is `s6` (`0x801EEA70`), the sting's `s4`. The
third gate `0x801EEAB8 slti v0,v0,0x2` reads `_DAT_8007BC20`, which is the
executable's own `xa_flag` debug counter (XA-drive state - `FUN_80016B6C`
prints it at `0x80016EB8..0x80016EC0`, `FUN_8004DA00` zeroes it on five arms),
not a character level. Live (`capture`, N = 4, probe
`autorun_w4d_melee_grunt_gate.lua`): every captured party swing skipped the
gate - `s7` came from the `+0x1EF` / `+0x1F0` / `+0x1F1` reaction-pose loads,
never `+0x1F3` - and neither `FUN_8003D53C` nor `FUN_8004FE5C` was called, so
"the grunt on an ordinary swing" was too strong: the grunt fires when a strike
commits the `+0x1F3` reaction (behind `sltu s0,s1` at `0x801EC878`), whose
threshold is not yet pinned. Shipped:
`World::fire_melee_impact_cue` + `read_battle_xa_clip_bank`.

### `FUN_8003EAE4` is a seek plus bookkeeping - and the driver is not untraced

*Status:* resolved - the routine seeks and raises `gp+0x908`; the CD-callback sequencer it does **not** arm is `FUN_8003D764`. Grade `disassembly`.

`FUN_8003EAE4` cancels any in-flight read, positions the drive on the clip
file's record (`FUN_8005C160(2, 0x801C6ED8 + slot*8, 0x8007BC10)` - the argument
is the record pointer, not the slot index), issues CD command `0x15` (`li
a0,0x15; jal 0x8005C034` at `0x8003EB68`) and stores `1` at `gp+0x908` /
`gp+0x910` and the slot at `gp+0x890`. The "which no dump follows" reading is
falsified in both halves. The driver is `FUN_8003D764` (dumped;
`functions/script-vms.md`), it dispatches solely on the state ring `gp+0x928`,
and the only writer of `gp+0x928` is `FUN_8003D53C` (`0x8003D6F4`,
`0x8003D724`), which also registers the callback. `FUN_8003EAE4` never writes
`gp+0x928` - it reads it as an entry gate that makes the whole routine a no-op
while a clip is armed. Of its own three cells only `gp+0x908` has any reader (a
"streamed clip busy" level); `gp+0x910` and `gp+0x890` are write-only across
`SCUS_942.54` and all 1233 PROT entries. So a lone `FUN_8003EAE4` call seeks and
books, and streams nothing.

### Formation species limit - what the battle setup does with 3 distinct monster ids

*Status:* resolved - `[a,b,c]` loads as `[c,c,a]` on half of all rolls, verbatim (three streamed blocks) on the other half; randomizer capped at 2 distinct species. Grade: **capture** (write watchpoints + cell readback + heap walk; the rebuild loop also read in disassembly).

Retail authoring never puts more than two distinct species in one formation,
and the battle setup `FUN_80055B6C` (loop at `0x80055C80..0x80055D2C`) is
written to that invariant: it counts copies of `cells[0]` and holds "the other
species" in a **single register**, then - behind a 50% coin flip
(`FUN_80056798() & 1`) - rebuilds the cells as `[other x n, first x m]` (the
species-order variety shuffle; an exact multiset swap for 2 distinct species).
A third distinct species overwrites the register: the middle id vanishes and
the last is duplicated. Pinned by `autorun_formation_cell_writers.lua` (write
PCs `0x80055D14/18`; installs of `[133,151,94]` / `[94,133,151]` /
`[151,133,94]` / `[32,34,14]` all read back `[c2,c2,c0]` at battle main, seat
1 sharing seat 0's record pointer) across both the map03 and rikuroa contexts.
On the verbatim side of the flip all three distinct blocks stream, which turns
an over-budget trio into a probabilistic battle-load hang (see the heap-budget
section of [`battle.md`](../subsystems/battle.md) for the freeze captures).
Consequence for the encounter randomizer: the unconditional battle-load safety
pass (`enforce_species_limits`) caps every random formation at 2 distinct
species and at the disc's authored heap-cost maximum.

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| Encounter MAN sub-section layout | resolved (header shape corrected) | `disassembly` | [details ↓](#encounter-man-sub-section-layout) |
| Battle-intro tile shatter - the side-face shade page | resolved (a resident field asset, not a transition upload) | `capture` | [details ↓](#battle-intro-tile-shatter---the-side-face-shade-page) |
| How long does a field-to-battle transition run (`DAT_801D2458`)? | resolved (132 frames; 252 for the swirl) | `disassembly` | [details ↓](#battle-intro-transition-length---dat_801d2458) |
| Which stage-dome objects does the battle backdrop draw? | resolved (drop index 1, not "keep index 0") | `disassembly` + `capture` | The registration edits the object list rather than truncating it: each backdrop actor owns a private `0x9c` part table at `+0x44` (allocated at `0x80021184`), and `0x80051ad4..0x80051bac` applies one `count -= 1` plus one `entry[i] = entry[i+1]` shift from index 1 to each. Object **1** is dropped and everything else kept, gated on `_DAT_8007b64b == 0`. Indistinguishable from "draw object 0" on the two-object shells; on the seven four-object domes it keeps sky, mountains and the ground ring. See [battle.md](../subsystems/battle.md#object-1-is-dropped). |
| Battle ground grid depth cue - the far colour | resolved (captured at the draw; port fogs the grid) | `capture` | [details ↓](#battle-ground-grid-depth-cue---the-far-colour) |
| Which element the screen-element `+0x0E` **kind pair** selects, per value | resolved (it is a table index, not a style enum) | `disassembly` + `capture` | [details ↓](#the-chrome-kind-byte-is-an-index-into-the-widget-class-table) |
| The element-**badge** palette selector | resolved (the badge record's own palette byte) | `disassembly` + `capture` | [details ↓](#the-element-badge-palette-selector) |
| The status-element badge sheet `0x18..=0x20` | resolved (nine 48x16 word tags; ladder assignment independently confirmed) | `disassembly` + `capture` | [details ↓](#the-status-element-badge-sheet-0x180x20) |
| Does the battle ground grid roll per-cell randomness? | resolved (no - a four-entry table walk) | `disassembly` | No. `func_0x801d02c0` builds sixteen literal UV words into scratchpad `0x1f800034` (`0x801d0304..0x801d03a0`) and the emit loop reads group `n` for quad `n`, advancing `0x10` each time. They decode to four fixed 32x32 sub-tiles of the `(192..=255)^2` window walked in `sub_row * 2 + sub_col` order, copied into the packet verbatim - no roll, no corner mirror. The grid origin also carries an extra `-0x200` bias on `z`, and pass 1's cull is a view-`z` bracket with **no** screen-space term (that is a separate pass-2 test). See [battle.md](../subsystems/battle.md#the-grids-own-constants-read-off-the-emitter). |
| Endless camera orbit (Gaza 2 softlock) - the `0x19` attack-approach park | resolved (caught live; root-caused; disc fix shipped) | `capture` + `disassembly` | [details ↓](#endless-camera-orbit---the-0x19-attack-approach-park) |
| Which `FUN_801D5854` case-6 arm a running fight takes, and where `ctx[+0x6DA]` is seeded | resolved | `capture` + `disassembly` | [details ↓](#the-in-fight-action-framing-and-the-yaw-counter-ladder) |
| Which framing the Done band (`0x50` / `0x51`) takes, and the idle-orbit rate at the Begin/Run prompt | resolved | `capture` + `disassembly` | [details ↓](#the-done-band-framing-and-the-two-orbit-writers) |
| `0x19` fallback approach drive - which anim-driver field does summon staging leave stale? | resolved (pinned + causally reproduced on the parked save) | `capture` + `disassembly` | [details ↓](#the-summon-then-melee-park-trigger---the-stale-field-is-0x1dc-bit-2) |
| Super / Miracle Arts trigger chain | resolved (all 15 Supers live-executed) | `disassembly` + `capture` | [details ↓](#super--miracle-arts-trigger-chain) |
| Xain "Bloody Horns"/"Terio Punch" ignore elemental guards (community mystery) | resolved | `disassembly` + `capture` | Not an element drop - a **resist-ladder bypass**. Capture-class casts (spell byte `+0` = `'c'`) run per-spell modules (PROT 944..966) whose damage calls pass the caster's seat but pick one of two wrappers: `FUN_801DD4B0` (finisher `param_5=0`, resist ladder runs) or `FUN_801DD6B4` (`param_5=1`, the whole party-defender jewel/guard block is skipped). BH (952) / TP (953) use the bypass wrapper for their main hits; enemy ESM (966) uses the respecting one (hence Cort reads as Dark). Element attribution law + live confirmation: [battle-formulas.md](../subsystems/battle-formulas.md); cast classes: [spell-table.md](../formats/spell-table.md#cast-classes-record-byte-0). |
| First boss trigger → Battle | resolved | `disassembly` | The scripted-battle arm is the field-VM op `3E FF <formation_row>` ([battle.md](../subsystems/battle.md#scripted-battle-entry-3e-ff-row)): Zeto = garmel `P2[12]` row 9 (lone `0x4B`), Caruban = rikuroa stager `P1[3]` row 17 (lone `0x49`, `World::run_boss_stager_record`). `DAT_8007b7fc` closed: writer-less across `SCUS_942.54` + every static overlay (validated absolute + gp-relative + address-materialisation sweep); readers pin it as the debug forced-battle formation id - battle init `FUN_80055b6c` → `FUN_8005567c` seeds the formation cells `DAT_8007BD0C+` from it, and `FUN_80046A20` routes a nonzero value to its mode-0 debug-menu exit. Retail never sets it. See [battle.md](../subsystems/battle.md). |
| How an enemy's signature cast picks the spell id it announces | resolved | `disassembly` | `FUN_801E9FD4`'s round-counter arm, `0x801EB7C0..0x801EB81C` (PROT 0898 file `0x1CFF0..0x1CFFC`): the round byte `ctx[+0x28A]` goes through the `0xAAAAAAAB` reciprocal, the arm returns unless `% 3 == 2`, then stores that remainder to `actor[+0x1DE]` (action category), loads the seat's byte from the formation cell `0x8007BD0C + seat`, and writes `actor[+0x1DF] = monster_id - 0x29` - the literal `0x2442FFD7`, with `sb v0,0x1DF(s4)` in the next `j`'s delay slot. Formation id `162` announces spell `0x79`, `163` `0x7A`, `164` `0x7B`, in the party spell table's own id space. See [randomizer.md](../tooling/randomizer.md#delilas-party-swap). |
| How a Delilas sibling's signature move stages its clips | resolved (walk order capture-pinned) | `capture` | The modules drive `actor[+0x1DA]` as an archive entry index; per-frame capture of natural duel playouts orders the walks. Lu (0960): `0x0E -> 0x0C -> 0x0D`, closing `0x0F`, all literals. Che (0959): `0x0A -> 0x0B`. Gi (0958): `0x0A -> 0x0B -> 0x0C -> 0x0A -> 0x0B`, closing `0x0D` - the `addiu v0,v0,-2` at `0x801F839C` is a **mid-cast rewind**, not a stray step. Sites + walks: [monster-animation.md](../formats/monster-animation.md#a-special-attack-can-be-a-chain-of-entries); anatomy: [cast-module.md](../subsystems/cast-module.md). |
| Is the player battle idle channel-delta encoded, like the readef "ME" bodies? | resolved (no - raw packed inline in `record[0]`) | `capture` | Each action entry in a player battle file's `record[0]` carries `[u8 parts][u8 frames]` at `+0xAC` then `parts * frames` raw 9-byte TRS records - the monster archive's own packing, no codec between disc and RAM. Entries tile the block: Vahn's slot 0 is 15 parts x 9 frames ending at `0x5CD`, slot 1 opens at `0x5D0`, and so on down the table (Noa's rig is 16 parts to Vahn's and Gala's 15). `battle_party_pose_live` byte-matches the disc stream against live PSX RAM at `record0_base + stream_off`. So an idle rewritten at its exact retail length leaves every later entry offset valid and needs no relocation. |
| Enemy-ally charm battle softlock | resolved (both tracks fixed) | `disassembly` | The state-`0x5A` victory arm's party-slot assumption OOB-indexes the win-pose roster `DAT_8007BD10` (via `0x801E6770`) when a living charmed ally is the acting actor at monster-wipe victory - the `FUN_801E7320` reroll theory is falsified ([`re-do-not-re-walk.md`](re-do-not-re-walk.md#battle--arts--level-up)). Fixed on both tracks: engine `victory_pose_fixup`/`charm_widen`, and the disc-side `legaia_patcher::charm_fix` guard - a single-word detour at the `0x801E6690` keep-branch into a SCUS dead-space liveness guard. Full chain + port: [battle.md](../subsystems/battle.md#enemy-ally-charm-at-the-end-of-action-gate-the-charm-battle-softlock). |
| Battle-actor `+0x16E` bit `0x400` applier (guard-disabling status) | resolved - exhaustive negative | `disassembly` | Bit `0x400` has **no retail setter**: a word-level decode of `SCUS_942.54` + every static-overlay image (all stores covering `+0x16C..+0x171`, pointer precomputes, `ori`/`sllv` bit-set shapes, the `+0x6F6` mirror, the `+0x21F` deferral) finds only clears - accessory cure `FUN_8004CE2C`, the per-round RNG waker `FUN_801F45A4`, item cures, the on-hit strip, battle-exit. The appliers (hit leg `FUN_801EC3E4`, cast leg `FUN_801E09F8`) map kinds 3/4/5/6 → `0x1`/`0x2`/random-`0x38`/`0x1000`, kinds 1-2 → the `0x380` deferral; none reaches `0x400`. Latent content. Writer inventory: [battle.md](../subsystems/battle.md#the-0x16e-status-halfword---retail-writer-inventory). |
| Who calls the battle on-screen test `FUN_8005126C`? | resolved - exhaustive negative | `disassembly` | [details ↓](#who-calls-the-battle-on-screen-test-fun_8005126c) |
| What spawns the battle XA voice selector `FUN_8004DA00`? | resolved | `disassembly` | Nothing calls it - it is the `+0x08` tick of the [static actor template](functions/runtime-libs.md#static-actor-templates) at `0x800767F4`, and the battle scene-loader `FUN_800513F0` spawns that record into the system actor pool at `0x80051D3C` as its last act before returning. The selector is therefore a per-frame pass resident for the whole battle. Port `legaia_engine_audio::battle_voice`; the same reading corrects the template's base and tick offset (the `+0x0C`-from-`0x800767F0` frame was skewed 4 bytes low). |
| Effect-VM pass-1 "state token algebra" (`FUN_801E0088`) | resolved + ported | `capture` | [details ↓](#effect-vm-pass-1-state-token-algebra-fun_801e0088) |
| Seru-magic summon visual (e.g. Tail Fire) | resolved (player visual; wired) | `capture` | [details ↓](#seru-magic-summon-visual-eg-tail-fire) |
| Why a disc-booted port fight never showed an enemy special | resolved (the boot catalog lacked the disc's monster-special ids) | `capture` | The picker rolled Gimard's `+0x21` Tail Fire (`0x27`, read off the `battle_gimard_tail_fire_a` RAM) on half its turns and `take_monster_turn` discarded it: `SpellCatalog::vanilla()` has no `0x27` and the boot catalog only layered the Seru block over it, while its fabricated placeholders sat on real ids (`0x26` at 9 MP over Thunderbolt's 18). The disc table now fills the block below `0x81`; see [`battle-action.md`](../subsystems/battle-action.md#the-monsters-cast-is-only-as-real-as-the-catalog). |
| The party cast trigger `FUN_801DBF9C` - outcome producer or params stager? | resolved (a params stager; every Seru id takes the summon arm) | `disassembly` | [details ↓](#the-party-cast-trigger-is-a-params-stager) |
| Player-summon presentation - label, readout, hide, flashes, creature seat | resolved (ported behind the band's own seams) | `capture` + `disassembly` | [details ↓](#player-summon-presentation) |
| Fade-actor hold word `-1` - "no hold" or "hold until killed"? | resolved (hold until the actor is killed) | `disassembly` | [details ↓](#the-fade-actors-hold-word) |
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
| Battle-intro tutorial boxes (Tetsu sparring fight) | resolved (machine pinned, ported and wired) | `disassembly` (machine exclusivity byte-anchored; prompt pool shared with 0968) | [details ↓](#tutorial-prompt-machine-exclusivity) |
| What arms the battle-stage id `1` (sparring tutorial) | resolved | `disassembly` + disc bytes | Not the formation, scene or monster: a one-shot system-flag arm. `FUN_801DA51C`'s battle-entry tail defaults the stage-id byte to `0` in a delay slot, tests flag `0x19`, and on a set flag writes `1` and clears the flag. The disc's only setter is town01's Tetsu record (`50 19`, two ops before its `3E FF` battle-entry op). Port `battle_tutorial::TUTORIAL_ARM_FLAG`. [details ↓](../subsystems/battle.md#who-writes-stage-id-1---the-one-shot-arm-flag-0x19) |
| Mid-battle second `_DAT_8007B64A` stage-id writer (the `0x801fd514` phantom) | resolved (coordinate re-keyed) | `disassembly` | `FUN_801E6968`'s tail arm (the Lost Grail Final Heal sweep's epilogue; `sb` at `0x801E6D2C`, battle-SM cleanup state `0x50`): formation head still `0xB5` (Cort) + first monster seat dead → stage id `3` (entry 969, the form-transition module), slot-B page-in issued same-frame. Port `overlay_loader::boss_transition_stage_id` via `World::battle_stage_id`. `0x801fd514` was a phantom printing off a base-tag-less dump; the trap stays on [re-do-not-re-walk.md](re-do-not-re-walk.md#a-second-stage-id-writer-at-0x801fd514-in-the-0897-band). [details ↓](../subsystems/battle.md#stage-overlay-dispatch-the-0x47-loader-band) |
| "No party seated" vs "party dead" in the wipe scan | resolved (port-only state; guarded) | `disassembly` + `capture` | Retail's `0x5A` wipe scan (0898 `0x801E6510..0x801E664C`) walks the actor table for exactly the seated count at `*(0x8007BD24)` - the `beq` at `0x801E6524` would fall straight into the wipe compare on a zero count; retail is saved by the count (never zero after battle load), not by a guard. "No party seated" is a **port-only state**, once read as a party wipe on the first end-of-action. `BattleActionHost::slot_seated` gates `PartyWipe` on `party_seated > 0` (`MonsterWipe` still resolves), and the pad ladders seed the retail New Game roster. Disc-free pin: `unseeded_battle_wipe_guard.rs`. [details ↓](../subsystems/battle.md#an-unseeded-party-reads-as-a-dead-one) |
| Battle command-flow byte `ctx[+0x06]` | resolved | `disassembly` | The *other* battle SM - `FUN_801D0748`, the menu half, distinct from the action SM's `ctx[+0x07]` and overlapping its value space. Its selection band is regular decimal tens `30..120` (turn prompt / category menu / escape / item / magic / arts entry / target / target confirm / commit / attack-mode), which is what identifies it as the tutorial hook table's key: the nine live hook slots are that band minus the magic window. Engine mirror `engine-core::battle_flow`. [details ↓](../subsystems/battle.md#the-command-flow-byte-ctx0x06---what-the-hook-table-indexes) |
| Action-SM state `0xFF` treated as battle end by the port | resolved (path was live-reachable; port fixed) | `disassembly` | [details ↓](#action-sm-state-0xff-treated-as-battle-end-by-the-port) |
| Spine flag `0x142` (Caruban beat / dolk-dolk2 switch) writer | resolved (disc writers + engine port + oracle) | `capture` | [details ↓](#spine-flag-0x142-caruban-beat--dolk-dolk2-switch-writer) |
| Spine flag `0x482` (Drake mist-wall) writer | resolved (writer-less; "direct code path" presumption falsified) | `capture` | [details ↓](#spine-flag-0x482-drake-mist-wall-writer) |
| CDNAME scene-window frame (`raw = extraction + 2`) in `Scene::load` | resolved (engine converts; misattributions corrected) | `capture` | Engine scene windows used raw-TOC defines as extraction indices - two entries late, dropping each block's first two retail entries and bleeding in the next block's. Corrections that fell out: the `.MAP` is the retail block's FIRST entry (not "two below"); "suimon == dolk2 MAN" and "rikuroa MAN = [18,70,20]" were next-block sidecars under the wrong label; "urudre1 tests 0x15E" and "0x63A has no writer" are falsified; "0x1BE = rikuroa Zeto gate" was geremi's arrival one-shot. Head blocks (defines 0/1, inside the TOC header rows) keep legacy windows. See [cdname.md](../formats/cdname.md#numbering-space). |
| Motion-VM (`FUN_80038158`) bytecode carrier + flag census | resolved (carrier pinned; spine flags negative) | `capture` | The second motion VM's bytecode source is **MAN tail-section 1** (installer `FUN_8003A9D4`; parser `legaia_asset::man_motion`; layout + op table in [`motion-vm.md`](../subsystems/motion-vm.md#the-second-motion-vm---fun_80038158)). Disc-wide op-7/op-8 census (`--motion-flag-census`): overworld walking-band choreography + one `town0b` clear; `0x142`/`0x482`/`0x1BE` and `549` appear in NO stream - the "549 set by op-7 bytecode" carrier claim is **falsified**. Anchor test `motion_flag_census_disc.rs`. |
| Debug-menu "STR trigger teleports + sets flags" mechanism | resolved (no per-FMV event table; dev-menu tools explain it) | `disassembly` | [details ↓](#the-_dat_8007ba78-census-is-closed) |
| Spawned-record player-channel (`0xF8`) ExecMove/HaltAcquire handshake | resolved (retail machine traced; the port's model diverges) | `disassembly` | [details ↓](#the-0xf8-halt-acquire-handshake) |
| Equipment stat-bonus table - slot model | resolved (slot model + passives) | `disassembly` | The stat-bonus table (`DAT_80074F68`, 8-byte stride) is decoded from `FUN_801CF650`/`FUN_801CF5D0` (`legaia_asset::equip_stats`): `+0`=INT, `+1`=ATK, `+2`=UDF, `+3`=LDF, `+4`=SPD (the earlier AGL/evasion reading is falsified). Five `lbu`/add pairs at `0x801CF6C0..0x801CF72C`; note the asymmetry that rules out a linearised-C reading - `equip+0` lands on the *last* accumulator, out of sequence with `+1..+4`. AGL takes no equipment add at all. The four `+7` categories are Legaia's four weapon/armour slots (body/head/footwear exact by name; none of the 77 accessories appear in this table). Wired: `DiscEquipInfo` gates `EquipSession`'s per-character list. |
| Flag `0x63A` - the vell/vozz `P2[7]` gate with NO script writer | resolved (script writers exist; the "no writer" premise was the CDNAME +2 skew) | `capture` | [details ↓](#flag-0x63a---the-vellvozz-p27-gate-with-no-script-writer) |
| cave01 `P2[16]` (the `0x15D` entry-key setter) - what spawns it | resolved (slot-counted spawn chain) | `capture` | [details ↓](#cave01-p216-spawner---the-slot-counted-interact-chain) |
| Drake Castle deep interiors (`jouinc`/`jouind`) depth decode | resolved (door-choreography families, not story gates) | `capture` | [details ↓](#drake-castle-deep-interiors-jouincjouind-depth-decode) |
| `scene_destinations` P1-table scan misses P2-only door names | resolved (P2 pass folded in) | `capture` | The P2-only class is the town/dungeon **exit door** (a P2 door-choreography record): `town01`→`map01` (Rim Elm's overworld exit; the P1 pass alone sees *zero* town01 destinations), `retockin`→`retona`, `geremi`→`map02`/`tower` - 13 scenes / 14 destinations disc-wide. The suspected `jouinb`→`jouina` exemplar is falsified: it is P1-visible (the over-walk resyncs across that record). Merged kernel `legaia_asset::man_edit::scene_destinations` (P1 pass as prefix + clean-gated P2 pass, `(name, index)` dedupe); the engine delegates to it; disc pins `scene_destinations_p2_disc.rs`. |
| `0x4C 0x51` byte `+3` = `[bit7 special-model \| facing nibble]` vs the glide-speed interim `depth & 7` reading | resolved (facing wins; the two readings were two different ops) | `disassembly` | [details ↓](#0x4c-0x51-byte-3-reconcile---facing-wins-no-motion-bytecode-synthesis) |
| How an NPC's facing changes **after** spawn - snap vs ramp, and which writer wins | resolved (two laws; order-of-execution priority) | `disassembly` | [details ↓](#npc-dynamic-facing---two-laws-and-an-execution-order) |
| dolk2/rikuroa MAN source (the "v12-embedded MAN" was an over-read) | resolved (streaming carrier) | `capture` | Their own `base+3` bundles are the MAN-less count=4 form `[1,2,6,0x14]`; the "embedded MAN at 0x1000" inside their SceneV12Table entries is an over-read onto the next scene's bundle (suimon's / geremi's; [scene-v12-table.md](../formats/scene-v12-table.md) § over-read). Retail sources their partition scripts from the block's standalone `data_field_streaming` entry's type-3 chunk (`dolk2` ext 70 `[29,73,17]`, `rikuroa` ext 157 `[13,29,64]`; live script-heap byte-match at the Caruban beat). Engine: `field_man_payload` streaming fallback (`streaming_man_payloads`) + retail-frame `Scene::load` windows; pins `v12_bundle_man_disc.rs`. |
| kor-family op-0x49 flag window `[0x138..0x13F]` - what the 8 flags gate | resolved (Uru Mais warp-pad destination memory) | `disassembly` | [details ↓](#kor-family-op-0x49-flag-window-0x1380x13f---uru-mais-warp-pad-picker) |
| Attack-band damage pacing - who calls `FUN_801EC3E4`, what paces the hits, when the total lands | resolved (the anim tick calls it every frame; the clip's `+0x10..+0x13` beats fire the hits; one HP write per combo) | `disassembly` | [details ↓](#attack-band-damage-pacing---the-hit-event-model) |
| The battle-**intro** enemy-name banner - which placement record raises it | resolved (**none does** - the composer places it itself) | `disassembly` + `capture` | [details ↓](#the-battle-intro-enemy-name-banner) |
| What does the battle overlay's private PRNG `FUN_801D0290` feed? | resolved (nothing in the battle model - it shapes one effect ribbon) | `disassembly` | All five draws sit inside `FUN_801CFA48`, the lightning effect-ribbon emitter (the `0x2000` arm of `FUN_8001ADA4`'s multi-target case; PROT 0973's dev harness labels it `THERNDER1`): inner / outer half-widths, a 1-in-8 heading kink, the per-segment turn and the advance length. The state word `0x801F6950` is re-seeded on every call at `0x801CFC18` from `param[+0x1C] >> 2`, so it is a shape hash, not a stream. `0x801CFB94` is a branch label inside that routine, not a function (the name came from a slot-A VA collision with a real entry in PROT 0970). |
| Is `0x801F6950` read across overlays (the `overlay_0897_*` dumps)? | resolved (no - three machine references on the disc, all in PROT 0898) | `disassembly` | The field overlay's content ends at `0x801F3818`, below the word. Three of the four `overlay_0897_*` hits are the address appearing as an *instruction* address in a mis-based print; the fourth is the seed store re-keyed by `0x167E8` into `FUN_801CFA48`. |
| What are the halfwords of a `0x80076C10` screen-element record? | resolved (a two-seat pair over shared width / height / string) | `disassembly` | Seat A `(+0x00 id, +0x02 x, +0x04 y, +0x0E kind)` and seat B `(+0x01, +0x0A, +0x0C, +0x0F)`, shared `+0x06` width, `+0x08` height, `+0x14` **content string** (measured by `FUN_80035F04`, the rendered-width kernel - not an animation descriptor). `FUN_801D8DE8` has one spawn arm per seat (`0x801D92E8` / `0x801D935C`, by `mode & 1`) and glides toward the *other* seat. Movers: `FUN_801D5718` land, `FUN_801D5778` launch (seat B pushed `-0x140`), `FUN_801D57E8` clone. `+0x10` is `13` on the framed rows and unread; `+0x12` is zero in all 103 records. Which seat is parked is per record. |
| How is a slot-B cast / summon module entered? | resolved (two link-time tables in PROT 0898; row = extraction - 903) | `disassembly` | The 32-slot cast-tick table `0x801CF4EC` behind `FUN_801F1ED4` (`jal` per arm, key `actor[+0x1DF] - 0x81`; called from the battle SM at `0x801E4B1C` / `0x801E4C7C` / `0x801E4CA8`), and the 64-word move-VM entry table `0x801F6734` copied into `gp[+0x714]` (`0x801E44C8` / `0x801E4630`) and called by move-VM op `0x20` (`jalr` at SCUS `0x80023764`). Both give `PROT extraction = 903 + row`, verified 13/13 and 12/12 against the extracted images. Slot-B images carry no internal `jal`, which is why no dump was ever attributable to them. See [`cast-module.md`](../subsystems/cast-module.md#the-entry-tables-and-where-the-addresses-live). |
| How is a **capture-class** cast module's tick entered? | resolved (a third link-time table in PROT 0898, keyed on the spell record's sub-id) | `disassembly` | `FUN_801F2160` (0898 file `+0x23948`, sole caller `0x801E50C8` in the battle SM) reads `caster[+0x1DF]`, indexes the static spell table `0x800754C8 + id * 12`, takes the record's `+1` capture-class sub-id (`sltiu 0x20`) and jumps through 32 arms at `0x801CF56C`; arm `i` is a hard-coded `jal` into PROT `935 + i`. Most arms land on a per-spell trampoline (88..204 bytes) that re-reads `caster[+0x1DF]` and calls one body per spell id. The earlier sweep looked only for a table keyed `id - 0x81`. |
| Is the whole slot-B band mapped? | resolved (all 64 entries carry a `static-overlays.toml` row; 61 are dumped, and 0915 / 0926 / 0935 wait only on their extraction images) | `disassembly` | Function extents recovered by frame matching (prologue → the first `jr ra` whose delay slot restores the same frame); every `0x801F6734` row and every arm of both tick tables lands on a recovered head in its image and no other. The band holds 196 framed functions (1..8 per image), 25 images with internal `jal`s. PROT 0915 / 0926 / 0935 were held back only by the extraction test's pointer-resolution gate, which counted references that legitimately leave the image (0898's data, the post-image `.bss`); the gate now counts only references landing inside another mapped image, and all three carry rows. |
| The slot machine's cabinet emitter | resolved (a mesh, not a packet: PROT 1200 descriptor 1) | `disassembly` | PROT 1200 carries three descriptors - `TIM_LIST`, a 2160-byte untextured Legaia **TMD** (65 verts, 76 prims, baked greys / dark reds / navy) and a `MOVE` table; the earlier read stopped at descriptor 0. The slot init `FUN_801CEC94` loads the entry (`FUN_8003EB98(0x4B2, ..)` at `0x801CEE2C`), `FUN_80020224(0)` at `0x801CEE44` dispatches all three, and `FUN_80020DE0(0x801D3618, ..)` at `0x801CEEA8` spawns the actor bound to model slot 0. Its extents enclose the paylines, dot matrix, reels and pedestals. See [`minigame-slot-machine.md`](../subsystems/minigame-slot-machine.md). |
| The slot machine's in-game BGM | resolved (host-scene authored, not the overlay's) | `disassembly` | `koin1` (PROT 543, Sol Tower's minigame floor, whose MAN carries the `0x3E` warps `op0 = 103 / 104 / 105`) picks the track in its entry script by spawn bbox: `2018` (M16, the casino floor), `2024` (the bar), `2045` otherwise; `balden` (the Vidna cabinet) plays `2058` / `2010`. |
| The Muscle Dome's Auto arm - which routine picks the commands? | resolved (none - it replays a record-stored string) | `disassembly` | `FUN_801DA34C` (called at `0x801D15C8` on the phase-`0x28` Attack confirm) reloads a 16-byte command string from the character record - `+0x1A7` when `actor+0x156 < actor+0x154`, else `+0x1B7` - and `FUN_801DA59C` (`0x801D22BC`) writes it back. Whether the pick screen shows at all is the option word `*(0x800846C4)`. |
| The dome's three UI cue ids (`0x21` / `0x22` / `0x23`) | resolved (accept / highlight moved / refused-or-back) | `disassembly` | 37 `jal FUN_8004FCC8` sites in `FUN_801D0748` (15 / 7 / 15), not 34. Pinned by content: every cancel-mask (`*(0x800846D4)`) press is `0x23`, every confirm-mask (`*(0x800846D0)`) press `0x21`, and the pre-pass `0x22` sites fire only when the pressed bit differs from `ctx+0x880`. Arm boundaries come from the compare chain `0x801D0C84..0x801D0DCC`. |
| The arena backdrop's object-1 dust decal | resolved (trimmed by the SCUS battle loader unless `_DAT_8007B64B` is set) | `disassembly` | `FUN_800513F0` spawns two backdrop actors and, when `_DAT_8007B64B` is zero (`0x80051ABC`), decrements both part counts and shifts each list down from index 1 (`0x80051AD4..0x80051BAC`). One writer on the disc, `FUN_801D9E1C` at `0x801DA0AC`. That the arena leaves it clear is `inference` (agrees with the mist-free capture). |
| `FUN_801F2410` / `FUN_801F2E10` - dome routines or shared? | resolved (neither is the dome's) | `disassembly` | `801F2410` is the cast **colour wash** - screen-wide `POLY_G4` onto `*(0x1F8003A0)` tinted `ctx[+0x27E..+0x280] * ctx[+0x27A] / 255`, called only from the cast dispatchers' epilogues (`0x801F2144`, `0x801F23F4`). `801F2E10` has no reference in SCUS or any overlay; its 11 callers are all in slot-B module PROT 0909. |
| What is per-character record `+0x131`, the second byte the new-game seed inits to 1? | resolved (nothing - it is write-only) | `disassembly` | `sb $v0, 0x6f9($s0)` at `0x800561C8` is its only writer on the disc; an opcode-decoding sweep for `lb` / `lbu` / `sb` at displacement `0x131` and at the four slots' block-relative `0x6f9` / `0xb0d` / `0xf21` / `0x1335` over SCUS + all 80 based overlay images finds no reader, and no wider access spans `+0x130..+0x131`. The magic-rank counter is a different byte, capture-pinned at `+0x9C`. |
| What does item-effect flag bit `0x40` mean? | resolved (the descriptor's target side - set = the enemy party) | `disassembly` | Read once, at `0x801D18E0` in `FUN_801D0748` (PROT 0898), forked with `andi 0x20` at `0x801D18E8` into phases `0x5B` / `0x5D` / `0x64` / `0x66` (one enemy / all enemies / one ally / all allies); the spell table's `+2` byte runs the same ladder at `0x801D1C50` / `0x801D1C58`. The five carriers are the Point Card and the two summon-flute pairs. `FUN_801D0F1C` never reads the bit (see the falsified row). |
| What is the arts-name table's `+4` halfword (`DAT_80075EC4`)? | resolved (an authored tier constant with no reader) | `disassembly` | Eight sites materialise the table base; a delta-tracked walk of every load off them reaches only `+0` / `+1` / `+2` / `+8` / `+0xC` / `+0x10`, and a five-form sweep of `0x80075EC8` finds no reference. The values are a per-art tier ladder keyed to list position (`60000` Miracle, `50000..30000` the elemental arts, `20000`, `15000..5000` by input count, `1` terminator) - neither AP nor input count. |
| What is the byte at `actor[+0x22C] + 0x80` the SFX-cue router folds into the category? | resolved (the actor's display object's sound-bank category, values `{0, 7, 8}` - not an element byte) | `disassembly` | [details ↓](#the-display-objects-sound-bank-category) |
| What moves the evolved-Cort battle off flow `ctx[+0x06] = 0x0C`? | resolved (the boss stage module hands the flow back after its own intro countdown; no input involved) | `capture` + `disassembly` | [details ↓](#the-evolved-cort-flow-park) |
| Is `_DAT_8007B64B` clear during a Muscle Dome contest? | resolved (yes - the dust decal is trimmed) | `capture` | `FUN_800513F0` entered once (`ra = 0x80046F7C`) with the byte `0x00`; at `0x80051ACC` the trim arm ran, and a write watch logged zero writes over 1800 vsyncs (`koin1`, modes `0x03 → 0x18 → 0x19 → 0x14 → 0x15`). The field handoff `FUN_801D9E1C` never runs on the arena path. Probe `autorun_w4d_dome_decal_flag.lua`. |
| Do the renderer's light-capable prim kinds 8..11 ever execute? | resolved-negative (no live light path) | `capture` | Zero hits on `0x8004409C` / `0x8004423C` / `0x80044434` / `0x800445B0` over 1111 `map03` overworld frames, 347 + 470 field frames (`map02`, `ropeway`) and ~1160 battle frames. The control had to change with the scene: a kingdom overworld never enters the SCUS prim-dispatch family at all (13 handlers armed, zero hits) - it renders through PROT 0901's eight replacements, all of which fired in the same run. Probe `autorun_w4d_light_kind_hits.lua`; [`renderer.md`](../subsystems/renderer.md). |
| What are PROT 1221 / 1222 (`other5` / `other6`)? | resolved (`int.tim` / `int2.tim` - the Muscle Dome's two ringside panel stills) | `disassembly` | [details ↓](#dome-ringside-panel-stills-prot-1221-and-1222) |
| What do the higher readef groups' aux slots carry? | resolved (no new kind - textures, the four ME archives, or a never-staged actor record) | `disassembly` | `FUN_801F12D0` is an 8-stage machine (jump table `0x801CF4CC`): stage 2 uploads slot `base` to CLUT row 488 / page `x = 512`, stage 4 uploads `base+1` to row 490 / `x = 640`, gated at `0x801F1500` / `0x801F150C` on `base >= 0x42 \|\| 0x0C <= base <= 0x36`. That gate partitions the file exactly: textures inside it, the four ME archives below it, and in the three excluded groups (`base` `0x39` / `0x3C` / `0x3F`, actions `0x14..0x16`) an actor record byte-identical to its `base+2` twin, which the applier never stages. See [`summon-readef.md`](../formats/summon-readef.md). |
| Is `cutscene_str` (PROT 0970)'s 123 KB code gap un-dumped code? | resolved (a zero-filled reservation) | `disassembly` | `0x801D1878..0x801F1A00` is 32,793 zero words of 32,866 - a `.bss`-shaped hole. The image's real un-dumped code is about 1.3 KB. Recorded in [`disc-coverage.md`](../tooling/disc-coverage.md). |
| Can a scene TMD pack member be grown in place? | resolved (nothing outside the pack holds a byte offset into it - rebuild is the only cost) | `disassembly` | [details ↓](#growing-a-scene-tmd-pack-member) |
| Is any of an enemy signature cast's choreography data-driven? | resolved (partially - the spawn layer is data in the art path's own record format; the lift and camera are module code) | `disassembly` | [details ↓](#what-of-a-signature-cast-is-data) |
| How many damage-clamp shapes does the slot-B cast band use? | resolved (**three**, and one module picks per hit) | `disassembly` | Over the 83 wrapper `jal` words in the 64 images, 79 of them inside a frame-matched function of the image carrying them: **70** take shape A (cap live HP, `sltu`, kills), **seven** shape C (cap `HP - 1`, `sltu` - neither kills nor heals), **two** shape B (cap `HP - 1`, signed `slt` - the two AoE stagers). Every shape-C site is a tick body or a body a tick calls. PROT 0910's applier picks its cap from its own slash counter at `0x801F8DAC` - `HP - 1` on slashes 1..3, live HP on slash 4 - so kill capability there is per **hit**, not per module. Image by image in [`cast-module.md`](../subsystems/cast-module.md#the-three-clamp-shapes). |
| Where does a capture-class cast get its damage constant? | resolved (baked into the module image, not read from a table) | `disassembly` | Each capture-class module carries its own power constant as an immediate in its own tick body, so there is no per-spell power row to patch and no table to randomise - editing the number means editing that image. Per-module values in [`cast-module.md`](../subsystems/cast-module.md#the-baked-power-constants). |
| Which slot-B images touch more than one seat? | resolved (two, and both are **stagers**) | `disassembly` | PROT 0927 sweeps the enemy row addressed from `ctx[+1]`, and PROT 0966 sweeps the whole actor table from `ctx[+0]`; neither subtracts HP. They stage clips and seats and hand off to a tick that applies the damage - which is why 0927 reads as "can never kill" if only its own image is examined ([falsified](re-do-not-re-walk.md#battle--arts--level-up)). See [`cast-module.md`](../subsystems/cast-module.md#the-two-aoe-sweeps). |
| How do the `0x801CF56C` capture-class arms reach a spell body? | resolved (through a per-image trampoline in six of them) | `disassembly` | Arm `i` is a hard-coded `jal` into PROT `935 + i`, and in six images the landing site is a **trampoline** of 88..204 bytes that re-reads `caster[+0x1DF]` and dispatches one body per spell id - PROT 0955 is the extreme case, a 20-word jump table serving six spells from one cell. Two power constants that looked missing are register reuse: PROT 0918 carries `0x12` at `0x801F8798` and PROT 0949 carries `0xC0` at `0x801F72F8`. See [`cast-module.md`](../subsystems/cast-module.md#the-trampolines-are-their-own-port-and-one-cell-holds-six-spells). |
| What calls `FUN_801E91E8`, and what reads its result? | resolved (one `jal`, and half the result is unconsumed) | `disassembly` | Its single call site is `0x801EE2C0` inside `FUN_801EC3E4` (PROT 0898), and the routine contains zero data words. The unconsumed half of what it returns is `ctx[+0x269]` - staged and never read on the capture leg. Row in [`functions/battle.md`](functions/battle.md). |
| Does the battle tutorial's wait state draw anything? | resolved-negative (kind `0xD` draws nothing) | `disassembly` | The tutorial's wait record is widget kind `0xD` at `0x801F75F0`, and `FUN_80031D00` dispatches on the kind alone - kind `0xD` has no draw arm. The pause between tutorial beats is a timer with no picture. |
| What are placement-record `+0x0E`/`+0x0F` and `+0x10`? | resolved (`+0x10` is the widget **kind**, `+0x0E`/`+0x0F` the frame **style**) | `disassembly` | `+0x10` reaches `FUN_8003541C` as `a1` from `0x801D8E8C` and lands on node `+0x1C`, the kind the widget dispatcher switches on; `+0x0E`/`+0x0F` land on node `+0x1D` and thence `gp+0x14C`, which `FUN_8002C69C` reads to pick a frame style from `{0x31, 0x33, 0x34, 0x35}`. No image reads `+0x11..+0x13`. Detail in [`battle-action.md`](../subsystems/battle-action.md#0x0e0x0f-is-the-frame-style-and-0x10-is-the-kind). |
| What is the battle attack angle `ctx[+0x6D2]`? | resolved (a bounded framing term, zeroed after the first hit) | `disassembly` | It holds `[0, 0x800]`: term `0` frames the strike face-on and the from-behind case contributes `atk / 32`. The battle SM zeroes it at `0x801EC888` once the first hit lands, so it shapes the approach and not the exchange. |
| Which spells are `0x27` and `0x81`? | resolved (`0x27` Tail Fire, `0x81` Gimard / Burning Attack) | `disassembly` | Both ids were read in the wrong space at least once. `0x27` is the enemy special *Tail Fire*; `0x81` is the first player Seru-magic id, whose loader-B call `FUN_8003EC70(0x81 - 0x79)` pages extraction PROT 903. |
| Where is the Spirit AP-halving flag? | resolved (character record `+0xF8`, bit `0x800`) | `disassembly` | It is accessory passive `0x2B` (*AP Used Down*) in the persistent per-character ability bitfield, tested by the queue builder `FUN_801EED1C` at `0x801EF364` and by the status panel `FUN_801D33D8` at `0x801D4520`. Not a per-battle actor bit ([falsified](re-do-not-re-walk.md#battle--arts--level-up)). |
| What is battle `ctx[+0xD]`? | resolved (two independent bits, not an enum) | `disassembly` | Bit 0 adds `0x800` of camera yaw; bit 1 adds `0x80` of pitch **and** drops `TR.y` by `0x100`. Nothing in either arm writes a Z angle. See [`battle-action.md`](../subsystems/battle-action.md#the-three-movers). |
| Does an Attack x2 consume the Super Art starter? | resolved (no - the starter survives) | `disassembly` | The compare is `bne v0,a1` with `a1 = 1` at `0x801E3A4C`: the build loop marks an accepted art's starter `1` at `0x801EF788` and the Super tail-replace `FUN_801EF9E4` marks `4` at `0x801EFBA8`, so a doubled ordinary attack tests against the wrong constant and leaves the Super starter standing. See [`battle-action.md`](../subsystems/battle-action.md#the-retail-queue-builder-fun_801eed1c-and-super-applier-fun_801ef9e4). |
| What are battle `ctx[+0x17]`, `ctx[+0x18]` and `ctx[+0x19]`? | resolved (a once-per-action teardown latch, a HUD element id, and a per-battle Spirit latch) | `disassembly` | `ctx[+0x17]` is the action teardown's once-per-action latch and `ctx[+0x18]` the id of the HUD element that action raised, both at `0x801E2F50`. `ctx[+0x19]` is read at exactly one site, `0x801E61CC`, which unloads HUD elements `0x0F` / `0x52` - so it latches "Spirit was used this battle" and nothing else. |
| What is battle `ctx[+0x270]`? | resolved (`FUN_801D5854`'s second camera ramp) | `disassembly` | Written over `0x801D5960..0x801D59B8` and read by one consumer, case 8's death re-frame. The clamp is `sb`-truncated, so values above a byte wrap rather than saturate - which is the behaviour to port, not the intent. |
| What does the dome's contest restore (`FUN_801D0ED8`) do? | resolved (one-shot per contest, and it leaves the accessories alone) | `disassembly` | The caller `FUN_801CEA6C` tests `_DAT_8007BAC0` at `0x801CEB58` and jumps past the `jal 0x801D0ED8` at `0x801CEBF0` when it is already non-zero, so a contest refills once when it opens and a leg boundary never does. It refills party slot 0's HP / MP / SP to their maxima (`+0x104` / `+0x108` / `+0x10C`), and its stripped arm zeroes only `+0x196` / `+0x197` / `+0x198` / `+0x19A` - the Seru-lock byte `+0x199` and the accessories survive. Detail in [`functions/minigames-debug.md`](functions/minigames-debug.md#801d0ed8). |
| Do the dome's hub screens run on frame counts? | resolved (no - they are fade / hold / fade envelopes) | `disassembly` | `DAT_801D1A80` and its siblings are `0..0x80` **brightness** levels, not durations, and the two holds end on `pad & 0xF4` rather than on a timer. A frame-count port ends the screens at the wrong instants and cannot be ended early by a button. See [`minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md#the-hub-screens-are-envelopes-not-frame-counts). |
| Which dome chip seats does the AP-cost re-centring apply to? | resolved (both `x` seats, `+0x2` and `+0xA`) | `disassembly` | The term is `(cost - 30) * K[slot] / 2` with `K = DAT_8007B650 = [2, 1, 1, 0]` and truncate-toward-zero halving, applied to each of the record's two seat-x halfwords; anchors are `176 / 216 / 216 / 256` from `0x80076BBC`. `K` makes the arm chip grow away from the D-pad glyph between the pair. |

### Attack-band damage pacing - the hit-event model

*Status:* resolved - disassembly (`FUN_801EC3E4` head `0x801EC41C..0x801EC494`, apply arm
`0x801EE984..0x801EEA78`, epilogue `0x801EECDC..0x801EECE8`; `FUN_80047430` call sites
`0x800478A0` / `0x80047BF0` and the bit-1 cut `0x80047900..0x80047948`; `FUN_8004AD80` `0x8004B064`)
+ capture (two PCSX-Redux hit-event timelines, a plain Somersault and a Tri-Somersault Super)

The strike loop of the action SM (`FUN_801E295C` state `0x1E`) stages one byte into `+0x1DA`
and sets `+0x1DC` bit 1; it never calls a damage kernel. The kernel runs from the **anim
tick**, every frame a battle clip plays, with the cursor frame in `a2`, and its own head decides
whether that frame is a hit: `ctx[7] != 0x5A`, the committed entry's byte 0 in `0x0C..=0x1F`,
the per-clip hit index `+0x1F4 < 4`, `entry[+0x10 + idx] != 0`, and `frame + 1 >=
entry[+0x10 + idx]`. An admitted hit resolves with `entry[idx]` as its power byte, adds the
damage to the target's combo word `+0x0` and its HP-bar word `+0x10` (`0x801EDB40` /
`0x801EDB58`) and bumps `+0x1F4`; every commit zeroes the index. Live HP moves **once**: the hit
that lands after the strike loop has parked the cursor at `0xFF` (`ctx[+0x15]`, `0x801EE9A4`)
and that is its clip's last listed beat (`entry[+0x11 + idx] == 0 || idx == 3`) subtracts the
whole accumulator from `+0x14C` and zeroes it (`0x801EEA10..0x801EEA74`). That gate is the
kernel's `s2 = 0` mode; the same register selects two other arms (`0x801EDEE4..0x801EE130`, and
its monster-attacker copy at `0x801EE790..0x801EE980`): a look-ahead over every remaining hit of
the action that finds none able to connect with the target's `+0x1E` size class applies the total
at once (`s2 != 0`), and the War God Icon's *Attack x2* (ability bit `0x0D`) with `ctx[+0x16] < 2`
withholds it so the pair lands as one (`s2 = 0xFF`). The tick's bit-1 cut
commits the next staged byte once `entry[+0x10] + 2 < frame` with `entry[+0x76] == 0`, which is
what chains one swing into the next mid-clip; the art records carry `+0x76 = 1` and play to
their natural end. The loop-window arm re-zeroes `+0x1F4` on every rewind for a party slot on
`0x11` with a latched id `>= 0x2B` (`0x80047840..0x80047878`), so a windowed Hyper / Super clip
re-fires its hits per cycle.

Both captures (N = 2, `scripts/pcsx-redux/autorun_hit_event_timeline.lua` over the
`party_basic_attack_vs_gobu_gobu` and `battle_vahn_tri_somersault_super` states) read exactly
that. A plain three-arrow Somersault: swing `0F` (`p0 = 0x18, e0 = 7`, lock `0`) commits, its hit
fires at frame 6, swing `0E` (`p0 = 0x13`) commits 8 vsyncs later at frame 10 - the bit-1 cut -
and hits at frame 6, the `0x19` starter (slot `0x10`, `e0 = 0`) plays with no events, `0x27`
installs at slot `0x11` (`e0 = 5`, lock `1`) and hits at frame 4 while the SM sits in `0x20` -
and the target's HP falls 76 → 23 in that one frame. The Tri-Somersault Super stacks seven hits
across `0F`, the two-hit `0x1F` (`p = [0x17, 0x17]`, `e = [8, 19]`), `0E` (`p0 = 0x1D`), the
`0x1A` SpecialStarter (slot `0x11`, loop window `[13, 14] x 5`, solo byte `1`) and three `0x2B`
clips, and lands 3542 → 2318 on the last one. One probe artifact worth knowing:
`*(actor + 0x22C)` alternates between two draw nodes on consecutive vsyncs, so a per-vsync
cursor sample can read the node the tick did not advance (three frames behind); the in-sync
stream satisfies the head guard on every hit. Engine seats: `legaia_engine_vm::battle_action::hit_event`
(the head), `World::tick_battle_hit_events` / `land_melee_hit` / `apply_combo_total`
(`engine-core`); write-up in
[battle-action.md](../subsystems/battle-action.md#a-tactical-art-is-an-ordinary-attack-band-action).
The port's own timeline (`legaia-engine play-window --battle 4`, `RUST_LOG=legaia_engine_core=debug`,
N = 2 runs) reads the same shape: a two-swing Auto queue commits `0C`, hits at frame 6 of its `e0 = 7`
entry and accumulates, commits `0D` at the boundary and lands the total on its hit; a typed `↑↓↑`
becomes `0F 0E 1A 27`, both swings hit at their beats, the starter parks `[13, 14] x 5` with no hit,
and the Somersault entry (`p0 = 0x18, e0 = 5`, lock `1`, the capture's bytes) hits at frame 4 and lands
the total once.

### The battle HUD's per-phase surfaces are the sub-draw script table

*Status:* resolved - `disassembly`, corroborated by the handle lists of sixteen
states.

The question was which of retail's two party readouts (roster card, full-width
pill) a given battle frame shows, and when the plaque, the AP plate, the
target plaque and the move name join them. Retail never computes it: the menu
SM `FUN_801D0748` runs `FUN_801D388C(step)` on every `ctx[+0x06]` edge
(`0x801D0EE4`, `0x801D109C`, `0x801D13F0`, `0x801D14C8`, `0x801D1658`, ...),
and `step` indexes `PTR_DAT_801F4D34` - fifty `[count][anim][panel]` +
`count` x `(placement record, mode)` records in the battle overlay's rodata,
walked at `0x801D4BA4..0x801D4CBC`. `anim = 1` hard-resets the handle list
first, so the pairs are the whole screen. `FUN_801D8DE8(record, mode)` takes
the record straight into `0x80076C10 + id * 0x18`; mode bit 0 picks the spawn
seat (`+0x02/+0x04` or `+0x0A/+0x0C`) and the glide runs to the other, bit 1
suppresses the glide (`0x801D92E0..0x801D93DC`). The action SM's openers are
the `0x0C` seed (`0x801E2F24`: bar for a party target byte `+0x1DD`, panels
for `8`) and the Item / Spirit pre-arm `0x3C` (`0x801E3DA0`), with every
close in the `0x51` band (`0x801E6170..0x801E6364`).

The rule that falls out - card at the round prompt and while a window is
browsed, pill in the ring and the target steps, nothing during a party
attack on a monster, the target's pill during a monster cast - is on
[`battle.md`](../subsystems/battle.md#the-per-phase-rule---what-the-sub-draw-script-builds).
Each `mednafen` state's `ctx[+0x1074]` walk agrees element for element; the
disc-gated `crates/engine-core/tests/battle_hud_subdraw_disc.rs` holds the
port's constants to the table's bytes. One corollary settles a widget that
had been ported twice: the item window's `0x64` "target strip" is record 7
again (step `0x12`: `07/0`, `34/1`), and its packet-pinned pens are the
ring bar's seat for seat - so the port draws the bar there and the item
window keeps only its breadcrumbs.

### The ring's element chip reads `-` or the Ra-Seru, and the gate is the equipment byte

*Status:* resolved - `disassembly`, corroborated across twenty-nine states.

`FUN_801D8DE8`'s case for record 10 (`0x801D8EC8..0x801D8F2C`) writes the
chip's string pointer as `0x801F4B9E + char_id * 10` - the run `Meta` /
`Terra` / `Ozma` at indices `1..=3`, a lone `-` at index 4 - when
`ctx[+0x25F + member]` is set, and the `-` entry when it is clear. The gate's
one writer is the party battle-actor init `FUN_80053CB8`
(`0x800541D0..0x80054270`): `lbu v0,0x761(v0)` off the `0x80084140` display
alias, which is the live character record's `+0x199`, the Ra-Seru slot of
the eight equipment bytes at `+0x196` - or `+0x760` (`+0x198`) on the arm
the member's character id `2` (Noa) takes. In every catalogued state the three
gate bytes equal `(+0x199 != 0)` for the seated members - `0` in the sparring
fight, `1` for Vahn from the Meta capture on, `0` for Noa beside Terra, all
`1` at the level-99 Rage state. Port `engine-core::battle_hud::battle_magic_chip`.

### The combo counter's glide is placement record 80's

*Status:* resolved - `capture` (the live glide record), the stepper by
`disassembly`.

The `HIT` / `TOTAL` / `DAMAGE` cluster hangs off screen-element record 80,
seat A `(328, 170)`, seat B `(168, 170)`, opened at mode 0 and closed in the
`0x51` band (`FUN_801D8DE8(0x50, 1)` at `0x801E6360`, gated on the damage
finisher's `_DAT_8007BD14`). `battle_melee_hit_spark` carries the glide record
live at `ctx[+0x11B4 + slot * 0xC]`: total `0x10`, elapsed `0x0C`, start
`(328, 168)`, target `(168, 168)`, handle at `x = 208` - which is the `+40`
every packet of the cluster shows against the settled seats of
`player_steal_skeleton_banner`. `FUN_801D9BBC` steps it linearly
(`start + (target - start) * elapsed / total`, snap on arrival), so the
cluster slides 160 px in over sixteen frames. Port
`engine-vm::battle_value_readout::combo_slide`.

### The chrome `kind` byte is an index into the widget-class table

*Status:* resolved - the correlation was a table lookup all along.

The dispatcher the thread asked for is `FUN_8002C69C`
(`ghidra/scripts/funcs/8002c69c.txt`, the `POLY_FT4` / `SPRT` emitter). It
reads the kind out of `gp+0x14C`, multiplies by `0x0C` (`sll`/`addu`/`sll` at
`0x8002C7A0..0x8002C7AC`) and adds `0x800732A4` - so a
[screen-element](memory-map.md#0x80076c10---one-table-three-names) record's
`+0x0E` byte is a **record index into the widget-class table**, and the record
is the sprite the element is framed in. Layout, classes, chains and the
palette decode are on
[`battle.md`](../subsystems/battle.md#the-widget-class-table---where-every-chrome-sprite-comes-from);
parser `legaia_asset::ui_widgets`, disc oracle
`crates/asset/tests/ui_widgets_real.rs`.

The join is exact over the whole placement table: every kind byte, high and
low, on all 103 initialised records names a real widget record. The values the
old row could only name by what sat at their seats resolve to art:

| Kind | Widget record | What it is |
|---|---|---|
| `0x01` | `(192, 0)` 16x20, sub-palette 4, class 3 | blue plate body - the command chips |
| `0x02` | `(192, 64)` 16x20, sub-palette 12, class 3 | carved-gold plate body - the name plaque |
| `0x03` / `0x04` / `0x44` | class 0, tile-set 0 | the rectangular gold 9-slice window |
| `0x07` | chain `0x07 → 0x08 → 0x09` | a roster panel: `HP` row, `MP` row, 102x48 plate |
| `0x2B` | chain `0x2B → 0x2C → 0x2D → 0x2E → 0x2F` | the active-actor bar's two label / separator pairs, then its plate |
| `0x33` / `0x34` / `0x35` | the panel chain plus the status marker | the three roster panels, one kind per party slot |

Two corrections fall out. The old row's "`0x32`/`0x33` the roster panels" is
**wrong about the record it named**: the panel placement records (6, 78, 79)
carry kind `0x07`; `0x33`/`0x34`/`0x35` are the sibling kinds that add the
level / status marker, special-cased at the head of `FUN_8002C69C` into
`FUN_8002C2E4(slot)`. And `+0x0E` is not a lone byte - it is a pair, `+0x0E`
and `+0x0F`, both indices; they are equal on all but five records, where the
pair reads `(gold, blue)`.

The chain field settles the rest of the surface. `+0x02` is a **signed** hop
(`lb v1, 0x2(s7)` at `0x8002FF00`, added into the index and re-entered at
`0x8002C780` unless zero), which is why one kind draws a whole readout.
Walking the `0x2B` chain against the bar's own pen `(16, 192)` reproduces
`(80, 194)`, `(136, 188)`, `(192, 194)`, `(240, 188)` - the four seats the
packet walk had measured independently.

### The element-badge palette selector

*Status:* resolved - the selector is the badge record's own palette byte.

Each badge is a widget record (`0x8B..=0x92`, `20 x 12` at a 32-texel pitch
from `u = 6` on row `v = 192`), and its `+0x03` palette byte is `0x40 + index`.
Bit 6 of that byte switches both draw routines onto a second CLUT decode -
`fb_y = 498 + ((b & 0x3F) >> 2)`, `fb_x = 896 + (b & 3) * 16` - which turns
the single walking byte into a 4-wide by 2-tall block of sub-palettes:

```text
badge i -> palette 0x40 + i -> CLUT ( 896 + (i % 4) * 16 , 498 + i / 4 )
```

That reproduces all four captured pairs (`u = 6` with `(896, 498)`, `38` with
`(912, 498)`, `166` with `(912, 499)`, `230` with `(944, 499)`) from the disc
alone. They looked unrelated to the badge index because the index is encoded
two-dimensionally - the low two bits pick the column, the next two the row.

The `v = 208` "winged" strip is a **separate** set of eight records
(`0x94..=0x9B`), `28 x 12` from `u = 2`, on the second CLUT block
(`0x48 + index`, rows 500 / 501). Record `0x9B` reads `v = 192`, so the eighth
wide badge samples the square-framed art on the plain row while its seven
siblings sample the winged row; the winged eighth badge exists in VRAM and no
record selects it.

### The status-element badge sheet (`0x18..=0x20`)

*Status:* resolved - nine 48x16 word tags, and the art confirms the ladder.

The nine ids `FUN_8002C2E4`'s exclusive ladder emits are widget records
`0x18..=0x20`: 48x16 cells in a two-column block on the resident system-UI
sheet (VRAM page `(896, 256)`), each on its own row-511 sub-palette, drawn by
`FUN_8002C488` at the caller's seat with no bias. Decoding the cells against
their palettes shows the art is a **word tag**, not an icon, so the ladder's
per-bit assignment is confirmed by a second, independent route:

| Sprite | Mask | Cell | Sub-palette | Reads |
|---|---|---|---|---|
| `0x18` | `0x0001` | `(0, 48)` | 9 | `Venom` |
| `0x19` | `0x0002` | `(48, 48)` | 10 | `Toxic` |
| `0x1A` | `0x0004` | `(48, 80)` | 16 | `Stone` |
| `0x1B` | `0x0078` | `(48, 112)` | 14 | `Rot` |
| `0x1C` | `0x0380` | `(0, 96)` | 17 | `Rage` |
| `0x1D` | `0x0400` | `(0, 64)` | 11 | `Numb` |
| `0x1E` | `0x0800` | `(0, 80)` | 15 | `Sleep` |
| `0x1F` | `0x1000` | `(48, 64)` | 13 | `Curse` |
| `0x20` | HP `== 0` | `(48, 96)` | 18 | `Faint` |

The accessory-guard derivation
([`accessory-passive-table.md`](../formats/accessory-passive-table.md#status-guard-clear-masks))
and the pixels agree everywhere, including the two the mask shapes make least
obvious - the `0x0078` group is `Rot` and the `0x0380` group is `Rage`. The
zero-HP arm reading `Faint` is what shows the KO test and the bit ladder are
one selector over one sheet.

Two corollaries. The block's tenth cell (`(0, 112)`) is other art, so there is
no tenth badge. And row 511's sub-palette strip is wider than the sixteen the
chrome plates use - these badges alone reach index 18, i.e. VRAM x 288.

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

### Battle-intro transition length - `DAT_801D2458`

*Status:* resolved - `disassembly`. 132 display frames, 252 for the swirl.

The intro overlay's own init seeds the duration two instructions before the
style switch it feeds: `addiu v0,zero,0x84` at `0x801CED14` and
`sw v0,0x2458(v1)` at `0x801CED2C`, then `sltiu v0,a0,0x5` on the selector
`DAT_801D2460`. The store is unconditional, so **every** style gets `0x84`.
Exactly one arm overrides it - jump-table slot `4` (the swirl, table at
`0x801CE840`, body `0x801CEFEC`) re-stores `0xFC` at `0x801CEFF4` /
`0x801CEFFC`. Read off PROT 0979's instruction words at load base
`0x801CE818`; the decompiler's rendering of `FUN_801ce8cc` reaches past that
dump's window, so the C alone would not have settled it.

Why it is worth a thread rather than a constant: it is the denominator of the
whole transition. Each fade ramp is a *lead* before it, the entity's ready bits
are raised at `- 0x1E` / `- 6`, and the tile shatter's records hold until
`delay < elapsed * 0x3C` with `delay = rand() % 5000` - so the grid needs ~84
frames merely to finish starting, and a shorter window leaves part of it at its
seeded pose for the transition's whole length. Port:
`engine-vm::battle_intro_styles::intro_duration_frames`; spec in
[`cutscene.md`](../subsystems/cutscene.md#how-long-a-transition-runs-dat_801d2458).

### Battle ground grid depth cue - the far colour

*Status:* resolved - the far colour is the backdrop's staged far colour,
read off the live GTE at the grid draw; the port's grid now fogs.

The emitter `func_0x801d02c0` runs `DPCS` per projected lattice vertex with
`IR0 = SZ >> 2` loaded bare (`srl` + `mtc2`, so no saturation - past
`SZ = 0x4000` the blend extrapolates until the DPCS output clamp bounds it),
and contains **zero `ctc2`**: the far colour it consumes is whatever the
control file holds on entry. A save-state read cannot attribute that value
to the grid pass, which is why the thread sat open on `(0, 0, 0)`-vs-
`(4096, 4096, 4096)` snapshot noise.

The probe `scripts/pcsx-redux/autorun_grid_far_colour.lua` attributes it by
construction - exec breakpoints on the emitter entry and its first `DPCS`
site (`0x801d061c`) dump control regs 21-23 at the draw:

- Every battle hit shows `FC` = the **backdrop far-colour staging word at
  `0x8007BB48`, times 16** into the 28.4 control registers. The grid shares
  the backdrop's per-battle far colour; there is no separate grid base.
- Settled values: `(0x40, 0x40, 0x40)` on ordinary stages (two town01
  battles, stage ids `0x15` / `0x0C`) and `(0xFE, 0xFE, 0xFE)` on an
  overworld battle (stage id `0x55`, on the 13-id `DAT_80078C1C` outdoor
  table at SCUS file `0x6941C`). Both are exactly the neutral base
  `0x808080` through `FUN_80050120`'s two derivation arms - `>> 1` indoor,
  `(c - 0x010101) * 2` outdoor - so the missing "base" is the neutral grey.
- The battle-intro fade ramps the staged word up from near-black at
  `+0x020202` per frame for ~28 frames before it settles: an early Queen
  Bee sample reading `(6, 6, 6)` was frame 1 of that ramp, not a per-stage
  ambience.
- `DQA = -64` / `DQB = 320 << 16` re-confirmed live at every hit, and a
  field state never enters the emitter (the aliased field-overlay code at
  the same VA idles at the white `(4096)^3` field FC - the source of the
  old snapshot confusion).

Port: `legaia_engine_vm::battle_ground_grid` carries the laws (`grid_ir0`,
`grid_far_colour`, `OutdoorCueTable`, the ramp constants) with the SCUS
table pinned by `crates/engine-vm/tests/battle_grid_cue_scus_real.rs`;
`engine-render` gained the per-draw `DrawCue` staging (retail sets the DPCS
inputs per drawn object), and the play-window battle grid draws under the
`SZ >> 2` ramp toward the per-stage far colour. The browser play page draws
the same grid under the same table: `play_battle_ground_cue_json` hands the
page the engine-resolved far colour and the page renderer attaches it as a
per-draw cue, so both hosts fog from one parse of one table.

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

### The in-fight action framing and the yaw-counter ladder

**Question.** `FUN_801D5854` case 6 has two arms; which one films an ordinary
action, and what is the yaw base `ctx[+0x6DA]` it subtracts the facing from?

**Answer.** The `0x801D64C4` arm, for party and monster alike, for the whole
of a running fight. The fork byte `DAT_8007BD71` (`0x801D5CEC..0x801D5CF4`)
is the battle-end signal and reads `0xFF` until a wipe or an escape; see the
falsified reading in
[re-do-not-re-walk.md](re-do-not-re-walk.md#the-case-6-party-arm-is-the-battle-over-framing).
The arm's pose is `pitch 0`, `yaw = (ctx[+0x6DA] − actor[+0x46]) & 0xFFF`,
`TR = (0, 0x500, ctx[+0x6D0])` (the depth prescaled by `FUN_801D829C`), focus
the negated `actor[+0x34/+0x38]` pair with the height left at zero, then the
`ctx[+0xD]` style tweaks (`1`/`3` add a half turn; `2`/`3` set `TR.y = 0x400`
and add `0x80` of pitch) and the character-`4` override.

**Evidence.** Three PCSX-Redux `.sstate` captures parked in `ctx[7] == 0x19`
with Gaza (seat 3) acting read the rotation / translation / focus trios
directly: `TR (0, 1280, 5324)` = `prescale(0xD00)` with `ctx[+0x6D0] = 0xD00`
in all three; focus `(−433, 0, −291)` / `(785, 0, −39)` / `(0, 0, −1490)`
against Gaza's `+0x34/+0x38` of `(433, 291)` / `(−785, 39)` / `(0, 1490)`;
the `ctx[+0xD] == 2` capture at pitch `0x80` over `TR.y = 0x400`; and in each
the live yaw eight units behind `(ctx[+0x6DA] − actor[+0x46]) & 0xFFF`
(`1657` vs `1665`, `1412` vs `1420`, `574` vs `582`) - the per-pass re-arm
chasing a counter that moves. The counter's seeds are stores in the action
SM and one SCUS routine, each read off the instruction: `sh zero,0x4(s7)` in
the round-begin arm (`0x801E2B40`), `li 0x800` in the `0x0C` seed arm
(`0x801E2CF8`), `li 0x200` beside the `ctx[7] = 0x14` store (`0x801E2F20`),
and `FUN_8004E13C`'s `(rand() % 2) << 11 + 0x280` (`0x8004E288..0x8004E2B0`)
under a three-way gate - argument `2`, `ctx[+0x243] != 2`, `ctx[+0x13] < 3` -
whose argument is the committed clip's header byte `+0x87` from
`FUN_8004AD80` (`0x8004BE18..0x8004BE2C`). The `battle_melee_hit_spark`
capture reads `ctx[+0x6DA] = 0x298` mid-art: `0x280` plus 24 frames of the
prologue's `max(1, 4 × frame_step / 3)` advance (`0x801E29E4..0x801E2A24`).

**Engine side.** `legaia_engine_vm::battle_cam_script`: `ActionFraming::
battle_over` names the fork byte truthfully, `action_framing` drops the
focus height, and `BattleCamera::observe_action_state` applies the seed
ladder on the action-state edges (the swing-clip commit stood in by the edge
into `0x1E`, since the engine's animation player does not expose the clip
header byte). Both hosts feed the same `action_state`.

### The Done band framing and the two orbit writers

**Question.** After an action resolves, `FUN_801E295C` sits in `0x50` /
`0x51` for the `ctx[+0x6D8]` tail (`0x3C` display frames by default). Is the
camera on the far framing there, and how fast does the Begin/Run prompt
orbit?

**Answer.** The tail is a **per-action framing chosen by category**, not the
far framing. Both Done arms read `actor[+0x1DE]` before the framing call
(`0x801E5E90..0x801E5EF4` in `0x50`, `0x801E5FC0..0x801E6018` in `0x51`):
category `5` (Run) takes no framing and runs the yaw orbit, category `3`
(Attack) `li a1,0x8`, a party slot whose target's live HP `+0x14C` is zero
`li a1,0x8`, anything else `li a1,0x6` - re-armed every pass. The far framing
returns at the end-of-action gate `0x5A`. The prompt orbits at `−2` yaw units
per display frame (`−4` per two-frame camera step).

**Evidence.** `zora_glare_petrify_post` (mednafen, `ctx[7] == 0x51`, slot 3
acting after a spell, `ctx[+0x6D0] = 0xC00`, `ctx[+0x6DA] = 1966`, Zora at
`(649, −47)` facing `3297`) reads pitch `0`, yaw `2735`, `TR (0, 1275,
4820)`, focus the negated `(624, 0, −40)` - case 6's in-fight pose
`(0, 0x500, 4915)` on the caster's seat with the tween one step short, and
yaw `(1966 − 3297) & 0xFFF = 2765` being chased. `evil_medallion_rage_battle`
(`ctx[7] == 0x0A`, flow byte `0xFF`) reads pitch `32`, `TR (0, 1280, 7920)`,
focus at the origin - case 9 over `±825` seats - so between actions retail is
on the far framing. The orbit rate: `scripts/pcsx-redux/autorun_battle_cam_orbit.lua`
on `battle_gaza2_prompt` (240 vsyncs, `ctx[+6] == 0x1E`, `ctx[7] == 0x00`)
with Exec breakpoints on both `sh v0,0x2(a0)` stores - the SM's at
`0x801E2A6C` never fires, the battle tick's at `0x801D07CC` fires once per
tick and the yaw drops `2 × DAT_1F800393` each time (`−6` per 3 vsyncs under
the interpreter's `step = 3`, i.e. `−2` per vsync).

**Engine side.** `legaia_engine_vm::battle_cam_script::done_band_phase` over
`DoneBandInputs` (category / party seat / dead target), filled by both hosts;
`DONE_STATES` split from `ACTION_END_STATES`. The orbit rate was already
`ORBIT_STEP = 4` per camera step.

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

### Super Art connectors and physical inputs - the tokenizer keeps the leading arrows

*Status:* resolved - disassembly (`FUN_801EED1C` `0x801EF2EC..0x801EF858`) + capture
(reproduces the in-the-wild Tri-Somersault queue byte-exact) + an independent table
(fourteen of fifteen walkthrough inputs agree)

The `0F` / `0E` "connector" directions in a Super's `find` pattern were held to be combo-specific
data no rule could derive, and the queue-builder was described as *compacting* a matched arrow run
down to `19 <art>`. Read off the disassembly, the normalisation loop does something else: on a
full match it writes the starter over the run's **last** arrow (`sb v1,0x1df(v0)` at `0x801EF6F8`,
`v1` = `FUN_801EFBFC`'s verdict `+ 0x18`), shifts the tail up one slot (`0x801EF708..0x801EF750`)
and inserts the constant after it (`0x801EF7A0`) - **the leading arrows stay** - and the outer walk
runs tail-first (`s8` from 15 down, `0x801EF848`) restarting at `s8 + 1` after every match, so
runs overlap. `↑↓↑` alone tokenizes to `0F 0E 19 27`; Tri-Somersault's `19 27 0F 19 1F 0E 19 27`
is what the seven-arrow input `↑↓↑↑↑↓↑` tokenizes to (Somersault 0..2, Cyclone 1..4, Somersault
4..6), and the capture's leading `0F 0E` are Somersault's own first two arrows, not "residual
input". Laying the three arts end to end tokenizes to four arts and never triggers.

Every retail Super's pattern derives to a unique shortest input, all 7..=9 arrows
(`legaia_art::tokenize::derive_super_input`, searched over the chain arts' directions), and the
curated walkthrough table agrees on fourteen; the fifteenth (Dragon Fangs, printed as six arrows)
drops one and never performs Swan Driver, so the curated entry was corrected to the seven-arrow
form. Port [`legaia_art::tokenize`](../../crates/art/src/tokenize.rs); table + citations in
[art-data.md](../formats/art-data.md#super-arts). What this bought downstream: `--show-super-arts`
draws each Super Art's real arrow string, sixty bytes for all fifteen instead of the ~330 a
concatenation of chain strings would have cost.

### The runtime art-record chase - the `+4` and the `- 0x10` grid origin

*Status:* resolved - `record(c) = *(*(DAT_801C9360[char]) + 0x58) + 4 + (c - 0x10) * 0xD0`,
with `+0x10` the name field and `+0x24` the power bytes

Both halves used to be soft. The `+4` came from reading the chase in isolation
(`0x8004B6FC..0x8004B718`: `lui/addiu` builds `DAT_801C9360`, `lw` the per-character block,
`lw +0x58` the array pointer, `addiu s0,v0,0x4`) with nothing pinning what `s0` was the base *of*;
the `- 0x10` grid origin came from an enumeration base solved by voting rather than read off
retail. `FUN_8004AD80`'s own three uses of `s0`, all in the same function, settle both at once -
each multiplies the action constant by `0xD0` with the identical `sll 1 / addu / sll 2 / addu /
sll 4` chain and then subtracts a constant:

| Site | Expression | Resolves to | Consumed as |
|---|---|---|---|
| `0x8004BBE8..0x8004BC10` | `s0 + 0xD0*c - 0xCF0` | `record + 0x10` | the art's display-name pointer (`sw` into `0x8007634C`/`0x80076344`) |
| `0x8004BC60..0x8004BC80` | `s0 + 0xD0*c - 0xCDC` | `record + 0x24` | the per-strike power bytes |
| `0x8004BCA0..0x8004BCC4` | `s0 + 0xD0*c - 0xCF6` | `record + 0x0A` | a byte handed to `FUN_8002B28C` |

`0xD0 * 0x10 = 0xD00`, so each constant is `0xD00 - field_offset`: the array is indexed from
constant `0x10` and `s0` is the base of record `0x10` itself, which is what the `+4` produces. The
two field offsets are the same `+0x10` name and `+0x24` power `super_art_power` edits off the
decoded `record0`, independently confirming that the runtime base and the file-side
`art_block_base` are the same address in two spaces. `--show-super-arts` chases the name through
this chain, and its planner re-proves it per disc by requiring all fifteen Super Art records to
carry their own names at `+0x10` before writing anything.

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

Spine-writer #2 of 3, closed. The story writers are **two** records, not the six
an arm count reports: rikuroa's post-victory `P2[50]` in the streaming variant
MAN (PROT 0157), whose own C1 gate is `0x142` itself - the self-latching
one-shot shape - and dolk2's carrier `P1[0]`, which re-asserts it. The other
four clean `51 42` / `61 42` arms are **developer flag-menu** rows: rikuroa
`P1[10..12]` is a nine-flag Set ladder with its mirrored Clear ladder, and
dolk2 `P1[1]` / dolk `P1[26]` sit in the same shape. A census that counts
opcodes cannot tell a beat from a menu row, because both decode clean.

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
chambers-only menu (`0x137`; kor3/kor4 carry per-pad record pairs, one per variant). One
softening worth keeping honest, and one former softening now pinned. The **menu
pixel height `rows * 16` reaches the renderer as a height** (`disassembly`): the
`sll v0,v0,0x4` at `0x801EF160` stores to `0x801F2B98 + 0x196` = record 14, field
`+0xE`, of the `0x1C`-stride window-descriptor array (stride from `FUN_801E9B3C`'s
`sll 3 / subu / sll 2`); the picker's command list at `0x801F3304` is `0001 000E`,
window id 14; and `FUN_80032434` moves `+0xE` into the live window's `+0x10`
(`0x80032484` → `0x800325D4`), the field the menu-side creator `FUN_800326AC`
fills from its descriptor's `h`. The kor pads themselves **never set `0x137`** - the
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

consumed by the field camera arrival handler `FUN_801dbec4` + camera-config `FUN_801dbc20`. The query side is ported: `legaia_engine_core::field_regions::zone_query` (`FUN_801dba20`, with the `FUN_80017fbc` `.MAP` region scan + `FUN_800180ec` attribute refresh) drives `World::refresh_field_regions` per tile crossing, and the section-3 body is the table the boot walk installs at `_DAT_801c6ea4 + 0x4`. Residual: the world-overview actor-placement section (consumed by `FUN_8003A1E4`), tracked separately (see world-overview threads); plus one loose end from the camera decode - the mask-kind records' `bytes[1..4]` side-copy to scratchpad `0x1F8003E8..EB` / mirrors `0x801F2778..84` is the visible tile window ([`encounter.md`](../formats/encounter.md#the-scratchpad-window-0x1f8003e8eb)).

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

**Records resolved - in-file, parsed.** Each `FUN_80021B04` call passes its record by absolute pointer (`lui 0x8020 / addiu`); under the correct link base `0x801F69D8` those resolve to PROT 0905 **file `0x180C..0x1E00`** (runtime `0x801F81E4..`), a contiguous table of variable-length records `[i16 model_sel][u16 reserved][move-VM bytecode @+4]`, `model_sel == -1` = transform/pivot node (dominant; mesh bound by the move-VM anim-bank ops), `>= 0` = `DAT_8007C018[model_sel + gp[0x754]]`. `legaia_asset::summon_overlay::parse` recovers them by scanning the spawn calls (disc-gated `summon_overlay_real`: 22 sites → 17 part records, all transform nodes, within the trimmed footprint; CLI `asset summon-overlay`).

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
(the iris/sprite/panel/letterbox set the **ten** ending scenes drive via
field-VM op `0x43`) is **dormant** here - an effect-actor-list walk of both frames finds
**zero** live widgets. The live effect is a single **move-VM part-actor** in the
part pool `DAT_801C90F0`, ticked per frame by the generic SCUS actor tick
`FUN_80021DF4` (→ `FUN_80023070`; this is the live capture that pins that
render-tail driver). Its `[i16 model_sel][u16 reserved][bytecode]` record
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

### The party cast trigger is a params stager

`FUN_801DBF9C(party, spell_id)` runs at the end of state `0x29`'s wait for a
party caster. Its disassembly (`overlay_battle_action_801dbf9c.txt`) has two
arms on `sltiu v0,a1,0x25`: at or above `0x25` it stores `actor[+0x1E0] = 9`,
`+0x1E1 = 0x12`, `+0x1E2 = 0xFF` and returns (`0x801DC064..0x801DC09C`);
below it, it indexes `0x801F4E64 + id - 1` for an 8-byte anim-pair list at
`0x801F4EDC` and copies the pairs into `+0x1E0..` until the `0xFF`
terminator. No store touches HP, MP or a target. So the trigger writes the
cast's anim stream and the summon sub-route the same state reads back
(`lbu v1,0x1e0(s3); li v0,0x9; bne` at `0x801E45EC`); the outcome is the
streamed module's. Every player Seru id is `>= 0x25`, so every player cast -
healing included - is a summon to this routine. The `0x12` it stages is the
argument the summon band hands `FUN_801DC0A0` each frame while the caster's
`+0x1D9` stays `9` (capture `gimard_summon_start`): a cast-effect id, not a
clip. Port: `BattleHostImpl::spell_anim_trigger` (engine-core), which arms the
engine's stager on the summon arm; the `< 0x25` table is not parsed.

### Player-summon presentation

Read off the capture corpus and the summon band's disassembly:

- **No spell-name label for a party caster.** `0x28`'s label block is
  skipped for an acting id `< 3` (`sltiu v0,v0,0x3; bne v0,zero,0x801e4460`
  at `0x801E43D8`); the mednafen `*_summon_mid_cast` display crops show the
  acting-actor plaque, the caster close-up or the flash's white and the
  additive burst, and no label. The party readout follows the hide: absent in
  every `0x33` / `0x34` crop and in `gola_gola` (`0x35`, all seats hidden),
  back under the caster in `vera` (`0x35`, stager phase 3, the caster's
  `+0x21C` cleared while the other seats stay `0xFF`).
- **The hide.** `0x34` zeroes the prim word and sets `+0x21C = 0xFF` on
  every party seat and every living monster (`0x801E4B30..0x801E4B6C`); the
  PCSX `gimard_summon_visible` / `_burning_attack` states read exactly that
  on slots 0..3 while the creature at slot 7 stays drawn; `0x36` restores.
- **The creature seat.** Slot 7 at `x=185, z=-2272` (caster `82, -542`) with
  the caster's facing on the idle clip at stager phase 6, then `z=-1606` on
  clip `1` at phase 11: it walks in from behind the party toward the target.
- **The two flashes.** Flash-in at `0x33` (additive, delay `0x14`, ramp
  `0x14` black → white, hold `-1`, id `1`) plus cue `0x63`; flash-out at
  `0x34` (additive, ramp `0x78` white → black, hold `1`). The `visible` state
  carries the flash-out actor finished (`+0x10 & 8`, hold counted below 0).
- **The duck.** `_DAT_8007B910` reads `161` against level `215` in both
  mid-cast states - the `75/100` floor of `0x35`, exactly.

Port: the summon band (`engine-vm::battle_action::summon`), the stager and
the hide/fade seams (`engine-core::world::battle::cast_band`), both hosts'
draw gates and `fade_prim` composition.

### The fade actor's hold word

`FUN_80020C14` (`80020c14.txt`): after the duration goes negative it sets
`actor[+0x62] |= 0x100`, then `lh v0,0x1e(a1); bltz v0,0x80020cd4` - a
negative hold skips the countdown entirely and the tick goes on ramping and
drawing; a non-negative hold counts down and, on expiry, sets
`actor[+0x10] |= 8` (finished) and returns `-1` (draw nothing). So `-1` means
**hold the landed colour until the actor is killed**, which is why the
escape white-out persists until the battle unloads and the summon flash-in
persists until `0x34` kills it. Port: `engine-core::fade::FadeState::step`.

### `summon.dat` / `readef.DAT` side-band streaming

*Status:* **resolved (entries + format)** - the two `0x10800`-slot battle streaming files are pinned and decoded; full reference [`formats/summon-readef.md`](../formats/summon-readef.md), parser `legaia_asset::summon_readef`, disc-gated `summon_readef_real`.

- **Entries pinned by arithmetic + bytes.** `FUN_800558FC` in retail ignores its path string (`_DAT_8007B8C2 != 0` verified live) and consumes the 4th argument as a raw-TOC index: `summon.dat` = `0x37F`, `readef.DAT` = `0x380` → **extraction PROT 893 / 894** (the −2 raw-TOC offset, same as the overlay loaders' `param + 0x381`). Both footprints divide into exactly 103 / 78 slots of `0x10800`. Byte-verified in `battle_gimard_tail_fire_a`: the stream buffer at `*0x8007BD74` equals entry 894 slot 1; slot 0's CLUT row / texture page match VRAM `(0,488)` / `(512,0)` byte-for-byte.
- **Format decoded.** Action id → base slot byte (`FUN_801E295C` case `0x32`): `3*(id-1)` for `id < 0x9A`, else `4*id + 0x63`; bit 7 selects the file. The applier `FUN_801F12D0` streams slots `base..base+3` (readef groups stop after `base+1` unless `base == 0x36`) and uploads CLUT rows + texture pages; `FUN_801F19EC` installs the final slot as the summon creature (via `FUN_80055468`). Summon group 0 (spell `0x81`) carries the "Burning Attack" record. Beyond the cast path, `FUN_801DABA4` seeds the group base **per turn** (party `3*(char−1)`; enemy `3 * monster_record[+0x1C]`) and the battle-end arms directly request `3*char+2` - the traced main-vs-base `"ME"`-archive pick (see [battle-data-pack.md § "ME" stream archives](../formats/battle-data-pack.md#me-stream-archives-readefdat)).

- **The `readef.DAT` aux-slot consumers are resolved.** The eight aux slots of
  readef groups 0..3 (slots `3c+1`/`3c+2`, c = Vahn/Noa/Gala/Terra) are the
  player **art-animation `"ME"` stream archives**, consumed by `FUN_8002B28C`
  out of the `*0x8007BD74` buffer - see [battle-data-pack.md § "ME" stream
  archives](../formats/battle-data-pack.md#me-stream-archives-readefdat);
  parser `legaia_asset::me_archive`. The main-vs-base pick is traced (per
  battle phase - turn staging vs battle-end win-pose staging; same doc
  section). The **loader** is `FUN_801F17F8` (raw TOC `0x380`, slot `*
  0x10800`), whose staging call is the single `jal` at `0x801F17A0` - which is
  why an address sweep for it comes back nearly empty - and the cast band
  originates no request of its own. Higher groups' aux slots are attributed as
  content too, by the aux-slot row in this area's table: textures, the four ME
  archives, or a never-staged actor record. The selection is the monster
  record's group byte `+0x1C`, staged per enemy turn by `FUN_801DABA4`. Group
  3's own archives were the last doubt - the ME read is gated to party seats
  `0..2` - and a sweep of `DAT_8007BD10` over the state corpus finds char 4 at
  seat 1 in one battle, so they are decoded.

Open residue:

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

**zero `mult`/`div`, zero `actor+0x14c` write, no power read** → the "magnitude is in this jump table" hypothesis is **falsified**; it is animation/GPU only.

**The magnitude is applied by each module's tick body, not by its stager.** The stager stages clips and seats; the `actor+0x14c` write lives in the `ctx+0x279` phase machine the `0x801CF4EC` table arms, which is a different routine in the same image. Reading a module's damage behaviour inside the stager's frame is what produced the split below, and three of its entries are wrong: PROT 0903, 0910 and 0913 are **damage**, and PROT 0909 is missing from it entirely.

**Damage bodies** call the shared battle kernel **`FUN_801dd0ac`** (`a0` = a
per-summon move-type const `0x10..0x12`, `a1` = a baked `7` on the player
half, `a2` = target slot), clamp - in one of [three
shapes](#battle--arts--level-up) - accumulate the popup at `actor+0x10`, then
subtract. **Two bodies heal**, and each has its own closed form over the
caster's per-magic **level** byte - the 32-slot search that matches
`actor[+0x1DF]` against the id list at `+0x705` and reads the parallel byte at
`+0x729` - rather than over a power byte: PROT 0905 (Vera) restores
`record[+0x729 + slot] * 0x20 + 0xe0`, clamped to the `+0x14C`/`+0x14E` pair,
skipped at `HP == 0` or `+0x16E & 4`, and pushed to `+0x10` as a *negated*
popup at `0x801F7D0C`; PROT 0911 (Orb) restores `(level << 6) + 0x1c0` over
the party row. Both unlock a status cleanse at level `>= 3`. The old inline
`(power_byte << 5) + 0xe0` describes neither.

`FUN_801dd0ac` (already dumped, `overlay_battle_action_801dd0ac.txt`) takes the **summon path** for `param_2 == 7`: roll = `rand % (INT@+0x168 + 1) + HP@+0x14c + DAT_801C9370[ctx+0x13]_INT * 2`, returns `roll - defender_mitigation` - so **summon "power" is caster/summon battle-state-derived, not a static per-spell scalar** (which is why SCUS spell-table `+5..+8` are zero and gamedata has no power column). `FUN_801dd0ac`'s **non-summon** branch (`param_2 != 7`, arts/physical) reads a real 26-byte-stride per-move power table at **`0x801F4F5C`** (arts power, **not** magic) - now located on disc as static battle-overlay data (PROT 0898, parser `legaia_asset::move_power`),

indexed via a 128-byte id→index map at `0x801F4E63` (`param_1 = map[actor[+0x1df]]`); **the full 26-byte record is now decoded** (`+0` power, `+0x02` strike-Y offset, `+0x04`/`+0x06` move/phase counters, `+0x08`/`+0x09` homing speed + tracking flag, `+0x0a` impact-effect selector, `+0x0b` trail texture page, `+0x0d` sound cue, `+0x0e` list-mode flag, `+0x12`/`+0x16` effect-id lists; `+0x0c` is an unused `C`/`E`/`G` designer tag) - see [`docs/formats/move-power.md`](../formats/move-power.md). The move-id space is the spell-table id space, so the records label cleanly: idx `0x10..=0x2b` = the named monster special-attacks (`0x25..=0x74`), idx `0x01..=0x0f` = the unnamed internal enemy-attack tiers (`0x04..=0x1f`).

The scale stage `FUN_801dd864` (8×8 element-affinity matrix `0x801F53E8` + status bits + the summon magic-power tail `roll += roll*(power-1)>>3`) and the finisher `FUN_801ddb30` (resistance bits, `rand%9+8` floor, 9999 cap, spirit-gauge, MP drain, stat debuffs) are now fully traced - see the `FUN_801dd864` / `FUN_801ddb30` rows in `functions.md` and the three-stage chain in `battle-formulas.md`.

**Ported:** the closed-form roll + scale arithmetic is now pure kernels in `legaia_engine_vm::battle_formulas` (`summon_attacker_roll` / `summon_defender_roll` / `summon_predamage` / the `apply_*` helpers / `heal_summon_amount`), hand-tested against the disassembly.

**Residual:** (1) the arts/physical kernel is now **wired into the live loop for monster special-attacks** - the move-power table loads onto `World::tables.move_power` (`engine-core::move_power::MovePowerCatalog`, PROT 0898) and `cast_spell_on_slots` overrides a damaging monster cast's magnitude with `arts_physical_predamage_lazy` seeded by that move's `+0` power (`World::enemy_move_predamage`: INT from `battle_accuracy`, defense terms from `battle_defense_split`; the attacker ×2 + defender ×1 `rand()` draws are taken up front and the bonus pair is drawn **lazily**, only when the bonus arm fires, so the shared RNG cursor advances by exactly three or five draws matching `FUN_801dd0ac`'s call order; gated on the table being installed so disc-free battles keep the placeholder + RNG stream).
The player-driven **summon** roll is now wired too (`World::player_summon_predamage`): summon-body HP/INT seed from the namesake `battle_data` creature record, caster INT from `battle_accuracy`, the caster magic-power byte from the character record's spell list (`+0x13D` ids / `+0x161` levels, the `FUN_801dd864` search), and the closed-form `FUN_801ddb30` finisher applies - including the per-caster summon power-percent table `0x801F5468` ((char_id-1)*8 + summon_element; PROT 0898 file `0x26C50`, parsed as `ElementAffinity::summon_power`, byte-pinned: own 100, opposed 40, Gala dark 60). Remaining residue: the live slot-7 actor's HP at roll time is modelled as the creature record's spawn HP (a mid-battle summon that has taken damage is not modelled), and status/guard default to none;

(2) the `FUN_801ddb30` finisher's **closed-form finalisation arithmetic is now ported** (`battle_formulas::damage_finish` - equipment elemental-resistance halving / guard halve / `rand%9+8` no-damage floor / summon power-% scale / 9999 cap - plus `spirit_gauge_fill`, both unit-tested); only its state-mutating tail (damage-popup accumulator, AI revenge table, MP drain, per-element stat-debuff switch) stays in the live battle context; (3) the affinity matrix `0x801F53E8` is now located + parsed off the disc (`legaia_asset::element_affinity`, PROT 0898 file `0x26BD0`, same link base as the move-power table) together with the per-character element table (`0x801F5480`: Vahn=fire/Noa=wind/Gala=thunder/Terra=wind), the matrix orientation is corrected (`matrix[attacker][defender]`;
the retail values are a ±4% nudge - diagonal 96 / opposite-pairs 104 / default 100, not a ×0/×2 weakness table), and the enemy element source is **pinned from the `FUN_801dd864` disasm itself**: the scale stage reads it **record-direct** - `lbu …,0x1d(record)` where `record = 0x801C9348[slot-3]` (the per-enemy record-pointer table, not a copied live-actor field) - so the element is `MonsterRecord::element` (`+0x1D`) consumed exactly as the parser exposes it (the same record the victory-spoils path reads `+0x44/+0x46/+0x48` from). This supersedes the earlier "loader copies `+0x1d` into `actor[+0x1d]`, copy not yet pinned" framing; the curated-element correlation (four party-table ids reproduce exactly + byte ∈ `0..=7` across every populated record) now only corroborates the id *labelling*.

**Wired (both directions):** the monster special-attack path scales by `matrix[enemy_element][party_member_element]` (`World::enemy_affinity_pct` → `enemy_move_predamage`), and the **player Seru-magic** path scales by `matrix[summon-creature element][target element]` (`World::cast_affinity_pct` in `cast_spell_on_slots`): the attacker element resolves off the summon **creature** by name (`World::summon_attacker_element`, the engine-side slot-7 `+0x1d`), the defender by slot (`World::battle_slot_element`). The player multiply is post-roll on the deterministic cast output (RNG untouched); the enemy scale is applied *inside* the roll, before the conditional bonus-arm threshold (so a non-neutral value can shift the lazy bonus draw - faithful to retail's scale→bonus order).
Both are gated so an uninstalled / neutral table reproduces the no-affinity baseline bit-identically (magnitude + RNG stream), keeping disc-free battles deterministic. The player-summon **base** magnitude is still the caster-state stand-in (the faithful slot-7 summon roll is open), so the player direction is the ±4% nudge on a placeholder, not yet byte-exact. See [`battle-formulas.md`](../subsystems/battle-formulas.md#element-affinity-matrix-fun_801dd864-0x801f53e8). The `0x801F4F5C` **arts** power table is located + parsed (`legaia_asset::move_power`), the `param_1` → move-id map resolved (`0x801F4E63`),

and **every record field decoded** (power / strike-Y offset / move + phase counters / homing speed + tracking flag / impact-effect selector / trail texture page / sound cue / list-mode flag / on-contact + launch effect-id lists; `+0x0c` is an unused designer tag with no runtime reader) - see [`docs/formats/move-power.md`](../formats/move-power.md). The auxiliary tables the record's selectors index are now parsed too: `EffectAuxTables` for the `+0x12`/`+0x16` effect-id lists' `0x801F6324` prototype-pointer + `0x801F6418` SFX tables, and `parse_impact_effect_table` for the `+0x0a` `0x801F53D4` config words (this corrects an earlier "pointer table" mislabel - the `0x801F53D4` entries are packed `u32` config words, not pointers).

**The `0x801F6324` spawn entries are decoded.** Each is an overlay VA to a *variable-length move-VM scene-graph record* in the **exact summon-part format** (`+0x00 i16 model_sel`, `+0x02 u16 reserved`, `+0x04` move-VM bytecode), spawned by `FUN_80050ed4` → the shared stager `FUN_80021B04` → the ported move VM, with `model_sel` indexing `DAT_8007C018` - the same machinery as `legaia_asset::summon_overlay`. The earlier "~0x20-byte struct" reading was a coincidence (packed records, not a fixed stride). The high-bit (`0x80`) list bytes route instead to the 2D `efect.dat` pool (`FUN_801dfdf0` → `EffectCatalog`, ported as `spawn_by_ui_id`).

Render wiring reuses the summon parser + move VM. The `model_sel` additive base `gp[0x754]` (global `0x8007BA6C`) - only *read* in the corpus - is **resolved from the save corpus**: it is `0` whenever no battle effect-model library is resident, and **`party_count + 2`** when a battle has installed it - `3` for the 1-member training party (Vahn alone), `5` for the 3-member party (Vahn / Noa / Gala). A PCSX-Redux exec-bp on `FUN_80021B04` first pinned the value `3` (probe `autorun_summon_model_base`, confirming the full `FUN_801e09f8 → FUN_80050ed4 → FUN_80021B04` chain - `ra = 0x80050F08`, `a3 = 0x1000`, prototype table `0x801F6324` + effect-list id `0x22` live in registers); reading `0x8007BA6C` + the party count `0x80084594` across the whole mednafen corpus generalised it.
So the base **tracks party size** (the two fixed pool slots + the live party-character meshes precede the effect-model library), and `model_sel` is *library-relative* - `DAT_8007C018[model_sel + gp[0x754]]` lands on the same library model regardless of party size; only the library offset shifts. There is **no per-summon base** - one per-battle value drives both move-FX and summon-part spawns. Pinned by `crates/mednafen/tests/summon_model_base.rs`.

The engine **renders the move-FX scene-graph**: `World::spawn_move_fx` parses a move's spawn-entry records (`MoveFx` via `MovePowerCatalog::fx_for_move_id`), stages them as a `SummonScene` at the effect-model library base (the engine registers PROT 0871 at a fixed `DAT_8007C018[3..]` and `model_sel` is library-relative, so this is the retail `party_count + 2 = 3` case for the 1-member slice; the layouts are equivalent), and drives them through the ported move VM (`tick_move_fx` / `active_move_fx_part_draws`; `play-window` `H` debug-spawn) - reusing the summon machinery wholesale, so it shares the same interpreted-transform caveat. A spawn also surfaces the move's two presentation fields: the **trail texpage** (`+0x0b` → `0x7700 + id`) on `World::active_move_fx_trail_texpage()`,
and the **sound cue** (`+0x0d`) as `World::take_pending_move_fx_cue()`, which the host routes through the now-ported `FUN_8004fcc8` dispatch decode (`legaia_engine_audio::classify_cue` → `CueDispatch`; the voice arm's third argument is a **read span**, not a pitch - `FUN_8003D53C` range-checks it against `0x2A31` at `0x8003D5C8` and touches no pitch register). The 2D afterimage *draw* `FUN_801e1ab0` (the streak pass that consumes the trail texpage) is ported as the pure `legaia_engine_render::afterimage::build_afterimage_quad` - jittered semi-transparent `POLY_FT4` (per-corner `rand` wobble, brightness band, UV/CLUT/texpage layout) from four projected corners + the trail id.
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

The `0x2000` stride is only the RAM buffer the loader stages - the parser derives the scattered disc offsets directly, **no capture needed**. Every prior byte-brute missed only because it used the bit-15-**set** runtime needle, not the disc bit-15-**clear** form. From-scratch parser **`legaia_asset::battle_char_palette`** (`find_record0` + `parse_record`; synthetic unit test + disc-gated `battle_char_palette_real` which passes byte-exact against extraction PROT `0863` with `record0` at file offset 0 - the identical digest the historical `0861`-window run produced; STP bit-15 set on upload). Tetsu fight is Vahn-only so Vahn (863) is byte-exact-validated + wired.

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

**Engine:** the field-carrier SM tick exists (`tick_field_carriers` / `install_field_carriers` / `engage_field_carrier`) and reaches Battle via formation index 4 (`training_battle.rs`); the carrier set is now **derived from the scene MAN** (`man_field_scripts::derive_field_carriers` + `World::install_field_carriers_from_man`), so the sparring carrier's identity and placement come from the real actor-placement partition.
The engage is now **driven by the field-VM dialogue-accept**, not a manual API: talking to the carrier's placement (the button-press interaction, which is no field-VM opcode - op `0x3E` with `op0 < 100` is the scripted-battle install, not a talk) arms the engage (`World::carriers.slots` → `pending_carrier_engage`) and accepting its prompt (the `0x4C` n5 sub-4 dialog dismiss) engages it.

`training_battle.rs` drives this end-to-end on disc data, reaching Battle with Tetsu without `engage_field_carrier`. The interaction probe is now ported faithfully: `World::tick_field_interaction_probe` (from-scratch `FUN_801cf9f4`) runs retail's `DAT_801f2254` facing probe - a radius-64 compass point ahead of the player's facing, box-tested at ±72 against the talkable NPCs' placement positions (`World::npcs.positions`) - and on the action button talks to the matched NPC and turns the player toward it, so facing the sparring partner and pressing X starts the fight with no script injection (`training_battle.rs::training_reaches_battle_via_interaction_probe`).

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

### The battle-intro enemy-name banner

*Status:* resolved - the question had a false premise.

No placement record raises it. `FUN_801D9D3C` - the flow-`0x0A` composer, whose
only reference in the whole corpus is the `jal` at `0x801D0DFC` - lays its
labels out itself and hands each straight to the text-actor spawner
`FUN_8003541C` with **immediate** geometry: one label per distinct monster
group, id = group index `0..=3`, class `0`, kind `3`, pen `(laid-out x, 48)`,
box `measured width x 12`. The only placement-table field the intro touches is
record 67's `+0x14` string-pointer cell, which holds the back-attack /
pre-emptive line the same routine then draws under id `4` at pen `(16, 12)`
288 wide - record 67's own fields, again as literals. Record 67 proper is
opened only afterwards, by the post-intro sub-draw, under its `+0x01` id
`0x2B`.

Live capture (`scripts/pcsx-redux/autorun_battle_intro_banner.lua`,
breakpointing the spawner and the teardown sweep `FUN_800355F0` and walking
the text-actor list at `gp[+0x148]`): an ambush raises `Queen Bee` at
`(176, 48)` 55 wide, `Killer Bee * 3` at `(78, 48)` 79 wide and `Ambushed!` at
`(16, 12)` 288 wide, all from `$ra` inside the composer, all holding those
exact seats for 120 frames; an ordinary encounter raises two labels for 90
frames and no line at all. Nothing glides - the composer calls neither
`FUN_801D8DE8` nor the glide `FUN_801DB7B0`. The width overwrite the old
residual described belongs to record 68 (disc `w = 0`, spawned at the measured
name width), not to the intro. Full law, naming rule and seat arithmetic on
[`battle.md`](../subsystems/battle.md#the-battle-intro-enemy-name-banner).

One monster suppresses the banner: monster-slot-0 id `0xB5` (evolved Cort)
skips the composer and parks `ctx[+0x06] = 0x0C`, a value the flow ladder at
`0x801D0C84` has no arm for - the writer that moves it on is an open row.

### What a port owes the slot-B module band

*Status:* resolved as a per-address verdict.

Of the 65 catalogued addresses in PROT 0903..0966, 58 are the module's
`0x801F6734` move-VM spawn-stager entry, 6 are cast tick bodies reached from the
module's own `0x801CF56C` trampoline, and 1 is a framed routine nothing
references. Forty-five are pure spawn choreography - an arm switch whose arms
only call `FUN_80021B04` / `FUN_80050ED4` / `FUN_801DFDF0` / `FUN_80024E80` with a
module-resident record and a scale literal - and are expressible as the
spawn-record data layer; thirteen carry game logic (two *stagers* apply damage:
PROT 0927 through `FUN_801DD0AC(0x12, 7, seat)` and PROT 0966 through
`FUN_801DD4B0(0x100, ..)`, each with the HP `+0x14C` clamp; four more write
actor or `ctx` state); seven are empty (`jr ra; nop`). Per-address table on
[`cast-module.md`](../subsystems/cast-module.md#the-band-as-a-port-worklist).
Grade `disassembly`.

### readef groups 19..21 and monster record `+0x1C`

*Status:* resolved-negative - nothing names them.

`+0x1C` is the readef animation-group index: the initiative scheduler
`FUN_801DABA4` reads it record-direct (`lbu v1,0x1c(v0)` at `0x801DB098` /
`0x801DB0C8`) and seeds the streaming applier's base slot with `3 * group`.
Across the 186 populated records the byte never leaves `0..=25`, and never takes
1, 2, 5, 12, 19, 20, 21 or 23; group 0 is the default. A second reader, the AI
spell picker `FUN_801E9FD4` (`0x801EBB90`), compares the first enemy seat's byte
against `0x17` and no shipped record satisfies it. Groups 19..21's duplicated
actor records are therefore unreachable through the enemy path (slots 58/59/71
and 64/65/68 are byte-identical). Census in
[`summon-readef.md`](../formats/summon-readef.md#which-monsters-name-which-readef-group);
parser field `readef_group` in `legaia_asset::monster_archive`. Grade
`disassembly` + disc census.

### The dome panel-still arming

*Status:* resolved - a generic battle-end teardown, not a dome condition.

`ctx[+0xC] = 1` is written at `0x800474CC` in the per-frame anim-node tick
`FUN_80047430`, for an enemy node (`node[+0x5A] >= 3`) under `gp[+0xA48] & 0x80`
and `gp[+0x9F4] != 0xB5`, together with `node[+0x10] |= 8`. `ctx[+0xC]` is then a
three-value teardown machine: `1` frees the actor table and writes `2`; `2` ticks
`FUN_80025358`, which stages PROT 0978 into the freed space - from three sites
(`0x8004E65C` escape, `0x8004F82C` victory tail, `0x80056428` in `FUN_80056208`).
The `ctx[+0x7] == 0x67` the thread cited is only the successful-escape hold
written by case `0x66` of `FUN_801E295C`. What *draws* the still stays open.
[`minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md#what-arms-the-load).
Grade `disassembly`.

### The Miracle marker is an equipment byte

*Status:* resolved - there is no input recognizer.

`ctx[+0x25F + slot]` has exactly one store in the dump corpus: `sb v1,0x25f(v0)`
at `0x80054270` inside the SCUS party battle-actor seeding routine
`FUN_80053CB8`, taken from the acting character's Ra-Seru equipment byte -
record-relative `+0x199`, or `+0x198` for roster character id `2`
(`beq v0,a3` at `0x800541E4`, the weapon-index table `_DAT_8007B42C` = `2, 3, 2`
naming the *other* member of the pair). The marker gates all four special art
records at `0x801EF4C8`, not only the Miracle. Memory cards corroborate: the
byte is small and per-character banded, zero for a member not yet bonded.
Grade `disassembly` (+ `capture` for the card corroboration).

### Two battle-context bytes read wrong

*Status:* resolved.

`ctx[+0x26]` is the **level-up banner's UI element id** (`0x65`): its reader
`0x801E61B4` passes the byte as `FUN_801D8DE8`'s element-id argument in a run of
sibling unloads with literal ids; writers `0x801E723C` (the only assignment),
`0x801E6D3C` (an increment), cleared at `0x801E2CFC`. It drives the Done band's
`0x50` seed override (`0x96` for `0x3C`) and the `0x51` banner skip (59
unskippable frames, then a press). `ctx[+0xD]` is the **per-action battle-camera
angle variant** `0..=3`, a four-way switch at `0x801D6510` / `0x801D6698` /
`0x801D689C` inside `FUN_801D5854`, seeded `rand() % 4` at ActionSeed and
narrowed per category from jump table `0x801CF144`. Grade `disassembly`.

### Stone and Curse in the SCUS effect applier

*Status:* resolved.

`FUN_800402F4`'s first-level jump table at `0x80014FA0` (132 entries, guard
`sltiu 0x84`) sends class `9` to the Stone arm and class `10` to Curse. Stone
makes three stores after the accuracy roll: `+0x16E |= 4`, a refund of the
target's reserved item through `FUN_800421D4` gated on `+0x1DE == 1 &&
+0x16C != 0` (`+0x16C` is the initiative key, not a cooldown), then `+0x1DE = 0`.
Curse sets its bit only. No item-effect record (`0x800752C0`, 130 records)
carries class `9` or `10`, so both arms are reachable only from the streamed
capture-class modules. Grade `disassembly`.

### Attack x2, the swing class and the apply-mode arms

*Status:* resolved.

`ctx[+0x16]` has two writers, both in the strike loop and both keyed on the
War God Icon (record `+0xF4 & 0x2000`). The end-of-stream refill at
`0x801E3A20..0x801E3A64` runs only while the counter is zero (`bnez` at
`0x801E3A18`), lifts it to `1`, rewinds the strike cursor and rewrites every
marked queue slot to `0x19`. The stage site's tail bumps it at
`0x801E37AC..0x801E37BC`, but only when it is already non-zero (`beqz` at
`0x801E37B4`), so the second pass's first stage lifts it to `2` - the value that
ends the damage kernel's carry arm. An earlier revision of this row named the
refill the sole writer; the stage bump sits in the same block and was missed
([`battle-action.md`](../subsystems/battle-action.md#the-war-god-icons-per-stage-bump)).
Monster record `+0x1E` is the limb-vs-height **swing class**, read through
`0x801C9348` by `FUN_801EED1C` (class `2` gets one low swing) and by
`FUN_801EC3E4`'s apply-mode look-ahead (class `2` connects only with power bytes
`0x01..=0x10`, class `3` only with `0x11..=0x15`); across the archive's 186
records it reads `0` x127, `1` x1, `2` x52, `3` x6, so both arms are ordinary
play. Both kernel copies gate everything on a monster target. Grade
`disassembly` + measurement.

### How a cast reaches its slot-B module

*Status:* resolved; the data half is ported.

Both PROT 0898 tick dispatchers key the same 64-entry band: `FUN_801F1ED4` on
the queued action id (`0x801CF4EC` row `id - 0x81` = PROT `903 + row`) and
`FUN_801F2160` on the spell record's `+0x01` byte (`0x801CF56C` row `sub` =
PROT `935 + sub`; single call site `0x801E50C8`, battle phase `0x70`'s hold).
The band's DATA half - the spawn records the modules pass `FUN_80050ED4` /
`FUN_80021B04` - is recoverable with the shared `summon_overlay` reader and
is an engine pool (`legaia_asset::cast_effect_pool`) staged at both retail
seams. The module CODE half (lift, camera, phase machine, the two damage
stagers) stays unported and disclosed. Grade `disassembly`.

### What an art body costs in AP

*Status:* resolved - computed, not disc data.

`FUN_801EED1C` derives it: multiplier `11` / `10` / `6` by the builder's *visit*
ordinal (`0`, `1..3`, `>= 4`) - not the arts-grid display index - halved before
the multiply under the actor's `0x800` flag (`srl` at `0x801EF378`), times the
art's command count; accrued into `actor[+0x224]` and spent from Spirit
`actor[+0x170]` at `0x801E5D74`. Port `engine-core::ap_gauge::art_spirit_cost`.
Grade `disassembly`.

### Tutorial prompt machine exclusivity

*Status:* resolved - the machine is byte-exclusive to 0967; the prompt pool is shared with 0968. Grade `disassembly`

The prompts are resident in stage overlay 967, so porting the battle SM alone
could never emit them. Exclusivity of the **machine** is byte-decided:
`FUN_801F6B70` is entry 967 file `+0x198` (`0x801F69D8 + 0x198` reproduces the
VA), and five needles from 48 to 2316 bytes - one a 116-byte window free of
`lui`/`j`/`jal`, which a relinked copy could not evade - return **one** physical
copy across every PROT entry, SCUS and DMY.DAT (967 is stored raw). The prompt
**pool** is not exclusive: entry 0968 carries a byte-identical 852-byte prefix
of it at the same offset `0xCAC`, inside a `0x5D8`-byte run the two entries
share; 0968 has its own 7-entry dispatcher and no copy of the machine.
`FUN_801F6B70` is a 91-entry jump-table hook on `ctx[+0x06]` with nine live
slots, each switching on `ctx[+0x28A]`. Port `engine-core::battle_tutorial`.
[details ↓](../subsystems/battle.md#the-sparring-tutorial-prompt-machine-
overlay-967)

### The _DAT_8007BA78 census is closed

*Status:* resolved - grade `disassembly`

The `_DAT_8007BA78` census is closed, not surveyed: a sweep in every reference
form over SCUS, all 1233 PROT entries and the overlay images - including the
`gp`-relative `0x760(gp)` an absolute-only scan cannot see, which yields
**zero** - finds exactly two stores (`0x801E30F4` = PROT 0897 `+0x148DC`, the
`4C E2` op; `0x801DDCE8` = PROT 0899 `+0xF4D0`, the title tick), four loads (all
PROT 0970), and one literal-word reference at PROT 0971 `+0x1238` = `0x801CFA50`
- the debug menu's editable-globals pointer table, the **static** witness for
the dev-menu editing the corpus states came from. The "PROT 0896 carries the
same span shifted by `0x9000`" note goes with the entry-size correction. A raw
scan cannot see code inside an LZS section. See
[cutscene.md](../subsystems/cutscene.md).

### The 0xF8 halt-acquire handshake

*Status:* resolved - retail machine traced; the port's model diverges. Grade `disassembly`

`0xF8` resolves to the player object `_DAT_8007C364` (`FUN_8003C83C`: `li
v0,0xf8` / `lw v0,-0x3c9c(v0)`; inlined again at `0x800377A0` and `0x80037E04`).
Three parts of the older description came from the port, not the arms:
**ExecMove arms nothing** (`0x801DE998` writes only `+0x5C` / `+0x5E` / `+0x56`
and calls `FUN_800204F8`); **the halt-acquire creates the wait object**
(`0x801DF384` sets `+0x94` and ORs `0x400` into `+0x10`, plus the caller's when
the target is the player at `0x801DF404`; `0x801DF5AC jal 0x801d25ec` spawns the
glide actor and the release helper `0x801D5D60`, which polls `andi v0,v0,8` at
`0x801D5DB4` and clears the halt at `0x801D5DD4` / `0x801D5DFC`); and **the
record parks at the next cross-context op**, in the prologue busy gate
`0x801DE90C..0x801DE944` whose unadvanced PC the run loop reads as "stop"
(`0x8003CFF0`) - the acquire itself advances 9 bytes (11 for sub-A/B), 0 on
failure. No backward resume PC: the operand `+3` / `+5` halfwords are
`FUN_801D25EC` tween arguments. See [cutscene.md](../subsystems/cutscene.md).

### Growing a scene TMD pack member

*Status:* resolved - rebuild is the only cost. Grade `disassembly`

Every external reference is an index: the mesh pool is the descriptor walk's
registration order (`FUN_8001F05C` → `FUN_80026B4C(buf + offsets[i] * 4)`),
placements name a pool slot, scene ANM records name a record number, and the
bundle's descriptors hold offsets into the bundle entry, not the separately
streamed pack. Disc-wide, each of PROT 0639's members `106 / 107 / 108` word
offsets occurs exactly once - in the pack's own table - the byte-offset form
zero times, and the declared length `347476` zero times outside its chunk
header. See
[`man-relocation.md`](../formats/man-relocation.md#the-same-question-for-an-assetpack-growing-a-mesh-member).

### What of a signature cast is data

*Status:* resolved (partially) - grade `disassembly`

At all 15 / 41 / 24 `jal FUN_80050ED4` sites in PROT 0958 / 0959 / 0960 the
record pointer `a2` is a module-resident constant and `a3` a scale literal; the
records are the summon part-record shape the art path's `0x801F6324` prototypes
use. Blocked by residency and capacity: the prototype table's 61 entries are all
populated, the module blocks sit above the slot-B base while the prototypes sit
below it, and the battle overlay has 247 bytes of zero slack. The lift is `sb`
into `+0x1DA` (16 / 6 / 8 sites) from the modules' phase arms, which no record
can express. See
[`cast-module.md`](../subsystems/cast-module.md#what-of-the-choreography-is-data-and-what-is-code).

### Op `0x23` MOVE_TO keys on ctx identity

*Status:* resolved.

The player arm is chosen by ctx *pointer* identity: `0x801DEC7C bne s5,v0`
compares the executing ctx against `_DAT_8007C364` (`0x8007C348 + 0x1C`), and
only that identity reaches the camera re-centre `func_0x80017EC8`
(`0x801DEC84..0x801DECA8`); every other ctx takes `0x801DECAC`'s facing +
movement-init arm on its own actor. A spawned partition-2 record inherits the
`0x1000000` player-class bit, so keying on the bit teleports the player to
wherever the record seats its own actor - which is what `keikoku`'s arrival
record did once op `0x45` stopped restarting it. Grade `disassembly`.

### Dome ringside panel stills PROT 1221 and 1222

*Status:* resolved - grade `disassembly`

Two 320x256 BGR555 stills in the dome's `other6` bundle, streamed in four
`0xA000` strips by `FUN_801F6B24` (PROT 0978) into VRAM `(384, 0)` (rect
`0x801F735C`; `320*256*2` = the entry size exactly, the first five sectors a
16-line top pad). No literal names them: `addiu a0,s0,0x4c7` at `0x801F6C3C`
with `s0 = (party slot 0 current HP < max / 2)`, so `int2.tim` is the
below-half-HP variant - the same failure shape as the gp-relative blind spot, a
computed index a literal sweep cannot see. Corroborated by the overlay's dev
path strings `h:\prot\field\other6\tim\int.tim` / `int2.tim` selected by the
same `s0`. See
[`minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md#inttim--int2tim---the-ringside-panel-stills).

### The display object's sound-bank category

*Status:* resolved - grade `disassembly`

`actor[+0x22C]` is the pointer to the actor's spawned display object
(`FUN_80024C88` → `FUN_80020DE0`), installed at `0x800515E8` / `0x8005196C` in
`FUN_800513F0`. Its `+0x80` is zeroed at spawn (`0x80020F50`) and set at battle
setup to `7` (party arm, `0x80051548`) or `7` / `8` (the four enemy seats, loops
at `0x80052238..` and `0x800522D0..`), each loop paired with `jal 0x8003E104`
carrying the same literal in `a1` - the bank-load slot. The router copies it
into the descriptor's category column (`0x8004FFD8..0x8004FFE4`,
`0x80050070..0x8005007C`). The monster element byte is record `+0x1D`.

### The evolved-Cort flow park

*Status:* resolved - grade `capture` + `disassembly`

The `0x0A` arm writes `0x0B` unconditionally and overwrites `0x0C` for formation
`0xB5` (`0x801D0DE0..0x801D0E14`); the ladder idles on it. PROT 0968 (loader-B
tracker `0x49`) runs a 7-phase intro cinematic off `ctx[+0x289]` (jump table at
`0x801F69D8`) and writes `0x0B` back at `0x801F713C` (`ra = 0x800564A0`) when a
dt countdown on its local word `0x801F73F8` expires - 3207 vsyncs later in the
capture, with `0x0B → 0x14 → 0x1E` following at once. A ten-button sweep
produced zero writes. Probe `autorun_w4d_cort_flow_writer.lua`; see
[`battle.md`](../subsystems/battle.md#flow-0x0c-is-the-boss-stage-modules-baton).

## Field / locomotion

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| Which clip does the player hold through a kind-0 warp? | resolved (idle) | `capture` + `disassembly` | The system channel `0x8007E694`, ticked by `FUN_801DA51C` after the player, runs `FUN_80039B7C`, whose `sw v0,-0x4228(v1)` at `0x80039D94` stores clip base `2` every field tick. The settle reads the base before it (`0x801D1D8C`, `0x801D1E08`), so while the pad step runs the reset is invisible; with the step skipped the settle reads `2`, and a write watch sees clip id `+0x5C` read `2` for the whole warp with Down held ([`field-locomotion.md`](../subsystems/field-locomotion.md#retail-capture-of-the-base-writers)). |
| How long does the warp timer's `-1000` sentinel live? | resolved (the rest of the landing tick) | `capture` + `disassembly` | `FUN_801D1EC4` parks `_DAT_8007B6B0` at `-1000` on the landing, and the same tick `FUN_801DA51C`'s tail compares it with `-1000` and stores `0` (`0x801DA7D8`), so no next-tick reader sees the sentinel in a scene whose system channel runs it ([`field-locomotion.md`](../subsystems/field-locomotion.md#retail-capture-of-the-warp)). |
| Where does an inn conversation end? | resolved (at the last page's close) | `capture` + `disassembly` | The sub-`5` acquire is the arm `0x801E2148..0x801E21DC` (table `0x801CEF48` entries `5`, `0xE`, `0xF`), refusing on `s7 = 0` (`beqz s7` at `0x801E21D0`). In `retock_innkeeper_talk_open` the stay ends when its last page closes, with the cursor parked on the `26 9D FE` loop-back; the next talk runs the loop-back and the acquire succeeds (`s7 = 5`) ([`script-vm.md`](../subsystems/script-vm.md)). |
| What does the region battle-setup half of `FUN_801D9E1C` store? | resolved (a backdrop variant, two Door gates, an object-keep bit, a world-map return point) | `disassembly` | On every region hit (`0x801DA058..0x801DA12C`): `_DAT_8007BD60 = region[+8] & 0x1F`, the stage variant `FUN_800513F0` loads; `0x1F800394 \|= 0x300000`, cleared per Door of Light / Wind by bits 7 / 6; `_DAT_8007B64B` = bit 5, keep backdrop object 1; and a return triple at `0x80084624..0x8008462C` from `region[+5]`, `[+9..+11]` ([`script-vm.md`](../subsystems/script-vm.md#the-region-battle-setup)). |
| Why does `kor5`'s `0x619` read set on entry and clear after the chain? | resolved (a spawn-section write) | `disassembly` + `capture` | `P1[2]`'s `SET 0x619` sits between its leading `0x25` and first `0x21`, so `FUN_8003A1E4` runs it at every MAN-loading entry; `FUN_801D6704` passes `loader mask & 4` to `FUN_8003AEB0`, which skips the spawn loop (`0x8003B8A0`) on a same-scene reload, so the post-battle reloads never re-set it after `P2[4]` clears it ([`script-vm.md`](../subsystems/script-vm.md)). |
| Where does a talk begin in an NPC record? | resolved (the interaction cursor `+0x9E`, never `script_pc0`) | `disassembly` | The interaction dispatch runs `FUN_80039B7C` from the cursor, so the spawn section runs only at scene load. Entering at `script_pc0` re-ran it on every talk and skipped the record's segment-selection prologue ([`script-vm.md`](../subsystems/script-vm.md)). |
| Who writes the field clip base `_DAT_8007BDD8`, and which clip is run? | resolved (the pad step; run is bank slot 2) | `disassembly` | `FUN_801D01B0` stores it every frame (`0x801D0424..0x801D04A4`): idle `2`, walk `1`, any faster step `3`, `99` under `_DAT_8007B6A8`. The hop machine writes `6` / `7` / `1`; `FUN_801D1EC4` writes `2` on one arm. Run is reached by Cross or R1 (run mask `0x48`) ([`field-locomotion.md`](../subsystems/field-locomotion.md#the-clip-base-and-the-settle-tail)). |
| What does a kind-0 walk-on tile do? | resolved (a timed warp, not an instant one) | `disassembly` + `capture` | `FUN_801D1EC4` arms `_DAT_8007B6B0 = 0x26` and two fades, counts it down without moving anything, and on the landing frame stores the pad hold `_DAT_8007B6B4 = 0x28`, re-rolls a spent encounter counter, seats the player and runs the landing tile's kind-1 record. The player tick `FUN_801D1344` drains the hold and skips the pad step while either word is live. From `s3_rimelm_freeroam` the landing comes `38` vsyncs after the crossing and the pad `40` after that ([`field-locomotion.md`](../subsystems/field-locomotion.md#retail-capture-of-the-warp)). |
| What does op `0x43`'s acquire do when the target is mid-arc? | resolved (waits and retries) | `disassembly` | The acquire (`0x801DF384..0x801DF40C`) fails on halt bit `0x400` while the scene word `*(_DAT_801C6EA4) + 8` is zero, and the failure leaves the PC on the op (`beqz` to `0x801DEE4C`, `move s8,s4`). That word is non-zero only while a spawn section is being pre-run (SCUS `0x8003B73C` / `0x8003B928`, `0x801E2820` / `0x801E2BBC`) ([`script-vm.md`](../subsystems/script-vm.md)). |
| Which `FUN_801CF8AC` arm do the ambient walkers take? | resolved (the class arm, all of them) | `disassembly` | `FUN_8003A1E4` ORs `0x20000` into every placement it seats (`0x8003A3A8..0x8003A3B4`), and `0x01000000` for a `>= 0xF0` party model, so the box test runs on `+0x10 & 0x01020000`; the no-class arm is for pool actors spawned elsewhere ([`motion-vm.md`](../subsystems/motion-vm.md)). |
| What does op `0x3E` do with `op0 < 100`? | resolved (the scripted-battle install, one body with `op0 = 0xFF`) | `disassembly` | `FUN_801DE840`'s arm reads `op0` twice - `beq 0xFF` at `0x801E06FC` and `sltiu 0x64` in its delay slot - and never again. Both sides force the player's cached region tile stale (`+0x8E` / `+0x8F = 0xFF`), call `FUN_801D9E1C(player, 0)`, skip on `_DAT_8007B868` or a missing `0xFB` context, set `sys[+0x8A] = 1`, point `sys[+0x94]` at MAN formation row `op1`, store the `FUN_801DDF48` reroll into `_DAT_8007B5FC` and request mode `0xE`. Ten clean non-`0xFF` sites (`town0b`, `stone`, `jagaroom`). Port `FieldHost::scripted_battle`; the region half is an open row ([`script-vm.md`](../subsystems/script-vm.md#0x3e-scripted-battle-op0--100)). |
| How many sites re-roll the encounter step counter? | resolved (five, into one word) | `disassembly` | `find-address-word-refs.py --prot` finds four `jal 0x801DDF48` and no other reference: SCUS `0x8003AC90` (`FUN_8003AB2C` adds half a roll while the counter is below 487), `0x801D1F6C` in `FUN_801D1EC4` (a full roll when the counter is `<= 0`), the op-`0x3E` arm `0x801E076C` and `4C EC` at `0x801E34F8`; `FUN_801D9E1C` carries an inlined copy (`0x801DA2D4..0x801DA35C`). All five store the word `_DAT_8007B5FC`, one counter across doors. `FUN_801DDF48` itself is two BIOS draws, `r1 % 487 - r2 % 487 + 0x3CE`. The port seats every site but `FUN_801D1EC4`'s. |
| Does P2[5] write `kor5`'s `0x436` organically? | resolved (yes) | `capture` (synthetic: two trigger-tile pokes, Gaza's HP held at `1`) | From `kor5_post_43a_checkpoint`, P1[0] spawns P2[5] on the battle's reload and it sets `0x436` at `+0xD0D` through `FUN_8003CE08` (`ra 0x801E3598`) 3,336 vsyncs after `0x464` clears; P2[8] then sets `0x6C4` on its first `(32, 86)` crossing (`kor5_post_436_organic`). The run predates `run_probe.sh` staging its disc, so it sits inside the open patched-disc audit. |
| Where does the op-`0x43` arc's chained record point, and what does op `0x34` sub-1 spawn? | resolved (a release watcher; an attached billboard) | `disassembly` | `FUN_801D25EC` allocates the arc actor from template `0x801F227C` (`0x801D2634`) and a second actor from `0x801F22AC` (`0x801D2760`), whose handler word is `FUN_801D5D60` - a watcher that clears the arm's halt bit `0x400` when the arc lands, not an emitter. Op `0x34` sub-1 calls `FUN_801E5668` at `0x801DFFE0`, which allocates from template `0x801F28B8` (handler `FUN_801E4470`) and copies the parent link, position and rect. |
| What is `FUN_801E58A8`? | resolved (an actor anim-clip pick) | `disassembly` | It writes `+0x5E = -2` and a clip index into `+0x5C` from `_DAT_8007BDD8`, the party leader `_DAT_8007B8F8` (stride 7) and the override `_DAT_8007B6AC`, then calls the clip selector `FUN_800204F8` (`0x801E58A8..0x801E59AC`). The same arithmetic is the tail of `FUN_801D1BA0` (`0x801D1D88..0x801D1EAC`). |
| When does `4C 86` install its reflection controller? | resolved (at scene entry, from the record's spawn prologue) | `disassembly` + `capture` | All ten shipped sites sit after the record's leading `0x25` and before its first `0x21` park, so no talk press is involved. A retail `conc` -> `conc2` crossing seats all three controllers on one frame, each call returning to `0x801E227C`. Over 132 in-rect tick pairs the image is `(x, y, 2*zz - z)` with facing `-0x800 - a`; 194 out-of-rect pairs leave it alone; the rect test quantises with `(v + 0x40) >> 7`. |
| Is `juui1` dark in retail outside its tint beats? | resolved (no) | `capture` (synthetic gate) | With `0x3E1` cleared and `0x3E5` set by RAM poke, the black run from vsync 1040 to about 1327 is the door fade - the departure push from `0x801DDE24`, then the arrival ramp from `0x80025034` - and after the last push the scene holds a dim purple vortex with the party visible, mean luma about 40 of 255. An organic save exists (the `rugi` block of the endgame card) but its route crosses the Rogue fight. |
| Which flags do the retock / doman / nilboa entries write? | resolved (per-scene entry families) | `capture` | Each from a pre-entry card-boot state, every write through the field VM's SET (`0x801E3598`) / CLEAR (`0x801E35C0`) arms: retock SETs `0x493`, `0x01F`, `0x52A` and clears `0x19B..0x1AA` and `0x527..0x52E`; doman SETs `0x49C`, `0x01F`, `0x6E7`; nilboa SETs `0x014`, `0x499`, `0x01F`, `0x52A`. retock's `0x502` is not an entry write - its one non-debug writer is spawned from Eliza's talk loop, which `0x33B` (set in `jagaroom`) unhides (`inference` from the card saves). |
| Why did a walk-on tile poke under the movement lock never fire? | resolved (a crossing made while locked is consumed) | `disassembly` + `capture` | `FUN_801D1EC4` stores the new tile into its last-tile mirror `0x8007BDC8` / `0x8007BDCC` on both failure branches - the cell test `cell & 0x600` at `0x801D2144` and the `+0x10 & 0x80000` lock at `0x801D214C..0x801D2158` - so a crossing made during the lock never fires later. The port's `dispatch_walk_on_trigger` returns before updating its mirror while a timeline runs, which defers the crossing retail discards. |
| Which records write the `kor5` tail, `doman` and `son` flags? | resolved (with `kor5`'s `0x436` input poked) | `capture` | `kor5`: P2[3] `0x43A` -> P2[4] `0x464` and battle 165 -> reload, P1[0] clears `0x464` and spawns P2[5] -> P2[8] writes `0x6C4` at `+0x75` (tile `(32,86)`, `C1 {0x6C4}`, `C2 {0x436}`). `doman`: P2[4] on tiles `(67,108..110)` writes `0x3FB` at `+0x98D`, organically from `korb2` across the world map by tile poke. `son`: the arrival seat fires P2[5] (`0x60D`); P2[3] `(18,85)` sets `0x3A6`, P2[4] `(24,76)` sets `0x3A7`; the entry clears `0x3A6..0x3A8` on every visit, so they are per-visit latches. |
| What are motion-VM ops `0x37` / `0x41`? | resolved (an eight-direction compass walk) | `disassembly` | Both step along the compass table at `0x80073F14`, not along one axis toward a target; `0x43` never completes, and target `0xF8` is the player, not the executing actor. |
| What do field-VM `4C E4` and `4C DB` do? | resolved (a box test with a relative skip; the CLUT blend fade) | `disassembly` | `4C E4` builds its box from tile corners `+0x20` / `+0x60` / `+0xA0` and takes a relative skip when the actor is outside (22 clean occurrences in three scenes). `4C DB` spawns from descriptor `0x801F2930` through `FUN_801E57F0` into the CLUT blend fade `FUN_801E4D8C`; jouine issues it four times. |
| What colour does retail clear the field to? | resolved (black) | `capture` | Uncovered pixels read `(0, 0, 0)` in five of five field-mode crops (mei_house_inside, keikoku_chest_pre, new_game_cutscene_intro_a, name_input_ui, v0_1_tetsu_dialogue_accept). |
| Does a scripted camera tile window survive into the next scene? | resolved (yes - by 78 vsyncs) | `capture` | A per-vsync poll across a real `map01` -> `town0c` door: the scene word flips at vsync `37` and the window at `0x1F8003E8..EB` keeps the previous scene's values until vsync `115`, when the entry stamps `(-7, -6, 5, 7)` and the per-region writes take over. So the port's re-stamp-per-entry is right and its stamped **value** is not - `FIELD_DEFAULT_VIEW_WINDOW` `(-8, -6, 6, 10)` is a later region's window. The same run settled a second thing: a Rim Elm **house door** is an intra-scene warp (`town0c` stays), so it is not a fixture for this question. [`encounter.md`](../formats/encounter.md#the-window-is-not-cleared-with-the-scene). |
| What is field-VM `4C 14`? | resolved (the **actor clone**, and it is eight bytes) | `disassembly` | The only eight-byte instruction in outer nibble 1: the nibble's prologue advances seven and the `0x14` arm at `0x801E0E80` reads a sixth payload byte and adds one more in a branch delay slot. That byte names a cross-context source actor; `FUN_801D835C` copies its position, rotation, bound model and `+0x68` onto a fresh pool node and writes the fade rate and modulation colour the two earlier operands carry. Ninety-four sites in six scenes, three rates; a seven-byte reading desyncs each record from its first occurrence. [`script-vm-menuctrl.md`](../subsystems/script-vm-menuctrl.md#0x4c-nibble-1-sub-4---the-actor-clone). |
| What do `4C 86` and `4C 87` do? | resolved (the reflection controller's install and teardown; neither parks) | `disassembly` | `4C 86` reads its **last** operand byte as a cross-context actor id and spawns from descriptor `0x801F2948`, handler `0x801E5154`, writing `+0x90 = executing ctx`, `+0x94 = resolved actor` and six `s16` into `+0x80..+0x8A`. The tick reads `+0x94` and writes `+0x90`, so the **named** actor is the source and the executing script is the image; the six words are the mirror line and tracking rect. `4C 87` retires every live one and advances two, as `4C 9F` does against another handler. [`script-vm-menuctrl.md`](../subsystems/script-vm-menuctrl.md#4c-86--4c-87-are-the-reflection-controllers-install-and-teardown). |
| Which model owns the field screen-effect fade? | resolved (the **push**; both of the port's representations were dead) | `capture` + `disassembly` | The measured beat is `FUN_80024EE4(kind, blend, packed)` once per step from the effect actor op `0x34` sub-0 spawns, with the global multiply tint `DAT_8007BCB8..BA` neutral on all 900 vsyncs - a `(kind, blend, packed)` triple, which is the push's shape. The arm is also a **pair**: a walk-out spawned from the previous target plus the walk-in, `blend = (op0 & 1) ? 2 : 1`, push kind `8` / `0` / `2` off the same sub-op byte, and an all-zero operand clears the live actor instead of ramping to black. [`cutscene.md`](../subsystems/cutscene.md#the-arm-is-a-pair-and-the-sub-op-byte-carries-both-selectors). |
| Why does a `juui1` name hijack draw black? | resolved (the tint belongs to the **departing** script) | `disassembly` | Of 498 clean op-`0x3F` doors, 446 are preceded by `34 05 FF FF FF 41 00` - a white `ColorIntensity` beat the leaving scene's own record runs as its door prologue. A hijack rewrites the destination name and never executes that prologue, which is why the hijacked frame is black and a hijack into a brightly-drawn control scene is equally black. The `conc` -> `conc2` -> `juui1` route is two ordinary doors (`conc` P2[11] `+0x001E`, `conc2` P2[20] `+0x0078`). |
| Do shipped scene scripts carry developer flag-setting menus? | resolved (nine scenes do) | `disassembly` | `map01`, `geremi`, `keikoku`, `suimon`, `town0b`, `town0c`, `doman`, `kor5` and `jou` ship records whose text is "Clear all flags", "Set all flags" / "Clear" / "Exit", "On" / "Off" / "Exit" or "=Back=", and whose arms are genuine `51` / `61` ops over nine-flag ladders. They are why a flag-writer census over-counts: the arms decode clean, so a beat and a menu row look identical to a scan that only reads opcodes. Classifier `man_field_scripts::debug_flag_menu_arm`; [`script-vm.md`](../subsystems/script-vm.md#shipped-scene-scripts-carry-developer-flag-setting-menus). |
| What gates the fishing catch HUD's depth and tension block? | resolved (one word, `DAT_801d91b4`) | `disassembly` | The catch HUD `FUN_801d1580` gates the depth readout and the tension bar on the single word set at the hook, and nothing else - not a phase, not a separate visibility flag. A host that adds an idle-phase gate of its own draws a different HUD from retail's on exactly the frames between the strike and the hook. [`minigame-fishing.md`](../subsystems/minigame-fishing.md). |
| Where does a field actor's heading live? | resolved (it is the **middle** of a rotation triple at `+0x24`) | `disassembly` | `FUN_8001ADA4` hands the whole vector to the rotation setter - `addiu a0,s0,0x24` / `jal 0x80026988` at `0x8001AF04`, and again at `0x8001B2A4` / `0x8001B2C8` / `0x8001B320` - so the actor carries pitch at `+0x24`, yaw at `+0x26` and roll at `+0x28`. A reader taking the yaw halfword alone sees a heading and loses the tilt, which is why per-actor pitch and roll were missing everywhere rather than in one renderer. |
| What is `_DAT_8007B854`? | resolved (the **ambient-particle master gate**, not an input lock) | `disassembly` | Field-VM op `0x4C` outer nibble 3 raises it and clears it, both stores in a `j` delay slot off the 16-entry table at `0x801CEEB8`: `0x801E0F38` is `sw v0,-0x47ac(v1)` with `v0 = 1` and `0x801E0F44` is `sw zero,-0x47ac(v0)`. Six references exist disc-wide and none is pad state - two SCUS clears, the field render pass at `0x80026EBC` which stages the particle table into scratchpad when the word is set and the game mode is 3, and the ambient emitter's own opening load at `0x801D605C`. A field script decides per scene whether ambience emits at all. |
| teien's hedge-base ground fill - does retail draw a kind-2 cell? | resolved (**no**; the question had a false premise) | `capture` | Retail ships no `0x0800`-cell draw channel. A live `teien` field-run pass visits 1536 window cells and emits 370 - exactly the cells carrying `0x1000` - and none of the 42 cells that are `0x0800`-only. Only 8 of 84 images reach `*(0x1F8003EC)` at all, only PROT 0900 / 0901 hold a per-cell pass, and each one's `andi 0x800` reads an **object record's** `+0x12`, not a cell. teien's `0x0800` cells are a 6x6 platform block, a ten-cell row and three strays - the same shape `edteien` has, which is why the proxy could not answer it either. Cell bit `0x8000` is a per-tile depth-sort flag (own minimum `SZ` against a fixed far bucket `(0x3FF6 >> ot_shift) * 4`). |
| Actor `+0x16` - heading, facing, or footing? | resolved (**Y**; three doc readings collapse into one) | `disassembly` | `+0x16` is the Y of the actor's `(+0x14, +0x16, +0x18)` position triple. Nothing on the disc masks it as an angle: 0 of 77 accesses in the field overlay 0897 are angle-masked, and the four masked accesses in SCUS are `actor[+0x96]`. `FUN_8003BC08`'s second arm is therefore a **ground-follow**: skip on `& 0x2`, authored `-actor[+0x8E]` on `& 0x20000000`, a global off-switch when `& 0x20200` is clear and `_DAT_8007B6A8 == 0`, an outright snap to the sampler when `& 0x2000` is clear, and otherwise a ramp clamped to `+-6 * DAT_1F800393` per frame. Detail on [`functions/game-modes.md`](functions/game-modes.md#8003bc08-ground-follow). |
| Which index does the fishing bring-up rewrite? | resolved (the **rod** index, not the lure) | `disassembly` | `FUN_801CF070`'s tail (`0x801CF35C..0x801CF39C`) probes the bag for item `0xA0 + _DAT_80084454` through `FUN_80042F4C`, steps the persistent rod index on a miss, wraps at 3 and gives up after six probes by storing `0` - so a rodless player fishes with rod `0`. The lure gate is a different routine over a different band: `FUN_801D712C` walks items `0x9D..0x9F` and rewrites `_DAT_80084450`. The same entry also seeds the 16-rung floor-height ladder at `0x1F80035C` and carries a dev grant of `999999` fishing points behind `_DAT_8007B9B0`. See [`minigame-fishing.md`](../subsystems/minigame-fishing.md). |
| What writes a scene's kind-2 (height-override) trigger cells at runtime? | resolved (exactly one writer) | `disassembly` | Field-VM op `0x4C` sub `0x83` at `0x801E20A8` - a rectangle re-floor through `FUN_801D5630(2, x, z)` with the coarse step in `op[5]`. Trigger kinds 0 and 3 have no writer at all, and the `.PCH` sidecar's `+N` fixup words have none either: all 97 on-disc tables carry zero there. The per-kind record strides are `gp[0..3]` = `4, 4, 4, 8`, read by `FUN_801D5AE0`. See [`script-vm-menuctrl.md`](../subsystems/script-vm-menuctrl.md). |
| The ending-scene widget family - how many sites, in how many scenes? | resolved (311 sites, **ten** scenes) | `disassembly` | The op-`0x43` sub-op is `InsnInfo::ActorCtrl`, not `Insn::extended` (that field is a cross-context target marker, `0x80`). Counted properly the family has 311 real sites across `edteien`, `edbylon`, `edbalden`, `edlast`, `edretoin`, `edkorout`, `edson`, `edstati3`, `edbubu` and `eddoman` - the docs listed eight, omitting the last two. |
| `FUN_801D6058` - a cutscene element? | resolved (a field-overlay template, spawned once) | `disassembly` | It is the `+0x08` handler of the plain template at descriptor `0x801F271C`, spawned exactly once by the field MAIN INIT `FUN_801D6704` (`addiu a1,0x271c` at `0x801D6FC0`, `jal 0x80024c88` at `0x801D6FD8`, `sh s0,0x1a(v0)` seeding the `+0x1A = 1` scene arm), and gated on `_DAT_8007B8B8 == 0`. Nothing about it is cutscene-specific. |
| Rim Elm's south gate - why a seated player exits and a walking one does not | resolved | `disassembly` | Neither walk-on band was the mechanism: the exit record is ungated, the other is five inert bytes, and the wall is a collision row the gate object's own script paints. [details ↓](#rim-elms-south-gate) |
| Town/field free-movement locomotion | resolved | `capture` | [details ↓](#townfield-free-movement-locomotion) |
| Field ambient animation - what makes jou's ground pulse and the water shimmer | resolved | `disassembly` | Three mechanisms: the bundle type-6 CLUT-walk table (12 carriers, 9 of them field scenes), the ambient move-VM tree the MAN P1 placements install at entry, and jou's flesh pulse = the mode-3 CLUT-cell HSV cycler (`FUN_80019D50`, lightning = flag `0x364`). Full chain + two move-VM decode corrections: [`field-ambient-fx.md`](../subsystems/field-ambient-fx.md). |
| Ambient render-mode 4 - what the op-`0x1E` seat animates | resolved | `disassembly` | A **cyclic VRAM-rect scroller**, and it is what makes waterfalls fall. Per fired period (`+0xC6` drained by the frame step alone, no speed scalar) the render tail rotates the seated rect `+0xD0..+0xD6` left by `+0xCC * frame_step` and up by `+0xCE * frame_step`, each axis as StoreImage / MoveImage / LoadImage over a bump-allocated strip (`80021df4.txt` `0x80022CB8..0x80022EE0`). Seventeen scenes carry one at plain entry; sixteen scroll upward over a texture-band rect, `tunnelc`'s second seat scrolls a CLUT row sideways. Ported as `engine-core::world::ambient::vram_scroll`. [details ↓](#ambient-render-mode-4---the-vram-rect-scroller) |
| Master ambient record 0 - what reads the 8-byte rows | resolved (it is not a stager record at all) | `disassembly` | The **per-scene sound-effect descriptor bank** for cue ids `>= 0x200`. Both SFX readers resolve those ids as `*(u32*)0x8007B8D0 + offsets[0] + (id - 0x200)*8` - i.e. record 0 of whatever bundle is installed at `0x8007B8D0`, which in field mode is the scene prescript bundle (`field_asset_loader` `0x8001F850..0x8001F864` stores scene buffer + `0x12800`). `offsets[0]` is the identical word `FUN_800252EC` reads for stager id 0. Rows are the 8-byte descriptor of [`sfx-table.md`](../formats/sfx-table.md), category 3 (a variable VAB slot). [details ↓](#master-ambient-record-0---the-per-scene-sfx-descriptor-bank) |
| town0e's morph-record installer | resolved (the census was shape-blind, not the disc) | `disassembly` | The install is the ordinary `0x34` sub-3 arg 0 (stager record 1) - it just rides **partition-1 placement 29**, a full placed actor with dialogue, as that record's second instruction, ahead of its `SysFlag.Test 0x1A` park/seat branch. A census that discriminates by script *shape* reports nothing here; the rule that finds it is the entry-slice one below. Residual, narrowed: the pre-run loop (`0x8003B8BC..0x8003B8EC`) covers every placement `1..count-1` unfiltered; the one unread step is bit 2 of the scene's load word at `0x801D6D98` in `FUN_801D6704` - `inference` until read. [details](../subsystems/field-ambient-fx.md#town0es-installer-is-a-placed-actor-not-an-effect-actor) |
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
| What transitions retail into game over? | resolved | `capture` + `disassembly` | Retail has **no** mode-`0x12` transition. A wipe exits battle to mode 2; MAIN INIT `FUN_8003AEB0`'s back-from-battle arm stores `game_mode = 0x16` (CARD INIT) + `_DAT_8007BB00 = 1` at `0x8003B5D4`, landing on the **title screen** - no GAME OVER art, no menu. Three more sites carry the identical pair (`FUN_8003C7EC`, `FUN_801D84B4`, the STR attract exit `0x801CF048`). Mode 18/19 + PROT 0902 are an unreachable dev harness. The port's three-row panel is **deleted**; both hosts hold, draw nothing, hand to the title. [details](../subsystems/battle.md#party-wipe--the-game-over-overlay) |
| Mid-visit NPC re-arrangement beats (dolk2 market crowd; garmel pre-Zeto staging) | resolved | `disassembly` + `capture` | dolk2: the swap is `P2[11]`, spawned by the `.MAP` fallback walk-on-trigger rows (C1=[`0x27C`], C2=[`0x142`]) - eight `CC <crowd> E3 <day>` seats (op `4C` nE sub-3, `0x801E3108`) put P1[53..60] on the day cohort's tiles and `A3` parks the day cohort at `(127,127)`. garmel: the Zeto stager `P2[12]` materializes P1[3]/P1[4] beside the player (n3 sub-7 player-coord copy `0x801E0FB0`); post-battle re-entries run `P1[0]`'s flag-consume arms. See [script-vm.md](../subsystems/script-vm.md#mid-visit-npc-re-arrangement-beats-dolk2-market-swap--garmel-boss-staging); pinned by `engine-core/tests/man_midvisit_rearrangement_disc.rs`. |
| Region story-flag gate families (record-header C1/C2 gates) | resolved as structure (play-order residual on the open page) | `capture` | [details ↓](#region-story-flag-gate-families) |
| Extraction-0874 §2 (`player.lzs`) F-variant pixels | resolved - installing event named | `capture` + `disassembly` | [details ↓](#extraction-0874-2-playerlzs-f-variant-pixels---a-one-shot-opening-face-frame-stamp-not-a-menu-writer) |
| Which chapter-1 scenes the engine can load, script, walk and leave | resolved as a per-scene verdict; four of the five late "one-way" rooms now leave in-engine | `disassembly` + `capture` | [details ↓](#chapter-1-scene-frontier) |
| How a player leaves the Uru Mais chain (`uru`, `urudre1..3`) and `jouine` | resolved (all five have walk-on exits carried by the scene's `.PCH` trigger sidecar) | `disassembly` + `capture` | [details ↓](#the-uru-mais-chain-and-jouine-exits) |
| Why did the port drop roughly half the disc's `0x3F` destinations? | resolved (the clean-label gate was lower-case-only; retail never compares the operand against a name table) | `disassembly` | [details ↓](#the-upper-case-destination-fold) |
| Why do `bubu1` and `edbubu` resolve no MAN? | resolved (both ship a `count = 5` asset table; the detector bounded `count` to 6 or 7) | `disassembly` + `capture` | [details ↓](#the-count-5-asset-tables) |
| Why did 28 scenes of the frontier closure become unwalkable at once? | resolved (a scene change mid-ledge-hop leaked the hop's one-way steering lock - an engine bug, fixed) | `disassembly` + `capture` | [details ↓](#the-ledge-hop-lock-leak) |
| What consumes the scratchpad window `0x1F8003E8..EB`? | resolved (the renderer's visible tile window; the `0x801F2778..84` mirrors are write-only) | `disassembly` | Four signed bytes `[nearX, nearZ, farX, farZ]`, tile offsets from the camera tile, written by the camera-zone loader `FUN_801DBC20` and field-VM op `0x46`. Read by the render library's cell emitters (`FUN_801F7088` at `0x801F7434..0x801F746C` and siblings), the camera scroll clamp `FUN_801DAA50`, the ambient emitter `FUN_801D6058` and dev-menu rows `0x12..0x15`. Invisible to the word scan: every access is `lui 0x1F80; ori 0x314; lb 0xD4(rX)`. Details: [`encounter.md`](../formats/encounter.md#the-scratchpad-window-0x1f8003e8eb). |
| What is the field run-button mask `0x800846DC`? | resolved (`0x48` = Cross \| R1, seeded once by the new-game data init; not configurable) | `disassembly` | The last of four button-mask words at `0x80084140 + 0x590..0x59C` (`0x44`, `0x21`, `0x10`, `0x48`), stored by `FUN_80034A6C` (`0x80034AA0..0x80034AB8`) and read by the field mover at `0x801D0364` against the held mask `_DAT_8007B850`. No other writer in SCUS or any overlay; it sits inside the saved block. The engine latches run off Square - a divergence disclosed in [`field-locomotion.md`](../subsystems/field-locomotion.md). |
| Who latches the clip-end bit for a conversation's cross-context clip pokes | resolved (port residual named) | `disassembly` + `capture` | The **poked actor's own anim tick**, on the poked actor's own `+0x62`. `FUN_8003C83C` short-circuits target `0xF8` to the live player object out of `_DAT_8007C364` before its actor-list walk, so an NPC record's `A2 F8 <clip>` / `AC F8 08` / `AD F8 08` reads and writes the *player's* clip words. [details ↓](#clip-end-latch-for-cross-context-clip-pokes) |
| What is the `scene_asset_table` header's `+0x04` word? | resolved (sum of the descriptors' decompressed sizes; never read) | `disassembly` | Equals `Σ descriptor.size` in all 105 containers of the family on the disc and exceeds the carrying entry in every one; `FUN_80020224` reads `+0x00` (`lw s3,0x0(s4)` at `0x80020288`) and steps descriptors from `+0x08`, and a corpus sweep for loads off `*(0x8007B85C)` finds offset `0` only. |
| How does a scene bundle reach `_DAT_8007B85C`? | resolved (whole-sector block copy) | `disassembly` | `FUN_8003D26C(*(0x8007B85C), *(0x8007B8C4), sectors << 6)` at `0x801D6918` (32 B per iteration = `sectors * 0x800`), into the `0x62C00` arena `FUN_8001E1B4` allocates at `0x8001E28C`. |
| What consumes the "field-pack" entries? | resolved (the scene texture pack at block `+4`) | `disassembly` | `FUN_800255B8` builds the path by mode (`tim.dat` `0x0A`, `move.mdt` `0x0F`, `<scene>.pac` `0x14`) and loads into `*(0x8007B85C)`; `FUN_8002541C` walks a bare pack into `FUN_800198E0` (mode `0x0A`) or the chunk chain into `FUN_8001F05C` (mode `0x14`). Format dissolved - see [`field-pack.md`](../formats/field-pack.md). |
| What do the asset-table `Flag(0x0A / 0x0F / 0x14)` descriptors do? | resolved (they are the streamed-file loader's mode argument) | `disassembly` | `FUN_8001F05C` returns `(case << 8)` for those types (`0x8001F574` / `0x8001F60C` / `0x8001F658`), `FUN_80020224` ORs the returns, and `0x801D6BF8` shifts right by 8 before `jal FUN_8002541C`. Corpus: 28 blocks `Flag(0x14)` carry a DATA_FIELD stream at `+4`, 4 `Flag(0x0A)` a bare pack, 64 without a flag a pochi filler. |
| Which screen opens menu window 46 (`FUN_801D603C`)? | resolved (the casino prize counter's Yes / No confirm) | `disassembly` | Script `0x801E4F2C` = `01 2E 00 00` (byte-verified in PROT 0899's widget-script pool), handed to `FUN_801D6628` by `FUN_801DC1CC` at `0x801DC408` / `0x801DC41C` - index `0x20` of the sub-screen pointer table at `0x801E4F40`, selected on entry-context kind 7 at `0x801DC8CC` - staging `_DAT_801E46D0` at `0x801DC414`, the state word the painter's marker decode reads. The `04 2E` closer at `0x801E4F38` has no reference in any image. |
| Which prescript copy does `FUN_800252EC` install from - the sister entry or the `.PCH` `+0x800` copy? | resolved (the next PROT entry, through the `efect.dat` window; there is no split) | `disassembly` | `FUN_800252EC` reads its `[count][offsets]` table from `_DAT_8007B8D0` (`0x800252F4`), which the field asset loader sets to `*(0x1F8003EC) + 0x12800` at `0x8001F864` - one sector past the `.PCH`, so the `.PCH` window at `+0x12000` is never the source. Writers of `gp+0x5B8`: `0x8001F864`, `0x8001FAC8` (the `bse.dat` battle buffer), `0x801CF018` (0975), `0x801CEEFC` (0977). |
| What does the `0x801F2858` template's tick do? | resolved (the scene **shutter blackout**, `FUN_801DD784` `0x801DD784..0x801DD9B4`) | `disassembly` | Two full-width bars ease in from the top and bottom and **meet**, holding the screen black - a scene-change shutter, not a cinematic letterbox ([falsified](re-do-not-re-walk.md#field--locomotion)). Its one spawner is `FUN_801DE754`, from field-VM op `43 0C`; `FUN_801CFF3C` is the same routine printed `0xE818` low. |
| What is field-VM op `0x4C` nibble 9? | resolved (the scene floor-height ladder) | `disassembly` | Sub-`0xE` installs all sixteen rungs (`-words[i]` into `0x1F80035C + i*2`), subs `0..2` set one rung oscillating through `FUN_801DDE34` -> `FUN_801DA930`, and sub-`0xF` retires every oscillator. Not a fade family: the destination is the elevation LUT `FUN_80019278` and `FUN_8003A55C` read. See [`script-vm-menuctrl.md`](../subsystems/script-vm-menuctrl.md#nibble-9-is-the-floor-height-ladder-not-a-fade). |
| What is `0x801D44CC` in the dance overlay? | resolved (the step-marker mesh flipbook) | `disassembly` | It selects the marker actor's mesh row from its `+0x50`, the `clip - 6` value the floor pass stamps at spawn - it flips the marker's picture and does not face a dancer. See [`minigame-dance.md`](../subsystems/minigame-dance.md#the-sprite-part-emit-dispatch). |
| Who calls the move-VM extension dispatcher `FUN_801D362C`, and is the port live? | resolved (one caller disc-wide, and the port is reached) | `disassembly` | The only reference of any form is SCUS `0x80023AE0`, the move VM's own op-`0x2F` arm; the world-map controller does not call it directly. The port is live through `MoveHost::ext_dispatch`. See [`move-vm-overlay-ext.md`](../subsystems/move-vm-overlay-ext.md#one-caller-and-it-is-ported). |
| How does the shop's buy list order and ink its rows? | resolved (a three-row hoist, with a last-rule-wins dim) | `disassembly` | Case `0x0B` splits the walked rows at `record_count - 3`: rows below the split stage into `0x801C6220` tagged `0x3000` and are appended **after** the last three, which go straight into the row buffer tagged `0xA000` and are inked 5 by the kind-4 kernel - a highlighted "new in this town" strip at the top. A row dims when `purse < price` **or** `held >= 99`. See [`shop.md`](../subsystems/shop.md#the-last-rows-come-first). |
| Where do the tile-board's cells live? | resolved (heap, one byte per cell, allocated at install) | `disassembly` | `DAT_801F35C0 = FUN_80017888(0, width * height)` at `0x801EF3E8..0x801EF3F4` - the one writer - with `width` / `height` re-read from `_DAT_8007B450[3]` / `[4]`. Nothing pads it. The walk SM's teardown state `0xE` frees it with the tile-actor table `DAT_801F35BC` through `FUN_80017B94` (`jal` at `0x801EFE78` / `0x801EFE88`); the "nothing frees it" this row once carried had read only the per-scene control-block reset. `FUN_80017888` is the logging wrapper over the best-fit allocator `FUN_8002B468`. See [`tile-board.md`](../subsystems/tile-board.md#where-the-board-comes-from). |
| What are the menu's HP / MP ink thresholds? | resolved (`FUN_800349EC` / `FUN_80035EA8`, with a fixed ailment arm order) | `disassembly` | The two routines carry the tier tests and the ailment arms are evaluated in a fixed order, so a readout's colour is decided by the first arm that matches rather than by a priority table. Rows in [`functions/menus.md`](functions/menus.md); law in [`field-menu.md`](../subsystems/field-menu.md#hp--mp-health-tier-inks). |
| Who clears the `-1` menu entry-context park? | resolved (`FUN_800353E0` and `FUN_8003540C`, two teardown leaves) | `disassembly` | Both zero `gp+0x148` and `gp+0x138`, which is the whole of the park release; the port is `World::release_menu_entry_context_park`. |
| What keys the menu entry-context byte? | resolved (the record **kind**, not the screen's position) | `disassembly` | `0x00` shop -> `0x1A`, `0x01` save -> `0x19`, `0x07` casino -> `0x20`, `0x0D` -> `0x04`, and the sentinel `1` -> `0x02`, the debug character-parameter editor. Reading the byte by position is what put the save entry on `0x02` ([falsified](re-do-not-re-walk.md#menus--ui)). |
| What arms `_DAT_8007B8B8`? | resolved (a one-shot entry-mode **argument**, not a latch) | `disassembly` | Ten writers and 25 readers over 84 images. One writer is `0x80016414` inside `FUN_80016230`, the mode-transition pass; two more are `0x80026094` and `0x80046E28` (the latter only when the word is already non-zero) and one is `0x801CEF18`; six sites write zero. Nothing retains it across a load, so the field MAIN INIT's `0x801F271C` spawn is **per-scene**, not boot-only. Its neighbours `_DAT_8007BACC` and `_DAT_8007B76C` have no writer at all, which makes the recentre-window form they gate unreachable. |
| Which way round is actor `+0x8A` bit 0? | resolved (it **suppresses** the scripted motion VM) | `disassembly` | `beq` at `0x80038194`: a zero byte runs the bytecode. The bit gates the player-engaged / actor-busy / off-map early returns at `0x8003819C..F4`, so setting it stops an actor rather than starting one. Op `0x12` waits for the bit to *change*, seeded `1`/`2` at `0x800396B4`, and consumes its tick unconditionally. |
| What are motion ops `0x06`, `0x0C` and `0x0E`? | resolved (a home-relative wander, a tint/draw-mode fade, and a two-base model bind) | `disassembly` | `0x06` draws `rand() & 6` and steps inside a box of signed 7-bit tile deltas taken from the actor's home at `+0x8C` / `+0x8D` - it never leaves one tile of where it was placed, and it is not a pad echo. `0x0C` fades `+0x74` (packed RGB tint, scheduler kind 3) and `+0x78` (draw mode), not a position channel. `0x0E` splits unsigned at `0xF0` between the scene model base `0x8007B6F8` and the second base `0x8007B824`. |
| How do motion ops `0x10` / `0x11` / `0x12` address their target? | resolved (one selector over five halfwords) | `disassembly` | The candidates are `+0x10`, `+0x12`, `+0x62`, `0x1F800394` and `0x1F800396`; byte 1's `0x30` field picks the low half, and `b1 & 0xC0 == 0xC0` asserts `0x3039` at `0x8007B828` and then dereferences null. Ops `0x02`, `0x0A` and `0x0B` are gated on the `0x801C6470` record's `0x8C` unset sentinel; op `0x09`'s callee `FUN_80035B50` is an SFX-cue enqueue; op `0x14` writes `+0x72`, the render scale `FUN_8001B964` reads. The table's slots `0x1A..0x1F` point at the loop test itself, so 26 op bytes cover 24 bodies. |
| How does the field follow camera get its pose? | resolved (record -> parameter block -> compose -> ease or snap) | `disassembly` + `capture` | `FUN_801DBC20` splits one MAN section-3 camera-region record into the parameter block at `0x8007B607..0x8007B627`; `FUN_801DAB90` turns that block, the player's position, the floor height from `FUN_80019278` and the scratchpad attribute box into a staging descriptor at `0x801F3580`; and `FUN_801DB510` walks the six-entry list at `0x801F2798` toward it by `delta >> shift` plus the sign, on frames the player moved. `FUN_801DB8EC` is the same walk as a copy. Over the nineteen walkable states the composed pose is exact in all eight settled free-roam ones. [details](#the-field-follow-cameras-pose-chain) |
| Who runs the camera zone query in retail? | resolved (the field VM, not the arrival actor) | `disassembly` | `FUN_801DBE9C` queries only on its `_DAT_8007B868 != 0` leg, and that word is the dev/dual-mode gate - zero in retail, with `FUN_80034A6C` storing `DAT_8007B606 = (B868 == 0)`. Retail's query is `FUN_801DE3E0(tile_x, tile_z)`, reached from field-VM arms `[4C 38]`, `[4C 39]` and `[4C C4 x z]` plus the op-`0x45` LOAD, and it installs a fixed miss set when no record covers the tile. |
| What draws field fog, and what raises it? | resolved (a pool of textured sheets, gated by one script word) | `disassembly` + `capture` | `FUN_801D629C` is a **spawner**: it maps the player's tile to a MAN section-4 region record (`[enable][x0][z0][x1][z1][angle][spread][speed][unread][u16 flag]`) and pops one of eighty `0x18`-byte records from the pool at `_DAT_8007B7E0`. The draw is SCUS - `FUN_8003F348` walks the pool, `FUN_8003F3FC` updates and emits, and `FUN_8003F86C` lays a ten-word `POLY_FT4` (code `0x2E`, texture page `0x27`, CLUT `0x7640`, OT `SZ2 >> 5`) per sheet. The emitter's width jitter `FUN_8003F838` is dead: the caller seeds the value with the rate first. The master gate is `_DAT_8007B854`, raised at 128 sites across 70 of 124 scenes, every raise in a P1 record. |
| What composes the field camera's `TR`? | resolved (the live eye trio **is** the eye-space translation; there is no eye-back depth constant) | `capture` + `disassembly` | `FUN_800172C0` builds the field view from the live globals, never from the composer's staging descriptor: `FUN_8005B4B8` copies the eye trio `0x800840B8/BC/C0` into a scratch matrix's `t` as three 32-bit words, `FUN_8003D344` MVMVAs the focus through the scaled rotation with `cv = TR` and writes `MAC1..3` back over that `t`, and `FUN_8005B6A8` uploads it. The focus is the **low signed halfwords** of `0x80089118/1C/20`. Held on every sampled frame of three field states. A `1x` renderer divides the trio by the base matrix's scale. [details](../subsystems/renderer.md#the-field-view-matrix-where-tr-comes-from) |
| Retail's field camera focus Y | resolved (`0`, on every sampled frame) | `capture` | Only X and Z of the focus trio are ever written in the field - `FUN_801DBE9C`'s retail leg and the focus clamp `FUN_801DAA50` write that pair and nothing else - so `0x8008911C` reads `0` on 19 of 19 library states while the player's footing on those frames does not. The composer's **staging** focus at `+0x1A/+0x1E/+0x22` does carry the footing, and the view build never reads the staging descriptor. Vertical framing therefore rides the composed eye Y. |
| Why the port's field framing sat low against retail | resolved (entirely the focus-Y term) | `capture` | Over the states whose pitch, yaw and `H` match, mean horizontal error is 0.1 px of 320 and mean vertical error 15.8 px of 240, always low - and it tracks the anchor exactly: 0.0 px at footing `0` (five states, IoU >= 0.996), +12.1 at `-24`, +31.3 at `-128`, +33.5..+38.9 at `-192`, with a pixel cross-correlation on `town01` of +35 against an analytic +33.5. Both sides run the same projection kernel, and the composed eye trio is exact on seven of nineteen states with the four settled misses all mid-ease, so neither was the gap. |
| The two scene-entry eye-trio writers - which runs last? | resolved (they do not race, and the first field frame reads neither) | `capture` | Field MAIN INIT `FUN_801D6704` calls `FUN_80025C24` at `0x801D698C` and then `FUN_8003AEB0` at `0x801D6DA8`, which reaches `FUN_801DE37C` at `0x8003B01C`; A before B on 3 of 3 entries, so B's `(0, 0x200, 0x4000)` stands. It does not matter either way - the follow composer replaces the trio within one vsync, and `town0c`'s entry value equals the door-warp library state's trio exactly. |
| Which sites re-query the camera zone? | resolved (seven `jal` sites plus one gated per-frame site; a bare tile crossing is **not** one) | `disassembly` | `FUN_801DE3E0` (query + load in one) is reached from the three nibble-3 arms, `[4C C4]`, the player seat / warp path at `0x801D1FE8..0x801D2014`, its sibling at `0x801D2BCC` and the SCUS field init at `0x8003B800`. The field per-frame controller has one more, at `0x801D17FC`, gated on scratchpad flag bit `22` - a bit the per-mode seed cannot set, because the seed copies a `u16`, so only a script raises it, in eight scenes (a count of fifteen matched the masked flag bit inside desynced records). The re-query rule is disc data, not engine policy. [details](../subsystems/script-vm-menuctrl.md#0x4c-nibble-0x380x3e---the-camera-zone-arms) |
| Is the port's camera-relative pad remap retail's? | resolved (exact over the whole input space) | `capture` | Eight camera octants times eight held directions, latched from the same call of `FUN_800467E8` on a live `town01` field state: 64 of 64 cells agree with `World::remap_pad_direction`, including the ring's not-found case, and the ring `DAT_800766FC` carries the same eight **values** in `SCUS_942.54`, in RAM and in the port's constant - not the same bytes: retail's ring is `u32[8]` and the port's `[u16; 8]`. Retail publishes no ring step of its own to read back - `gp+0x2D8` is scene-authored (the free-roam state holds `0`), so the probe supplies the index and says so. Probe `scripts/pcsx-redux/autorun_field_pad_ring.lua`. |
| Is retail's actor facing the engine's heading plus a half turn? | resolved (`player+0x26 == render_26 + 0x800`) | `capture` | 61 of the same 64 cells agree exactly; the three misses are the sweep's three 1536-unit turns, each 512 short at the cell's last held vsync because the actor eases toward the new facing. |
| Does a zero field focus Y frame retail's field? | resolved (yes - it was the whole vertical error) | `capture` | With the port's focus anchored on the footing, the eight library states whose camera words match retail's framed 15.8 px of 240 low on average and exact only where the footing was zero; with focus Y zeroed the mean falls to 2.2 px, the four states with exact camera words become pixel-exact (player-box IoU 1.000), and a pure-shift pixel fit on `town01` agrees to 0.3 px. Oracle `crates/engine-shell/tests/field_camera_zone_oracle.rs` with `LEGAIA_CAMERA_ORACLE_DUMP`. |
| What is the bag-normalize gate's base register? | resolved (`0x80084140`, the live game-state block) | `disassembly` | `lui v0,0x8008` at `0x801E0580` and `addiu s0,v0,0x4140` at `0x801E0584` sit immediately above the `lbu 0x454(s0) == 2` / `lhu 0x458(s0) == 0x100` pair at `0x801E05B0..C8` that guards the sole `jal 0x800423E0` (`0x801E05D0`, PROT 0897). `FUN_800423E0` itself opens with the active-window setter `FUN_8004313C` and walks the bag over `gp[0x2D2]..gp[0x2D4]` skipping zero ids. |
| Is the resident scene-name string an input to the loader? | resolved (no - it is the loader's output) | `capture` | Overwriting `0x8007050C` leaves it reading the replacement for the rest of the run while the game loads the original scene anyway; a capture that reads a scene name out of RAM is reading what the loader said, not what it will do. |
| What does `edbylon` select when the tile query misses? | resolved (nothing selects - the block is **held** from an earlier tile) | `disassembly` + `capture` | The ending-vignette state stands on tile `(94, 43)`, outside the attribute box `[77, 34, 104, 50]`, and all three of the scene's section-3 records are kind `0` with anchors outside it, so no record covers the tile and 12,655 of 16,384 tiles select record `#0` anyway. The scene carries exactly one `[4C 38]` site, and with no per-frame re-query the parameter block simply survives from wherever the player last crossed a queried tile. There is no alternative selection rule to find. |
| Is the Throw Out cursor a bag slot or a list row? | resolved (a **bag slot**, and the list hides empty slots) | `capture` + `disassembly` | `_DAT_8007BB88` is the list kernel's selected-row payload and the pause menu's Throw Out confirm `FUN_801D8734` zeroes `bag[cursor * 2]` with it directly. A driven capture over a bag holed at slots 1/3/6 puts the cursor on 0, 2, 4, 5, 7, 8 and never on a zero-id slot across 561 vsyncs, so the displayed list is compacted while the payload is not. The bag itself is **not** compacted on menu open: `FUN_800423E0` runs zero times, and its sole reference is a field-VM arm at `0x801E05D0` gated on two words of the live block. A port removing by display row throws the wrong stack away on a holed bag. |
| Is `gp+0x2D8` (the pad-ring octant) camera-derived? | resolved (**authored** - it is scene content) | `disassembly` | Six writers disc-wide, all field-overlay: op `0x4C` nibble `2`'s arm `0x801E0EB8` storing `sub_op & 7`, the tile-board walker's four (`0x801EF8B0` / `0x801EF8B8` / `0x801EF8CC` / `0x801EFE7C`), and a clear at `0x801E5664`. Two of its four readers are in `SCUS_942.54`: the pad remapper loads the word at `0x800467E8` and `0x80046840`, which is what makes the octant a rotation at all. The arm also turns the player with it, so an author picks the octant to match the camera the same script installs. A free-orbiting port has no authored index to read and must derive one. [details](../subsystems/field-locomotion.md#gp0x2d8-is-authored-not-computed) |
| Who reads `0455_urudre1`'s descriptor 0? | resolved (nobody - it is a switched-off duplicate) | `disassembly` | The descriptor's type byte is `0x0A`, a pure-flag arm the walk never dereferences, and its payload is SHA-256-identical to the first 343,480 bytes of the live copy the scene actually loads. It is an authoring leftover that happens to hold a valid pack, not a slot with an unfound consumer. The three towns' reserved descriptor-0 payloads hash alike for the same reason. |
| The `0x4C` outer dispatch table, and its two non-handler arms | resolved (sixteen entries at `0x801CEE60`; nibbles `B` and `F` are the error printer) | `disassembly` | The outer table sits at `0x801CEE60`, materialised by a `lui`+`addiu` pair and indexed by `op0 >> 4` (`srl v1,s3,4` at `0x801E0C44`, `sltiu v0,v1,0x10` at `0x801E0C48`). Arm `0x801E3550` (nibble `B`) and arm `0x801E3538` (nibble `F`) each materialise their own string pointer with a `lui`+`addiu` pair and converge on retail's message printer at `0x801E3558` (`jal 0x8001A068`), so the two share a tail rather than a body; only `4C FF` branches away first, to the ordinary continue at `0x801DF098` (`beq` at `0x801E3540`). The disc agrees - the opcode census finds no coherent `4C Bx` or `4C Fx` in any scene. |
| What the field state machine's slot 7 holds | resolved (the submode **return** state, not a mode of its own) | `disassembly` | The enter half installs it at `0x801F140C` and parks it in `scene[+0x40]` at `0x801F148C`, after which `+0x50` is overwritten with the op-`0x49` sub-op's own slot. So slot 7 is where the field returns *to* when a submode ends, which is why a port that collapses the chain and keeps no `scene[+0x40]` has nothing to return to. |
| What does a field submode return to? | resolved (nothing reads the parked word) | `disassembly` + `capture` | The op-`0x49` enter parks a state twice through the scene pointer `0x801C6EA4` - `scene[+0x2E] = -1`, then `scene[+0x40] = s4[+0x50]` at `0x801F1400` and again at `0x801F148C`; the pair idiom tiles PROT 0897 **22** times. Nothing reads `+0x40`: `SCUS_942.54` and all 86 extracted overlay images yield zero loads at that displacement off a register loaded from `0x801C6EA4`, and a two-byte read watch records none while the pointer still names the scene struct. Later hits are the GPU working buffer the block is recycled into (`0x80043F74`, `0x800455E8`, `0x80044054`, `0x80059DE4`). [details](../subsystems/field-locomotion.md#the-submode-return-state---a-parked-word-nothing-reads) |
| Does the field passive-ability badge column have a suppress gate of its own? | resolved (no - it sits inside the party readout's) | `disassembly` | `FUN_801d095c` has exactly one caller, the `jal` at `0x801D130C` inside `FUN_801D0D38`, eight bytes above the epilogue that routine's suppress arm jumps to - so the badges carry the readout's suppression exactly and none of their own. The gate is the readout's entry block `0x801D0D38..0x801D0DBC`, four terms: `_DAT_8007B868`, `_DAT_800845C4 == 2`, `_DAT_8007B850 & 0xF000` and scratchpad `0x1F800394 & 0x00800000`. A host gating the two columns on different predicates draws one of them over dialogs, cutscenes and battles. |
| What do the fishing bite tick's two per-frame map reads do? | resolved (one gates water, the other drifts the lure) | `disassembly` | Both take the same point - the tick's own actor `+0x14` / `+0x18`, the lure the cast spawned - and are otherwise unrelated. The water gate is bit `0x4000` of the `+0x8000` cell halfword (`andi v1,v1,0x4000` at `0x801D3374`), whose class word `FUN_800180EC` rebuilds at `0x801D3384`. `FUN_801D7030`'s `+0x4000` walk-grid probe feeds none of that: a hit drifts the lure's 24.8 `x` accumulator `0x801D9174` by `frame_delta << 11`, signed by the low bit of the lifetime cast counter `_DAT_80084460`. [details](../subsystems/minigame-fishing.md#the-lure-the-bite-tick-probes) |
| Which screen opens window 25, and which opens window 41? | resolved (different screens; no program opens both) | `disassembly` | The menu image holds exactly one open-op (`01 <win> 00 00`) for each window: `0x801E4DDC`, inside the Equip screen's candidate script `0x801E4DC8` (sub-screen `0x14`, beside window 24), and `0x801E4E7C` inside the shop-entry script `0x801E4E64`. No program names both, and the equipment-buy recipient sub-screen `FUN_801DB380` adds only window 36 over the set already up. The earlier reading - that the shop's recipient flow opens both - is [falsified](re-do-not-re-walk.md#menus--ui). |
| What is the equip compare panel's category byte? | resolved (the **accessory passive index**) | `disassembly` + `capture` | The lookup reads the item record's class byte first, then one of two tables indexed by that record's `+1`: class `1` takes the equipment bonus row `0x80074F68 + row*8` byte `+5`, everything else the item-effect descriptor `0x800752C0 + row*4` byte `+3`. Of 255 non-zero ids, 104 are class `1` and every bonus row they reach carries the `0x40` no-passive sentinel; 80 of the other 151 carry a real index - 9 under `6`, 4 in `10..=12`, the rest ATK / UDF / LDF. So the byte names the passive, and the panel shows the stats it moves. [details](../subsystems/field-menu.md#both-category-arms-are-live-on-retail-data) |
| How many equip slots does the menu stat block sum? | resolved (**five**) | `disassembly` | `FUN_801CF650`'s loop counter is bounded by `slti a2, 5` at `0x801CF744`, so the block behind windows 22 / 25 / 41 sums the first five equip bytes only; the battle-side aggregator `FUN_80042558` walks all eight. On retail data the difference is inert - the accessory class the extra slots hold is absent from the equipment table - but the two are different routines, so a port sharing one kernel has to zero the tail for the menu side. |
| What is the equip browse row's index space? | resolved (a two-table **slot map**, not the equip-byte index) | `disassembly` | Row `0` is the **weapon** row and takes a per-character halfword from `0x8007B42C`, which reads `2, 3, 2` out of `SCUS_942.54` - Vahn and Gala's weapon byte is `2`, Noa's `3`. Rows `1` and up index `0x801E43E8`, whose bytes run `00 01 00 04 05 06 07`, so the on-screen order is weapon, helmet, body, footwear, then the three Goods slots. `slti v0, s0, 4` silences exactly the four gear rows, so retail resolves a compare category for the three Goods rows only - that guard and the `0x40` sentinel on all 104 class-`1` rows are one design, not two facts. |
| Which way does an obstructed sub-cell drift the fishing lure? | resolved (by the cast counter's low bit) | `capture` | Forcing the branch at `0x801D2E28..0x801D2E58` 949 times gives 447 ADD hits, every one on an odd `_DAT_80084460`, and 502 SUB hits, every one on an even value - zero violations - with the 24.8 accumulator `0x801D9174` moving by exactly `frame_delta << 11` per hit. Retail rarely sees it: at the catalogued fishing venue the walk-grid probe returned zero on all 949 calls, so the lure never drifts there. |
| What writes the fishing cast counter `_DAT_80084460`? | resolved (the bite tick itself, once per hook) | `disassembly` | Reached through the save-block base rather than its own address - `t0 = 0x80084140`, then `lw` / `addiu` / `sw 0x320(t0)` at `0x801D2954..0x801D296C` - which is why a displacement scan finds only the `0x4460` read. The increment sits on the hook arm, the one that raises cue `0x204` and sets SM state `0x19`, so it steps once per hooked fish and the lure's drift direction therefore alternates between casts. |
| What consumes the fishing bite tick's per-cell fish weight? | resolved (it is the modulus of the caught fish's **size** roll) | `disassembly` | One use: `div s0, s4` at `0x801D3728`, remainder taken with `mfhi a3` at `0x801D3750`, summed with three other terms and stored at `DAT_801D91B8` (`0x801D3814`). The same value plus `0x400` becomes the render scale of the object the catch just spawned (`sh a0, 0x72(s1)` at `0x801D381C`, on the object from the preceding `jal 0x80024C88`). It touches neither the species roll nor the bite credit - so a deeper cell makes a bigger fish, not a different one. |
| What is the Baka Fighter VRAM-rect table `DAT_801DBE84`? | resolved (exactly two records) | `disassembly` + disc bytes | `(0x340, 0xC8)` and `(0x340, 0xE0)`, both `6 x 0x18`, both blitted to the same fixed destination `(0x340, 0x86)`. Its eight bytes end at `0x801DBE8B`, and everything above that to PROT 0976's `0xE000` end is zero - so it is the last initialised data in the overlay, and an index past the second record reads the destination's own column back rather than a third rect. |
| Does retail's equip screen offer a Goods-slot candidate, and out of which id space? | resolved (yes - item class `2`, and the filter carries no character mask) | `disassembly` + measurement | The screen has **two** candidate families. The slot-browse step writes window 23's `+0` content id per row from the eight-byte table `0x801E4DC0` = `00 17 15 16 18 1C 1D 1E` (PROT 0899 file `0x165A8`), so the three Goods rows reach `FUN_80030628` cases `0x1C` / `0x1D` / `0x1E`, whose filter is item-record class `2` (`bne` at `0x800317D8`) plus item-effect `+3 != 0x41` (`beq` at `0x800317F8`). The armament cases `0xE` / `0xF` / `0x10` carry their own character mask; the Goods cases carry none. On the USA disc 80 of 151 class-`2` ids pass, and all 71 rejected carry exactly `0x41`. |
| What does the shop quantity window print? | resolved (quantity **/** bound, not quantity x price) | `disassembly` | Window 35's value row is a pair with a separator glyph between them (`FUN_8003C1F8(6)`): the second number call at `0x801D563C` loads `DAT_801E46B8`, the word the buy picker's phase 0 fills with `min(gold / price, 99, 99 - held)`. The price appears once, in the running total, and a currency pictogram draws at `(WX + 0x58, WY + 0x24)`. The window scripts are buy phase 0 `0x801E4EB0` (ending `[01 23]` -> window 35), buy confirm `0x801E4ED4` (`[04 23]`), sell phase 0 `0x801E4F08` (`[01 25]` -> window 37) and post-sale `0x801E4F10` (`[0A 26]`). |
| How does sub-screen `0x15` exchange two list rows? | resolved (a two-press latch in one word) | `disassembly` | Bit `0x1000` of the second cursor word means **nothing is latched**. A confirm taken with it set stores the hovered row index and clears the bit; the next confirm swaps the two rows and re-raises it; a cancel taken with a latch pending only drops the latch, leaving the list alone. So one word carries both the pending row and the fact that one is pending, and only the spell list's running step carries the arm at all. |
| Which equip byte is the Ra-Seru slot? | resolved (per character, from `0x8007B424`) | `disassembly` + disc bytes | The table reads `3, 2, 3` (`lui 0x8008` / `addiu -0x4bdc` at `0x801DA5D0`) - the exact complement of the weapon table `0x8007B42C` = `2, 3, 2`. So Vahn and Gala wear the Ra-Seru in byte `3` and the weapon in byte `2`, and Noa the other way round; a reader taking either table as a constant gets one of the three characters wrong. |
| Is the `0x801E43E8` byte run one table or two? | resolved (**three** tables and a pad byte) | `disassembly` | The browse row -> equip-byte map is seven bytes and stops. `0x801E43EF` is alignment - no word, no `lui`/`addiu` pair and no branch in any image reaches it - while `0x801E43F0` (4 bytes, the per-character equip **mask bits**, `and`ed with the equipment record's `+6`) and `0x801E43F4` (8 halfwords, the per-row slot **pictogram ids**) each have three materialisation sites of their own. Bytes `[7..10]` looking like the gear-slot indices is the pad plus the mask table's first three entries. [`field-menu.md`](../subsystems/field-menu.md#the-0x801e43e8-run-is-three-tables-not-one) |
| What does retail's equip candidate step draw? | resolved (three stacked panels; the compare category follows the hovered **item**) | `capture` | One script, `0x801E4DC8`, opens window 25 (name + one stat-row set), window 24 (item name with owned count, description, bonuses) and window 24's reserved box at `(WX, WY + 0x38)` sized `0x90 x 0x28` for an accessory passive. A pad ladder driven to each of the seven rows populates all seven lists. The `slti v0, s0, 4` guard at `0x801D137C` silences the four gear rows, so only the three Goods rows resolve a category - and they disagree with each other, because the row set follows the hovered item. [`field-menu.md`](../subsystems/field-menu.md#what-the-candidate-step-draws-measured) |
| Where is retail's own entry to sub-screen `0x15`, and which lists can it reach? | resolved (the root picker's row 3 = Status; steps 3 and 4 only) | `disassembly` + `capture` | `0x801D6C4C`, in `FUN_801D6B20`'s row-3 arm, is the only site in PROT 0899 that writes `0x15` into `DAT_801E46A4`. Window `0x15` is a **different id space** - the Equip screen's party window, opened by script `0x801E4DA0`. Inside the screen the character picker's confirm arm folds the second cursor and dispatches: folded `0` / `5` hop columns, `1` buzzes, `2` writes step `3` and `3` writes step `4`. Nothing writes step `2`, so the abilities list is decoded and has no door. [`save-screen.md`](../subsystems/save-screen.md#which-screen-raises-it-and-which-of-the-three-lists-it-can-reach) |
| Does retail's wall slide fire in ordinary play? | resolved (yes - and the oracle that said the port could not wire it was measuring agreement) | `capture` | On `s3_rimelm_freeroam` the resolver `FUN_80046494` answers the held mask on 261 of 276 calls and widens it on 15, all on one held-`LEFT` run down the `town01` exterior wall (`0x8000` -> `0xC000`). The blocker the port's marker carried - "the rests are pinned against captures taken on the non-sliding stepper" - does not hold: at both pinned wall-press rest positions the resolver hands back the held cardinal, so those legs are slide-neutral. [`field-locomotion.md`](../subsystems/field-locomotion.md#the-skid-measured-on-retail) |
| Is the camera's visible-tile window a property of the scene? | resolved (**no** - of the region) | `capture` | A 3000-vsync pad-driven `town01` walk never holds the window constant and never holds the port's default, alternating between `(-8, -6, 8, 12)` and `(-10, -6, 8, 14)` four times each as the player crosses regions - the camera-region record's mask-kind side-write doing its job. The walk stays in one scene, so what a window does at a door is still open. [`encounter.md`](../formats/encounter.md) |
| What does field-VM `4C D8` spawn? | resolved (a **morph-weight** actor; the two `u16`s are envelope rates) | `disassembly` | `FUN_801D77F4` allocates from the morph-weight descriptor `0x8007068C`, whose `+0x8` handler is `0x8002174C`; the tail wires a VDF body off `0x8007B7DC` by operand 1, a TMD off `0x8007C018` by operand 2 - a **scene-bank** index the op's arm has already offset by `0x8007B6F8` - and a rest pose *snapshotted* from the live vertices rather than loaded. The handler reads `+0x3C` / `+0x3E` at `0x80021890` / `0x800218B4` as the rise and fall steps of the weight at `+0x6E`, so the port's `kind` / `variant` names are the generic allocator's. [`script-vm-menuctrl.md`](../subsystems/script-vm-menuctrl.md#what-the-0x4c-0xd8-spawner-builds) |
| Which target flow does a confirmed menu spell open? | resolved (the spell's own stats `+2` bit `0x20` picks) | `disassembly` | Set routes to the no-pick **group** sub-screen `0x10` (`FUN_801D9280`, `FUN_801D688C` called with `count = 0`); clear routes to the per-member picker `0x11` (`FUN_801D9594`). A party-wide heal is therefore one confirm and a per-member resolve, not a folded single-target cast. [`field-menu.md`](../subsystems/field-menu.md#the-two-target-flows) |
| Does retail's cold entry into `conc` clear flag `0x6DE`? | resolved (yes, twice - and the flag is a live **position predicate**) | `capture` | A single-flag write watch from the memory-card load screen sees the scene load clear it from `P1[1]`'s spawn prologue (`+0x10`) and the `P1[0]` entry script (`+0x18`) - `ra 0x801E35C0`, the field overlay's CLEAR site - and then `P1[0]`'s per-frame body SET it every other frame from `+0x10B` (`ra 0x801E3598`) behind `CD F8 0A 0E 33 48` at `+0x100`, for as long as the player stands outside tiles `10..=51` x `14..=72`. Poking the player inside stops it (60 SETs before, 0 in 280 ticks after). The card-boot save stands at `(17, 97)`. [`script-vm.md`](../subsystems/script-vm.md#a-system-flag-can-be-a-live-position-test-not-progress) |
| What position does a system script's `CD F8` box test read? | resolved (the live player's, on every evaluation) | `disassembly` + `capture` | Retail resolves cross-context target `0xF8` to the player object each time. The engine's ctx-`0xFB` system context had its anchor seeded only by the load-frame pre-run of `opdeene` / `opstati` / `opurud`, and the script install reset it to the origin, so everywhere else every per-frame box test ran from tile `(-1, -1)` and answered "outside". `World::sync_field_ctx_player_anchor` re-seats it before each field frame slice. 661 of the 668 clean `0x4D` sites disc-wide are the `CD F8` form, over 92 carriers. |
| What do the two screen-effect pushers of a `conc` -> `conc2` door write? | resolved (two **channels**, told apart by the OT bucket) | `capture` | Departure: 33 passes at a two-vsync cadence from the field overlay's `ra 0x801DDE24`, two concurrent pushes - bucket `2` an ambient `0x00003030` decaying, bucket `0` a white-out ramping to `0xFFFFFF`. Arrival: 15 passes from SCUS's `ra 0x80025034` on bucket `1`, `0xEEEEEE` falling to `0`. The first argument of `FUN_80024EE4` therefore selects a channel with two callers, and red sits in the colour word's low byte, as the disassembly said. |
| What does `4C D8`'s model operand index? | resolved (the **scene bank**, not the global pool) | `disassembly` | The arm adds the scene-bank base before the call - `lhu s0, -0x4908(v1)` (`0x8007B6F8`) then `addu` at `0x801E2DE0..0x801E2DE8` - and `FUN_801D77F4` reads `DAT_8007C018[slot]` unadjusted. On all seventeen sites the one-record block's `first_vertex + delta_count` equals the vertex count of object `0` of scene model `n` exactly, `balden`'s operands `99` / `100` / `109` / `110` included (through `balden2`'s count-5 MAN-less table). [`script-vm-menuctrl.md`](../subsystems/script-vm-menuctrl.md#the-model-operand-is-a-scene-bank-index) |
| Does retail walk the morph block with one record pitch? | resolved (**three**, and they agree only on a one-record block) | `disassembly` | The spawner's size sum steps `0xC` (`0x801D78D0..0x801D7900`), its rest-pose copy `n_vert * 8` (`0x801D792C..0x801D799C`), and the apply pass `FUN_8002174C` `n_vert * 0x60` (`0x800217B4`, `0x80021860`). Every block the disc ships is one record naming TMD object `0`, so the disagreement is unobservable - a different claim from the pitches being equal. [`script-vm-menuctrl.md`](../subsystems/script-vm-menuctrl.md#three-record-pitches-over-one-morph-block) |
| Does any beat take op `0x34` sub-0's forked arm? | resolved (**no** writer of the gate bit executes) | `disassembly` + `capture` | The arm forks on `_DAT_1F800394 & 0x800000` (`lui v1, 0x80` at `0x801DFD1C` and `0x801DFEB0` in PROT 0897; set -> `FUN_80024E80`, clear -> `FUN_801DE2B0`). A writer census of bit 23 over SCUS, every mapped overlay and every scene carrier finds no store the disc executes (`scratch_global_bit_writers_real`, and the field-op census), so the forked arm is never taken on retail. A block copy into the scratchpad is not excluded by a store census. |
| Does any MAN author motion-VM ops `0x10` / `0x11`? | resolved (no) | `disassembly` | Zero of the 573 section-1 stream variants carry either, so the `b1 & 0xC0 == 0xC0` assert arm that stores `0x3039` into `0x8007B828` has no shipped driver. |
| What does `FUN_8001FA00`'s one caller seed? | resolved (the **fog-particle pool's** free stack) | `disassembly` | Its only `jal` is at `0x801D7384` in MAIN INIT (PROT 0897), passing `(pool, pool + 4, 0x50)`: an identity list of eighty slots under a one-based top index. The "cutscene sprite list" reading named the wrong consumer; `FogPool::reset` now seeds through the port. |
| Is the 192-byte block at `0x801F21B4` one twelve-row probe table? | resolved (**three** tables) | `disassembly` | Three consumers form three bases: the actor-collision probes at `0x801F21B4` (six rows; `lui`/`addiu` at `0x801CFE74` and `0x801D5A70`), the leading-edge wall probes at `0x801F2214` (four rows; `0x801CFEE8`, `0x801CFFC0`, `0x801D009C`) and the interact facing compass at `0x801F2254` (eight `±64` points; `0x801D0834`). The "later rows have no caller" reading was the wall and compass tables seen as rows of the first. [`field-locomotion.md`](../subsystems/field-locomotion.md), `legaia_asset::field_probe_tables`. |

### The field follow camera's pose chain

*Status:* resolved - the chain is pinned by disassembly and the pose measured over the walkable state population

Retail derives the follow camera per scene and per tile, and the three values a
port is tempted to pin as constants are the tail of a four-stage chain.

- **Load.** `FUN_801DBC20` splits one 18-byte MAN section-3 camera-region
  record into the parameter block at `0x8007B607..0x8007B627`, choosing among
  three layouts on the high nibble of `record[5]`.
- **Compose.** `FUN_801DAB90` reads that block, the player's position, the
  player's floor height and the walk-region attribute box at scratchpad
  `0x1F800384..87`, and writes a staging descriptor at `0x801F3580`. It samples
  the height through `FUN_80019278` with the MAN's **static** elevation LUT
  swapped in, so a scripted floor-tier bob moves the player without shaking the
  camera. The value the pitch couples to is that floor height, not a heading.
- **Ease.** `FUN_801DB510` walks the six-entry descriptor list at `0x801F2798`
  toward the staging fields by `delta >> shift` plus the sign of the delta, with
  the shift from the sixteen-byte table at `0x801F2804` indexed by
  `DAT_8007B60B >> 4` - and only on frames the player's position changed, which
  is why retail leaves the camera short when he stops mid-glide.
- **Snap.** `FUN_801DB8EC` is the same compose plus list walk with a plain copy.

Two identities fall out of the same read and correct earlier pages: the trig
LUT pointers are `_DAT_8007B81C` sine and `_DAT_8007B7F8` cosine (an encounter
page had them reversed), and `FUN_8005B0B8` is a PsyQ-shaped `SquareRoot0` over
the 192-entry mantissa table at `0x80078E84`, not a bit-packing helper.


### Chapter-1 scene frontier

*Status:* resolved as a per-scene verdict; the five scenes that read as sealed are not - see [the Uru Mais and jouine exits](#the-uru-mais-chain-and-jouine-exits)

The chapter-1 reachable set is the BFS closure of `town01` over each scene's
own decoded `0x3F` destinations, and it terminates at exactly one kingdom
boundary: `jiji -> map02`. It is 27 scenes, and it contains the whole Drake
kingdom past the Ravine - the boss chain, the four-deep Drake Castle interior,
the Uru Mais rooms. Every one of the 27 loads its assets, parses its MAN,
enters in `Field` / `WorldMap`, and settles its entry script; all can be walked
out by pad alone - the last holdout, `urudre2`, was held by a mis-decoded
op-`0x45` CAMERA APPLY, not by the scene
([falsified](re-do-not-re-walk.md#field-vm-op-0x45-sub-0xc0-returns-the-operand-s16-as-the-next-pc)).

The closure is a *reachability* partition, not a narrative one, and it is a
closure over `0x3F` only - scenes reached by the sibling `0x3E` door warp
(which carries a scene-type selector rather than a name) are outside it.

Three door shapes come out of running three decoders over the same MAN - the
clean per-partition fall-through walk, the recovering destination-table pass,
and the `.MAP` gate-1 trigger to partition-2 record to `0x3F` join:

| shape | scenes | what has a door |
|---|---|---|
| op, table and walk-on trigger | 22 of 27 | all three decoders |
| op behind text pages, trigger in the `.PCH` sidecar | `uru`, `urudre1`, `urudre2`, `urudre3` | all three decoders, and the sidecar for the trigger; the clean walk once desynced in the inline `0x1F` text kilobytes before the `0x3F`, and crosses them now that a text segment is one decoded stride |
| FMV hand-off, trigger in the `.PCH` sidecar | `jouine` | no `0x3F` at all - the exit is `4C E2 08` (FMV 8 → `town0e`), already on the FMV dispatch table |

All 27 have a door. The two probes that once made the bottom rows a
playability statement were measuring their own caps: the walk-on sweep stops
at 48 deduped gate-1 tiles (`uru` carries 118 and its exit band sits at
positions 63..66, `urudre2` carries 186), and the 24-tick post-step budget
cannot run a record whose body spends 300+ frames in explicit waits or, for
`jouine`, a 6.8 KB boss cutscene. The `.MAP` gate-1 join was also only half
the join: every one of these exits is carried by the scene's `.PCH` trigger
sidecar, which the fallback read of `FUN_801D5630` reaches. Details and the
live pin: [the Uru Mais and jouine exits](#the-uru-mais-chain-and-jouine-exits).

One locomotion residual, and it is a script rather than a defect: on a **first
visit** `izumi`'s C1-gated spring record relocates the player about thirty
tiles with the pad released, so a driven probe there cannot beat its own
released-pad control. A revisit probe walks normally.

Measured by `crates/engine-core/tests/chapter1_frontier_ladder.rs`; nine of
the closure's scenes are additionally cross-checked against the capture
library's own main RAM, and all nine enter in-engine.

### The Uru Mais chain and jouine exits

*Status:* resolved - grade `disassembly` + `capture` (the `uru` exit fired live).

Every one of the five carries a walk-on exit, and every exit band lives in the
scene's `.PCH` sidecar ([`scene-v12-table.md`](../formats/scene-v12-table.md)),
not its `.MAP` - `urudre2`'s alone is doubled into the `.MAP`:

| scene | exit record | gate-1 band | tail op (MAN offset) | destination |
|---|---|---|---|---|
| `uru` | `P2[42]` | `(36..39, 5)`, `.PCH` rows 23..26 | `0x3F` at `0x0D4B7` | `MAP03` `(0x24, 0x46)` |
| `uru` | `P2[37]` (`C2 = [0x36F]`) | `(37..39, 44)` | `4C E2 07` at `0x0CB11` | `uru2` via FMV 7 |
| `urudre1` | `P2[2]` | `(35..37, 22..24)` | `0x3F` at `0x01804` | `uru` `(0x40, 0x40)` |
| `urudre2` | `P2[9]` | `(26, 14)` + `(24, 13)` | `0x3F` at `0x01D78` | `map01` `(0x26, 0x51)` |
| `urudre3` | `P2[0]` | `(51, 90)` | `0x3F` at `0x02461` | `uru` `(0x40, 0x40)` |
| `jouine` | `P2[16]` | `(17, 17..19)`, `.PCH` rows 3..5 | `4C E2 08` at `0x03E90` | `town0e` via FMV 8 |

`uru` also carries the three dream entrances (`P2[29]` → `urudre1`, `P2[33]` →
`urudre2`, `P2[31]` → `urudre3`), all `.PCH`-only and ungated. The four `0x3F`
records share one byte-exact tail: `B1 F8 13` (set flag 19), `34 05 FF FF FF
41 00` (white fade), the `0x3F`, then the `26 FF FF` / `21` / `26 FE FF` park
pair. `jouine` has no `0x3F` anywhere; its exit is the FMV hand-off the
[`str-fmv-table.md`](../formats/str-fmv-table.md) row already pins
(`0689_jouine` → `fmv_id 8` → `MV6.STR` → `town0e`, door `0x2E5`), which the
engine's `fmv_post_play_handoff` already maps.

Live pin: from `uru_field_run` (tile `(38, 6)`), holding the pad toward
`(38, 5)` fires `FUN_8003BDE0(36, 5, 42, 1)` and then `FUN_8001FD44("MAP03")`
with `ra = 0x801DEB1C` - the field VM's `0x3F` arm - and the scene leaves
`uru → map03` (probe `scripts/pcsx-redux/autorun_uru_exit_probe.lua`). The
row `(36, 5, 42, 1)` exists only in the `.PCH`; `uru`'s `.MAP` gate-1 records
are `[1, 3, 43]`. `jouine` cannot be pad-probed from the catalogued state,
which is already inside `P2[16]` running the evolved-Cort fight ahead of the
FMV tail.

What is ruled out: no `0x3E` door warp with `op0 >= 100` in any of the five,
no `0x4C` staged-menu-warp arm, no kind-0 teleport rows in any of the five
`.PCH` files, and the scripted-motion VM has no scene-change opcode at all.
One decoded oddity stands unexplained: `urudre2` returns to `map01` (Drake)
while `uru` itself exits to `MAP03` (Karisto). Layout and the three
instrument artifacts that produced the "sealed" reading:
[`world-map.md`](../subsystems/world-map.md#uru-mais-and-jouine-exits-carried-by-the-pch-sidecar).

### The upper-case destination fold

*Status:* resolved - grade `disassembly`

`FUN_8001FD44` `strcpy`s the operand into `0x80084548` and the field asset
loader `FUN_8001F7C0` (`0x8001F7E8..0x8001F88C`) `strcat`s it into
`DATA\FIELD\<name>.MAP` for `FUN_8003E6BC` → `FUN_800608F0`, the ISO file open -
ISO 9660 identifiers are upper case, so case cannot matter. 40+ distinct
upper-case destination names appear across the 99 scene MANs (`MAP01/02/03`,
`KOR*`, `DREAM`, `RETOCK*`, `ROPEWAY`, `NILBOA`, the `ED*` ending chain), every
one a CDNAME label, and the disc carries **no** mixed-case `0x3F` name run.
`legaia_asset::field_disasm::clean_scene_name` now accepts a uniformly-cased
3..=12-byte alphanumeric label and folds it; the chapter-1 closure roughly
doubles (68 scenes, 20 kingdom-boundary edges).

### The count-5 asset tables

*Status:* resolved - grade `disassembly` + `capture`

Retail bounds the count nowhere - `FUN_80020224` reads `+0x00` and loops that
many descriptors - so the bound was a detector heuristic; the strong signal is
descriptor 0's anchor at `8 + count * 8` (`0x30` here). Their tuple is
`(TimList, Man, Move, Anm, Flag(0x14))`, the canonical seven minus `Tmd` and
`Vdf`. A census of every PROT entry parsing as this table finds 1 / 2 / 10 / 4 /
8 / 80 entries at counts 1 / 3 / 4 / 5 / 6 / 7, and of the sub-6 set only these
two carry a MAN, so the detector now admits `count >= 4` gated on a type-3
descriptor and exactly two entries change class. See
[`scene-bundles.md`](../formats/scene-bundles.md).

### The ledge-hop lock leak

*Status:* resolved (engine bug, fixed) - grade `disassembly` + `capture`

`start_field_ledge_hop` ORs `0x0008_0000` into the player's `move_state.flags`
(retail `0x801D25A8..0x801D25B8` on the player context's `+0x10`) and only the
hop phase machine's end arm clears it; a transition landing mid-hop tore the
machine down without that arm, and the locomotion step returned early on the bit
in every scene afterwards. Retail cannot reach the state - its hop always
finishes before a transition. Pinned by walking the closure in one host: `tower`
hops and ends at tile `(0, 0)`, and all 28 scenes after it reported the flag
with zero driven tiles while each walked normally on a fresh host; three
isolation builds had ruled out both candidate engine changes first.
`SceneHost::enter_field_scene` now drops the hop and the lock together.

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
`FieldPlayerAnim` off `World::locomotion.player_move_cues`, an NPC's by the host's
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

### Rim Elm's south gate

*What it looked like:* the first scene exit of the game fires when an oracle
seats the player onto `(25, 46)` and never fires when a player walks there, so
the walk-on dispatch looked broken.

*What it is:* neither of the gate's two `.MAP` kind-1 gate-1 bands is the
mechanism the symptom suggested.

| Record | Tiles | Script |
|---|---|---|
| `P2[10]` | `(24..26, 45)`, `(25, 44)` | `21 21 26 FE FF` - `Nop; Nop; JmpRel`-to-self. Five bytes, no scene change. |
| `P2[0]` | `(24..26, 46)` | `CFlag.Set`, an `Effect` fade, `0x3F` naming `map01` at entry `(0x60, 0x19)`. `C1=[] C2=[]`. |

The exit record is **ungated**; the other record is **inert**. What holds a
player inside Rim Elm is the collision grid - grid row 47 walls
`z ∈ [5888, 5951]` across the doorway - and that row *is* the gate. It is cut
by `town01` `P0[20]`, the gate object's own record, bound by the gate-0 kind-1
trigger at tile `(23, 43)` and executed by the scene-init bind prologue
(`FUN_8003A55C`). The record clears the approach with three `4C 70` paints and
then branches on system flags `327` / `321`:

| `327` | `321` | paints | gate |
|---|---|---|---|
| clear | - | none; the base row-47 wall stands | shut |
| set | clear | re-blocks rows 44..46, seats the gate at `(24, 44)` | shut |
| set | set | `4C 70 18 2D 19 2E` - cols `24..25`, rows `46..47` | **open** |

So a cold boot cannot leave Rim Elm in the port *or* in retail, and the disc
says so rather than the engine. The port already executes the whole chain:
measured on its loaded grid, the three flag states give exactly the three
collision states above, with col 26 correctly re-blocked in the open one.
Pinned by `crates/engine-core/tests/south_gate_disc.rs`; the pad-driven exit is
a rung of `crates/engine-shell/tests/critical_path_replay.rs`.

Carrier note, because it decides whether a port can open the gate at all:
`town0c` holds the same paint sequence **twice** (its entry script `P1[0]` and
`P0[20]`); `town01` holds it only in `P0[20]`. An engine that applies nibble-7
deltas from entry scripts alone leaves `town01`'s gate sealed in every story
state.

### Town/field free-movement locomotion

*Status:* resolved

The player free-movement controller is `FUN_801d01b0` (field overlay 0897), pinned by a runtime write-watchpoint on `*(0x8007c364) + 0x14/0x18` (`autorun_player_pos_watch.lua`). It camera-remaps the held pad (`func_0x800467e8` + `FUN_80046494` → direction bits `& 0xf000`), computes a per-frame speed (`base_step * player[+0x72] >> 12 * DAT_1f800393`, with terrain-slow + diagonal modifiers), then steps the player position 2 units at a time with per-axis collision via `FUN_801cfe4c`. Sets facing `player[+0x26]`. Full write-up in [`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md). The `801db81c..801dbf9c` cluster previously suspected here is the field *camera* system, not movement.

**Collision derivation - resolved (capture-proven; engine realigned).** `FUN_801cfe4c` is fully decoded (overlay `0897` @ `0x801CE818` + the on-disc bias table `DAT_801f2214`): three **leading-edge footprint probes** (~47 units ahead, ±16 lateral), each sub-cell derived as `zc = (z>>6)+2`, `xc = ((x+0x3f)>>6)−1`. Two cheat-free Rim Elm wall-press captures settled the long-open indexing question:
**The `+2` Z bias is authored into the wall bits.** In the down-press capture (`rimelm_wall_press_down`, screen-down = world `Z−`) the player legally rests at a position whose plain floor-indexed cell is an all-quads wall byte (unreachable under floor indexing); the biased read places that wall band one tile north, exactly where the press blocks with a step-exact 47-unit standoff. The left-press capture (`rimelm_wall_press_left`) pins the X side: probe reads the wall column's last sub-cell, one 2-unit step shallower reads clear; retail's `ceil−1` equals the floor except at exact 64-multiples (parity-unreachable). The **floor sampler** (`FUN_80019278`) reads the *same bytes* with plain floor indexing - one byte's two nibbles live under two world→cell mappings.
**Engine realigned with proof in hand:** [`World::field_tile_is_wall`] now uses retail's exact derivation (`sample_field_floor_height` keeps the floor, matching its own retail source). **The three-probe leading-edge footprint is wired too** (`World::field_dir_blocked` over the disc-pinned `DAT_801f2214` table - 48-unit edge in the positive directions, 47 in the negative, ±16 lateral - gated by `World::locomotion.leading_edge_wall_probes`, on by default in `play-window` (`--no-edge-collision` clears it); the centre test stays the `World` field default for the oracles + nav drivers): driving the engine stepper over each capture's live grid reproduces both retail rest positions **byte-exactly** - and the full-scene legs reproduce them through a real `enter_field_live` scene entry.
**The actor-collision probe is decoded, modelled, and capture-classed.** `FUN_801cfc40` (bits `1`/`4`) walks the active-actor table `DAT_801c93c8`, box-testing the three `DAT_801f21b4` probe points (disc-pinned: 64/63 ahead, ±32 lateral - wider than the wall edge) against each actor: a static entity anchors at its MAN object record (`tile*128 + sub*16`) with the `0x40+0x10` half-extent; a moving actor uses its live position with caller extents (`±40` from the locomotion). The locomotion gates each 2-unit step on the actor bits and the wall bit together, so NPCs block exactly like walls.
The `rimelm_npc_press_tetsu` capture (player pressed into the sparring partner) pins the class from live RAM: the mutual `+0x98` collision link is active in-frame both ways and the NPC's `flags+0x10 = 0x08020884` carries the `0x20000` bit - **village NPCs take the moving-actor arm (bit `1`, ±40 box)**, not the static prop arm. Engine: `World::field_actor_dir_blocked` ports that arm over `field_npc_positions`, gated by `World::npcs.solid` (on by default in `play-window`, `--no-solid-npcs` clears it); disc-gated leg `npc_press_pins_moving_actor_arm`.
**The touch/interact dispatch and the static prop arm are decoded and modelled too.** `FUN_801d5b5c` (decoded from a live overlay image - the static 0897 copy is garbled at that VA) posts the touch event: player engaged flag `0x80000`, actor touched mark `0x100`, counters, facing saved to `+0x5A`, and the `FUN_8003c9ac` NPC-motion pause kick. The dispatch in `FUN_801d01b0` fires it automatically per contact step for static props (bit `4`), and on the just-pressed interact button through the third probe table `DAT_801f2254` (disc-pinned at overlay file `0x23A3C`: a radius-64 compass point per 45° facing sector, extents `0x20` → ±72 NPC box) for NPCs - with a face-the-NPC turn (`func_0x80019b28`).
The static-entity anchor formula (record footprint offset incl. the `+0x52 & 8` correction from record flag bit `0x8`) is live-verified against four captures' spawned static actors; the engine models props via `Scene::field_object_placements` collider centres (`field_prop_colliders_live.rs`) and the interact probe via `World::field_interact_probe_slot`.

**NPC motion and the prop walk-touch event are modelled engine-side.** Field NPCs walk: `man_field_scripts::placement_motion_route` decodes each placement's own pre-text `0x4C 0x51` move-to-tile waypoints and `World::tick_field_npc_motions` drives them through the ported motion VM (`FUN_8003774C`), live positions written back into `field_npc_positions` so the ±40 box and the interact probe follow (autonomous patrol gated by `World::npcs.animate`, on by default in `play-window`, `--no-live-npcs` clears it; an interaction prologue's `0x4C 0x51` walks the interacted NPC regardless).
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

*Status:* resolved as structure across the chapter-2/3 regions; play order is capture-confirmed for `retona`, `dohaty`, `taiku`, the Sebucus spine, `korb3`, the `kor5` chain head and the `map03` hub latch (see the play-order captures paragraph below); the remaining play-order residual is tracked on [`open-rev-eng-threads.md`](open-rev-eng-threads.md#region-story-flag-gate-families)

Every field scene's MAN carries one **partition-2 record** per cutscene or story beat, and each record's *header* holds two flag lists that the spawn evaluator `FUN_8003BDE0` checks before running it: a **C1** one-shot list (the record is suppressed once any listed flag is set) and a **C2** requires-all list (the record spawns only when every listed flag is set). Regional progression is expressed almost entirely through these header gates.

Because they live in the record header rather than as inline `0x50`/`0x60`/`0x70` opcodes, the inline flag census (`man-scripts --system-flag-census`) cannot see them — the recurring cause of several "write-only flag" false alarms. `legaia_engine_core::man_field_scripts::partition2_record_gates` decodes them, and the census-file anchor tests named below pin each region's exact lists.

Two reader-only flags first exposed the pattern. `0x1BE` (Jeremi's arrival at `geremi`) is a self-latch: `geremi P2[0]` both sets it and lists it as its own C1 gate (anchor `geremi_p2_0_is_the_0x1be_self_latch`). `549`/`0x225` (the Rim Elm opening) is read the same way across the Rim Elm variants and turned out to be the same self-latch shape once the `4C 0xE_` op widths were fixed.

**Chapter 2 — Sebucus (`map02` and its dungeon spokes).** The progression spine needs no chapter-specific engine code: each beat's script latches its flag through the ordinary field-VM `SysFlag.Set` path, so the generic seeder drives the whole arc. The chain runs `teien` (`0x1C8` → `0x1C9` → `0x332`) into `tower` (`0x1C7`, gated on the teien arc) into a post-tower `geremi` beat, with `balden` self-latching `0x5B3` and `map02 P2[9]` mirroring the teien arc onto the overworld. Proven by `chapter2_sebucus_spine_oracle`, `chapter2_sebucus_gate_spine`, and `chapter2_sebucus_hub_sweep_disc`, which drives the arc through real `0x3F` scene transitions. Each spoke's family is pinned disc-static:

- **`taiku` / `doman` / `rayman`** — self-latch pairs plus a linear `0x201` → `0x1FB` → `0x200` → `0x1FC` chain in `rayman`; `rayman2` is the same MAN with a shared C1 on the low flag `0x7`, a variant discriminator. `rayman`'s streaming variant adds a `P2[18..20]` tail latching `0x34D`/`0x34C` (`P2[18]` body `+0x2C2`, at a `JmpRel` branch-arm after `0x1FE`/`0x1FF` tests). The taiku variant's `P2[16]` beat SETs the pair `0x380` + `0x382` at its head (body `+0x11`/`+0x21`, between `SceneFade` and the particle emitters) — `0x382` is a **cross-chapter gate**: `son P1[14]` branches its NPC dialogue on it (body `+0x4A`), and the clean census reads span `doman(V)`/`retockin`/`ropeway`/`ropeway2`/`map03`/`koin2`/`korout`. Anchor `chapter2_dungeon_gate_families`.
- **`balden` / `balden2` / `station`** — `balden` is an arc around its reached-flag `0x1D5`; `balden2` is a sibling carrier with an identical gate family, so the variant is selected by the streaming slot rather than a flag. Cross-scene: `balden` gates on the `ropeway2` switches, and `station`/`station3` gate on `taiku`'s `0x38F`. Anchor `chapter2_balden_station_gate_families`.
- **`ropeway` / `ropeway2` / `jiji`** — the first spokes the capture corpus walked organically, so their play order is confirmed. `ropeway2` hosts a four-bit switch puzzle (`0x3FF`–`0x402`); its payoff records `P2[31..=34]` are gated via C2 on all four switches plus the `0x359` commit, an internal consumer the inline census had earlier mistaken for an external one. `jiji P2[8]` latches `0x304` from three branch arms of one cutscene (each `4C CD` → `Set` → `JmpRel` to the shared tail; bodies `+0x912`/`+0xCD6`/..). Anchor `chapter2_ropeway_jiji_gate_families`.
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
victorious beat. `0x5D6` is a **script self-latch**, and the earlier
writer-less verdict was a walk that stopped at a record's first text segment: `koin4`
`P1[15]` sets the flag it gates on - behind the "If you have money" line sit `48` (a
one-byte no-op) and `55 D6` - and its `P2[3]` twin carries the same block. The old scan
looked for the bytes `D6 05` rather than `55 D6`. The native-space sweep
(`scripts/asset-investigation/flag_helper_call_sweep.py`, the move-VM ext flag sub-ops,
the motion-VM census) still stands as a negative for **native** writers; what it could
not settle is script space, which is where the writer was. See
[script-vm.md](../subsystems/script-vm.md) § native flag-bank writers; the guard is now
`koin_gates_0x50a_writer_less_0x5d6_self_latched`.
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
The poll corpus also pins the `0x7` discriminator's own SET: it latches inside the opening-commit beat (the opdeene→town01 handoff of a fresh new game, alongside `549`/`0x226` — `captures/state_poll/2026-07-29T20-20-05Z`), so the rendition selection is armed from the start of play, not by a mid-game chapter transition.

**Play-order captures (poll-tier).** A poll-tier playthrough corpus (`captures/state_poll/2026-07-29T20-20-05Z` / `2026-07-29T22-21-04Z` / `2026-07-29T22-53-56Z`, mined via `analyze_state_poll.py --only flags`) confirms the live SET order for families previously proven as structure only. One screening rule applies throughout: a save-state load emits a sub-bulk flag delta the beat filter does not catch — its signature is a mode churn plus `flagclr` rows plus a full inventory re-key at one tick, with the destination scene registering ~43 ticks later — and every burst carrying that signature is excluded below.

- **`retona`** — `0x354` @(67,80) → `0x355` @(69,78) → `0x356` @(68,80) → `0x3AD` @(66,79); `0x357` then latches during the *next* scene entry (mode-2 transition frame after an overworld round-trip) — the `P1[0]` backstop converting `0x3AD`, exactly as pinned — followed by `0x367` @(67,78). (`0x353` was already latched in the session's starting state.)
- **`dohaty`** — one straight corridor walk: `0x343`+`0x63D` @(23,42) → `0x344` @(23,50) → `0x345` @(23,53), leads `0x39F`/`0x65A` interleaved; the `P2[10]` `0x344` one-shot and the `0x63D` pair both fire live. The walk's last beat @(23,56) latches `0x1D4` — a flag in `balden P2[0]`'s C1 list, a cross-scene edge captured live.
- **`taiku`** — `0x517` @(54,17) → `0x519` @(54,7) → `0x38F` @(54,40) → the `P2[16]` pair `0x380`+`0x382` @(16,28), one beat one tick. The other self-latch pair `0x390` did not fire on this walk — branch/optional content, still unconfirmed.
- **`kor5`** — the chain head in order: `0x43A` @(32,92) → `0x436` @(32,40). The `0x6C4` tail appears only inside a load delta, so the last step is not organically captured.
- **`korb3`** — `0x41D` @(36,21) (its requires-all C2 includes kor5's `0x436`, satisfied organically beforehand) → `0x41E` @(36,23) → `0x41F` @(37,24). The collection group plays backwards from its gloss on this walk: `0x403` latches on the `map03` overworld at the castle-approach node (72,41) *before* any collection flag, and all nine C2 flags (`0x43E..=0x444`, `0x459`, `0x45C`) then mint together in a single korb3 arrival burst (BGM stop → nine sets → new BGM, no load signature) — the group's records were C1-retired before ever spawning, so the "all done" latch is armed first here, not accumulated.
- **`map03` hub** — the `P2[15]` `0x378` latch fires live @(93,69).
- **Sebucus spine** — teien `0x1C8` @(41,45) → `0x1C9` @(44,43) → `0x332` @(44,47) → tower `0x1C7` @(13,69) → geremi `0x1BF` @(34,117): the spine the disc oracles proved now has a live capture. `tunnelc`'s `P2[6]` `0x34A` fires @(23,45); its `P1[4]` `0x360`/`0x362` did not fire (branch arms). balden's reached-flag `0x1D5` latches at the tunnelc exit tile (60,16); its `0x5B3` self-latch did not fire on this walk.
- **Rim Elm opening** — the `549` latch has a live organic SET at the opening commit, with `0x226` in the same beat.

Regions walked in the corpus **without** an organic family SET stay play-order-unconfirmed: `retock`/`retockin` (entered with `0x357` already latched; `0x502` never fired), `doman` (only the unpinned lead `0x379` fired; `0x3FB` did not), `nilboa`/`nilboa2` and `son` (entered mid-arc from loaded states; the only nilboa flag burst is a load frame).

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

### What the op-`0x49` entry-context kind byte is, and which screens it selects

*Status:* resolved (disassembly + disc measurement) -
[`save-screen.md § Root command picker`](../subsystems/save-screen.md#root-command-picker-fun_801d6b20)

The pause / save driver `FUN_801DC6B4` routes on `*_DAT_8007B450`, and the
question was what can put a value there. Two routines write a
**dereferenceable** pointer, out of ten `sw rt,0xb450(rs)` sites across
`SCUS_942.54` and every extracted PROT entry: the field VM's op-`0x49` Idle
arm (`0x801E09A8`, storing the script's *operand pointer*, whose first byte
the arm read at `0x801E0984` - so the kind byte **is** the sub-op) and
`FUN_801D0B90`'s countdown expiry (`0x801D0D04`, pointing at the static
record `DAT_801F2278`, kind `0x0B`). The other eight store `0` or the `1`
Done sentinel, and the resume clears the slot at `0x801E08D8`.

Four kinds select a screen, each at exactly one selector write in PROT 0899
(of 66 `sw rt,0x46a4(rs)` writers): `0` -> sub-screen `0x1A`, `1` -> `0x19`,
`7` -> `0x20` (the casino prize exchange), `0x0D` -> `4`. Kind `0x0D` also
sends the root picker's cancel to sub-screen `3` (`0x801D6D18`).

What kind `0x0D` *is* comes from the two screens' own string pointers rather
than from their routing: sub-screen `4` draws window `6`, six static VAs in
the overlay pool loaded at `0x801d636c..0x801d6448` - a pre-battle briefing;
sub-screen `3` draws window `5`, whose two headings (`0x801CEC78` /
`0x801CEC94`) are a **battle-start ready check**, not the "really leave?"
gate the routing suggests. So the context is a scripted pre-battle party
menu, briefed on entry and ready-checked on exit.

Reachability is disc-measured, not inferred:
`crates/engine-core/tests/op49_sub_op_census.rs` walks every scene MAN's
field-VM script and tallies the sub-op operands twice - a bounded,
offset-deduped opcode walk and a raw byte upper bound - and kinds `7` and
`0x0D` both appear in both tallies. The two tallies disagree in both
directions by design: the walk decodes no tile-board sub-op (`5`) that the
byte bound plainly finds, so an absent walk row means "not decoded here",
never "not on the disc".

Ports: `World::record_op49_park` / `World::menu_entry_context_kind`,
`FieldMenuSession::{open_entry_screen, Notice, ReadyConfirm}`.

## Text / fonts / dialog

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| Does retail wrap dialog text to the box? | resolved (no - no text surface wraps) | `disassembly` | `FUN_80036044`, once filed as the wrap pre-pass, returns the typewriter glyph count; `FUN_80036888` draws the expanded string with no clip or length test, and the pager `FUN_801D84D0` never measures a row. Line breaks are authored `0x7C` bytes and `0x1F` lines. [`dialog-font.md`](../formats/dialog-font.md#line-width-and-wrapping). |
| How wide can a field dialog row be? | resolved (244 px at one extra pixel per glyph) | `disassembly` + `capture` | Rows draw at the box x with `DAT_800740E8 = 1` (`0x801D97D8`) in a `0xF4`-wide centre rect (`0x801D99CC`), three rows a page; the `v0_1_tetsu_dialogue_accept` display list steps glyphs `widths[c] + 2` apart. Menu, shop and battle columns: [`dialog-font.md`](../formats/dialog-font.md#menu-shop-and-battle-columns). |
| What ends an NPC talk? | resolved (`FUN_80038050`'s verdict on the byte after the box) | `disassembly` + `capture` | `FUN_80039B7C` hands the byte after a scanned box to `FUN_80038050` (`jal` at `0x80039C84`) and ends the talk when it returns `0` (`0x80039C8C`). Its jump table `0x80010F38` (bytes `0x21..0x4C`) continues on `0x24` / `0x25` / `0x48`, the option bytes `0x27..0x2A` and `0x4C FF` / `FE`; `0x21` steps past itself and ends; every other byte ends with the cursor left on it. The retail inn capture parks on the `26 9D FE` loop-back, which the next talk runs ([`script-vm.md`](../subsystems/script-vm.md)). |
| When does op `0x4C`'s sub-`5` acquire refuse? | resolved (two tests) | `disassembly` | In the arm `0x801E2148..0x801E21DC`: a target other than the player whose `+0x94` is zero (`0x801E2148..0x801E2164`), and a target already carrying `0x400` while the scene word `*(_DAT_801C6EA4) + 8` is `0` (`0x801E2168..0x801E218C`). Either leaves `s7 = 0` and the PC on the op. The second is the same test as op `0x43`'s acquire and the dispatcher's halted-target early-out. |
| Where does an NPC's first dialog line start when it opens on an escape? | resolved (at the escape) | `disassembly` | 113 of the 1586 talkable partition-1 records open their first line on an escape, `1F C1 00 ...` (the lead's name) the common case; the `0x00` is `C1`'s argument, not a terminator. A scan to the first `0x00` started one line late. `man_field_scripts::first_inline_dialog_offset` walks lines with `dialog_box::line_end` ([`mes.md`](../formats/mes.md)). |
| What draws the battle command chips' words? | resolved (text: placement-record payload strings) | `disassembly` | Each chip is a screen-element placement record whose `+0x14` payload points at a string: `Auto`, `Command`, `Attack`, `Item`, `Run`, `Begin` in the executable's small-data pool `0x8007B658..0x8007B690`, `Spirit` and the Ra-Seru names in the battle overlay ([`battle.md`](../subsystems/battle.md#where-the-words-come-from)). |
| How does a localized build lay out its UI strings? | resolved (by length, so pools move while their references stay) | `disassembly` | The Spanish build's `Automatico` sits in another pool from the USA `Auto` while the placement record pointing at it is the same record: strings of up to eight bytes land in the `$gp` small-data pool, longer ones in read-only data (the compiler rule is `inference`). The lift pairs a pool string through the code and data words that reference it ([`pal-localizations.md`](../tooling/pal-localizations.md)). |
| Do PAL builds keep the narration crawls' page counts? | resolved (no - they reflow with blank pages) | `disassembly` | A crawl's page count is part of the script (`CC F8 80 N`), and the PAL builds reflow a block into more pages with blank `" "` pages between paragraphs - `opdeene` is 14 + 8 pages on USA and 16 + 9 on the Spanish disc ([`pal-localizations.md`](../tooling/pal-localizations.md)). |
| Where does the narration crawl's geometry come from? | resolved (a scene config block seeded by `CC F8 E8`) | `disassembly` + `capture` | `FUN_80037174` reads `*0x801C6EA4` `+0x4C` top, `+0x4E` line slots, `+0x50` divisor and `+0x52` release count; `FUN_8003A024` resets them to `0x40 / 8 / 4 / 0` and a seed op before each block overwrites the first three. Line pitch is a fixed 16 (`addiu s3,s3,0x10`). The cold-boot `opdeene` capture holds the frame-step floor `DAT_8007B9D8` at 3, so at divisor 4 the crawl climbs a pixel every two frames ([`cutscene.md`](../subsystems/cutscene.md)). |
| Dialog font extraction | done - kept for reference | `capture` | Earlier "blocked on runtime trace" framing was wrong; tile-page lives at VRAM `(896, 0)..(960, 256)`, extracted by `legaia-font::font-extract` from any in-game save state. The **on-disc carrier** (previously "unclassified") is now pinned too: a plain 4bpp TIM at `PROT.DAT` offset `0x7F40` (framebuffer `(896, 0)`, CLUT `(0, 510)`), so the font is decodable **without** a save state (`legaia_font::Font::from_disc_tim_and_scus`; the WASM site's pause menu uses it). Byte-verified vs the save-state extraction. Listed here only so the older "open" framing doesn't get re-opened. |
| Inline dialog-box format (`0x1F`-lead segments) | resolved (init-arm count corrected; session-end semantics open) | `disassembly` | [details ↓](#inline-dialog-box-format-0x1f-lead-segments) |
| Tetsu 4-option spar menu mechanism | resolved | `capture` | The menu is a standard `0x29` 4-option **MES inline picker** in the sparring partner's dialogue (cursor `*(0x801C6EA4)+0x0C`; confirming **index 2** "I want to practice with you." starts the spar - live `0x03->0x09->0x15`, driven by the dialog SM not the field VM). It uses the **immediate-labels** form (labels straight after the N jump entries, no continuation byte) - `parse_picker_at` rejected it, now fixed, so town01 decodes the spar menu + its other pickers. Engine: `World::CarrierMenu` presents the picker and engages the carrier only on the index-2 fight option (was any-accept). Tests: `parses_immediate_labels_picker`, `tetsu_spar_picker_disc`, `carrier_spar_menu_*`, the updated `training_battle` legs. |
| Pause Items/Magic screens: remaining sub-flows | resolved (dim-bit residual closed) | `disassembly` + `capture` | [details ↓](#pause-itemsmagic-screens---remaining-sub-flows) |
| What does PROT 0892 (`card_data`) carry, and who loads it? | resolved (the memory-card screen's JIS X 0208 level-1 kanji font; an `asset::pack` of two TIMs, not a truncated stream) | `disassembly` + `capture` | [details ↓](#prot-0892-is-the-card-screen-kanji-font) |
| Which routine samples the card-screen kanji page (VRAM `(320..447, 256..511)`, CLUT rows 475..482)? | resolved-negative (nothing does - it is loaded and parked) | `disassembly` + `capture` | A sampler would have to carry tpage `0x15`/`0x16` and CBA `0x76C0 + plane * 0x40`; a byte sweep of SCUS plus every based overlay finds neither as an immediate or a data halfword, the uploader `FUN_800198E0` records no handle, and a GPU-FIFO watch at card-screen entry (mode `0x16 -> 0x17`) draws nothing from the page. The USA build ships the Japanese glyph page and never reads it. See [`save-screen.md`](../subsystems/save-screen.md#the-card-screens-kanji-page-is-never-sampled). |
| What is the battle plaque's element badge, and how is it selected? | resolved (a caret escape inside the monster's own name string) | `disassembly` + census | The badge is not a separate field: the name carries `^A`..`^H`, the decoder takes `letter - 'A'` as the index into the badge strip `0x8B..=0x92`, and element -> caret is the fixed permutation `[4, 3, 0, 2, 1, 5, 6, 7]`. The map is a **zero-exception bijection** over the shipped records - the "`^H` Cort is an exception" reading came from a name census, not from the decoder. Parser `MonsterRecord::plaque_badge`; see [`field-menu.md`](../subsystems/field-menu.md#status-element-badge-on-the-roster-panel). |
| How is the text-cell placement run seeded, and what is the `overlay_0897_801dbc30` dump? | resolved (three placement records, four stores each in a fixed order; the dump is a chimera of two PROT entries) | `disassembly` | The loop is PROT 0898's `0x801D3FC0`, based at `0x80076C10 + 0x408` (placement record 43, bound `sltiu 0x2E`, so records 43..45 - not 46 scratchpad records). Per record it writes `+4`, `+0xC`, `+0xA`, `+2`, and the `+2` store carries the record's **previous** `+0xA`, read before the `+0xA` store lands. The `overlay_0897_801dbc30` dump splices PROT 0897's `0x801EA448` onto this 0898 loop past its `j 0x801EA7AC`, through 0897's over-read. See [`script-vm.md`](../subsystems/script-vm.md#the-overlay_0897_801dbc30-dump-is-a-chimera-of-two-prot-entries). |

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

**Engine consumer (faithful path):** `engine_core::inline_dialogue` / `World::step_inline_dialogue` (PORT `FUN_80039B7C`) drives the whole inline script through the real field VM, so a chosen option's branch handler executes its `SET`/`CLEAR` flag ops + scene changes before the reply (`World::toggles.use_vm_dialogue`; `play-window` runs this path by default, `--simple-dialogue` opts out).

**Pre-first-segment prologue now runs (VM-dialogue path):** the field-VM dialogue runner (`World::toggles.use_vm_dialogue`) executes the interaction prologue before the first segment. The engine keeps the truncated `field_npc_dialog` buffer for the default renderer and stores the **untruncated** record alongside it (`man_field_scripts::placement_inline_prologue` → `field_npc_dialog_prologue`, body + entry PC + first-segment offset); on interaction the runner is started via `InlineDialogue::with_prologue` from `entry_pc` so the prologue's `SysFlag.Test`/`JmpRel` chain selects which segment the box opens at per story state, falling back to the first segment if the prologue can't reach one (never worse than the truncated path).

**Open on this thread:** retail's interaction cursor is one instruction past the record's spawn-section `0x21`, not `script_pc0` (see [`script-vm.md`](../subsystems/script-vm.md#the-interaction-cursor-one-record-two-consecutive-scripts)). Entering at `script_pc0` trips that terminator on the first step and takes the fallback before the interaction section runs at all. The port derives the cursor (`placement_interaction_record`) but applies it to **door** records only: moving the NPC path onto it regresses `retock`'s innkeeper (picker resolves, charge and restore do not run).

Disc-gated `field_interact_dialogue_disc` pins the prologue map's byte-consistency + non-vacuous presence on town01; synthetic `inline_dialogue_prologue_selects_segment_by_story_flag` / `…_falls_back_when_it_cannot_reach_a_segment` pin the selection + fallback.

**Multi-segment box packing resolved:** the SM packs **consecutive** `0x1F` lines into one window of `_DAT_801F2740 = 3` rows - a line's `0x00` terminator immediately followed by another `0x1F` is "same box, next row" - and the box ends after at most three rows at the post-page control byte. `FUN_80039B7C`'s state-`0x2` advance (`for (; 0x1e < *pbVar4; ...)`) masks `(*pbVar4 & 0xF0) == 0xC0` and consumes the escape's data byte, so a `0xC?` escape whose argument lands in `0x00..=0x1E` (e.g. `0xC1 0x00`) doesn't terminate the line early.

Decoded by `legaia_mes::dialog_box` (`pack_box` / `pack_boxes`, `LINES_PER_BOX = 3`, `Dispatch` for the terminating control byte); disc-gated `field_dialog_boxpack_disc` pins it on real town01 bytes (all 561 packed boxes ≤ 3 lines; the Tetsu sparring opening packs as three `0x24`-chained 3-row pages → a 4-option `Picker`; the `Mist appeared, .., but` line survives its `0xC1 0x00`). The contiguous box run stops where the pool hands control back to the field VM (a non-pager control byte → `Dispatch::Unknown`), which the faithful `World::step_inline_dialogue` path runs as bytecode. Nothing further open on this thread.

### PROT 0892 is the card-screen kanji font

*Status:* resolved - grade `disassembly` + `capture`

Loaded only by mode-22 `CARD INIT` `FUN_8002574C`: alloc `0x19000` at
`0x800257D0`, retail leg `li a0,0x37e` + `jal 0x8003eb98` at `0x8002580C` (raw
TOC `0x37E` = extraction 0892), then a pack walk at `0x8002581C..0x80025850`
handing each member to `FUN_800198E0`. Two `0x8220`-byte 4bpp TIMs (CLUT `(0,
475)`, pages `(320, 256)` / `(384, 256)`) whose CLUT is a bit-plane selector
over a 1bpp font packed four planes deep - 12 px pitch, 2,965 inked cells = the
level-1 kanji count, plane 0 in ku-ten order. The "12 MB LZS container" figure
was the superseded over-reading span; the entry is 33 sectors. See
[`data-field.md`](../formats/data-field.md).

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
| How does a Muscle Dome round enter battle mode? | resolved (the arena stores mode word `0x14`) | `disassembly` | PROT 0977 stores `0x14` into `0x8007B83C` at `0x801D15B8` (`sh v0,-0x47c4(v1)`), so a round takes `FUN_8001DCF8`'s mode-`0x14` arm like any battle. A round's slot residency itself is not captured (open row in [`open-rev-eng-threads.md`](open-rev-eng-threads.md)). |
| When does battle mode init close the field bank? | resolved (only when the mode word reads `0x14`) | `disassembly` + `capture` | `FUN_8001DCF8`'s close-slots-6-and-3 and latch-clear arm is gated `lh 0x8007B83C` / `bne 0x14` at `0x8001DF74..0x8001DF80`. A battle transition reaches it; a minigame overlay's call with the mode word `0x18` skips it, so after the Baka Fighter's warp slots 2 and 6 are both enabled over one region - slot 6's header PROT 0876's, the samples PROT 0869's ([`audio.md`](../subsystems/audio.md#retail-capture-of-the-slot-2--slot-6-residency)). |
| Which field scripts push SFX cues, and how are the ids keyed? | resolved (op `0x36` sub `0` / `4` and motion op `0x09`, ids straight from the bytecode) | `disassembly` | `jal 0x80035B50` at `0x801E0348` and `jal 0x80035BAC` at `0x801E03D8` (PROT 0897), `a0 = (s16)word1`; motion op `0x09` at `0x80039178`. Ids below `0x200` key the static table, the rest the scene prescript's record 0; across every scene MAN only `balden2`'s `0x20B` falls one row past its bank ([`sfx-table.md`](../formats/sfx-table.md#the-fields-producers-op-0x36-and-the-motion-vms-op-0x09)). |
| Which bank does an op-`0x36` sub-`1` request stream? | resolved (a `vab_01` bank, by four overwriting arms) | `disassembly` + `capture` | `FUN_800243F0` at `0x800248B4..0x8002494C`: `< 2000` -> `vab_01 + 2`, `2000..=2999` -> `+ id - 2000` in slot 3, `>= 3000` -> `+ id - 3000` in slot 6, `0x1000` parks. `*(0x8007BBE4)` reads `1072` in the catalogued states ([`sfx-table.md`](../formats/sfx-table.md#the-side-band-bank-a-field-script-selects)). |
| Who writes the frame-step floor `DAT_8007B9D8`? | resolved (each mode's init, and the scene through move-VM ext `0x2F`) | `disassembly` + `capture` | Field init writes `2` (`0x801D6990`); jump-table slot `0x2F` of `0x801CE868` stores the halfword operand (`0x801D45E4`), and five scene prescripts carry it - `opdeene`'s record 16 opens with operand `3`, which the cold-boot capture holds. Name entry `FUN_801F03F0` holds `1` while open ([`actor-vm.md`](../subsystems/actor-vm.md)). |
| How do the field bank and the class-2 bank share SPU memory? | resolved (one region, refilled per mode, behind a latch) | `disassembly` | Slots 2 and 6 share a base. The field init loads PROT 0876 into slot 6 only while `0x8007BAFC` is clear and closes 2 / 7 / 8 / 11; battle init closes 6 and 3 and clears it (`0x8001DFC0`), as do the minigame warp (`0x800259A4`, `gp+0x7E4`) and op `0x36` sub `3` ([`sfx-table.md`](../formats/sfx-table.md#one-region-per-mode-slot-2-and-slot-6)). |
| What does a cue whose bank is closed play? | resolved (nothing) | `disassembly` | The drainer `FUN_80016B6C` skips a cue whose mixer record's `+0xB` byte is zero (`lb v0,0xb(v1)` / `beqz` at `0x80016CE4..0x80016CEC`), and the VAB close `FUN_8001FF58` zeroes that byte in the delay slot of its `jal 0x80068C80`. So a category-6 cue in battle is silent, and `FUN_80035BD0(0)` is a cancel in the field. |
| What is `FUN_80035B50`? | resolved (a round-robin ring of four, not a delay queue) | `disassembly` | It writes the cue and a zero timer into slot `gp+0x158`, latches that slot into `gp+0x15A` and advances the cursor modulo four with no free-slot search; `FUN_80035BAC` sets the latched slot's countdown and `FUN_80035BD0` overwrites its cue. A fifth pending cue replaces one ([`sfx-table.md`](../formats/sfx-table.md)). |
| Why does the port sustain more sounding voices than retail on the same track? | resolved (two instrument defects, no engine change owed) | `capture` | The pairing was never aligned: frame 0 of an engine trace is tick 0 of a track while a capture sits wherever play parked it, and the `s3_rimelm_freeroam` window aligns at engine frame `3111` of `3601`. Aligned, the sides carry the same ten packed ADSR words, the same per-tone key-on count on nine of ten tones, and `71` engine key-ons against `67` retail. The rest is the envelope channel, which a per-vsync save-state capture cannot measure at all - PCSX-Redux steps ADSR on an audio-paced thread. The surviving comparand is the key-on **rate**. See [`audio.md`](../subsystems/audio.md#align-the-windows-before-comparing-them). |
| Which voice clip does a Seru cast play? | resolved (the **module** names it, not the caster) | `disassembly` | Each slot-B cast image hardcodes a literal cue id near its head - 62 of the 64, with PROT 0936 and 0937 forming theirs at run time - so the mapping is per module. Resolved through the cue dispatcher the ids land on seventeen streamed-audio files: `XA7`, `XA9..15`, `XA18..20`, `XA22`, `XA23`, `XA25` and `XA34`. Three of the literal ids sit **below** the `0x100` streamed threshold and are ordinary ring cues instead. |
| What bounds the cue dispatcher's streamed-voice leg? | resolved (two decline gates, and **nothing** bounds the table index) | `disassembly` | `FUN_8004FCC8` tests `sltiu v0,s0,0x100` at `0x8004FCD4` and nothing else: above the threshold it declines on the context byte `*(gp+0xA0C)+0x276` being non-zero and on the drive-idle poll `FUN_8003DE7C(1)` returning non-zero, then indexes `DAT_800788B8` at `id - 0x100` with no upper bound (`lhu` at `0x8004FD44`). The clip slot is `(id - 0x100) >> 3` with slots 1 / 3 / 5 remapped to `0x1A` / `0x1B` / `0x1C`. Measured off the executable the table is `0x110` entries with live rows as high as `0x10F` and several interior zero runs (`0x37..0x40`, `0x78..0x88`, `0xE8..0x10A`); a reader that stops at `0x40` drops every cast cue. |
| Which tracks do the in-world minigames start? | resolved (through the **piecewise** extraction map) | `disassembly` | The loaders name PROT entries `1043` / `1048` / `1054`, which the `music_01` bank's piecewise map sends to global BGM ids `2055` / `2060` / `2066`. Subtracting a single `990 + slot` base gives `2053` / `2058` / `2064` - three tracks off, and the gap at `1056`/`1057` is why one base cannot work. See [`music-tracks.md`](music-tracks.md). |
| What does the sequencer's pause do to sounding notes? | resolved (it is a **key-off**, not a freeze) | `disassembly` | `FUN_800628F0` mode `0` falls through both compares (`bne a2,v0(=1)` at `0x80062988`, `bne a2,zero` at `0x800629D8`) to `ori v0,v0,0x2` / `sw v0,0x98(v1)`, raising slot flag `0x2`. The per-tick `FUN_80062F98` tests `andi v0,v0,0x2` and calls `FUN_800638D8`, which calls `FUN_800684CC` to release every sounding voice whose owner halfword matches the `(sequence, channel)` key, stores `0` at the channel's `+0x14`, and clears the flag with `li v1,-0x3` / `and`. The play cursor is untouched, so resume continues where it stopped - the notes do not. |
| What drives the CD / XA transport? | resolved | `disassembly` | `FUN_8003D764` is an 11-state machine over the transport word at `0x800111C4`. The port models it as `xa_transport`, tagged `REF:` rather than `PORT:` - the states are the shape the engine's own streaming follows, not a routine a host calls. |
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
| What do `bse.dat`'s record columns mean? | resolved (the static SFX-table columns; `+4` is a `u8` category) | `disassembly` | [details ↓](#bsedat-record-columns-and-the-gp0x678-consumers) |
| Who consumes the `bse.dat` record-table pointer `gp+0x678` (`0x8007B990`)? | resolved (cue router `FUN_8004FE5C` + the overlay-0971 sound test, via `lui`+`lw` the word scan cannot see) | `disassembly` | [details ↓](#bsedat-record-columns-and-the-gp0x678-consumers) |
| When does `bse.dat` load? | resolved (battle-scene setup, not boot) | `disassembly` | `FUN_8001FA88`'s only caller on the disc is `0x80051A3C` in battle init `FUN_800513F0`, reached from the battle tick `FUN_80046A20` at `0x80046F74` under `ctx[+0x11] == 0`. |
| `bse.dat` 888 vs its 1195 sibling | resolved (one format, two occupants of the `>= 0x200` bank role) | `disassembly` | 888 is the battle occupant; a scene prescript's record 0 fills the same slot in the field (`FUN_8001F7C0` at `0x8001F864` repoints it). 1195 is such a prescript and the detector should keep matching it. |
| Is there a second `bse.dat` record family past the table (0888 `0x94C`, 1062 `0x51AD`)? | resolved (no - a neighbouring file's PsyQ `VagAtr` tone rows left in the sector) | `capture` | [details ↓](#no-second-bse-dat-record-family) |
| What is `_DAT_8007BA9C`? | resolved (the BGM-swap **force-reload** latch) | `disassembly` | Read at `0x8002457C` and XOR-paired with `_DAT_8007BAB8`; when the two disagree the next track request re-reads its bank instead of reusing the resident one. It is the barrier op-`0x35` sub-op `9` waits on before it makes sub-op 1's own `_DAT_8007BAC8` store. |
| What is `FUN_80065034`? | resolved (`SsUtKeyOnV`) | `disassembly` | Its arguments are `(voice, vabid, prog, tone, note, fine, voll, volr)` - the **second** is the VAB id, which is what makes the cue's bank explicit rather than implied by the resident bank. The `CUE_LEVEL` name an overlay port gave the argument ("channel mixer level") describes no parameter of it. |
| How is a retail VAB carried in its DATA_FIELD stream? | resolved (**two** chunks, and the bodies are the second) | `disassembly` + `capture` | The 4-byte word in front of every `pBAV` is a chunk header whose payload is the bank's **header part** only - `0x20 + 0x800 + 0x200 * ps + 0x200` - which holds in all 424 retail banks. The VAG bodies are a separate chunk of the same stream, and that chunk's own header is the "+4 skew" the decoder had recorded as a property of the format. Six entries put the SEQ chunk **before** the body chunk, so a fixed `{0, 4}` probe cannot recover their origin; `legaia_vab::vag_body_origin` walks the stream for it. |
| What is the resident `monster.snd` index? | resolved | `disassembly` | `[u32 reserved][u32 count][u32 start_sector[count + 1]]` at `0x801C8980`, staged by PROT 0895's `memcpy` of `0x400` bytes at `0x801CEF74`. The reader `FUN_8003E104` bounds its argument against the count word, streams sectors `[table[i], table[i + 1])` relative to raw TOC entry `0x37D`'s start LBA, and forks on the build-mode flag for the dev filename path. The trailing entry is the archive's own sector length rather than a bank, which is what makes the span arithmetic total. |
| A VAB's VAG-body origin - is the `+4` a format property? | resolved (two wrongs that cancel on all but six carriers) | `disassembly` + `capture` | The parse reports a body origin four bytes early and the upload re-slices by the same four, so the pair cancels on 212 of 218 carriers and the skew read as a property of the format. Six entries break the cancellation - `0886`, `1058`, `1059`, `1063`, `1064`, `1065` - because their SEQ chunk precedes the body chunk, so the four bytes uploaded as sample data are SEQ bytes; the legal-filter share on those rises from 0.53-0.84 to 1.00 once the origin is resolved off the stream. `legaia_vab::vag_body_origin_at` + `parse_in_stream` answer it; [`vab.md`](../formats/vab.md). |
| What is in PROT 1062? | resolved (a single-chunk DATA_FIELD stream carrying a SEQ) | `disassembly` | One chunk, type byte `0x02`, payload `pQES`. It is the shape that made the stream walker's coverage of that entry read as partial rather than the entry being unparsed. |
| Where does a scene's entry BGM come from? | resolved (the scene's own script, not the loader) | `disassembly` | The BGM request word `_DAT_8007BAC8` has four `sw` sites disc-wide and all four are in PROT 0897: `0x801E012C` (field-VM op `0x35` sub-1), `0x801E0254` (sub-9), `0x801EAD1C` (the dev menu's `BGM CALL` row) and `0x801D6844` - a conditional `sw $zero` inside the per-scene field initializer `FUN_801D6704`. So the loader **clears** the id and the script installs it, and a port expecting a scene load to select the track has nothing to read. The sweep has to be `gp`-relative: 14 hits across the disc, none of them visible to an absolute-address scan. |
| What is the dev menu's `BGM CALL` arm? | resolved (it plays a track; cycling is a different routine) | `disassembly` | The arm at `0x801EACBC..0x801EAD20` inside `FUN_801EA9B0` indexes the sound-test table at `0x801F2E94` by the cursor `_DAT_801F2E90` - a 10-byte stride of `[i16 global bgm id][8-byte ASCII label]`, 71 rows plus an `OFF` row whose id reads `-1` and which raises `_DAT_8007B438` instead. The row index is **not** the id: the rows carry `2000..=2043` then `2045..=2071`, so cursor and id part company above the missing `2044`. The input half - stepping the cursor - is `FUN_801E9F64`. [details](../subsystems/world-map.md#fun_801ea9b0---dev-menu-row-action-dispatcher-1000-bytes) |
| What makes a captured SPU voice audible? | resolved (the envelope **level**, not the phase word) | `capture` | Mednafen's `ADSR.Phase` has no `Off` member: it runs `0` Attack through `3` Release, and a key-off parks a voice in Release forever, so a phase test reports every voice a state ever keyed as live. The audibility predicate is `ADSR.EnvLevel != 0` (the PCSX-Redux analogue is `ADSRInfoEx.EnvelopeVol`). Over the 98-state retail corpus, 1624 of 2352 voice slots sit at level zero, and reading the level gives a median of 7.5 audible voices per state - the same order as the engine's 4-8. Everything a phase-keyed comparison concluded about retail's voice counts was the predicate talking. |
| What is BGM id `0x1000`? | resolved (a **park sentinel**, shared by both streaming slots) | `disassembly` + `capture` | `4096` is outside the `2000..=2077` pool and the resolver never tries: at `0x8002454C` it compares `_DAT_8007BAC8` against `li v0, 0x1000` and falls into `0x80024560`, which copies the pending index `_DAT_8007BAB8` onto the loaded-index barrier `_DAT_8007BA9C`, so the equality test four instructions later skips the load. The block at `0x800244C0` applies the same test to the second slot (`_DAT_8007BABC` -> `_DAT_8007BAA0`). In the catalogued corpus only the ending states carry it. [`audio.md`](../subsystems/audio.md#0x1000-is-a-park-sentinel-not-a-track) |
| Does a save state's *scene* decide which BGM it holds? | resolved (**no** - `0x8007BAC8` does) | `capture` + `disassembly` | Of six catalogued `town01` PCSX-Redux states only one holds `2000`; the rest hold `2016`, the id `town01`'s own prescript selects. The `2000` state walked into the town from the world map, and `0x8007BA9C` / `0x8007BC64` corroborate through the resolver's own `base + (id - 2000)`. Pairing a retail capture with an engine trace by scene rather than by this word compares two pieces of music. |
| What is retail's reverb depth and work area? | resolved (`vLOUT` = `vROUT` = `0x3264`, `mBASE` -> `0x79020`) | `capture` | The depth pair is what `SpuSetReverbDepth` writes and sits **outside** the 32-register preset block, so matching the Studio C coefficients says nothing about it; the work-area size `0x6FE0` is Studio C's own, a second confirmation of the preset. Per-voice `EON` reads `0x00FFFFFF` - all 24 voices - on every frame of a 90-frame per-vsync capture, where a `.mc` freeze shows only the 15-22 a given moment has keyed. The engine's placeholder `0x4000` is replaced by the measured value. |
| Is slots-per-note a difference between the port and retail? | resolved (**no** - it is a property of the arrangement) | `capture` | Sounding voices over distinct `(sample, pitch)` pairs reads retail `1.158` / port `1.002` on track `2000` and retail `1.002` / port `1.037`-`1.061` on track `2016` - the sign reverses with the track. A second retail capture taken in the same town on the same walk reports `1.002`, which also disposes of the "town SFX in the retail window" caveat. The sequencer's three note-drop paths fire **zero** times across the window. [`audio.md`](../subsystems/audio.md#comparing-per-voice) |

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

### bse.dat record columns and the gp+0x678 consumers

*Status:* resolved - grade `disassembly`.

`gp = 0x8007B318` (`lui gp,0x8008; addiu gp,gp,-0x4ce8` at `0x80026CA8`), so
the record-table pointer `FUN_8001FA88` stores at `0x8001FBC0`
(`sw a0,0x678(gp)`) lives at `0x8007B990`. Seven readers exist and every one
forms the address as `lui rX,0x8008` + `lw rY,-0x4670(rX)` - the pair the
five-form address scan does not accept, which is why the consumer read as
untraced: `0x8004FFAC` / `0x8004FFE0` / `0x80050078` in the battle SFX-cue
router `FUN_8004FE5C`, and `0x801CEE48` / `0x801CEFC0` / `0x801CF038` /
`0x801CF0A0` in the overlay-0971 debug sound test. Sweep:
`scripts/ghidra-analysis/find-gp-relative-refs.py`.

The columns are the static SFX-table columns, pinned by **shared code** rather
than analogy: `FUN_80016B6C` picks its arm at `0x80016C24` (`slti v0,s0,0x200`),
resolves either `0x8006F198 + id*8` or `record[id - 0x200]` off `gp[0x5B8]`,
and then falls into one block of field reads from `0x80016CB0` - `+0` program,
`+1` tone (`+i` per voice), `+2` note level, `+3` low-5 voice count / `0x20`
sustained, `+4` category into the 12-byte mixer record `0x80091508 + cat*12`.
`+4` is a `u8`: `+5..+7` are zero in every retail row. The router's two tinted
legs (`sb v1,-0x31c(v0)` at `0x8004FFEC`, `sb v1,0x40c(v0)` at `0x80050084`)
both reduce to `record[cue_id - 0x200] + 4`, and the sound test stores `7` at
`0xDC(gp[0x678])` before enqueuing cue `0x21B` = `0x200 + 27` - row index is
cue id minus `0x200`, and the category byte is rewritten per cue. Layout and
carriers: [`bse-dat.md`](../formats/bse-dat.md).

### No second bse dat record family

*Status:* resolved (negative) - grade `capture`

Entry 888's 1,716-byte tail is byte-identical to entries 886 and 1063 at the
same file offsets; entry 1062's 1,616-byte tail matches entry 1056 (two bytes
differ where its SEQ padding clipped a row). The builder fill `C0 00 C1 00 C2 00
C3 00` occurs in 221 entries, always at offset `≡ 0x1C (mod 0x20)` (`VagAtr +
0x18`), and only these two carry it without a `pBAV` of their own.
`bse_bank::detect` stops on a residue row's structurally-zero `vib / por` fields
- reliable, but a foreign row's field. 1062 as a whole is a SEQ-only `music_01`
entry (sound-test track 72) borrowing another entry's bank. See
[`bse-dat.md`](../formats/bse-dat.md).

## Title / boot / overlays

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| How many rows does retail's title menu have? | resolved (two) | `disassembly` | The title tick wraps its row counter with `andi v1,v1,0x1` at `0x801DDC00` and the confirm arm branches on row 0 against everything else (`overlay_title_801dd6b8.txt`), so no title route yields Options; retail reaches Options from the pause menu ([`host-drift.md`](../tooling/host-drift.md#the-boot-options-screen-is-the-pause-menus-on-both-hosts)). |
| How does a slot-B module hand a spawn call its record? | resolved (three shapes beyond an adjacent pair) | `disassembly` | The pair can complete in the `jal`'s delay slot, the pointer can arrive through a saved-register copy formed up to 256 words back, and `switch` arms can each load `$a2` in the delay slot of a `j` to one shared call. The delay-slot shape was 52 of the 97 out-of-image drops the band reported as neighbours' records ([`slot-b-module-layout.md`](../formats/slot-b-module-layout.md#resolving-the-pointer-a-spawn-call-is-handed)). |
| Which images take the slot-B layout walk? | resolved (the seventy at the link base, not the sixty-four in the index band) | `disassembly` | The walk's three regions are each recovered by resolving a word against `0x801F69D8`, so it belongs to every image mapped there: the 64-entry cast band plus 0900 / 0901 / 0967 / 0968 / 0969 / 0978. Selecting on the band left the six without a head-table claim (0968 `60.4% -> 99.8%`, 0969 `78.7% -> 99.4%`, 0978 `51.3% -> 91.6%`). `is_slot_b_module` keeps the band question - which images the cast dispatcher reaches - and `is_slot_b_image` the layout one. |
| What is PROT 0967's residue? | resolved (a head table, dumped code, one leaf, a consumed prompt pool and a neighbour's tail) | `disassembly` | 408 bytes of head jump table, 2,744 bytes of dumped code, one 92-byte frameless leaf `FUN_801F7628` at file `0xC50..0xCAC`, 1,757 bytes of prompt pool and 1,143 bytes inherited from PROT 0966. The pool is consumed: twenty-eight strings formed by the module's own code at forty-one sites between file `0x278` and `0xA7C`. The leaf is reached by `jal` from `0x801F7184` and `0x801F7460` inside the image, so a slot-B image can call itself. The link-base layout walk admits 0967 at 96.6%; the cast band (`0903..=0966`) is a different predicate. [`byte-accounting.md`](../tooling/byte-accounting.md#what-the-overlay-residue-that-is-left-actually-is) |
| What is in the PROT 0898 head block below `0xDF8`? | resolved (a string pool and **twenty-two** jump tables; two more sit above it) | `disassembly` | Each consumer is `sltiu` / `beqz` / base / `sll 2` / load / `jr`, so a table's extent is its `sltiu` bound times four: twenty-two bases tile the head, 850 arms in all, among them `0x801CF1CC` (179 arms, `jr` at `0x801EA9FC`) and `0x801CF49C` (5 arms, `ctx[+0x28A]`). Above the head sit nine arms at `0x801CF614` (`jr` at `0x801F3AC0`) and seven at `0x801CFA2C` (`jr` at `0x801F3EB4`), with the Seru side-effect banner pool between them. The old count of nine measured VA-word runs, which merge where tables abut. `legaia_asset::battle_jump_tables`; [`byte-accounting.md`](../tooling/byte-accounting.md#the-0898-head-is-twenty-two-jump-tables) |
| What is PROT 0970's 131,172-byte zero hole? | resolved (the overlay's own **uninitialised data region**) | `disassembly` | It decomposes exactly, in the image's own operands: the `0x50` decode context at `0x801D19A0`, two `0x7800` slice staging buffers at `0x801D19F0` and `0x801D91F0`, four globals at `0x801E09F0` and the `0x11000` STRv2 VLC table destination at `0x801E0A00`, ending flush against the unpacker. The loader transfers the whole sector extent, so an overlay's `.bss` travels with its code and reads as a hole. The table's packed source is in the same image at `0x801F1AE8`. [`byte-accounting.md`](../tooling/byte-accounting.md#an-overlays-uninitialised-data-region-travels-with-its-code). |
| What is PROT 0974's 10,836-byte run of readable text? | resolved (an 81-record `0x84`-stride **roster**) | `disassembly` | Not a string pool: the loop operands in `FUN_801CED68` walk a table at `0x801CEF40` on a `0x84` stride, and the text inside each record is Shift-JIS read as little-endian `u16`. The image is a USA-build dev module carrying Japanese dev text - 34 of 37 SCUS `jal` targets land on this disc's function heads, where the foreign-build control scores 0 of 42 - so "Japanese text" is not evidence of a foreign build. Parser `legaia_asset::other3_roster`. |
| What is PROT 0975's undumped `0x801D4138` run? | resolved (PROT 0972's **code**, inherited at the same file offset) | `disassembly` | The 1,760 bytes from file `0x5920` are byte-identical to PROT 0972 at the same file offset - the packer's buffer is indexed by file offset and never cleared, so a short entry ends in the previous image's bytes. That is why the run has no prologue, no `jr ra` and no caller, and why every reading that tried to make it 0975's own (a jump table, a data block, a function interior) had to fail. [`disc-coverage.md`](../tooling/disc-coverage.md). |
| Which of PROT 0978 / 0979 / 0980 is the dance overlay? | resolved (**only** 0980) | `disassembly` | 0978 is `field_back_read`, the staged background loader; 0979 is the battle-intro image; 0980 is the dance overlay. A doc line listing all three as dance variants named one of them right. |
| The front-end mode chain, re-derived independently | resolved (the six stores, the four-word edge and the INIT hand-offs all hold) | `disassembly` | A three-form opcode scan for stores to the mode word over 84 images finds **53** store sites, and every row of the boot chain, the mode-change edge's four cleared `gp` words and the per-handler INIT hand-offs survive it unchanged. Mode 18 was the one gap: it *does* have a hand-off - `li v0,0x13` at `0x801CE8D4` then the store at `0x801CE8DC`, inside `0x801CE844` in PROT 0902. |
| Which mode does the retail title screen run under? | resolved (**card** mode `0x17`, not `0x10`) | `disassembly` | The front end is a chain of six mode stores, each written by the handler that hands off, not by a table's `next` field: `0x8001D5B8` -> `0x10`, `0x801CEC94` -> `0x11`, `0x801CF4D4` -> `0x16`, `0x80025974` -> `0x17`, `0x801DFC00` -> `0x02`, `0x80025E50` -> `0x03`. Mode `0x10` is one frame of logo INIT; the alternate arm at `0x801CF4E4` is the dev CONFIG route on the entry word. `0x80025974` is `li v0,0x17` + `sh v0,-0x47c4(at)` at `0x8002596C`; `0x801CEC94` is the `sh` in a `jal` delay slot. See [`boot.md`](../subsystems/boot.md). |
| What reads the title entry word `_DAT_8007BB00`? | resolved (two readers, both real) | `disassembly` | `0x801CF4B0` in the boot image and `0x801DD97C` in the title overlay - the port's `title_overlay::ENTRY_WORD_ADDR`. The claim that the word had no engine counterpart was wrong in both directions: it has readers on the disc and a port site. |
| What does a mode-change edge clear? | resolved (**four** gp words, not three) | `disassembly` | The edge at `0x800161F4` clears `0x8007B938` alongside the words at `gp+0x538` and `gp+0x55C`; `gp+0x564` and `gp+0x494` are mode *copies*, not cleared state. |
| `_DAT_8007B8C2` - how many writers? | resolved (exactly one) | `disassembly` | `main()` at `0x80015F08` (`sh v0,0x5aa(gp)`), confirmed across 70 access sites in 84 images. The build-mode selector is set once at startup and never re-written, which is what makes the dev/retail fork in `FUN_8001F87C` a load-time decision. |
| `_DAT_8007B8C2` polarity, and its writer | resolved (docs were backwards) | `disassembly` + `capture` | [details ↓](#_dat_8007b8c2-polarity-and-its-writer) |
| Actor-VM (`FUN_801D6628`) program source - which carrier, what selects one | resolved (menu-overlay-resident program table) | `disassembly` | The interpreted programs are data in PROT 0899's own data segment (file `0x16260..0x16740`), one per `jal FUN_801D6628` caller via `lui`+`addiu` (or a forwarded register); byte 1 of each instruction indexes the window descriptor table at `0x801E4738`, making the VM the menu's window-widget choreographer. No per-scene carrier exists - resolution is per-boot. Spec [window-script.md](../formats/window-script.md); scanner `legaia_asset::widget_script::scan`; engine wiring `engine-core::menu_widget`. Superseded sprite-VM readings: [re-do-not-re-walk.md](re-do-not-re-walk.md#menus--ui). |
| `title.pak` PROT entry | resolved | `capture` | [details ↓](#titlepak-prot-entry) |
| Does `FUN_801CE9C0` pin the per-logo quads? | resolved (no - it uploads; the quads are a descriptor table in the same image) | `disassembly` | [details ↓](#the-publisher-logo-quads) |
| The title menu's law (`FUN_801DD35C` sub-mode `0x10`) | resolved + ported on both hosts | `disassembly` | [details ↓](#the-title-menus-law) |
| `FUN_801D71F0` and its "shared armament placer" `0x801E5AE8` | resolved (one routine, `FUN_801E5A08`, unreferenced) | `disassembly` | [details ↓](#fun_801e5a08-the-per-slot-equip-applier) |
| Save-screen info-panel view mode `4` ("Return") | resolved (dead - the grid has 15 cells) | `disassembly` | [details ↓](#the-dead-return-view-mode) |
| Title screen mode-table PROT | resolved (no row is named for it - it runs under the `CARD` mode pair 22/23; `FUN_801DD35C` is PROT 0899 `+0xEB44`) | `disassembly` | [details ↓](#title-screen-mode-table-prot) |
| Load-screen panel 9-slice geometry | resolved (engine renders byte-perfect) | `capture` | Pinned in [`subsystems/save-screen.md`](../subsystems/save-screen.md#pinned-9-slice-tile-rects-system-ui-tim-clut-row-2): retail composes the 81×29 panel at dst `(6, 4)` from 14 textured-sprite primitives (GP0 cmd `0x64`) sampling the system-UI sheet with CLUT `(32, 511)`. The exact per-tile rects are exported as `legaia_asset::title_pak::OVERLAY_SYSTEM_UI_PANEL_*` and emitted by `legaia_engine_render::save_select_chrome_draws_for` (covered by `save_select_chrome_emits_9slice_panel_and_pills` test). No interior fill sprite is drawn - the "marbled blue" look is the dimmed title art bleeding through the empty middle of the frame. |
| Key-item area consumers (`0x800859E8..0x80085A40`) | resolved (narrow negative; the reader enumeration is closed) | `disassembly` | [details ↓](#key-item-area-consumers) |
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
| PROT 0968 - what it is, who loads it, and how big it really is | resolved - identity and extent by disassembly, residency by capture | `capture` | [details ↓](#prot-0968---the-cort-battle-stage-overlay) |
| `0x80010390` - the SCUS word that looked like a lead on 0968 | resolved: it is the slot-B overlay destination pointer, shared by every slot-B entry | `disassembly` | [details ↓](#0x80010390-is-the-slot-b-overlay-destination-pointer) |
| Who registers the MDECin DMA callback wrapper `FUN_801CFE98`? | resolved (nobody - linked libpress residue; retail hooks only the MDEC-out twin) | `disassembly` (the residue reading `inference`) | Zero references in eight forms over 1234 images (SCUS, 31 based overlays, every raw PROT entry). Its twin `0x801CFEBC` - the same nine instructions with channel 1 - is what PROT 0970 calls, at `0x801CF524` (clear) and `0x801CF9C4` (install). See [`cutscene.md`](../subsystems/cutscene.md). |
| PROT 0895 (`init.pak`) identity + base | resolved (slot A `0x801CE818`; mode 16's whole body is its `FUN_801CE9C0`) | `disassembly` | Base recovered from the image's own `jal` graph (8 votes, 22/25 pointer resolution, 7 string anchors); `0x801CE9C0` is a clean `addiu sp,sp,-0x230` entry at file `+0x1A8`, 784 bytes, building the four logo primitive records and storing mode `0x11` at `0x801CEC94`; the code region closes byte-exactly at `+0x216C` (20 functions) ahead of the first TIM at `+0x21C4`. SCUS `FUN_8002612C` (mode 16) is a frame, that `jal`, and an epilogue. |
| Does anything draw the `init.pak` WARNING screen? | resolved-negative (the TIM is uploaded and never drawn) | `disassembly` + `capture` | PROT 0895 uploads the health-warning TIM to VRAM `(704, 0)` and gives it descriptor `1` of the six-record sprite table at `0x801F369C`, but none of the five `FUN_801CFBB8` call sites in that image passes id `1`, nothing else on the disc references the table, and a boot capture never draws from that page. The USA build loads the screen and skips it. See [`boot.md`](../subsystems/boot.md#the-health-warning-is-never-drawn). |
| Which title sub-mode does a cold boot show - `0x02` or `0x10`? | resolved (`0x10`, always) | `disassembly` + `capture` | [details ↓](#a-cold-boot-always-shows-title-sub-mode-0x10) |
| How the slot-B module pager turns a request into a PROT entry | resolved (`extraction entry = a0 + 895`, with no upper bound) | `disassembly` | `FUN_8003EC70` adds a constant `895` to its argument and pages that extraction entry into slot B; nothing clamps the top of the range. Its site at `0x8005269C` passes `_DAT_8007B64A + 71`, so selector byte values `2` and `3` reach PROT `0968` and `0969` while `0` skips the load entirely. What writes the selector is the part still open. |
| Where does PROT 0896 link? | resolved (`0x801D4DF0` - and it calls no entry of this disc's executable) | `disassembly` | `recover_base` reports it on ten corroborating targets of eleven distinct internal `jal` targets; all 218 internal `j` instructions land inside the file there and none does at either previously cited base; ten of the eleven `jal` targets land on an `addiu sp, sp, -X` prologue; and three runs of consecutive in-image VA words resolve every word, one of them holding the base itself. Of its 322 calls into the SCUS range, **zero** land on a `SCUS_942.54` function entry, where `0897` scores 1203 of 1307 and `0899` 793 of 884 - so no USA loader reaches it and no residency capture is owed. [details ↓](#prot-0896-bat_back_dat-identity) |
| What is PROT 0970's run above its one-shot init flag? | resolved (two **MDEC command packets**, then the register-pointer block) | `disassembly` | `0x801D0D58` holds the quant packet header `0x40000001`, with the luma and chroma matrices at `0x801D0D5C` / `0x801D0D9C`; `0x801D0DDC` holds the IDCT packet header `0x60000000` and a static IDCT matrix. `0x801D0E60..0x801D0E9B` is fifteen hardware-register pointers (`0x1F801080..0x1F8010B8` DMA channels 0-2, `0x1F801820` / `0x1F801824` MDEC0 / MDEC1, `0x1F8010F0` DPCR). Neither uninitialised data nor code; claimed off `legaia_asset::fmv_dispatch::MDEC_*_PACKET_VA`. |
| What does PROT 0970's `FUN_801CFCDC` do? | resolved (the MDEC **quant table upload** - not an output-rect stager) | `disassembly` | It copies sixteen words from `a0[0..0x40]` to `0x801D0D5C` and sixteen from `a0[0x40..0x80]` to `0x801D0D9C`, then calls `FUN_801CFFDC(pkt, 0x20)` twice - the quant packet `0x801D0D58` (`0x801CFD48`) and the static IDCT packet `0x801D0DDC` (`0x801CFD58`); only the quant packet is written. `FUN_801CFFDC` ORs `0x88` into DPCR, programs DMA0 (`MADR = pkt + 4`, `BCR = (n >> 5) << 16 \| 0x20`), writes the packet header to MDEC0 through the pointer at `0x801D0E90`, and starts DMA0 with `CHCR 0x01000201`. [`cutscene.md`](../subsystems/cutscene.md) |
| What is PROT 0899's 3,552-byte zero run at file `0x1EB28`? | resolved (inter-asset fill, not uninitialised data) | `disassembly` | It sits between the save-menu atlas's end and the save-slot icon sheet at `0x1F908`, and no instruction in any image forms an address inside it (`find-gp-relative-refs.py --prot`, zero hits). An overlay's `.bss` is named by its own operands; this run is named by none. |
| What are PROT 0899's options-screen tables? | resolved (four, each bound to a consumer) | `disassembly` | The display layout `0x801E4404` (ten `[u16 row_id, u16 advance]`), the string-pointer table `0x801E442C`, the row-node list `0x801E44B8` (eight-byte nodes, zero-terminated - `lw` / `bnez` at `0x801D2A0C`) and the casino prize table `0x801E4518` (`0x60`-byte blocks of eight-byte rows). All four are formed by `lui`/`addiu` pairs in the options row renderer and the prize-exchange session. [`field-menu.md`](../subsystems/field-menu.md#options-screen) |
| Where does PROT 0901's own content end? | resolved (file `0x252A`, donor PROT 0900) | `disassembly` | 0901's code ends at `jr ra` on file `0x24DC` (`0x801F8EB4`), and the run from `0x252A` opens mid-routine on 0900's epilogue of the routine at `0x801F8E6C` (its prologue at 0900 file `0x2494`); only 0900 references it. The packer-buffer prediction was right and the sibling cut at `0x26B0` was not. The "0900 / 0901 shifted copy" notes in `static-overlays.toml` were an old entry-size artifact and are gone. |

### A cold boot always shows title sub-mode `0x10`

*Status:* resolved. Grade: **disassembly** + **capture**.

`FUN_801DD35C`'s `Init` arm (`0x00`) writes `state[+0x204] = 0x02` and then
overwrites it with `0x11` whenever the entry word `_DAT_8007BB00` reads
non-zero. On retail that overwrite always happens, so the `0x02` two-row menu
(rows y 107 / 120, confirm mask `0x44`, advancing to `0x14`) is unreachable
from a cold boot. Two independent legs hold it:

- The boot `init.pak` raises the entry word itself, unconditionally -
  `li s2,0x1` / `sw s2,-0x4500(s0)` at `0x801CEB84` with `s0 = 0x80080000`, in
  the mode-16 body, before it hands off. The three sites that store zero back
  are all behind dev-flag or pad-hold arms.
- The tick's own epilogue rewrites a surviving `0x02`: the `AttractDelay`
  (`0x11`) arm writes `0x10` at `0x801DDAC4` when its hold has drained, and the
  shared epilogue's exits at `0x801DFED8` / `0x801DFEF8` do the same.

A per-vsync cold-boot poll agrees: `_DAT_8007BB00` goes `0 -> 1` in the frame
the master mode steps `0x10 -> 0x11`, the sub-mode is written `0x11` on the
frame after the title mode is entered and `0x10` about 75 vsyncs later, and
`0x02` is never observed. Returning to the title from the attract FMV the word
reads `2`, so the overwrite holds on the second entry too.

The tick itself is the same routine: PROT 0899 at file `+0xEB44`, 12 104 bytes
/ 3 026 instructions, dispatching `0x801F0204` through the 24-word jump table
at `0x801CF244` (slot `0x11` = `0x801DDA90`) with 56 stores to that word and
one shared epilogue at `0x801DFC3C`. `_DAT_8007BAB4` is its pre-attract hold,
not an active-submenu index. Two readings fell with it -
[the sub-mode word's address](re-do-not-re-walk.md#the-title-sub-mode-word-lives-at-0x801dd920-and-0x02-is-a-screen-a-player-can-see)
and
[the slider clamp](re-do-not-re-walk.md#the-title-slider-state-0xeb4-is-clamped-to-0-0x2c).
Body: [`boot.md`](../subsystems/boot.md#a-cold-boot-always-shows-sub-mode-0x10-never-0x02).

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

*Status:* resolved for every entry, **0968** included - its residency capture is the [section below](#prot-0968---the-cort-battle-stage-overlay)

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
- **0968** = the **evolved-Cort battle's stage overlay**, battle-stage id `2` -
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

*Status:* resolved, residency capture included - grade `capture` on the
residency leg, `disassembly` on the loader chain and extent

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

Formation id `0xB5` is monster-archive id **181 = Cort's evolved second form**
(HP 65535; the first-form fight is a separate formation whose head is `0xB4`,
id 180, HP 50000 - both slots carry the display name "Cort"), read straight
off PROT 867 (`asset monster-archive --id 181`). So stage id `2` → param
`0x49` → extraction entry **968**, and the evolved-form Cort fight is its
only gate. The same byte against the same constant is what pages **0969**
mid-battle when a Cort form's HP reaches zero, which is the corroboration:
one boss, two modules, one formation-id test each.

**Residency is capture-confirmed.** The
`cort_evolved_battle_first_menu` PCSX-Redux state (evolved-Cort battle,
scene `jouine`, first command-input screen, before any cast; fingerprint in
[`scenarios.toml`](../../scripts/scenarios.toml)) shows the closing pair
exactly ([`check-0968-residency.py`](../../scripts/mednafen/check-0968-residency.py)):
the loader-B current-id tracker `0x8007BC4C` reads **`0x49`** and entry 968
is **100% byte-resident** at the slot-B base `0x801F69D8` over its own
`0xA28` extent, with the formation head `*(u8 *)0x8007BD0C = 0xB5` - the
same observation pair that pinned 0967 for the Tetsu tutorial. The
field-side ladder states bracketing the fight
(`cort_evolved_approach_cutscene`, `cort_evolved_pre_battle`) both show the
general co-resident library **0900** at 100% with tracker `0x05` instead,
so the page-in is bracketed to the battle load itself.

**Why the mid-cast library could never show it.** A sweep of the six
catalogued `cort_*_mid_cast` mednafen states with the same script reads
`*(u8 *)0x8007BD0C = 0xB4` in the four first-form states and `0xB5` in the
two evolved-form states - the selector's constant, live, and only in the
evolved-form phase. In every one, the slot-B page at `0x801F69D8` is 100%
byte-identical over all `0x1000` bytes to the state's documented cast
stager (0938 / 0940 / 0944 / 0961 / 0962 / 0966) with the loader-B tracker
`0x8007BC4C` reading that stager's id, while 0968's own `0xA28` window
matches at chance level (10.5-12.1%). A cast stager is a full slot-B page:
the first special attack or summon of the fight evicts the stage overlay,
which is why the closing capture had to be the battle's first command menu.
The stage-id byte `_DAT_8007B64A` reads `0x00` in all nine mid-fight /
bracketing states - the byte is transient around the load, not a persistent
mid-battle marker.

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

*Status:* resolved on the narrow negative; the reader enumeration is closed by three measured structural facts

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
quantity byte. Second, **the enumeration is closed, by three measured structural
facts.** The array sits `0xA640` above `gp` and ends at `gp + 0xA840`, so no
`imm(gp)` instruction can address any byte of it - the one form that has
produced false negatives elsewhere is arithmetically impossible here. Neither
`0x80085958` nor the block base `0x80084140` occurs as a literal 32-bit word
anywhere in SCUS or the 1233 PROT entries, so no pointer table offers an
indirect route. And every access materialises the block base inline
(`lui`+`addiu 0x4140`, `sll slot,1`, `addu`, `lbu|lb|sb ...,0x1818/0x1819`).
Decoding for that displacement pair gives **125 sites over six images**: SCUS
51 (13 functions, `0x8003004C..0x800430A0`), PROT 0899 **55** (19 functions;
four hide behind a `lui` in a branch delay slot), 0897 5, 0898 2, and the cast
modules 0941 7 / 0954 5. Every other based overlay and every other PROT entry:
zero. The earlier "156 hits across 11 files" is reproducible as a
displacement-only match whose tail is 31 data-byte coincidences in five scene
entries. Two absolute sites exist, both the new-game seed's slot 0
(`0x80034B10` / `0x80034B18`); **no instruction on the disc names an address
inside the key-item band**. Consumers the old list missed: cast modules **PROT
0941** (Steal) and **PROT 0954** (Fatal Decision) read the bag with no active
window - 0941 clamps its random slot against a live count (`0x801F7828`), 0954
does not (`rand & 0xFF` over all 256 slots, `0x801F81A4..0x801F81B0`, retry
cap `0x400`), making it the only reader that reaches the key-item band outside
the menu window. The `& 0x3ff` mask belongs to four packed-handle sites, not to
readers generally, and it admits slot 1023 - `0x5FE` past the array - so it is
not a 256-slot bound.

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

*Status:* resolved - **no row is named for it, but it is a mode**: the `CARD` pair, 22/23. Grade `disassembly` + disc bytes.

The narrow negative stands and is now an enumeration rather than an inference:
the 28 x 24-byte records at `0x8007078C` are a fixed-size array, and every
`+0x00` name pointer read out of `SCUS_942.54` yields the fourteen even/odd
pairs `CONFIG / MAIN / MONSTER / TMD / EFECT / TEST / MAPDSIP / MAP / READ /
GAME OVER / BATTLE / CARD / OTHER / STR`. None says "title", which is why the
row looked missing.

The mechanism attached to that negative is **falsified**. `main()` seeds
`_DAT_8007B83C = 0x10` (mode 16 `READ`) in `FUN_8001D424` at `0x8001D5B8`, and
its one pre-loop overlay load is `0x8001612C jal 0x8003ebe4` with `a0 = 0` -
loader param 0 = extraction **0895**, the boot `init.pak`, not the title.
Mode 16's init `FUN_8002612C` calls `0x801CE9C0` (0895 file `+0x1A8`), the
publisher-logo pass, which sets mode 17 at `0x801CEC94`; 0895 writes mode 22
at `0x801CF4D4`; mode 22's init `FUN_8002574C` loads PROT 0899 (`0x800258B4`,
`a0 = 4`), spawns descriptor `0x800706D4` whose handler `0x801E36A0` (0899
`+0x14E88`) calls the title tick every frame (`jal 0x801dd35c` at 0899
`+0x14E94`), and sets mode 23. A consequence for the loader census in
[`boot.md`](../subsystems/boot.md): param 0 *is* producible - by `main()` -
so 0895 is statically reachable; 0896 still is not.

**The ownership sub-question is closed.** `FUN_801DD35C`'s 48-byte prologue
occurs exactly once in all of `PROT.DAT`, at extraction entry **0899** file
`+0xEB44`, and `0x801CE818 + 0xEB44` reproduces the printed VA; it is absent
from `SCUS_942.54`. Its own master-mode stores are `0x801DDCF0` (`0x1A`,
attract → STR) and `0x801DFC00` (`2`, NEW GAME → field). The
`overlay_801dd35c.txt` dump that reads differently is a 436-byte routine
`FUN_801DD310` from PROT **0897**, a VA alias, not a second copy. The engine
still ports the routine twice (`menu.rs` and `title_overlay.rs`, see
[vm-inventory.md](../subsystems/vm-inventory.md#one-function-two-ports)); that
is now a code-hygiene item, not an RE question.

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

**The link base is recovered: `0x801D4DF0`.** Point 2 above is still right
about the `0x801C5818` vote - it is the over-read speaking - but "0896's true
link base is unrecovered" is not. Over the corrected `0x9000`-byte entry the
call-graph recovery lands on `0x801D4DF0` with ten corroborating targets of
eleven distinct internal `jal` targets, ten of which are `addiu sp, sp, -X`
prologues; all 218 internal `j` instructions land inside the file there and
none does at either base previously cited; three runs of consecutive in-image
VA words resolve every word, one of them holding the base itself; and the
base reconciles the corpus's two phantom import programs, so the function
printed at `0x801C6534` and at `0x801C0D1C` is one function at file `+0xD1C`.
That also ratifies the `~0x801D0000` estimate this section made from the head's
own code. The reading that no base could fit rested on a `lui`-pair resolution
ratio, which is one-sided
([falsified](re-do-not-re-walk.md#measurement-readings)).

**And the residency capture is not owed.** Of the image's 322 calls into the
SCUS address range, **zero** land on a function entry of this disc's
`SCUS_942.54`, where the same entry test scores `0897` at 1203 of 1307 and
`0899` at 793 of 884, and no constant shift of the executable within
+-`0x20000` brings more than 7 of its 42 distinct SCUS targets onto an entry.
It is a foreign-build image - internally consistent, externally linked against
an executable this disc does not carry - which is the same conclusion points 1
and 4 reach from the loader side, and it makes "no USA capture finds it
resident" a property of the link rather than an absence in the corpus. The map
row and the measurement are on
[`static-overlay-pipeline.md`](../tooling/static-overlay-pipeline.md#a-resolution-ratio-is-not-a-base-test).

What is left is only what its bytes *do*: a base is what lets an image be
imported and dumped, and the dump corpus is a code image's parser, so with the
base committed the image's code region is dumped and accounted - the base bought
coverage, not a caller
([`byte-accounting.md`](../tooling/byte-accounting.md#a-base-is-not-a-dump-and-a-dump-is-not-a-caller)).


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
Grade: `disassembly` (full window) + `inference` (the half-window sub-case, on a *different* ground than before).

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
  `<= 255` so the id ceiling does not forbid a fill, and the earlier reason -
  "the real disc item population is far below 128" - is **false**: the static
  item-name table `0x80074368` carries **250** non-empty names over its 256 ids
  (blank: `0x00`, `0x12`, `0x1A`, `0x52`, `0xB9`, `0xFD`), so 128 distinct live
  ids is arithmetically reachable. What bounds the half-window case is how much
  of that population is obtainable while a character travels alone - a progress
  bound, not a capacity one, and nobody has measured it. That is the residual
  `inference`. One reader reaches the whole array with no window and no
  live-count clamp: cast module PROT 0954 (Fatal Decision), `rand & 0xFF`
  over 256 slots.

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

### The publisher-logo quads

*Status:* resolved - the routine asked about does not draw.

`FUN_801CE9C0` (PROT 0895 `+0x1A8`) uploads the four `init.pak` TIMs through
`FUN_800198E0` after writing their CLUT and pixel VRAM rects, selects the
640x480 display env (`FUN_8001DAF8(0x400)`), and spawns the two boot actors. It
contains no primitive emit. The quads come from a six-record, 20-byte
sprite-descriptor table at `0x801F369C` (file `+0x24E84`, immediately after the
fourth TIM): `[u32 scale][u16 tpage][u16 clut][u8 u,v,w,h][rgb top][stp][rgb bottom][tpage adder]`,
emitted as opaque `POLY_GT4`s by `FUN_801CFBB8` and sequenced by
`FUN_801CEFD4` (13-arm jump table at `0x801CE8E8`). Each record's `tpage`/`clut`
matches one of the rects the uploader wrote (`0x9A/0x7ED4`, `0x9C/0x7F54`,
`0x0A/0x7F14`, `0x0B/0x7E80`), which is how a record binds to a logo. Retail's
order is SCEA, Contrail, PROKION; the fade is the PSX texture blend
`texel * colour / 128` over a vertex colour scaled by a `0..0x80` level, not
alpha. Full tables in [`boot.md`](../subsystems/boot.md#the-per-logo-quads).
Grade `disassembly`; the descriptor table reproduces from the extracted image.

### The title menu's law

`FUN_801DD35C` sub-mode `0x10` (`0x801DDB74..0x801DDCF4`): two rows (`andi 0x1`
at `0x801DDC00`); Down `0x4000` +1 / Up `0x1000` -1 with cue `0x21`; confirm mask
`0x844` with cue `0x20`; row 0 -> mode `0x16`, row 1 -> mode `0x18` stashing
`state[+0x200] = 1`; the `0x5DC` attract countdown is re-armed whenever the held
word `_DAT_8007B850` is non-zero, decremented by scratchpad `0x1F800393`, the
input block is skipped while it reads below `0x11`, and underflow writes
`_DAT_8007BA78 = 0` and mode `0x1A`. Ported as `legaia_engine_vm::title_overlay`'s
executable half and driven by `engine-core::title` on both hosts. The
cold-boot default sub-mode is `0x02`, not `0x10` - an open row. Grade
`disassembly`.

### `FUN_801E5A08`, the per-slot equip applier

`ghidra/scripts/funcs/overlay_0897_801d71f0.txt` (and the `_801d7210` sibling)
is mis-based by `0xE818`: the bytes are PROT 0897 file `+0x171F0`, true VA
`0x801E5A08`, 324 bytes. Its four class arms end on `j 0x801E5AE8` /
`j 0x801E5AEC` - intra-function jumps whose targets print correctly while the
body prints low, so a self-jump read as a call to a "shared armament placer".
`0x801E5AE8` is the routine's own inline placer at `+0xE0`. Class -> equipment
byte: 0 -> 0, 1 -> 1, 2 -> the weapon index (`_DAT_8007B42C`, `2/3/2`), 3 -> 4
(an `addiu v1,zero,4` in the delay slot at `0x801E5AB0`). No `jal`, data word or
`addiu` materialisation of `0x801E5A08` exists in any image, so the routine is
dead; the live equip confirm is `FUN_801D9C14`'s candidate arm. Port
`legaia_engine_vm::dev_equip_commit::commit_equip`. Grade `disassembly` + bytes.

### The dead Return view mode

`FUN_801E3F74`'s mode `4` ("Return", `0x801CF384`) is real and unreachable: its
only caller forms the cell as `col + row*5` (`0x801E06D0`), the shared stepper
clamps `col` to `0..=4` and `row` to `0..=2`, and the linear seed `_DAT_8007B7CC`
has three references on the disc, all in PROT 0899, whose single writer
(`0x801DED2C`) stores the same `col + row*5`. Fifteen cells, no sixteenth. Grade
`disassembly` + bytes.

## Rendering / camera

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| In what units are a field attached light's extents? | resolved (view space, six times world scale) | `capture` + `disassembly` | `FUN_800195A8` adds them after `FUN_8003D344` has transformed the parent through the view matrix carrying `_DAT_8007BF10 = 24576 * I`, so the rim radius is `H * ext / vz`. The retail `dolk` state holds extents `0x1000` and puts the rim at 201 px round `(153, 93)` with `H = 768`, `vz = 15635` ([`script-vm.md`](../subsystems/script-vm.md#the-extents-are-view-space-units-retail-capture)). |
| Why did `vell`'s fog not show on the native host? | resolved (the effect-texture pool was missing under the scene VRAM) | `capture` + `disassembly` | The fog page `0x27` selects VRAM `(448, 0)` (page-y bit clear), cells `v 0x40..0x6F` of the PROT 0874 section-2 effect pool with CLUT row 473. Retail keeps that pool resident under every field scene - the cells are byte-identical in nine PCSX-Redux states from `retona` to `vell` - and loads it before the scene, so the scene wins where they overlap. The native window VRAM lacked the pool, so every texel read zero and was discarded; both hosts now underlay it ([`field-ambient-fx.md`](../subsystems/field-ambient-fx.md#where-the-fog-texels-come-from)). |
| What sets the field-fog cap? | resolved (MAN header bits 0 and 2) | `disassembly` | `FUN_8003AEB0` stores `MAN[1] & 1` into `_DAT_8007B6A8` (`0x8003AF54`) and at `0x8003B6BC..0x8003B6E8` writes `0x48` into `_DAT_8007BCB0` when `MAN[1] & 4` or that byte is set, `0x18` otherwise. Four of the 101 scene MANs raise it: `map01` / `map02` / `map03` (bit 0) and `opurud` (bit 2). |
| Why did the port's `vell` fog run denser than retail's? | resolved (a raw LCG state where retail calls BIOS `rand`) | `disassembly` + `capture` | `FUN_80056798` is the BIOS `A(2Fh)` thunk and returns `(seed >> 16) & 0x7FFF`; the spawner tests low bits of it. A raw 32-bit state's low nibble cycles with period 16, so the burst gate passed on every draw of one frame in sixteen. Shaped (`bios_rand_shape`), the port's pool reads 19 to 35, mean 26.4, against a retail poll of 21 to 38, mean 25.9. The stale live count the spawner compares was already modelled. |
| What are `FUN_80024EE4`'s three arguments, the per-frame screen-effect push? | resolved (one quad, never a family) | `disassembly` | The routine builds exactly one full-display `POLY_F4` from the scratchpad display rect (`0x80024F68..0x80024F98`). `a0` is the ordering-table bucket, floored at zero by `bgez s1` at `0x80024F00` and used as `AddPrim(OT + a0*4)` twice; `a1` is the ABR blend equation, packed `(a1 << 5) \| 0xE` into the draw-mode word at `0x80024FB0`; `a2` is a GP0 colour word with red in the **low** byte (`0x80024F54`) - the opposite channel order from every other kernel in `screen_prim`. Both hosts composite it through `screen_prim::screen_effect_push_prims`. |
| What does the scene-entry camera reset write? | resolved (six stores, and `H` is not one of them) | `disassembly` | `FUN_80025C24` is seventeen instructions: three `sw` seating the eye triple at `0x800840B8` / `+4` / `+8` to `(0, -0x100, 0x4024)`, and three `sh` seating the angle triple at `0x8007B790` / `+2` / `+4` to `(0x1B8, 0x64, 0)`. It never touches `_DAT_8007B6F4`, the camera height, so a port that resets `H` on scene entry is adding a store retail does not make. |
| Is the field follow camera's framing one set of constants? | resolved (**no** - retail derives it per scene and per player tile) | `capture` | Measured across the walkable state population, a single pinned height matches 12 of 19 states, a single pitch 8 of 19 and a single yaw 1 of 19 - so the pins are one state's values. Retail's arrival handler queries the MAN section-3 zone table and hands the hit's camera-region record to the config loader, which writes the angle trio, the height and the eye trio with hold and glide rates. Camera roll is a separate matter and is genuinely zero in 51 of 51 sampled frames. |
| Which `FUN_8002C69C` arm lays the post-battle report's nine-slice? | resolved | `disassembly` + `capture` | The routine reads the style from `gp[+0x14C]` (`0x8007B464`), indexes `0x800732A4 + style*12`, and `jr`s the table at `0x80010D18` on descriptor byte 0. Style `0x03` is kind 0 with tile set 0 at `0x80073A00` - eight tiles, byte-identical to a 36-`SPRT` capture - CLUT byte `0x02` -> `0x7FC2`, `dx = dy = -8`, rect `(16, 160, 288, 42)`, so `x` runs `8..312` and `y` `152..210`. All 782 emits arrive with `ra = 0x800323EC` (inside `FUN_80031D00`). Styles `0x01` / `0x02` are the HUD plaques (kind 3, sets 3 / 4, CLUTs `0x7FC4` / `0x7FCC`). The "Gimard fight carries no report band" reading is falsified - the same capture holds 152 entries. |
| `FUN_80058490` - a sound-driver lane? | resolved (**`MoveImage`**) | `disassembly` | It moves a VRAM rect to `(0xE0, 0x1DC)` - CLUT row `y = 476` - so the table feeding it at `0x801F6418` holds VRAM **x** coordinates and is a CLUT map, not a cue list. `FUN_801E22C8` builds the `{map[id], 0x1DC, 0x10, 1}` argument block at `0x801E2400`. Two consumers were reading those bytes as sound-effect ids. |
| How does `FUN_80043390` unpack an actor's afterimage colour? | resolved | `disassembly` | Actor `+0x74` is `[R][G][B][mode]` - byte 0 is red. The routine splits `a1` **low byte first** into `cr21/22/23` at `0x800434C8` and `cr13/14/15` at `0x80043464`. The ribbon literals in `FUN_8005112C` are a *different* word, reached through `FUN_80048310` -> `FUN_800485BC` at byte 2, which is what made the two look like one field. |
| Which routine draws the world-map / field ground pass? | resolved (PROT **0900** ships a depth-cued / flat **pair**, not one emitter twice) | `disassembly` + `capture` | `FUN_801F69EC` (file `+0x14`, 860 B) runs `GTE.dpcs` at `0x801F6C44` + `swc2 $22,4($t5)` at `0x801F6C4C`; `FUN_801F6D48` (`+0x370`, 832 B) replaces exactly that pair with a plain `sw $s2,4($t5)` at `0x801F6F88` - the same loop with and without the depth cue, not a duplicate. Selector `0x801F79A0` on `_DAT_8007BB4C`, non-zero arm calling `SetFarColor` first. Both gate `cell & 0x1000`; the decoration pass `FUN_801F7088` gates `0x2000`. Rows on [`functions/renderer.md`](functions/renderer.md). |
| Which ordering-table slot does each screen effect take? | resolved | `disassembly` | Letterbox bands at OT `+0x4`, sprites at `+0xc`, the panel at `+0x10`, mask borders at `+0x1c`. The port batched the letterbox with the mask borders, which put the bands behind geometry that retail draws them over. See [`renderer.md`](../subsystems/renderer.md). |
| Does any retail shot author a non-zero camera roll? | resolved (yes) | `capture` + `disassembly` | Eight scenes stage a reachable, executing op-`0x45` slot-2 roll, from `10` units (0.9 deg) to `-660` (-58 deg). [details ↓](#does-any-retail-shot-author-a-non-zero-camera-roll) |
| Does retail stack coincident curved shells? | resolved (no) | `capture` | Field-run display-list reads in `jouine` / `jouind` find zero screen-coincident surface groups; every surface is submitted once. [details ↓](#does-retail-stack-coincident-curved-shells) |
| `FUN_80045BB4` - a function entry, or interior residue? | resolved (the bank-3 kind-19 textured Gouraud quad handler) | `disassembly` | It is word 12 of the bank-3 cluster-A primitive-handler table at `0x8007668C` (kinds 8..19), which is what makes it reachable - no `jal` names it. Frameless, 1272 bytes, register-entered from the dispatcher `FUN_80043390` and leaving by `j 0x80043580` rather than `jr ra`, which is why a gap census reads its middle as two interior runs. Row in [`functions/world-map.md`](functions/world-map.md). |
| How does PROT 0901's `0x801F7644..0x801F8EB4` band split into routines? | resolved (nine: eight tail-call leaves plus a framed ninth) | `disassembly` | The band has no prologue and no `jr ra`; every leaf ends `j 0x80043580` and the next starts at that jump's delay slot plus four. Twenty `j` sites sit in the band and only eight leave it, so cutting at every `j` over-splits by nine - **the boundary rule is the `j` that leaves the band**, which `scripts/ghidra-analysis/split-tail-call-band.py` reads off the image. The ninth routine `0x801F89B8` is a different shape: four local `j`s to its own tail and a real `jr ra` at `0x801F8EB4`. Per-leaf sizes in [`functions/world-map.md`](functions/world-map.md#the-prot-0901-draw-leaf-band-split). |
| Which base does the world-map overlay's per-prim handler table use? | resolved (`0x801F8968`, with words `0..7` zero) | `disassembly` | `FUN_80043390` forms it with `lui s4,0x8020` / `addiu s4,s4,-0x7698` at `0x800435F4..0x800435F8` and adds the same `(group_flags >> 1) * 4` index it adds to the SCUS table `0x8007657C`. Both tables have words `0..7` zero, `8..11` the four shared emitters and `12..19` the per-bank leaves, so the two paths agree kind for kind; the only difference is the alpha-bank offset `s2`, which the SCUS arm adds at `0x800435E4` and the overlay arm does not. Naming the first non-zero word as the base re-keys every kind by eight - [falsified](re-do-not-re-walk.md#measurement-readings). |
| Does `FUN_8002C69C` draw the post-battle report windows? | resolved (yes - driven from SCUS, off the retained widget list) | `disassembly` + `capture` | PROT 0898 contains no `jal` to it, which is a true absence and a misleading one: `FUN_80031D00` (SCUS) reaches the emitter with `jal 0x800323E4` every frame a battle is up, walking the retained widget list. The results chrome is 36 `SPRT` nine-slice cells (CLUT `0x7FC2`, tpage `0x1E`; 4x4 corners at `(160,0)` / `(188,0)` / `(160,28)` / `(188,28)`, 24x4 edges at `(164,0)` / `(164,28)`, 4x24 at `(160,4)` / `(188,4)`) over the rect `x 8..312`, `y 152..210`. Body in [`level-up.md`](../subsystems/level-up.md#the-gold-frame-band-is-a-nine-slice-off-the-system-ui-atlas). |
| What is `FUN_80019D50`? | resolved (the CLUT-cell HSV cycler) | `disassembly` | It rotates hue / saturation / value over one CLUT cell block and pushes the result with a single `LoadImage` at `0x8001A030` - one palette upload per call, no primitives. It is what drives jou's pulsating flesh and the lightning; the "BGR555 cell-grid emitter" reading is [falsified](re-do-not-re-walk.md#field--locomotion). Port `engine-core::clut_cell_fx`; see [`field-ambient-fx.md`](../subsystems/field-ambient-fx.md#the-clut-cell-hsv-cycler-the-pulsating-flesh). |
| What order does `FUN_80026988` (`RotMatrix`) compose? | resolved (`Rx(vx) * Ry(vy) * Rz(vz)`) | `disassembly` | The sin / cos tables it walks are the pointers at `0x8007B7F8` (cos) and `0x8007B81C` (sin). Composition order is the one thing a capture of a single rotation cannot distinguish, so it has to come from the instruction sequence. |
| Who reads the GTE light matrix `FUN_8001ADA4` writes inline? | resolved (five sites, all in SCUS, all in the world-map NCC handlers) | `disassembly` | A disc-wide GTE command-word census finds exactly five light-matrix consumers: `NCCS` at `0x800441C8`, `0x800443C8` and `0x80044750`, `NCCT` at `0x80044540` and `0x80044724` - the kind-8..11 handlers. No overlay image contains one, and `NCS` / `NCT` / `NCDS` / `NCDT` occur nowhere. `MVMVA`'s `mx` field is the consumer such a census would miss, and disc-wide none selects the light matrix: 29 rotation, one colour, none light. Denominator 238 GTE command words across 84 images. [details](../subsystems/renderer.md#lighting) |
| How often is the field view matrix built? | resolved (**three** times a vsync, from three different callers) | `capture` | An exec-count probe over a field run puts `FUN_800172C0` at three entries on 133 of 134 vsyncs, from `0x801D0F98` and `0x801D185C` in the field overlay and `0x80016678` in SCUS. The "once per frame" figure came from counting one caller. Which of the three a given frame's geometry is drawn against matters, because on a scene-entry frame two of them read different live camera words. |
| Is the field view's pre-multiplied matrix a pure scale? | resolved (`_DAT_8007BF10 = 24576 * I`, zero translation) | `capture` | Read out of a retail `town01` field state with `mednafen-state extract`: the diagonal is `0x6000` (6.0 in 1.3.12) and the off-diagonals and translation are zero, so the uploaded rotation is six times a pure rotation and a 1x renderer reproduces the frame with the eye trio divided by six. |
| Does anything on disc select the light matrix through `MVMVA`? | resolved (nothing) | `disassembly` | Of the 31 `MVMVA` words in code across 87 images, 29 select the rotation matrix, one the colour matrix and one the reserved encoding; the two libgte light wrappers at `0x8005B850` / `0x8005B8B4` have no `jal` site. The light matrix is consumed only by the five canonical `NCCS` / `NCCT` words in SCUS. |
| How many call sites does the field view builder have? | resolved (seventeen `jal` sites disc-wide; three run per field vsync) | `disassembly` | The three that run each field vsync are the `jal`s at `0x801D0F90`, `0x801D1854` (PROT 0897) and `0x80016670` (SCUS) - each eight bytes below the return addresses a probe reports. |
| What the dance count-in banner is made of | resolved (one textured quad off the hall's own texture page) | `capture` | PROT 1230's member at VRAM `(512, 0)` with its CLUT at `(0, 500)` is byte-identical to the parked minigame state's VRAM - 16,384 of 16,384 halfwords and 256 of 256 CLUT entries - and widget `0` is the `READY...` cell. The numerals, `GO!` and `FINISH!` are **not** that widget: they belong to the sprite spawner `FUN_801D3FD0`, and `0x77` / `0x78` are y **seats** rather than widget ids (the emitter clears `a2` at `0x801D2EBC` / `0x801D2EEC` / `0x801D2F04`). |
| The field passive-ability badge column, host against retail | resolved (pixel-exact) | `capture` | The panel's origin matches retail with `dx = dy = 0` on both library states that draw one. Retail takes the anchor's X from the **first** projected head point and its Y from the **third**, so a port projecting a single point and using its pair is wrong even when the two points are close. |
| What stages the dance widgets' second texture page at VRAM `(960, 256)`? | resolved (boot-resident system UI; no PROT entry stages it) | `capture` | The page is in no PROT entry: it is uploaded from `PROT.DAT`'s unindexed head gap (`0x1800..0x3C800`). Two members of that gap cover what the three widget records sample, each declaring its rect in its own TIM header - `PROT.DAT[0x11218]`, the menu-glyph atlas, origin `(960, 256)`, `64 x 256` halfwords, i.e. texpage `0x001F`; and `PROT.DAT[0x1AED0]`, origin `(976, 272)`, `8 x 32`, whose first three rows are the CLUT ids the records carry. A per-entry sweep cannot source it. [details](../subsystems/minigame-dance.md) |
| Which of a field frame's view builds does the drawn geometry use? | resolved (the first; the last frames none of it) | `capture` | Splitting every ordering-table link by GPU command code over three runs - including a real `map01` -> `town0c` entry, the case the question was asked about - gives 4289 polygons: **3861** under site A (the `jal` at `0x801D0F90`), 428 under B, and **0** under C (`0x80016670`) or either of PROT 0901's slot-B sites. C's whole share is attribute packets and 2D rects, so the build a frame ends on frames no scene geometry at all, and the earlier link-count ranking was counting non-geometry primitives alongside polygons. [details](../subsystems/cutscene.md#what-a-build-census-actually-measures) |
| Which routine draws a field or world-map actor's mesh? | resolved (three arms of one bracket, chosen by `actor[+0x42]` then `+0x7A`) | `disassembly` | Each of `FUN_8002735C`'s three SCUS `jal` sites - `0x8001B594` in `FUN_8001ADA4`, `0x8001BD88` in `FUN_8001B964`, `0x80048FE4` in `FUN_80048A08` - is the **far** arm of an `lh`/`lhu r, 0x42(s0)` then `bne` pair (branch sites `0x8001B454`, `0x8001BC64`, `0x80048EA4`). The near arm calls `FUN_80029888` when `actor[+0x7A]` is non-zero and `FUN_80043390` otherwise. So the three are not three renderers for three kinds of mesh; they are one two-level choice, and the outer level is a per-actor gate. |
| Does any sampled mode enter the table-driven mesh renderers? | resolved (no) | `capture` | Over 720 vsyncs across four states and three game modes - battle, field, cutscene and the slot machine - the census records **0** entries into `FUN_8002735C` and **0** into `FUN_80029888`, against 5089 hits on the `+0x42` gate, every one of them reading zero, and 10621 entries into `FUN_80043390`. The per-prim leaf draws everything those modes put on screen. Probe: `autorun_w4c_mesh_path_census.lua`. |
| Does the disc ship writers of `actor[+0x42]`, the mesh-renderer gate? | resolved (**yes**, two families - one dev-gated, one content) | `disassembly` | `FUN_80020DE0` clears the halfword at `0x80020EAC` and writes `2` at `0x80020EC0` when `_DAT_8007B6D0 & 2` - the world-map **dev** counter, booted clear by `sw zero,0x3b8(gp)` at `0x80015F64` and raised only by the debug menu and the pad ring. The content raiser is move-VM op `0x10`, `sh $v0,0x42($s2)` at `0x8002342C`, which shipped programs issue with non-zero operands. Two SCUS `sh rt,0x42(rs)` sites inside the render brackets are not raisers - their base is the scratchpad packet block. [`renderer.md`](../subsystems/renderer.md#the-disc-does-ship-writers-of-0x42) |
| Which emitter does effect render mode 4 take? | resolved (always the default - the two flagged arms have no writer) | `disassembly` | `FUN_8001ADA4`'s case `4` picks on `actor[+0x9E]`: `0x4000` -> `FUN_8002A5A4`, `0x2000` -> the battle overlay's ribbon builder `FUN_801CFA48`, else `FUN_80028158`. The allocator `FUN_80020DE0` zeroes `+0x9E` at `0x80020ECC`, and a byte census over `SCUS_942.54`, all 86 based images and all 1233 PROT entries - covering `sh` at `+0x9E`, `sb` at `+0x9E`/`+0x9F`, `sw` at `+0x9C` and stores through an `actor + 0x80` base - finds no site writing a value that carries either bit. A value loaded out of an effect record is the one path the census cannot see. [`effect-vm.md`](../subsystems/effect-vm.md#the-three-render-mode-4-emitters-and-which-one-the-disc-uses) |
| Where does a fade block's delay and hold end, frame for frame? | resolved (delay `n` suppresses `n - 1` frames; hold `0` retires the frame **after** the ramp lands) | `disassembly` | `FUN_80020C14` subtracts the frame byte from the delay at block `+0x1C` (`0x80020C20..0x80020C40`) and returns `-1` only while the result stays positive (`0x80020C48..0x80020C54`), so the frame that reaches zero already steps the ramp. The engine's `fade::FadeState` latched on the target where retail keeps accumulating and clamps on the delta's sign; it now steps through the transcription. [`live-audit-triage.md`](../tooling/live-audit-triage.md#the-fade-two-models-of-one-block) |
| Does any asset set an actor's render mode `+0x56` to `4`? | resolved (**no** - every write of `4` is code) | `disassembly` | The four `sh` sites that store `4` into `+0x56` are move-VM arms `0x80023460` / `0x800237E4` / `0x80023F98` and `0x8004D574`; no carrier holds the value. The port's move VM names the same halfword its sub-state. A sweep for a `0x2000` / `0x4000` store into the `+0x9E` selector over SCUS, 86 overlays and all 1233 entries finds none, so the effect ribbon stays unwired - nothing does its job. |

### Does retail stack coincident curved shells?

*Status:* resolved - **no**.

Field-run display-list reads inside `jouine` and `jouind` report zero
screen-coincident surface groups above a 16 px² floor; every surface in the
live ordering table is submitted exactly once (1218 packets walked in
`jouind`, 972 in `jouine`). The coplanar kernel's same-position curved-shell
residual is therefore a property of how the *port* assembles a scene's env
TMDs, not something retail resolves by ordering - there is nothing for retail
to order. The only coincidence in either image is one mesh drawn three times
in a single texture family (`clut=0x7F86 tpage=0x001F`), which is the
multi-pass semi-transparency idiom rather than a mesh stack: its members share
a material, and two different env TMDs would not.

Two false positives this measurement has to avoid, both of which turn a clean
frame into an apparent stack. Retail **double-buffers**, so ordering tables
come in pairs holding frame N and frame N-1 with near-identical packet counts -
merging a pair makes every surface look stacked with itself. And distant
geometry projects to 1-3 pixel slivers that coincide constantly without saying
anything about meshes, which is what the area floor exists for.

Coverage limit, stated because the result is a negative: each read is one
frame and therefore one camera, and the corpus holds no second viewpoint for
either scene - the curated library's `jouine`/`jouind` entries are
byte-identical backups of these two (a library filename is the sha256 of its
contents). `chitei2` is not covered at all and inherits the conclusion rather
than being measured. What carries the negative anyway is its scale: a stacked
shell would double many adjacent surfaces at once, and in cave interiors the
walls are the dominant on-screen geometry.

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
| Does any code reseed BIOS `rand`? | resolved (no; one kernel stream) | `disassembly` | `FUN_80056798` is the `A(2Fh)` thunk (`li t2,0xA0; jr t2; li t1,0x2F`) and returns the high half of the kernel LCG. `SCUS_942.54` carries no `A(30h)` (`srand`) thunk, so every caller in every image draws on one kernel seed. The port models that as `bios_rand_shape` over one world stream (`World::next_rand`). |
| Why do PROT 0896's `jal` targets miss this disc's SCUS entries? | resolved (a foreign-build link) | `disassembly` | Imported at its own base `0x801D4DF0`, the image cites SCUS-range targets that each land on a body instruction or delay slot of this disc's `SCUS_942.54`, none on a frame adjust, and no other image calls them; rebasing cannot move a decoded target. They are baselined as a measure of the executable it linked against ([`call-target-integrity.md`](../tooling/call-target-integrity.md#the-image-at-its-own-base-still-misses)). |
| What is PROT 0970's run at `0x801D0E9C..0x801D199C`? | resolved (an unreferenced AC VLC table) | `disassembly` | Every word is zero or `(len << 26) \| (run << 10) \| level`; no word, `jal`, `j`, `lui` pair, `gp` or base-plus-displacement access reaches it, and the decoder reads the table `FUN_801F1A00` unpacks at `0x801E0A00` ([`byte-accounting.md`](../tooling/byte-accounting.md)). |
| What sizes the debug monitor rows and the 0972 / 0977 tables? | resolved (their consumers) | `disassembly` | `FUN_8001C93C` draws `$a0` rows of `0x28` bytes (PROT 0976 hands it eleven); PROT 0971 walks twenty-two such rows inline (`sltiu v0,s0,0x16` at `0x801CECE0`); 0972's species records are ten of 8 bytes, 0977's hub sprites seventeen of `0x14`; PROT 0896's pool is twenty-eight programs for its own interpreter `FUN_801D896C` ([`byte-accounting.md`](../tooling/byte-accounting.md#a-table-its-consumers-pin)). |
| Which catalogued save states carry a patched executable? | resolved (measured and tagged per family) | `capture` | `SCUS_942.54` is read once at boot, so a state made on a patched disc keeps that build's code on every load. `patch_taint_audit.py states` compares each state's resident SCUS at the patcher's hook sites and records `resident_patch` per scenario; every capture-graded claim re-checked against its family stands, and `w3a/registrar_sol` is void ([`pcsx-redux-automation.md`](../tooling/pcsx-redux-automation.md#patched-disc-taint)). |
| What are PROT 0897's short data tables? | resolved (window layouts, sound-test rows, a handler table, window programs) | `disassembly` + `inference` | 27 `0x1C`-byte window records at `0x801F2B98` (index `a3 * 28` at `0x801ECA24`), 73 ten-byte sound-test rows at `0x801F2E94` (index `(n + 1) * 10`) - not the six-byte stride once read - a 52-word `jalr` table at `0x801F33B4` indexed by actor `+0x50`, and fifteen window programs from `0x801F3340` run by the overlay's own thirteen-arm interpreter `FUN_801E9B3C`. The two counts sized by layout rather than by a bound are `inference` ([`byte-accounting.md`](../tooling/byte-accounting.md#a-runtime-index-states-its-stride-and-a-consumer-its-count)). |
| What is PROT 0976's sparse table? | resolved (Baka Fighter's per-fighter move table) | `disassembly` | Seventeen per-fighter blocks of nine `0x60`-byte move records tile `0x801D7E28..0x801DB788`, pointed at by the seventeen-word table `0x801DB8B8`; `FUN_801D553C` walks and formats them, `FUN_801D57BC` is the runtime reader (fighter `a0`, move `a1 * 0x60`, point count `+0x1C < 8`), and the name tags sit at `0x801DB7A8` ([`byte-accounting.md`](../tooling/byte-accounting.md#a-pointer-bump-loop-states-its-arrays-length)). |
| What does the indexed form `lui; addu; lw lo(at)` hide? | resolved (512 accesses, none in 0897 / 0899, no verdict moved) | `disassembly` | A strict register walk completes the form 512 times over SCUS and the mapped overlays (273 SCUS, 223 PROT 0896, 9 PROT 0971). The earlier count of 23 / 44 sites in 0897 / 0899 came from a scanner that never dropped an overwritten register. Both reference sweeps now share that walk (`scripts/ghidra-analysis/mips_walk.py`), and re-running them over the 89 addresses the ignore list and docs call unreferenced changed no verdict. |
| How often does a live `// PORT:` tag describe its routine correctly? | resolved (about seven in ten) | `disassembly` | 316 live, dumped tags in the VM / world / audio crates read against their disassembly: 222 match, 45 misname the routine, 43 port only part of it, 6 name the wrong address. The window VM and anim VM were worst. `check-port-provenance.py` carries a `non-entry` signal from that audit; a tag naming an address that is no function entry is the shape it catches. |
| Does the byte account cut an overlay's inherited tail the way `disc-coverage.py` does? | resolved (yes - one rule feeds both) | `disassembly` | `legaia_asset::inherited_tail` ports the sibling fixpoint and adds the **packer-buffer leg**: byte `k` of a tail is the byte of the nearest earlier entry reaching `k`, searched over the whole entry. `tail_cuts` / `tails_in` feed `disc-coverage.py`, the attribution sweep and the byte account alike; 85 of the 87 mapped images end in a tail, 96,986 bytes, and the leg reproduces 82 of the sibling rule's 83 cuts offset for offset. It adds a non-overlay donor (PROT 0898 and 0895 end in 0894's bytes) and moves 0901. [`disc-coverage.md`](../tooling/disc-coverage.md#the-packer-buffer-leg) |
| What did the framed-text walk change in the op-`0x49` census? | resolved (55 sites lost, 54 gained, the spine-flag negative unmoved) | `disassembly` | Reconstructed at the pre-#470 disassembler and diffed site for site: every one of the 55 lost sites read a printable-ASCII base pair (dialogue decoded as an op), and five scenes left the census entirely; the 54 gained are real instructions behind a record's first line - 48 are the `[49 03 N]` / `[26 delta]` picker-resume ladder after a `0x1F` prompt in sixteen scenes, plus two `49 09` in `bylon` and four sub-`0x00`. `0x142`'s only near-miss is still the same 24 `kor` windows at distance 3. |
| Can a green ladder rung assert its invariant vacuously? | resolved (yes - and a canonical one did) | `capture` | A rung meant to show that re-applying an inventory Arrange leaves the bag unchanged drove Throw Out first, which emptied the bag; retail's own dispatch then swallowed the confirm, so the assertion compared two empty bags and passed. A rung's **order** is part of its claim: an assertion whose subject an earlier step can empty has to check the subject non-empty before it compares. |
| Does a "not observable" reach verdict survive the next export? | resolved (**no** - it is a link-time property) | `capture` | Whether an address is observable at all depends on what the profile links in, so a verdict recorded against one export does not carry: the same addresses come back unobservable on the next run rather than staying converted. Read such a cell as a statement about the export that produced it, and re-run the export before planning work off it - the same law the reach audit states for the prose beside an address. |
| Can a per-vsync save-state capture measure an envelope statistic? | resolved (**no** - the emulator's envelope is not on emulated time) | `capture` | PCSX-Redux runs its SPU on a thread paced by the audio device accepting samples and steps ADSR once per sample that thread produces, so a capture that writes a ~19 MiB state every vsync advances the envelope by an uncontrolled amount per captured frame. Measured: the wall-clock gap between captured vsyncs averages `169` ms against the emulated `16.67`, and two captures of one state with no input agree on the voice pitch register for `5285` of `6000` voice-frames but on `env_level` for `404` of `1000`. Anything written by the emulated CPU - a key-on, a pitch - is still comparable. |
| Are `FUN_801D14B0` and `FUN_801D6710` two routines? | resolved (**one**, linked into two overlays) | `disassembly` | Both are 24 instructions and 22 of them are identical; the two that differ are the `lui`/`lw` pair materialising the gate word - `0x801D1AB4` in PROT 0977, `0x801DBF00` in the Baka Fighter image. A byte comparison reports nothing in common because the relocated pair comes first, which is what let the pair read as unrelated routines. |
| Can `replay-port-coverage.py`'s `PARTIAL UNION` line tell a failing ladder from one nobody exported? | resolved | inference | No. Both leave no `cov-*.json`, and the report names them identically; a red canonical member therefore reads as a forgotten export while every row it was written to convert keeps reading *never entered*. Read the line against the export run's own log ([`reach-triage.md`](../tooling/reach-triage.md)). |
| What does `DAT_8007BB38` hold? | resolved (the id **just issued**) | `disassembly` | `FUN_80026B4C` publishes it through `gp[+0x820]` *before* the increment, so it is the last installed index and not the next free one. A walker bounded by it as a count reads one entry long. |
| Can a mode-seat parity witness sample the mode word? | resolved (**no** - an INIT mode never survives the call) | `capture` | An INIT mode lasts one frame *inside* the seat's own entry call, which resolves it and hands off to RUN before returning, so no post-call sampler on either host can observe one. The witness that does work is the **edge count**: five on each host with the hooks in place, three without. A parity check reading a mode word instead is measuring the wrong global as well as the wrong moment. |
| What does a reach-triage cell assert? | resolved (nothing - it is prose beside an address) | `inference` | The page audit checks the addresses a row cites, not the sentence next to them, so a cell can keep describing work a ladder has already done. Nine of eleven "not driven" rows were converted by canonical ladders whose cells were never rewritten. Treat a reach cell as a claim to re-measure, not as a measurement. |
| Does a PCSX-Redux `.sstate` carry the scratchpad? | resolved (**yes**) | `capture` | The state carries a 64 KiB `hardware` blob (protobuf field 4) with the scratchpad at its own offset 0. A `teien` field-run state reads the object-grid pointer at `+0x3EC`, the camera's visible tile window at `+0x3E8..EB` and the floor-height ladder at `+0x35C` out of it. Only the repo's reader lacked the accessor, which is why the format was described by its reader's surface. Body in [`crates/pcsxr/README.md`](../../crates/pcsxr/README.md). |
| Is the overlay corpus's uncovered-byte residue un-dumped code? | resolved (**sixteen of twenty runs are not code**) | `disassembly` | Four of the twenty ranked runs on the bytes-derived dump worklist hold real routines. Three are slot-A: `FUN_801CE8EC` (PROT 0973, 144 B), `FUN_801CF870` (PROT 0977, 2184 B, not the 1748 previously dumped) and `FUN_801D03C4` (PROT 0980, 636 B, of which a 500-byte dump was a short read and `0x801D05B8` a fabricated interior head). The fourth is slot-B and is what makes the count sixteen: PROT 0949's `0x801F7630` run is the body band of an eight-arm leaf table, invisible to frame matching because the leaves have no frame. The other sixteen are the image's own data tail or a neighbouring image's bytes. |
| Why does every extracted overlay image end in another image's bytes? | resolved (the build buffer) | `disassembly` | An image's extracted extent runs past its own content into another module's code at the same file offset - a build-buffer leftover, the `inherited_tail` shape, cut from the image's own code denominator. The rule's two original restrictions were both wrong: the buffer is indexed by **file offset**, so the donor need not share a load base, and `content_bytes` is a sector extent, so it need not be longer. Same base + strictly longer reported **66 of 83** images; the gate now cuts on the packer-buffer leg (row above). Measured case: PROT 0918 `0x801F9388..0x801F99D8` == PROT 0899's file `+0x29B0`, 1616 bytes. See [`disc-coverage.md`](../tooling/disc-coverage.md). |
| What is the slot-B modules' unclassified data tail? | resolved (the **spawn-record band**) | `disassembly` | `[i16 model_sel][u16 reserved][move-VM bytecode]` records, addressed by the consumer's own `lui`/`addiu` and handed to `FUN_80021B04` / `FUN_80050ED4` in `$a2`; `+0x02` is zero in every record with no reader. 62 of 64 images carry a band - 1027 records, 114,412 bytes. PROT 0926 is a null stub; 0952's two spawn sites sit in its **inherited tail** and point at `0x801F8348` / `0x801F836C`, two of 0951's records past 0952's own end. No earlier shape rule reached the band: a record's bytecode carries a `lui $rt,0x80xx` word by accident, which `in_data_segment` rejects. [`slot-b-module-layout.md`](../formats/slot-b-module-layout.md). |
| Does `FUN_8003E8A8` return an LBA? | resolved (**no** - a sector count) | `disassembly` | `0x8003E90C` is `subu s0,v0,s2` over `TOC[idx+3]` and `TOC[idx+2]` and `0x8003E948` returns that difference; the start LBA is the side effect `sw s2,0x8f0(gp)` and the index lands at `gp+0x90C`. `FUN_8003E800`'s `a1` is therefore a sector count too, and `FUN_8005E4D4` is `(sector_count, lba, dest_buffer)` - the documented `(buf, dir_entry, size)` order was reversed. Rows on [`functions/asset-loading.md`](functions/asset-loading.md) and [`functions/runtime-libs.md`](functions/runtime-libs.md). |
| Where is the libcd directory cache? | resolved (`0x801CB408`) | `disassembly` | `FUN_8005DEA0` forms it as `lui at,0x801d` + `sw ...,-0x4bf8(at)`, stride `0x2C`, capped by the two `slti a3,0x80` tests at `0x8005E0E0` / `0x8005E100`; the routine forms no address in `0x801C4***` at all. Where the long-standing `0x801C4BEC` came from is **not** settled - `0x4BEC` occurs at no word in SCUS, so the "offset half of a `lui`/`sw` pair" story is not supported; the only relation the bytes carry is `0x801D0000 - 0x4BEC = 0x801CB414`, which is a record's `+0x0C` name field. |
| The asset descriptor's `+0x04` word | resolved (Σ of the decompressed sizes; never read) | `disassembly` | 105 PROT entries carry a descriptor table, and in all 105 the header word equals the sum of the descriptors' decompressed sizes. It is dead at runtime: 69 sites across 84 images touch `_DAT_8007B85C` and every dereference is at `+0`. The "zero carriers over 1233 entries" figure came from a detector, not from the bytes. See [`asset-descriptor.md`](../formats/asset-descriptor.md). |
| `record[0] + 0x5C` in the player battle files | resolved (no reader exists) | `disassembly` | A sweep of 518,656 words across 84 images finds 8 non-`sp` accesses at that displacement and none of them reads a `record[0]`. The two hits previously read as "both in PROT 0900" are one hit in 0900 and one in 0901 seen through an over-read. |
| Two minigame overlays carry private copies of the two-window tile lookup | resolved | `disassembly` | PROT 0972 at `0x801D617C` and PROT 0980 at `0x801D3F1C` each hold their own copy, which is why the same VA disassembles differently depending on which image is resident. See [`overlay-va-aliases.md`](overlay-va-aliases.md). |
| Do the feature views measure distinct features? | resolved (**no** - twelve measured one blob) | `capture` | A BFS from a feature root spills through the title tick into the save UI, the effect spawner and the move VM, so `title-screen`'s anchor set was a strict *subset* of `muscle-dome`'s. Localized, the same views separate: muscle-dome falls from 781 anchors to 72 and battle-action from 882 to 504. The grade is `capture` because the evidence is a corpus measurement, not an instruction. See [`port-catalog.md`](../tooling/port-catalog.md). |
| What is in the `SCUS_942.54` code gap the citation graph never listed | resolved | `disassembly` + `capture` | [details ↓](#what-is-in-the-scus_94254-code-gap) |
| Why three plausible-code blocks in `SCUS_942.54` can never be a function body | resolved | `disassembly` | The BIOS kernel-patch cluster copies `0x8006EF78` and `0x8006F058` into kernel RAM, and `0x8005BBB8` is the stock PSX exception-handler prologue with no reference of any of the five forms anywhere on the disc. Each is real MIPS that executes only after relocation, so nothing calls it at its link address and no function record exists. See [`runtime-libs.md`](functions/runtime-libs.md#the-payload-blocks-are-code-that-is-never-a-function). |
| Is the unattributable dump-extent residue a re-dumping problem | resolved (no) | `capture` | Three shapes remain and only one is dump-shaped: short windows no image reproduces at that VA, bytes in no extracted image at any VA (needs an **extraction**), and extents where two dumps genuinely resolve to different images (an answer). The earlier "repaired by re-dumping" reading is on [`re-do-not-re-walk.md`](re-do-not-re-walk.md#measurement-readings). |
| Can the inner of two nested overlay spans be measured at all | resolved (yes) | `capture` | Address ambiguity is total for it by construction; byte attribution places most of its extents anyway and the row reports. What is structural is that it can never be resolved *by address* - a statement about one method, not about the image. |
| Where does a slot-B image's highest spawn record end? | resolved (its own move-VM program bounds it) | `disassembly` | Walk the opcode widths to a terminator - `0x08` HALT, or an armed `0x19` / `0x1B` idle loop not immediately followed by one - and round the end up to 4, because the records are word-aligned. 991 of 1027 extents come out exact and 58 of 62 image tops bounded. The residue's direction is **not** uniform: over the 1023 pointer-credited extents, 33 misses split 14 past the end / 13 exactly on it / 6 below. The invariant is narrower - no miss ever *terminates* above the end, because only a terminator produces a claim. Measurement in [`slot-b-module-layout.md`](../formats/slot-b-module-layout.md#bounding-the-highest-record). |
| When does SCUS's `jal 0x801F7B88` run? | resolved (ordinary in-battle frames, while PROT 0920's effect budget is non-zero) | `capture` | One Slippery cast (action `0x92`, loader-B tracker 25 = PROT 0920) fires it **212** times, across phases 6..10 and `0xFF`, with `_DAT_8007BD71 = 0xFF` - battle running - every time. Its gate `_DAT_8007BDC0` has five writers inside 0920: `0x801F6BBC` zeroes it, `0x801F6FEC` seeds `0x204`, `0x801F70CC` drains 8, `0x801F712C` floors it at 4 and `0x801F7B4C` clears it - so it never reaches zero on its own, and the three-site table the doc carried was incomplete. |
| Is a VA that prints in two neighbouring slot-B images one routine? | resolved (usually, but not always - diff the bytes) | `disassembly` | `0x801F81DC` is **two** routines: a 272-byte stager in PROT 0951 and a 2040-byte applier in PROT 0910, differing at the first instruction. The other six "also in" cells around it are residue, verified pair by pair. The band shares a load base and a build buffer, so neither "same VA, same routine" nor "same VA, residue" is safe as a default. |
| How long is `FUN_801DD9D4`? | resolved (588 bytes / 147 instructions, `0x801DD9D4..0x801DDC20`) | `disassembly` | Every one of its seven dumps stops at 276 bytes, because the decompiler stops at the `jr v0` jump table at `0x801DDA88` and the `beq` at `0x801DDA78` branches past it; the body ends at the `jr ra` at `0x801DDC18`. Seven dumps agreeing is agreement about the decompiler, not about the function. The extent belongs in the shared dump header parser - a CSV-only correction leaves the dump claiming the short length and makes the whole body VA-ambiguous. |
| `content_bytes` vs `clean_copy_bytes` - which is an overlay's length? | resolved (the entry's sector extent is the length; `clean_copy_bytes` is how much a RAM capture verified) | `disassembly` | On PROT 0899 they differ by `0xF174`, and using the shorter one made the menu overlay read 99.9% covered while measuring a prefix; every overlay image equals its corrected PROT extent exactly (31 of 31), so `content_bytes` is the denominator disc-coverage uses. See [`disc-coverage.md`](../tooling/disc-coverage.md). |
| Does `PROT.DAT`'s TOC leave gaps between entries? | resolved (no - it is a gapless partition) | `disassembly` | Zero LBA gaps across every row; each entry's extent abuts the next. Any "unindexed gap between entries N and N+1" reading is the superseded over-reading entry size (`prot.md`). |
| PROT 0867's trailing `0x14000` slots | resolved (raw 4bpp PSX TIMs, not LZS monster blocks) | `capture` | Slots 187..194 open with the TIM magic `0x10` where a block carries `dec_size`: CLUT to VRAM `(0, 482)` 256x1, page to `(384, 0)` 64x256 halfwords, 33,312 bytes, rest zero; row 482 sits under the monster CLUT base 484 and `x = 384` is monster page origin 1. The same TIM shape sits inside PROT 0892. LZS-decoding the magic as a length "succeeds" against the zeroed ring buffer. Instrument: `asset account` ([`byte-accounting.md`](../tooling/byte-accounting.md)). |
| `summon.dat`'s seven unclassified slots | resolved (the big-summon third members) | `inference` | PROT 0893 slots 77 / 81 / 85 / 89 / 93 / 97 / 101 classify as bare payload, each directly before an actor-record slot, and the big-summon id band `0x9A..=0xA0` has exactly seven ids; the module's `RAW_SLOT_*` constants tile each slot exactly. No consumer read yet. |
| Do a MAN's partition offset tables tile its data region? | resolved (exactly, data-region-relative) | `inference` | Record offsets are relative to `data_region_offset` and tile `[data_region_offset, data_region_offset + u24_at_28)` with no gaps across five real MANs (one byte of padding left). Measured by `asset account`. |
| Where does an overlay image's code stop? | resolved (at the word after its last `jr ra`) | `disassembly` | Every complete MIPS body ends in `jr ra`, so the data segment begins after the last one; the only exception is a body the sector-granular PROT extent cut short, which still carries a RAM-page `lui` in its tail (PROT 0902 / 0977 / 0979). Corroborated by the slot-B frame-matched partition ending at the same word in all thirteen images. `disc-coverage.py`'s `data_floor`. |
| Is a caller-site citation checkable, or only assertable? | resolved (checkable - the disc word at the cited address settles it) | `disassembly` | A citation of the form "called from `0x...`" is true exactly when the word at that address decodes as a `jal` / `j` / conditional branch to the entry. `check-port-provenance.py`'s `site_relation` reads it off the bytes, so a citation that names the wrong site now fails rather than reading plausibly. |
| Which dumps do the `overlay_dance_*` prints above `0x801D6818` really come from? | resolved (PROT 0972, the fishing overlay) | `disassembly` | PROT 0980's own content is `0x8000` bytes from base `0x801CE818`, i.e. it ends at `0x801D6818`; `0x801D73B8`, `0x801D7D44`, `0x801D7DD8`, `DAT_801D8610` and `DAT_801D8778` all sit above that and below PROT 0972's `0xB000`. Byte-checked: `0x801D73B8` is `overlay_fishing_0972.bin` file `0x8BA0` instruction for instruction. Rows in [`overlay-va-aliases.md`](overlay-va-aliases.md). |
| Which status effects is each enemy immune to? (community: SPD-down lands, ATK-down never) | resolved (no per-monster immunity - it is the scripted-fight boost profile) | `disassembly` + `capture` | Seru-magic stat debuffs are the side-effect `FUN_801F3D3C` stages (`0x801F6870`, 5-20% by magic level) and `FUN_801DDB30` applies per hit. In a scripted fight (`ctx[+0x287]`) the stager drops the effect when the target's **base** stat differs from the record: the boost has moved ATK / UDF / INT (never land), SPD / AGL land once, MP every hit; four casts in five are also suppressed unless the enemy is weak to the element. Random encounters: everything lands. [battle-formulas.md](../subsystems/battle-formulas.md#seru-magic-side-effects---the-element-debuffs-fun_801f3d3c--the-finisher-switch). |
| What is PROT 0981's link base? | resolved (slot A, `0x801CE818`) | `disassembly` | Decoding the image's own `lui`+`addiu` pairs against each candidate base scores 21 of 23 resolvable at `0x801CE818` against 0 of 23 elsewhere, and its 324-byte routine at `+0x1AC` is byte-identical to the dump already taken at `0x801CE9C4`. `asset overlay scan` had answered `0x801D58B8` off a **single** prologue - the instrument degenerates to whatever one prologue implies when an image offers only one, and says nothing about that in its output ([falsified](re-do-not-re-walk.md#measurement-readings)). |
| Are `FUN_801D14B0` and `FUN_801D6710` two routines? | resolved (one source routine linked into two overlays) | `disassembly` | Both are twenty-four instructions and **twenty-two of them match one for one**; the pair that differs is the leading `lui`+`lw` that reads the gate global - `_DAT_801D1AB4` in PROT 0977 against `_DAT_801DBF00` in PROT 0976 - and no branch relocates, because every one is PC-relative and the bodies are the same length. A byte compare that reports zero bytes in common is measuring where it started, not how much the two share. |
| Which scene bundles carry a type-`0x14` FLAG descriptor? | resolved (28 of 105 tables, always the last slot) | `disassembly` | The FLAG descriptor is not a property of a bundle's descriptor count: it appears on 28 of the 105 descriptor tables, whose counts run 4 to 7, and in every one of the 28 it is the last entry. The "every count-4 or count-5 bundle" reading pairs two properties that are only correlated. |
| Why do the save sub-screens read as unreached by every ladder? | resolved (an entry-**context** gap, not a wiring gap) | `capture` | `SaveScreenFlow` only ever constructs the memory-card context, so the sub-screens behind the other entry contexts have no route in even when a ladder drives the screen; the shop entry `0x801DC89C` reaches three bodies by contrast. Adding a ladder does not move the rows - the constructor has to offer the other contexts first. |
| Do `battle_cursor_pose`'s four items have a caller? | resolved (**none anywhere** - link-time dead code) | `disassembly` | A five-form reference sweep finds no site in any image that reaches them, so they are not an un-wired port but code the link dropped. A row that leaves the worklist this way needs an ignore-file entry naming the mechanism, not a reclassification. |
| Does the browser play page emit audio? | resolved (**yes** - the first "silence" was the observer) | `capture` | The probe registered its listener **before** the mixer installed `onaudioprocess`, so it watched a handler that was later replaced and saw silence by construction. Intercepting the setter instead: town01 gives 82 non-zero blocks out of 82 and the boot chain 76 of 76, and the attract FMV plays with audio. The failure mode is the one this repo keeps meeting - an observer placed inside the thing it observes. |
| Why three big PROT entries carry a third of their bytes as residue | resolved (the consumers declare it - the extents are **slot fill**) | `disassembly` | Entries `867`, `893` and `894` are read by seek-and-fixed-length transfers, so the slot size is the claim, not the payload: `FUN_800542C8` seeks `(id - 1) * 0x14000` and reads a literal `0x28` sectors (`0x80054608`), and `FUN_801F17F8` seeks `index * 33 * 0x800` and reads `0x10800` (`0x801F1948` / `0x801F1958`). Each entry's extent is an exact multiple of its own slot stride - 194, 103 and 78 slots - so the bytes past a slot's content are declared fill, not unexplained data. Last-sector slack past a stream terminator is the same shape. |
| The "64 runs" in the residue reports | resolved (an instrument truncation cap, not a disc figure) | `inference` | The sweep printed at most 64 runs per entry; the real counts are 194, 92 and 84. No entry's *largest* run was ever capped, which is why the summaries looked right while the totals did not. Any run count taken off that report before the cap was lifted is a floor. |
| Which shipped scenes carry field-VM ops `4C EA` and `4C 52`? | resolved (three and seventy; a census now answers it for every arm) | `disassembly` | `4C EA` has one carrier per kingdom map bundle; `4C 52` is the chest script's item consume, seventy sites across twenty-five scenes, one text line into each record (the first census read one and three: its walk ended at every record's first `0x1F` segment). The instrument is `asset field-op-census` over every carrier, cross-checked against the independent camera-arm census; its zeros separate "no fixture drives this" from "there is nothing to drive". [`field-op-census.md`](../tooling/field-op-census.md) |
| Why do two identically-shaped `toc[p + 2]` expressions disagree? | resolved (they use different `p`) | `disassembly` | `0x801C70F0` receives a **3-sector** read of `PROT.DAT` from byte 0 - header included (`a0 = 3`, `a1 = 0x801C70F0`, `a2 = 0x80`, `jal 0x8005E9A4` at `0x8003E624`) - so the runtime table is the file's words, header and all. `FUN_8003E8A8(a0)` therefore reads RAM words `a0 + 2` / `a0 + 3`, which are `toc[a0]` / `toc[a0 + 1]` in [`prot.md`](../formats/prot.md)'s header-excluded convention: the **runtime** index is the extraction index **+ 2**. Checked against entry 981 - the TOC word at file `8 + (981 + 2) * 4` gives LBA 47798, and the extracted image is byte-identical there. |
| Is a resolution ratio over `lui` pairs a base test? | resolved (**no** - it is one-sided) | `disassembly` | A base whose two high halves catch **few** pairs scores perfectly on all of them, so the metric ranks a base by how little of the image it explains. On PROT 0896 it reports 65 of 65 at the refuted slot-A base and 110 of 177 at the base the call graph recovers, and it cannot see the call graph at all - which is where an image's own structure is. [`static-overlay-pipeline.md`](../tooling/static-overlay-pipeline.md#a-resolution-ratio-is-not-a-base-test) |
| When may a dump's filename credit an extent? | resolved (only where something else in the same image confirms by bytes) | `disassembly` | An extent whose printed instructions are outside the encodable grammar is unverifiable, and the byte-accounting walker credited it on the dump filename alone. Where nothing in an image confirms, that filename *is* the evidence - and that is exactly the case where the dump program's base was wrong, so the extents land at arbitrary offsets in a file that never held them. Three such extents landed inside `0896`. The guard costs nothing where the corpus is real and reports what it declines. [`byte-accounting.md`](../tooling/byte-accounting.md#a-filename-is-not-corroboration-on-its-own) |
| Can a measurement tool be stale the way its cache is? | resolved (yes, and more quietly) | `capture` | A committed sweep CSV and a "fresh" per-entry run disagreed on two entries, and the stale side was the *binary*: both "fresh" figures are this page's own historical numbers - `0970` before the zero-match rule, `0895` before the `init_pak` walker took the entry back - reproduced exactly by a `target/` built in another checkout. A documented historical figure reappearing is a stale instrument, not a finding about the disc. [`byte-accounting.md`](../tooling/byte-accounting.md#a-stale-binary-is-the-same-failure-one-layer-down) |
| Do two host kernels paired by name do the same work? | resolved (**not necessarily** - and an empty body pairs with anything) | `inference` | The host-drift gate's tier-11 alias row paired the native `tick_field_prop_anims` with the browser's `drive_npc_clips` on a reason asserting both "advance the scene's posed actors"; the native body was `{}`. Tier 12 answers the next question with the only evidence a source scan carries - the set of engine functions each paired body reaches, host helpers followed transitively - and states its own hole: names that are also ordinary std methods are excluded wholesale, so a divergent engine call spelled `insert` is invisible. [`host-drift.md`](../tooling/host-drift.md#tier-12---content-do-two-paired-kernels-call-the-same-engine) |
| What is PROT 0976's pointer table at `0x801D7134`? | resolved (eleven lines of the Baka Fighter help panel) | `disassembly` | The eleven words run `0x801CE948` **down to** `0x801CE818`, which is the slot-A base itself - they address the image's own head string pool at `+0x00..+0x130`, emitted in reading order over a pool the compiler laid out backwards, and the lowest word points at the leading NUL, so the eleventh line is a deliberate blank. `FUN_801D6CBC` walks them at `0xD` px pitch inside a 240x143 frame; its one caller is the cabinet SM `FUN_801CF388` at `0x801D1638`, the "How to Play" arm. [`minigame-baka-fighter.md`](../subsystems/minigame-baka-fighter.md#the-help-panels-pointer-table) |
| What is a scene bundle's last-sector residue? | resolved (the **packer's buffer** - an earlier entry's bytes) | `disassembly` | The bytes after a bundle's last descriptor's LZS stream are byte `k` of the nearest earlier TOC entry whose extent reaches `k`: 80,337 of 80,337 bytes across 90 of 90 bundles. The same leg closes the `lzs_container`, `pack` and `bse_bank` tails, and the donor can be any entry. `legaia_asset::inherited_tail::buffer_run`; [`byte-accounting.md`](../tooling/byte-accounting.md#a-bundles-last-sector-is-the-packers-buffer) |
| Is the packer's buffer run confined to an entry's last sector? | resolved (**no**) | `disassembly` | PROT 0976's tail is `0x98C` bytes of PROT 0970, longer than one sector, so the suffix is searched over the whole entry. The last-sector bound was a property of the scene bundles, whose residue happens to fit one sector. |
| Where does a slot-B record chain stop? | resolved (at eight zero bytes) | `disassembly` | No consumer loop bounds the record band, and the Rust walk had chained an image's zero padding as a `[model_sel 0]` record on into its donor. Both walkers now stop at eight zero bytes, which moves twelve images and puts PROT 0944's cut at `0x199C` with donor 0942 on both instruments - the last Rust / Python disagreement. [`slot-b-module-layout.md`](../formats/slot-b-module-layout.md#the-chain-stops-at-zero-padding) |
| Why did PROT 0780 (`edteien`) rank as `scene_event_scripts` residue? | resolved (its prescript holds **two** records, under the standalone count floor) | `disassembly` | The walker never started, so "walker tail" was the wrong verdict. It is selected by the class - the entry is already placed - and now reads the records positionally, as the scene loader does. |
| Is a dump that opens on data credited as code? | resolved (not any more) | `disassembly` | `FUN_801CF5D0` walks 3,264 bytes of PROT 0898 read-only data - two switch tables and the Seru banner pool - printing table words as `lb ra, 0xNNNN(zero)`. An extent opening on that `$zero`-absolute signature is now refused as code; 68 such extents in 0898. [`byte-accounting.md`](../tooling/byte-accounting.md#a-dump-over-data-is-not-code) |
| Is a `#[wasm_bindgen]` export visible to any cargo gate? | resolved (**no** - it is a property of the impl block) | `capture` | `boot_title_backdrop_draws_json` sat in the plain `impl LegaiaRuntime` rather than the exported one: it compiled, passed the `wasm32` tier, `cargo check`, clippy and the drift gate, and was `not a function` in the browser, where the page's `try` fell back to `null` and drew nothing. Only a headless run of the built bundle sees it. |
| How do you find a host's call sites for a web-only name? | resolved (scan `[.:]name(`, not `.name(`) | `inference` | A method-call scan misses path calls (`Type::name(`), which is how 31 of 32 "web-only" battle-presentation names turned out to have native call sites; the 32nd, `packet_color::hybrid`, is the page's WebGL vertex-colour stream, which the wgpu fragment shader replaces. |
| Can a probe cross a field door without a pad ladder? | resolved (yes - by tile poke) | `capture` | A walk-on door is an exact tile match in the `.MAP` kind-1 trigger table, so writing the player object's position onto a door tile crosses it in about ninety vsyncs; a door that opens a Yes/No picker needs one confirm press too. It measures arrival, never locomotion. `scripts/pcsx-redux/autorun_w5a_poke_walk.lua`. |

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
