# Settled threads: World map / kingdom bundles

One area of the [settled reverse-engineering threads](../re-settled-threads.md) register.
The evidence grades (`disassembly` / `capture` / `decompiled-C` / `inference`) are defined on
[the index page](../re-settled-threads.md#the-evidence-column).

This area covers the kingdom overworld - the `map01` / `map02` / `map03` world-map scenes - and the
kingdom bundles that feed it: how the continent ground and its decorations are drawn, depth-cued and
ordered, which camera frames the walk view, and what the bundle slots and the TMD pool hold. Anyone
porting, modding or re-rendering the overworld finds here which questions are closed and on what
evidence; the working detail lives on [`world-map.md`](../../subsystems/world-map.md).

## Detailed write-ups

Threads whose answer needs more than a table cell. Every other thread is a row of the table under [Threads](#threads).

- [World-map walk-view continent ground render](#world-map-walk-view-continent-ground-render)
- [Field decoration path - does it dispatch the NCC light handlers?](#field-decoration-path---does-it-dispatch-the-ncc-light-handlers)
- [Kingdom slot 4 - per-record semantic](#kingdom-slot-4---per-record-semantic)
- [DAT_8007C018 liveness rule](#dat_8007c018-liveness-rule)

## Threads

| Thread | Status | Evidence | Answer |
|---|---|---|---|
| Is the overworld ground depth-cued, and from which far colour? | resolved (yes, toward a literal `0x100` per channel) | `disassembly` + `capture` | `FUN_801F69D8` calls `SetFarColor(0x100, 0x100, 0x100)` (`FUN_8005B7D8`) at `0x801F729C` just before `jal 0x801F89B8`; the emitter cues each cell with `IR0 = max(SZ1 - 0x5000, 0) >> 3`, `SZ1` being corner `(x1, z0)`. Scratch `0x1F800398` is the base `RGBC`, not the far colour. A PCSX-Redux watch saw far colour `0x1000` on all 6000 cue hits, and on keikoku's retail frame 566 of 584 continent cells match the port's `engine-core::overworld_ground_cue` exactly, all within 3. The emitter tests no `0x1000` cell bit; both hosts draw the cue ([`world-map.md`](../../subsystems/world-map.md)). |
| Are the overworld decoration cells depth-cued? | resolved (yes, one `IR0` per object toward `0xD0`) | `disassembly` | Before each `jal 0x80043390` (`0x801F7254`) the decoration sweep `FUN_801F69D8` forms `IR0 = min(max(TRZ - 0x5000, 0) >> 3, 0x1000)` from the object origin's camera depth (`0x801F7200..0x801F7220`) with `a1 = 0x00D0D0D0`; the dispatcher makes `a1` the far colour and parks `IR0` at `0x1F800038`, which all eight PROT 0901 prim leaves load before their `DPCS` / `DPCT`. The landmarks are not this sweep's. Both hosts stage it per draw (`engine-core::overworld_ground_cue::decoration_draw_cue`; [`world-map.md`](../../subsystems/world-map.md#ground-texturing)). |
| Why do the overworld fog sheets read denser and brighter than retail's? | resolved (the texture blend's 5-bit truncation) | `disassembly` + `capture` | The sheets are additive (`ABR 1`, every row-473 CLUT entry has `STP` set) and texture-blended; the GPU writes `(t5 * c8) >> 7` per channel at the framebuffer's 5-bit depth, so at the median packet colour `39` a wisp texel of `1..3` of `31` contributes nothing, and a sheet delivers about half the light an unquantised float blend adds. Both hosts' screen-primitive shaders run the truncating product, `engine-ui::screen_prim::psx_texture_blend` ([`field-ambient-fx.md`](../../subsystems/field-ambient-fx.md#the-texture-blend-is-5-bit)). Colour, count, shape, draw order and the camera offset match independently of it. |
| Is the sparkle burst `FUN_801E5338` reachable? | resolved (no) | `disassembly` + `capture` | Its only reference is the tick word of template `0x801F2978`, which only `FUN_801E5834` materialises, and nothing on the disc references `FUN_801E5834` in any form (both reference scans plus a raw `jal`/`j` scan). No live actor carries the tick in any of the 98 mednafen states. Filed `[unreferenced]`; no port. |
| Which ordering-table bucket does the overworld ground link at? | resolved (`(max corner SZ >> 5) + 14`) | `disassembly` + `capture` | The bulk terrain emitter `FUN_801F89B8` takes the largest of a cell's four corner SZ values and links it at `(max SZ >> 5) + 14` off `*0x1F8003F4` with no shift (`0x801F8DC8..0x801F8E20`); the fog links at `(SZ - 0x10) >> 5` off the same base, so the keys share one scale and a cell covers a sheet in its own bucket (the fog is first in the chain in all 11 shared buckets). On `keikoku_chest_preload` 538 of 584 matched cells sit at exactly this key and 32 one bucket off. Pinned by `overworld_draw_order_retail_capture_disc.rs`. |
| Which camera does the overworld walk use? | resolved (the field zone camera) | `capture` + `disassembly` | The overworld is a mode-`0x03` field-run scene. On `keikoku_chest_preload`, `sebucus_overworld_resident` and `karisto_overworld_resident` the live pitch, yaw and eye trio `0x800840B8` equal the zone composer `FUN_801DAB90`'s staging descriptor at `0x801F3580`, with `H = 368`; the region ease is `FUN_801DB510` ([`world-map.md`](../../subsystems/world-map.md#walk-view-camera-retail-model-ram-pinned)). |
| What does `0x1F800394` bit 0 select? | resolved (the kingdom overworld) | `disassembly` + `capture` | Set on every non-battle kingdom-overworld state in the mednafen library and clear on every other state, overworld battles included. `FUN_801D629C` (PROT 0897, `s4 = 0x1F800314`, `lw 0x80(s4)`) takes an overworld arm on it: a camera-space depth test before the slot pop (`jal 0x8003D344` at `0x801D6460`, `slti 0x4001` at `0x801D6470`) and a further `-0x28` lift of the particle height (`0x801D651C..0x801D653C`); `map01`'s system script raises the pool's gate (`4C 30`) after widening the view window ([`field-ambient-fx.md`](../../subsystems/field-ambient-fx.md#the-pool-on-the-kingdom-overworld)). |
| Does the disc carry one `4C EA` occurrence or more than one? | resolved (three - one per kingdom map bundle) | `disassembly` | Three: the scripted game-over has one clean site in each of `map01` / `map02` / `map03`. The count needs text segments and pickers decoded as strides of the stream; a census walk that ends at each record's first `0x1F` text segment and starts partition-0 records three bytes late reads one fewer than the op-arm oracle ([`field-op-census.md`](../../tooling/field-op-census.md#4c-ea---the-scripted-game-over---has-one-carrier-per-kingdom-map)). |
| The ninth routine of PROT 0901's draw band | resolved (it is not a per-prim leaf) | `disassembly` | `FUN_801F89B8` is the **bulk terrain-cell emitter** - a hand-written GTE loop that draws one textured quad per map cell. It reads the four signed bytes of the camera's visible tile window at scratchpad `0x1F8003E8..EB` and takes `max - min` on each axis as its two loop counts (`0x801F89FC..0x801F8A28`), which is what makes that window a draw extent rather than a fog hint, and it indexes the floor-height ladder at `0x1F80035C` (`0x801F8A7C`). `jal`'d from `0x801F733C` in its own image. Row + detail on [`functions/world-map.md`](../functions/world-map.md#801f89b8-draws-terrain-cells). |
| The world-map camera's sub-9 arm table, and where its horizon gates store | resolved (subs `0xA..0xC` store in the leaving `j`'s delay slot) | `disassembly` | Field-VM op `0x4C` nibble-4 sub-9 dispatches an arm table spanning `0x801E1480..0x801E162C`. Subs `0xA` / `0xB` / `0xC` write the horizon gate in the **delay slot** of the `j` that leaves the arm (`0x801E1648` / `0x801E1688` / `0x801E16C8`), so a scan that stops at the branch names the instruction before; sub `0xD` scales `_DAT_8008457C >> 12` into `0x8007B910`. The bit-24 arm of sub 9 writes **two** words - `ctrl[+0x4A]` and `_DAT_8007BCAC` - not one. See [`script-vm-menuctrl.md`](../../subsystems/script-vm-menuctrl.md). |
| World-map walk-view continent ground render | resolved | `capture` | [details ↓](#world-map-walk-view-continent-ground-render) |
| Walk-view decoration draw gate - which grid bit stamps the `.MAP` record meshes on the overworld? | resolved (`0x2000` alone; no flag or mesh-0 test) | `disassembly` + `capture` | The resident slot-B kernel `FUN_801F69D8` (PROT 0901, byte-matched against a `map01` walk capture) tests only `cell & 0x2000` at `0x801F6ECC`, skips placed records, and reads `+0x10` with no zero test and no flag-`0x2` test; Y is the 2x2 corner average. Mode 3 reaches it through `FUN_80026CE4`, which picks it over the field sibling `FUN_801F7088` on `_DAT_8007BA90`. The big enterable mountains are `0x2000`-only cells, so a `0x1000` gate drops exactly them - addresses and the cell census in [world-map.md](../../subsystems/world-map.md#placing-the-continent-terrain-engine-port). |
| Walk-view untextured landmark prims - does retail draw the `F*`/`G*` colour prims of the slot-1 pack meshes (Rim Elm's hut roofs)? | resolved (yes, same per-prim dispatch as the textured prims) | `disassembly` | `FUN_80043390` selects the renderer by the group header's `flags >> 1` (`0x80043614`) and skips a group only on a null table entry. Slots 12..=15 (F3 / F4 / G3 / G4) are populated in the SCUS row `0x8007657C` and in the world-map overlay row `0x801F8968` (`0x801F7644 / 0x801F7838 / 0x801F7F78 / 0x801F8198`, cued like the textured leaves). Rim Elm (Drake slot 29) is 24 `G3` roof triangles over textured walls; 34 slots across the three packs carry colour prims - [world-map.md](../../subsystems/world-map.md#top-view-bulk-terrain-render-path-overlay-replaced-per-prim-renderers). |
| `DAT_8007C018[45..53]` mid-load vertex-pool pointers | resolved (structural) | `disassembly` | [details ↓](#dat_8007c018-liveness-rule) |
| Field decoration path - does it dispatch the NCC light handlers? | resolved (yes - the light-source rows, flags `0x10..=0x17`; exec breakpoints in `cave01`) | `capture` | [details ↓](#field-decoration-path---does-it-dispatch-the-ncc-light-handlers) |
| Kingdom slot 4 - per-record semantic + consumer | resolved (the world-map scene's type-`0x05` **ANM animation bank**; the "vertex pool + cluster-A stream" reading is falsified) | `disassembly` + `capture` | [details ↓](#kingdom-slot-4---per-record-semantic) |
| The placed-actor mesh resolver on the world map | resolved (`FUN_80020F88`, at spawn time - not in the draw loop) | `disassembly` | Called from the allocator `FUN_80020DE0` at `0x80020F18` and from `FUN_80024E08` at `0x80024E60`: `actor+0x64 = .MAP_record[actor+0x60][+0x10] + DAT_8007B6F8`, render mode from `rec[+0x12] & 3`, then `FUN_80024D78` fills the `0x9C`-byte chain at `actor+0x44` from `DAT_8007C018[actor+0x64]`. The port's `field_objects::pack_mesh_index` + `FIELD_ACTOR_PACK_BIAS` implement it. See [`world-map.md`](../../subsystems/world-map.md#placed-actors-and-the-mesh-resolver). |
| MAN sections 2 and 5 - what do `_DAT_801C6EA0` / `DAT_80073EE0` carry? | resolved (both are place-name carriers) | `disassembly` + `capture` | Section 2's body is the **scene display name** the on-entry banner draws (and the save screen's location row); section 5, the universal zero-length terminator, leaves its pointer on the **world-map location table** the kingdom MANs trail - 29 records of `region + map x/y + discovery flag + 24-byte name`, walked by the label pass that draws each place's name at its map position. Together with the SCUS quick-travel cells that makes **three** independent carriers of one place name, which is why a rename has to edit all three. Layout + provenance: [place-names.md](../../formats/place-names.md). |
| Where does the model-pool id space split at `0xF0`? | resolved (in the **callers**, not in the resolver) | `disassembly` | `FUN_80024E08` resolves a model id against the pool and does no splitting at all. The `0xF0` fork lives at `0x800393B8` and at `0x8003A2DC` - the latter inside `FUN_8003A1E4`'s placement installer (`0x8003A2CC..0x8003A328`) and its op-`0x0E` sibling - each choosing between `DAT_8007B824` and the scene base `DAT_8007B6F8` before it calls. A port that documents the split on the resolver names a routine that cannot perform it. |
| Is `FUN_80026B4C` the only writer of the TMD pointer table? | resolved (**no** - PROT 0976 zeroes a slot directly) | `disassembly` | The registrar is the only routine that *installs* an entry, but `0x801CF1E0` inside PROT 0976 stores zero into the table without going through it. A liveness rule derived from the registrar alone therefore has one hole; `DAT_8007B6F8` is stored with `sw` and read back with `lhu`, which is the other thing a pool walk has to respect. |
| What is PROT 0981? | resolved (the world-map **top-view debug image**) | `disassembly` | A `0x1000`-byte slot-A image at `0x801CE818`. Its `monster_test` label is CDNAME inheritance from the block opening at extraction 0978; the image's own operands are world-map ones throughout - `DAT_80073EE0`, the kingdom filter `uRam8007b970`, the camera pair `_DAT_80089118` / `_DAT_80089120`, the eye trio `0x800840B8`. One framed function and three leaves: the top-view tick `0x801CE850` (six-arm dispatch, `sltiu a0, 6` on `0x801CF76C`, table `0x801CE838`), the enter / reset `0x801CF4AC`, the record stepper `0x801CF5E8`, the camera clamp `0x801CF678`. [details](../../subsystems/world-map.md#the-top-view-image-on-the-disc-is-prot-0981) |
| Which image holds the dev-menu row strings at `0x801CF344`? | resolved (PROT 0897, the field overlay) | `disassembly` + `capture` | File offset `0xB2C` at base `0x801CE818`. The argument is formed by `lui`/`addiu` pairs inside the renderer's own body - `0x801EAE44` / `0x801EAE48` and `0x801EB320` / `0x801EB324` - and a `map03` state's `0x801CE818..0x801D0000` RAM window is byte-identical to 0897's first `0x17E8` bytes. PROT 0981 aliases the same VA as the other slot-A occupant, and the two are never co-resident, so the address is attributable only by image contents. |
| What writes the overworld camera's vertical-offset control word `scene_ctrl[+0x4A]`? | resolved (`map01`'s entry script, through op `4C 49`'s delta arm) | `capture` + `disassembly` | `P1[0]`'s park loop reaches `4C 49 3C 00 00 00` at `+0x1B2` once its `CD F8 00 00 7E 7E` box test passes; the prologue's `2E 19` has raised `_DAT_1F800394` bit 25, which sub-9 tests first (`0x801E1488`), so it stores `60` at `0x801E14BC` and `60 - player[+0x16]` into `_DAT_8007BCAC` at `0x801E14D4`. A PCSX-Redux watch across the castle-to-`map01` entry sees the reset at `0x8003A0D4` and then that single store ([`field-ambient-fx.md`](../../subsystems/field-ambient-fx.md#the-sheet-is-a-view-space-billboard)). |
| What does `FUN_800271A8` build? | resolved (the overworld's screen-Y curvature table, not a depth ramp) | `disassembly` + `capture` | `FUN_8003AEB0` calls it as `(0x28, 0x2AB980)` when `_DAT_8007B6A8` is set. It fills `0x8007BB08` with a `0x4000`-entry quadratic drop, then `RTPS`es `(0, ramp[3n/2], 2000)` at `H = 0x3C0` and stores `SY - 0x78` into the `0x2000`-entry `i16` table at `0x8007BB04`. Its readers are `FUN_80043390`'s overworld leaves and `FUN_8003F86C` at index `(SZ >> 5) + 1`, and `FUN_8001C394` at `(ΣSZ >> 2) >> 5` with no `+1`; checked entry for entry against capture RAM ([`renderer.md`](../../subsystems/renderer.md#frame-setup--present)). |
| Is the overworld's curvature per sheet or per vertex? | resolved (per vertex, overlay rows 12..19 only) | `disassembly` | The world-map leaf at `0x801F7644` bends each corner's `SY` by the table entry for its own `SZ` after `NCLIP` and the ordering-table depth are taken (`0x801F7770..0x801F77E4`). Only the overlay's rows 12..19 read the table pointer; the SCUS lit rows 8..11 draw flat. A fog sheet takes one entry at its particle's depth ([`renderer.md`](../../subsystems/renderer.md#frame-setup--present)). |
| Does any overworld draw reach the lit rows `8..11` the curvature skips? | resolved (no - the port's bend-everything is inert) | `disassembly` | The overworld dispatch takes a group's row as its header word `>> 17` (`srl s5,s7,0x11` at `0x800435A4`), i.e. `flags >> 1`, so a lit row is a `flags 0x10..=0x17` group. A census of every TMD a magic sweep finds in the three kingdom bundles' scene entries and in the party pack PROT 0874 counts none; the lit-row groups on the disc sit in field scenes and a few non-overworld entries. So bending every overworld draw matches retail pixel for pixel. [details](../../subsystems/renderer.md#frame-setup--present) |

### World-map walk-view continent ground render

*Status:* resolved - Evidence: `capture`. Heightfield geometry plus per-cell, terrain-keyed multi-page
texturing; shipped in the engine.

The continent ground is a procedural heightfield drawn as one textured quad per cell, not instanced
meshes; the slot-1 pack meshes are only the sparse placed landmarks.

- **Height:** `FUN_80019278` (SCUS, always resident) reads an entity's XZ, gates on the object-grid
  `0x1000` cell bit and bilinearly interpolates the floor height from the 2x2 block of `+0x4000`
  nibbles (`grid[0]`, `[1]`, `[0x80]`, `[0x81]`, each `& 0xF` -> `DAT_1F80035C[nibble]`, weighted by
  the sub-tile position, `>> 0xE`).
- **Landmarks:** `pool = record[+0x10] + prefix`, resolved 14 of 14 against the live render list via
  `FUN_8001ADA4` case 5 / `FUN_80024D78` / `FUN_80020F88`; spawned by `FUN_8003A55C`, gated on
  `flags & 0x4` (about six objects, pools 36 / 34 / 11 / 7 / 19 / 21). The bulk `0x1000` cells carry
  `+0x10 == 0`.
- **`.MAP` source:** a raw (uncompressed) `0x10000` region at `PROT.DAT` `0x655800`. The loader's
  retail branch resolves PROT index `*(0x80084540) = 0x55 = 85` -> `toc[87] = 3243`. The per-entry
  extraction's `0085_map01.BIN` (a count-46 pack at `0x668000`) is the field object / script pack; the
  `.MAP` sits under the overlapping manifest entry 83.
- **Texturing:** per-cell `POLY_FT4` (cmd `0x2C`), one 32x32 quad per visible cell in a row-major
  world-cell sweep, selected from the cell's object record: `+0x14` = 8x8 atlas tile
  (`u = (id % 8) * 32`, `v = (id / 8) * 32`), `+0x15` = PSX `tpage` (`0x1A` grass, `0x0C` mountain,
  `0x1B` / `0x1C` water, `0x0B` forest), `+0x16..+0x18` = PSX `clut` word. Tile, page and clut match
  the record 100% across mountain and coast captures
  (`scripts/ghidra-analysis/analyze-walk-ground-tiles.py --verify-rule`).
- **Port:** `legaia_asset::field_objects::build_walk_heightfield` / `Scene::walk_heightfield` - a
  quad per `0x1000` cell, corner Y from the `+0x4000` LUT, per-cell UV and `[clut, tpage]` baked into
  `WalkHeightfield::uvs` / `::cba_tsb`; verified against the disc.
- **Not** a positional `(col % 3, row % 3)` pick on a single `0x1A` grass page: grass cells' `+0x14`
  values land in the atlas's top-left 3x3 block, which imitates one. The per-cell terrain renderer is
  overlay-resident (`0x801F76xx`), so a static SCUS consumer sweep does not see it.
- **Not** a combined walk + overview mesh pool: 0085's and 0093's slot-0 atlases target the same VRAM
  pages, so they are mutually exclusive sets that clobber each other if co-loaded.

Owning page: [`world-map.md`](../../subsystems/world-map.md#placing-the-continent-terrain-engine-port)
(texturing under [Ground texturing](../../subsystems/world-map.md#ground-texturing)).

### Field decoration path - does it dispatch the NCC light handlers?

*Status:* resolved (yes - the light-source rows) - Evidence: `capture` (`cave01_attached_light` exec
breakpoints and frame) with the dispatcher disassembly.

The field decoration pass `FUN_801F7088` emits through the per-prim dispatcher `FUN_80043390`, and
TMD groups with flags `0x10..=0x17` reach its four `NCCS` / `NCCT` light handlers.

- **Dispatch:** the body is picked by `flags >> 1` (`0x800435A4`); kinds 8..11 are `FUN_8004409C` /
  `FUN_8004423C` / `FUN_80044434` / `FUN_800445B0`, the ROM's only hardware-light code.
- **Frame:** `cave01`'s rock columns are all flag-`0x11` groups. `cave01_attached_light` holds the
  scene-load light (angle trio `0x994 / 0x9CC / -0x62C`, back colour `0x202020`) and the light matrix
  `0x1F8003A8` that trio builds, element for element. Retail's frame shows the wall turned from the
  light at an eighth of its texel and the facing wall above neutral; shading the lit rows with `NCCS`
  under those inputs reproduces it to within the image channel's noise, where a neutral `0x80`
  leaves both walls at their raw texel.
- **Exec count:** `autorun_w4d_light_kind_hits.lua` on that state (no warp) hits kind 8
  (`FUN_8004409C`) 882 times and kind 9 (`FUN_8004423C`) 200 times in 60 vsyncs, every hit returning
  to `0x801F78D4`, the decoration pass's call into the dispatcher.
- **`town01` is consistent:** a cold-boot `dirty_exec_hot` sweep (~46M interpreted hits) finds zero
  hits in `[0x800445B0, 0x80044798)`, which is the kind-11 body alone, and `town01` sets a white back
  colour (op `4C 8A`) under which a lit row draws at or above neutral and looks baked. Its houses do
  carry lit rows, and a ~31K-hit `town01` window shows the kind-11 body hot.
- **Coverage limit:** that sweep is the script-locked prologue arrival, effectively one viewpoint;
  `map02` / `map03` and free-roam multi-screen towns are unsampled.

Three instruments cannot test this, and each reads as a negative:

- `gte_ring` records only `RTPS` / `RTPT` (`gte_rtp_record`, func `0x01` / `0x30`) and `INTPL` (func
  `0x11`), never `NCCS` / `NCCT` / `DPCS` / `DPCT` (`gte.cpp` record hooks). A GTE-ring "zero NCC" is
  vacuous; `dirty_exec_hot` is the valid liveness probe.
- `fntrace` catches only dispatcher round-trips. The SCUS render handlers are natively compiled and
  directly called, so `FUN_80043390` records 0 hits while `fntrace_arm all` catches ~300k
  dispatches/s.
- `map01`-class world maps dispatch through the replaced table `0x801F8968` to the PROT 0901 leaves
  (`dirty_exec_hot` hot at `0x801F6E6C`), not the SCUS `0x8004xxxx` handlers - a different renderer,
  not a light-path test.

Owning page: [`renderer.md`](../../subsystems/renderer.md#the-light-source-rows).

### Kingdom slot 4 - per-record semantic

*Status:* resolved - Evidence: `disassembly` + `capture`.

Slot 4 is the world-map scene's asset-type-`0x05` **ANM animation bank**, structurally identical to
every field scene's type-`0x05` section.

- **Container:** `[u32 count][u32 byte_offsets]`; each body is one clip - an 8-byte header (marker
  `0x080C`, `part_count`, a `u16` frame count, an interpolation flag, the sub-frame divisor
  `1 / 2 / 4`) then `parts * frames` 8-byte entries, frame-major:
  `entry(f, p) = body + 8 + (f * part_count + p) * 8` (`0x8001BAC0..0x8001BAEC`).
- **Entry:** three 12-bit signed translations in bytes `0..4` (sign-extended at
  `0x8001BF44..0x8001BF6C`, pushed through GTE `MVMVA` at `0x8001C0E0`) and three 8-bit rotation
  angles in bytes `5..7` (`angle = byte << 4`, read at `0x8001C0CC` / `0x8001C0E4` / `0x8001C0E8`).
  The only unread field is byte 4's high nibble, zero in all 22,228 entries.
- **Load:** `FUN_8001F05C` case 5 stores the buffer at `_DAT_8007B888` (`0x8001F3A8`). The pointer
  has six references across SCUS and all 32 extracted overlay images - the store, a reset in
  `FUN_8002541C`, one read in `FUN_800204F8`, three in the Baka Fighter overlay - none in a render
  path.
- **Clip select:** `FUN_800204F8` picks one of three banks (`0x80020534..0x80020598`): party flag
  `actor[+0x10] & 0x01000000` -> `_DAT_8007B75C`; else `actor[+0x5C] < 0x400` -> `_DAT_8007B888`
  (slot 4); else `_DAT_8007B840`, the type-`0x0B` "MOVE2" bank. The lookup is **1-based**,
  `rec = bank + *(u32*)(bank + (id & 0x3FF) * 4)`, stored at `actor[+0x4C]`, with the 1/16-frame
  cursor at `actor[+0x68]`.
- **Draw:** the animated renderer `FUN_8001B964` (`FUN_8001ADA4` render mode 1, table `0x8001042C`)
  walks the frame's parts, refusing unless `chain[0] == part_count` (`0x8001BAF0`), and per part
  calls the pose decoder `FUN_8001BE80` (`jal` at `0x8001BB20`) then `FUN_80043390` (`0x8001BC84`).
- **Live:** in a `map01` field-run state `_DAT_8007B888 = 0x8011A624`, the 32,304 bytes there are
  byte-identical to the disc payload, and four animated actors carry `+0x5C` = 5 / 11 / 14 / 15 with
  `+0x4C = base + offsets[id - 1]` and clip `part_count` = 2 / 12 / 14 / 2, equal to their pool TMD's
  `nobj`. The capture's return addresses `0x8001BB28` and `0x8001BC8C` are the two `jal`s above.
- **Detector:** `asset player-anm extracted/PROT/0086_map01.BIN --desc-count 7` reports one
  player-ANM bundle with `record0 marker_1 = 0x080C`.
- **Not** a GTE vertex pool `(i16 x, y, z, attr)` walked by a "cluster-A command stream": the field
  boundaries fall on nibbles, not halfwords, and `attr` is the Y / Z rotation pair, read every frame.
  `FUN_80044C14`, a per-kind prim handler that does read vertices that way, does not own this buffer.
- **`ra = 0x801F78D4`** in the warp capture is not a slot-4 reader: it is the return of
  `jal 0x80043390` at `0x801F78CC` in PROT 0900 (PROT 0901 holds no call there), a draw wrapper
  handed a TMD group pointer.

Which actor plays which clip is scene data, not format: the ids are scene-script literals in
`actor[+0x5C]`.

Owning page: [`world-map-overlay.md`](../../formats/world-map-overlay.md).

### DAT_8007C018 liveness rule

*Status:* resolved (structural) - Evidence: `disassembly`.

An entry of the TMD pointer table is live only at indices `0 ..= _DAT_8007BB38`; there is no
per-index semantic, and `45..53` in a small field scene is stale carryover.

- **Registrar:** `FUN_80026B4C` writes `DAT_8007C018[cursor]` (`sw a0,0x0(v1)` at `0x80026BA8`,
  `v1 = 0x8007C018 + cursor*4`) and mirrors the **pre-increment** cursor into `gp+0x820` =
  `_DAT_8007BB38` (`0x80026BBC`).
- **Walker:** `FUN_801D8280` loads that counter (`0x801D8284`) and runs `i = 0 ..= counter`,
  stepping the base by 4; entries above the counter are never visited.
- **Counter writers:** an all-forms sweep finds exactly one store corpus-wide, and it is
  `gp`-relative, so an absolute-only scan finds none.
- **Per-stage reset:** `FUN_8001E1B4` at `0x8001E3AC`.
