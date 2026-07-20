# Asset loader

How the runtime stitches per-scene assets together: given a scene name, which
files get pulled off the disc, in what order, and how the meshes, textures and
palettes end up wired to each other in RAM and VRAM.

**Where it lives.** There is no single loader. Each scene *type* has its own
entry point, all sharing one dev/retail split: the dev branch loads via PROT
indices; the retail branch resolves the same dev path strings through
`FUN_8003E6BC` (the path-based opener) into those same indices via
[CDNAME.TXT](../formats/cdname.md). The main ones are the battle bundle
(`FUN_800520F0`), the field / town chain (`FUN_8001F7C0` + `FUN_800255B8`), and
the descriptor walker they both feed (`FUN_80020224` → `FUN_8001F05C`).

**Port counterpart.** `engine-core`'s `SceneResources::build_targeted` (see
[CLUT-data scattering](#clut-data-scattering)) plus the `legaia_asset` parsers -
`scene_asset_table`, `scene_tmd_stream`, `battle_data_pack`.

**The thing that catches people out:** the slot→asset mapping is **positional**.
There is no slot→asset indirection table anywhere; the descriptor's own
`data_offset` field *is* the indirection, and slot `i` is simply the `i`-th
8-byte descriptor. People go looking for a lookup table that does not exist.

**A second one:** a scene's descriptor offsets are relative to the *extended*
on-disc footprint, so they routinely point past what the per-PROT TOC crops off.
An offset that looks out-of-bounds usually is not. See
[Field / town scene loader](#field--town-scene-loader-fun_8001f7c0--fun_800255b8).

## Battle bundle (`FUN_800520F0`)

The 11-case battle scene loader. Case 6 loads the `befect_data` bundle (PROT 0x369–0x36B). Case 0xE initialises the runtime [effect 2-pack wrapper](../formats/effect.md) via `FUN_801DE914`. Case 0xFF dispatches `0x801F17F8` (the side-band streaming-effect handler that streams `summon.dat` / `readef.DAT` - extraction PROT 893 / 894, see [`formats/summon-readef.md`](../formats/summon-readef.md)).

Two cases call `FUN_8003E104(monster_idx, slot, dst_buf)` to populate slots 7 and 8 with the active battle's monster sound banks - the per-monster body of `h:\mpack\monster.snd`. Each monster has a `(start_lba, end_lba+1)` entry pair in the TOC at `0x801C8980 - 0x10`. See [`subsystems/audio.md`](audio.md) → "Monster sound bank" for the full loader contract.

The asset-viewer's `--bundle battle` mode mirrors this loader's PROT 865–890 set so character meshes have the right CLUT bindings.

### Per-PROT walker (`FUN_8001FE70`)

One battle-scene state (in `FUN_800513F0`, around `0x80051a50`) calls `FUN_8001FA88(scene_base + slot, 0, dst_buf)` to load a per-scene PROT entry into the working buffer, then `FUN_8001FE70(dst_buf)` to walk its chunk list. The walker is the dispatch path for the [`scene_tmd_stream`](../formats/scene-bundles.md) layout - leading TMD body followed by streaming chunks - and is *different* from the standard `FUN_8002541C` streaming walker:

- First chunk: read `chunk0_header = u32 LE` at offset 0. Low 24 bits = TMD body size. Round up to 32-byte alignment, allocate a buffer of that size at `_DAT_8007B864`, copy the TMD body in via `FUN_8003D26C`.
- Loop: advance by `(prev_size & ~3) + 4` to the next chunk header. Read `header`; if `header & 0xFFFFFF == 0`, exit (terminator). Otherwise:
  - If `(header >> 24) == 0x01` -> call `FUN_800198E0(payload_ptr)` (LoadImage).
  - If `(header >> 24) == 0x02` -> exit (explicit terminator).
  - Other types are skipped silently (the loop advances without uploading).
- Returns once the terminator is hit. **It returns `param_1 + 1`** - a pointer to the word just past the terminating header, i.e. the start of the *next* region. The `FUN_80026B4C` TMD register call that follows hooks the parsed TMD into the per-scene mesh pointer table at `0x8007C018 + idx*4`.

This is the path that uploads field NPC palettes to VRAM row 479 - they're plain PSX TIMs wrapped in type-0x01 chunks, dispatched only during battle init. See [`docs/formats/npc-palette.md`](../formats/npc-palette.md) for the cross-save corroboration and [`docs/formats/scene-bundles.md`](../formats/scene-bundles.md#streaming-tail---fun_8001fe70-walker) for the type-byte table.

### Concatenated sub-streams (the "two-list" / continuation case)

Some scene_tmd_stream entries hold **more than one** complete `[chunk0 TMD][type-0x01 TIM chunks][terminator]` sub-stream concatenated, each starting on a `0x800` (sector) boundary with zero padding filling the gap (`0006_town01`: sub-stream 0 at `0x0`, sub-stream 1 with its **own** leading TMD at `0x14000`; verified across the town01 / town0b / town0c clusters). The bytes earlier notes called a "continuation TIM list" are really the second sub-stream's TIM chunks - sub-stream 1 is self-contained, not a bare tail of sub-stream 0.

`FUN_8001FE70` walks exactly **one** sub-stream. Its return value (`param_1 + 1`, past the terminator) lands on the next sub-stream's region, so a sector/slot-indexed caller can walk the rest by re-invoking the walker on that boundary. The single static caller `FUN_800513F0` (battle init) calls it **once** (the `s3 < 4` loop above the call is the 4-party-member setup, not a sub-stream loop), so in battle only sub-stream 0 is uploaded. The multi-sub-stream caller is the per-scene field/town dispatch (`FUN_8001F7C0` → `FUN_80020224` → `FUN_8001F05C`, overlay-resident / descriptor-driven), still capture-blocked. `legaia_asset::scene_tmd_stream::sub_streams` enumerates the blocks properly (each a full sub-stream with its own TMD);
`battle_tim_chunks` reports sub-stream 0's TIMs as `Tail` and the later sub-streams' as `Continuation`. The engine's field-mode loader uses both to **skip** these battle-only TIMs (row-479 palettes aren't field-resident - matching retail).

## Field / town scene loader (`FUN_8001F7C0` + `FUN_800255B8`)

The town/field scene-init chain. Builds paths under `DATA\FIELD\` and `h:\PROT\FIELD\<scene>\` (per-scene extension family: `.MAP` collision/trigger file at raw block base `+0`, `.PCH` trigger patch at `+1`, event-scripts sister at `+2`, `.LZS` asset bundle at `+3`, BGM base at `+6` - raw CDNAME-define index space). The loader walks the [scene asset table](../formats/scene-bundles.md) at the `.LZS` slot to pull each typed file in turn.

The on-disc form of the scene asset table is the canonical 7-typed-asset bundle (`07 00 00 00` lead). The descriptor offsets past the first are **file-relative against the loaded raw footprint** (= the bundle entry's extended on-disc footprint, `Archive::read_entry`), not relative to a decompressed working buffer: the walker hands `base + data_offset` to the dispatcher as the *source* of an independent LZS stream, which it then decompresses into a separate malloc'd target. The offsets routinely run past the TOC-indexed end into the trailing-overlay sectors the per-PROT TOC crops off (e.g. `0588_juui1.BIN`'s indexed view is 67584 B but `desc[4].data_offset` is 177413, valid against the 186368 B extended footprint).
See [scene-bundles.md](../formats/scene-bundles.md#scene_asset_table---count-prefixed-asset-bundle) for the byte-level layout.

The asset chain for any given scene is "load the scene asset table, walk each descriptor, then load each typed sub-asset via the dispatcher." The slot-to-asset mapping itself is **positional + offset-based** and fully pinned (see the walker section below); what remains partial is the runtime *cross-reference stitching* between already-loaded sub-assets (e.g. a placed actor in the MAN naming a TMD-pack index), which the loader resolves from live pointers.

### WARP opcode → minigame door-warp flow (sub_id)

The field VM opcode `0x3E` with `op0 >= 100` triggers the minigame door-warp: `sub_id = op0 - 100`. This is stored in `_DAT_8007ba34` and the game mode switches to `0x18` (24, OTHER INIT), whose handler is `FUN_80025980`. Because only 7 destinations exist, a genuine warp's `op0` is always `100..=106`; this range (plus the absence of the `0x80` cross-context prefix) is what the placement classifier uses to reject text-desync phantoms - see [`world-map.md` → classifying the entity kind](world-map.md#classifying-the-entity-kind-from-its-script).

`FUN_80025980` loads a **minigame code overlay** via `FUN_8003EBE4(sub_id + 0x4D)` (`sub_id >= 6` adds 2 first). Only 7 distinct warp destinations exist (sub_id 0–6). The loader values `0x4D..0x55` are raw loader parameters, not extraction PROT indices - they resolve to **extraction entries 0972..0980** (`prot_index = param + 0x37F`; fishing 0972, slot machine 0975, Baka Fighter 0976, dance 0980, plus dev modules). Each overlay's init entry sits at an overlay-resident address (`0x801CF070`, `0x801CE8A0`, etc.). Full chain + sub-id table: [`script-vm.md` § 0x3E WARP](script-vm.md#0x3e-warp-mode-24-minigame-door-warp).

The op carries **no destination scene name**. The **scene name** (stored at `DAT_80084548`) is pre-set before `FUN_80025980` executes (by the `0x3F` named scene-change below); `FUN_80025980` backs the active name up into `0x8007BAE8`, and the return handler `FUN_80026018` restores it on minigame exit, so the door-warp round-trips back to the departure scene. The overlay entry function reads the name buffer and passes it to `FUN_8001F7C0` and `FUN_80020118`.

Key globals:
- `DAT_80084548` - scene name string (max 8 chars; e.g. `"izumi"`, `"town01"`)  
- `DAT_8007050C` - mirror of the scene name; the executable's static default is `"town01"` (loaded from the dev file `initmap.txt` by `FUN_8001D424`)  
- `DAT_80084540` - current scene's PROT base index (short)  
- `DAT_8007b768` - pending destination PROT index; `0xffff` = none  
- `DAT_8007ba34` - pending warp sub_id (0–6); read by `FUN_80025980`  
- `DAT_8007bae8` - WARP handler's backup of the previous scene name (8 bytes)  

The `DefaultMapIdResolver` in `engine-core::scene` uses CDNAME blocks in ascending PROT-index order as a positional approximation. The actual retail warp only supports 7 destinations, not the full CDNAME scene list.

### Name-based scene change (`FUN_8001FD44`) + the transition streamer (`FUN_80021934`)

Distinct from the map-id door-warp above, the engine has a **name-based scene-change packet** that sets the destination scene by its CDNAME string. **`FUN_8001FD44(name_ptr)`** copies the target name into `0x8007050C`, syncs it to the active buffer `0x80084548` (`FUN_8001D7F8`), and stages the destination index (retail: `_DAT_8007B9A0` at `gp+0x688`; dev flag `_DAT_8007B8C2` set: directly into `DAT_8007B768`), both from the resolved halfword at `0x80084540`; `s_ERR_CHANGE_PACKET` guards re-entry while a packet is pending (`_DAT_8007BA3C`).
This is the path the opening's intro-skip packet uses - see [`subsystems/boot.md`](boot.md#the-opening-scene-chain--the-fun_801d1344-intro-skip). Because the destination name is supplied as a code/data string (the skip's `"town01"` is the overlay literal at `0x801CE82C`), this path is **not** constrained to the 7 door-warp map_ids. `see ghidra/scripts/funcs/8001fd44.txt`.

It then spawns the **scene-transition streaming actor** into the system pool at `_DAT_8007C34C` via `FUN_80020DE0(&DAT_80070734, _DAT_8007C34C)` - descriptor `0x80070734` of the 24-byte spawn-descriptor family at `0x800705FC..0x80070763` (just below, and phase-misaligned with, the mode table at `0x8007078C`; layout `[+4 0xFFFF0000][+8 handler][+0xC flags]`; the spawner copies `+8` into `actor+0x0C` and zeroes the state counter `actor+0x1A` - `see ghidra/scripts/funcs/80020de0.txt`). The handler is **`FUN_80021934`** (real entry 3 instructions before the `0x80021940` prologue; `see ghidra/scripts/funcs/80021940.txt`), a 5-state SM over `actor+0x1A` (jump table `0x80010760`):

- **State 0**: `DAT_8007B648 = 0x80`, seed the 70-frame countdown `gp+0x710 = 0x46`.
- **States 1/3**: poll stream progress via `FUN_8003DE7C(1)`.
- **State 2** (dev): stream **raw TOC entry `DAT_8007B768 + 3`** - the destination block's `.LZS` / [`scene_asset_table`](../formats/scene-bundles.md#scene_asset_table---count-prefixed-asset-bundle) slot - into `_DAT_8007B85C` by index (`FUN_8001EEF0` → `FUN_8003EB98` → `FUN_8003E8A8`); byte size cached at `gp+0x73C`.

`FUN_8001EEF0` returns a **sector** count, not a byte count: it biases its byte result by `0x7FF` and arithmetic-shifts right by 11, but re-biases by a further `0x7FF` first when the sum went negative. That second bias makes the shift truncate toward zero instead of flooring, so a negative error return stays an error rather than becoming `-1` sectors of apparent payload. Ported as `legaia_asset::boot_overlay::bytes_to_sectors`. In a retail build the dev/index branches are unreachable and the whole routine collapses to the path-based resolver plus that rounding.
- **State 4** (retail, once the countdown lapses): rotate the name buffers (`0x80084558` ← `0x80084548` ← `0x800915C8`), set `DAT_8007B768 = _DAT_8007B9A0`, build the literal path `DATA_FIELD\<scene>.LZS` (base string `0x800106C4`, suffix `0x8007B3CC`) and stream it into `_DAT_8007B85C` by name, then hand off with `_DAT_8007B9EC = 1` and **`_DAT_8007B83C = 2`** (mode 2 MAIN INIT).

So the descriptor table `FUN_80020224` walks at `_DAT_8007B85C` is the **raw `.LZS` bundle streamed there at transition time** - the count word lands at the buffer base because the whole file starts with it; there is no relocation step. Dev and retail converge on the same content: raw `scene_base + 3` **is** the `.LZS` entry (extraction index `define + 1`, e.g. `map01` → extraction `0086`, verified descriptor heads across the corpus). Sibling descriptors in the same family include `0x800705FC` (handler `FUN_801D1344`, the intro-skip dialog actor) and `0x8007065C` (the op-`0x49` name-entry setup record, `see boot.md`).

The field VM reaches this packet through **opcode `0x3F`** (named scene-change), which carries the destination name *inline in the bytecode* (`[0x3F][i16 index][u8 name_len][name][entry_x][entry_z][dir]`) and calls `FUN_8001FD44(name, index)`. So most in-game scene transitions - including overworld town/dungeon entry, which a scene's controller script lists as a table of `0x3F` ops - carry a recoverable destination name; see [`world-map.md` → scene destinations](world-map.md#scene-destinations) and [`script-vm.md`](script-vm.md). (`0x3F` is not a dialog opcode, despite an older mislabel.)

## Asset descriptor walker (`FUN_80020224`) - the slot→asset mapping

Walks the [asset descriptor format](../formats/asset-descriptor.md) and calls the asset-type dispatcher per descriptor. Its sole runtime caller in retail is the town overlay's `FUN_801D6704` (MAIN_INIT) at `0x801D6B0C` with `a0 = 0`. The result is stored at `0x80087AF8`. So the walker IS exercised by retail gameplay, just not from a static call site inside `SCUS_942.54`.

The mapping a scene loads is **positional** - there is no separate slot→asset indirection table; the descriptor's `data_offset` field *is* the indirection. The full chain, traced from the field init at `FUN_801D6704`:

1. **`per_stage_init` (`FUN_8001E1B4`)** allocates a single 0x62C00-byte asset buffer once at boot and stores its base at `_DAT_8007b85c` (`FUN_80017888(0, 0x62c00)`).
2. **The transition streamer `FUN_80021934`** (spawned by `FUN_8001FD44` - see the scene-change section above) has already streamed the destination's raw `.LZS` bundle (raw TOC entry `scene_base + 3`) **wholesale into `_DAT_8007b85c`** during the transition fade, so the count word sits at the buffer base with no relocation step. **`field_asset_loader` (`FUN_8001F7C0`)** separately stages the scene's three sidecars into the per-scene buffer `*(0x1F8003EC)`: `DATA\FIELD\<scene>.MAP` at `+0`, `DATA\FIELD\<scene>.PCH` (the [`scene_v12_table`](../formats/scene-v12-table.md) walk-on trigger patch; zero-fill `0x800` when absent) at `+0x12000`, and `efect.dat` at `+0x12800` (`= _DAT_8007B8D0`). `see ghidra/scripts/funcs/8001f7c0.txt`.
3. **`descriptor_pair_walker` (`FUN_80020224`)** reads `count = *base`, then for `slot in 0..count` it advances an 8-byte (2-word) cursor and calls `asset_type_dispatch(base + descriptor[slot].data_offset, descriptor[slot].type_size, scene, 0)`, OR-ing the per-slot return codes into a status word. Descriptors live at `base + 8 + slot*8`.
4. **`asset_type_dispatch` (`FUN_8001F05C`)** splits `type = type_size >> 24` and `size = type_size & 0x00FF_FFFF`, then jumps via the dispatch table at `0x80010638 + type*4` (type bound: `< 0x15`). For LZS-payload types it `FUN_8001A55C`-decompresses from the `base + data_offset` source into a fresh malloc'd target.

So **slot `i` ⇒ the `i`-th 8-byte descriptor; payload at `base + data_offset`; handler keyed by `type_size >> 24`.** `legaia_asset::scene_asset_table::resolve` returns the table plus the base it is relative to for **both** the bare variant (count word at offset 0) and the prescript-prefixed [`SceneScriptedAssetTable`](../formats/scene-bundles.md) variant (count word at a 0x800-aligned offset past the event prescript); `SceneAssetTable::slots` reproduces the positional walk and `SceneAssetTable::payload_range(slot, base)` resolves a slot's payload span. A disc-gated corpus test (`scene_asset_table_walk_real`) verifies the walk against every classified entry (88 bare + 79 scripted): the first slot anchors at `base + header_end` and every slot's type is a legal dispatcher type.
The base the walker receives is `_DAT_8007b85c` because the transition streamer put the raw bundle there (statically pinned - the former "relocation is capture-blocked" note is closed); for the scripted variant the count word sits at its 0x800-aligned in-file offset past the prescript, which the static resolver reconstructs structurally.

## CLUT-data scattering

Many character meshes reference CLUT rows that live in **different PROT entries** from their TMD source. The runtime asset chain stitches them together - the loader puts the relevant TIMs into VRAM before the TMD is rendered.

Engines that drive a clean-room scene loop call [`SceneResources::build_targeted`](../../crates/engine-core/src/scene_resources.rs) once per scene transition. The builder:

1. Parses every TMD in the scene's CDNAME block (plus the optional shared blocks - see below).
2. Collects the union of all prim-target rectangles (CLUT rows + texture-page UV bboxes the meshes will sample).
3. Walks every TIM and decides per-block whether to upload it, suppressing the image block when it would land on a CLUT row another mesh references and vice-versa.

This matches what the retail field loader does (DMA only the texture bytes the current scene's meshes need) and avoids the 4bpp-vs-256-wide CLUT collisions that previously dropped 80%+ of textured prims through the prim filter. See [`renderer.md`](renderer.md#engine-side-targeted-upload--shared-blocks) for the engine-side wiring.

### Field-shared CDNAME blocks

`FIELD_SHARED_BLOCKS = ["init_data", "player_data"]` is the set of CDNAME blocks the retail field engine keeps resident in VRAM across scene transitions. The **field-character meshes and textures both originate from PROT 0874** (the `player.lzs` container `FUN_8001E890` loads by disc index `0x36c`): §0 = the 5-TMD character mesh pack that populates `DAT_8007C018[0..4]`, §1 = effect / `vdf` models, and §2 = the field-character texture pack - eight TIMs uploaded to VRAM (the three Vahn/Noa/Gala atlas pages at texpage `(832, 256)` with per-character CLUTs on row 478). See [`character-mesh.md` § Textures (field form)](../formats/character-mesh.md#textures-field-form) and [`world-map-overlay.md` § Disc-side source of `[0..4]`](../formats/world-map-overlay.md#disc-side-source-of-04).
PROT 876 (`player_data`) is a separate streaming file - VAB + an empty `TIM_LIST` + a SEQ trailer - and carries neither the meshes nor the player textures (an earlier reading that placed the player atlas there, at `fb=(768, 0)` / CLUT `(0, 500)`, is **falsified**). `init_data` (PROT 0) holds shared UI / sprite tiles. `SceneHost::enter_field_scene` passes both blocks to `build_targeted` so the player atlas survives every town / dungeon transition without being re-uploaded per scene.

### Field vs battle dispatch (`SceneLoadKind`)

`SceneResources::build_targeted_with_options(scene, shared, BuildOptions { kind })` lets callers pick the dispatch path the build mimics:

- `SceneLoadKind::Battle` (legacy default of `build_targeted`): uploads every TIM the scanner finds AND parses every TMD the scanner finds. Includes the leading TMD + type-0x01 TIM chunks inside `scene_tmd_stream` PROT entries (which `FUN_8001FE70` walks during battle init) and every TMD + TIM embedded inside `battle_data` records (which the `FUN_8001E890` chain loads at boot for battle init). Town01 keep ratio: 99.3% under disc-gated test.
- `SceneLoadKind::Field`: skips both sources. Mirrors retail field/town scene-load:
  - `scene_tmd_stream` entries are excluded entirely - their leading TMDs go to the battle character TMD register (`_DAT_8007B864`), never rendered from a field scene, and their type-0x01 TIM chunks upload the same mesh's textures. Retail field saves carry row 479 fb_x=0..256 = zero.
  - `battle_data` records are skipped at parse + upload time - the pack is battle-init resident, not part of field VRAM.

`SceneHost::enter_field_scene` uses `SceneLoadKind::Field` so the engine port matches the retail dispatch boundary. The `BuildOptions::default` remains `Battle` so legacy `build_targeted` calls keep their previous semantics.

### Entries the sweep must never touch (all kinds)

The block sweep walks *entries*, and two classes in a scene's CDNAME block are not assets at all:

- **v12 trigger sidecars** (`Class::SceneV12Table`) - streamed through the conditional `.PCH` trigger channel, never the VRAM/TMD pre-pass. The exception is a sidecar that *hosts* the scene's v12-embedded asset bundle (`rikuroa` / `dolk2`).
- **pochi-filler slots** (`Class::PochiFiller`) - reserved-but-unused slots (`pochipochi...`). The scratch **after** the fill prefix parses as real assets: most scene-block pochi slots carry a stale `256 x 256` 4bpp TIM at fb `(768, 0)` (tpage `0x0C`) and another at `(832, 0)`, left over from the block's battle-side character pages. Uploading them clobbers the scene's own **ground-tile atlas**, which lives on tpage `0x0C` in most field scenes - the ground quads then sample character texels (see [`pochi.md`](../formats/pochi.md) and [`world-map.md`](world-map.md#ground-texturing)). Retail never dispatches a reserved slot; neither does the build.

Both skips apply in every `SceneLoadKind`. Regression: `crates/engine-core/tests/field_ground_texture_pages_disc.rs`.

### Battle-boot pre-load (`build_battle_boot_vram`)

`SceneResources::build_battle_boot_vram(battle_data_scenes)` builds a VRAM blob from the player battle files (`BATTLE_BOOT_BLOCKS = ["edstati3", "battle_data"]` - the two extraction labels covering the retail `battle_data` block, extraction 863..866; non-pack entries in either block fail detection and are skipped). It walks every record's LZS stream, uploads any standard-PSX-TIM textures it finds, and invokes the descriptor-driven CLUT pass via `battle_data_pack::clut_uploads`. The retail engine performs an equivalent pre-pass via `FUN_8001E890` at boot or first-battle entry so battle-init has the character meshes resident before the scene-specific `FUN_8001FE70` walk fires.

Today the CLUT pass is a documented no-op until the `battle_data` post-TMD descriptor at `u32[3..0x20]` is pinned (see [`battle-data-pack.md`](../formats/battle-data-pack.md)). Engines that want the API in place can call `build_battle_boot_vram` to walk the pack and accept any TIM-shaped textures it does carry; once descriptor decoding lands, the same call also surfaces the per-record `(fb_x, fb_y)` CLUT placements without further wiring changes. The returned VRAM is intentionally separate from the scene's field VRAM - battle init merges it with the scene-specific upload pass; field rendering does not.

The legacy [`SceneResources::build`](../../crates/engine-core/src/scene_resources.rs) (no shared blocks, unfiltered upload) is preserved for tests + diagnostic surfaces; engines should prefer `build_targeted` for production scene loads. The asset-viewer's `--vram-extra-dir` flag remains the manual workaround for browsing extracted `tim_scan/` dirs that aren't tied to a CDNAME scene.

To find which PROT entry provides a missing CLUT row, run:

```bash
asset clut-finder <vram_x> <vram_y> --extracted-root extracted
```

This walks `extracted/tim_scan/<entry>/*.tim` and reports every TIM whose CLUT (or image, with `--clut-only` off) covers the requested VRAM cell. The output gives `(entry, tim, kind, fbx, fby, w, h)` rows so the user can pick the matching entry directory and pass it via `--vram-extra-dir`. This is the principled discovery step before adding a viewer overlay.

For scene-level diagnostics that don't need a pre-extracted tree, `legaia-engine clut-trace --scene <name> --disc <bin>` walks every dropping `MissingClut` prim in a CDNAME scene, identifies the unique missing CLUT rows (by `(cba, depth)`), and reports the suppliers found across the whole PROT corpus by **rectangle containment** (PSX TIMs commonly pack 16 distinct 16-entry 4bpp palettes into one 256-wide CLUT block, so a CBA's 16-pixel slot sits inside a wider supplier rect). Pair with `--runtime-vram <bin>` (mednafen save state captured via `mednafen-state vram-dump --out-bin`) to mark which missing rows are populated at runtime (engine loader gap) vs absent everywhere (mesh references unreachable CLUT - likely needs a sub-pack walker port).

### Row-479 NPC CLUTs: scene_tmd_stream type-0x01, not battle_data

The four town01 NPC TMDs at field intersections sample CLUT row y=479 slots x=128..240 (`CBA = 0x77C8..0x77CF`). An earlier hypothesis was that those palettes lived inside the `battle_data` block (the player battle files, extraction 863..866) and would land in VRAM via a boot pre-load - the [byte-match corpus](../formats/battle-data-pack.md#vram-byte-match-corpus) refutes that. The actual source is the matching `scene_tmd_stream` entries in *town01's own CDNAME block*, wrapped in type-0x01 chunk headers that `FUN_8001FE70` dispatches during battle init (see [`npc-palette.md`](../formats/npc-palette.md)). Retail field saves carry row 479 = zero because retail field-mode rendering never has those CLUTs resident either.

`SceneResources::build_targeted_with_options(.. SceneLoadKind::Field)` matches this dispatch boundary: it excludes both the leading TMD and the type-0x01 TIMs from every `scene_tmd_stream` entry, so the field-mode TMD pool drops the battle character meshes that retail wouldn't render either. The 388-prim "MissingClut" measurement that surfaced under the previous battle-mode default disappears - those prims belonged to meshes that simply aren't loaded in field mode.

The [player battle file](../formats/battle-data-pack.md) parser remains the entry point for battle-init: `legaia_asset::battle_data_pack` decompresses every TMD slot's LZS stream and exposes the embedded Legaia TMDs + 32-byte layout header. The post-TMD texture/CLUT pool layout is partially TBD - the descriptor at `u32[3..0x20]` points at specific palette positions but the encoding isn't pinned. `build_battle_boot_vram` wires the API in place so once descriptor decoding lands (see [`battle-data-pack.md`](../formats/battle-data-pack.md#open-questions)), battle scenes pick up the per-slot `(fb_x, fb_y)` CLUTs without further integration work.

### VRAM oracle (engine vs runtime)

`legaia-engine vram-oracle --scene <name> --disc <bin> --runtime-vram <bin>` rebuilds the scene's targeted VRAM and reports per-band overlap counts (top half / texpage primary / texpage CLUTs) against the runtime ground truth. `--diff-png <path>` writes the same colour-coded RGBA8 diff as `info --vram-diff-png` (red = runtime-only / gap, green = engine-only / extras, blue = both non-zero with different content, greyscale = exact match). `--tiles` adds a 64x64 tile-by-tile breakdown so a specific page-X region's coverage can be inspected. The runtime VRAM blob comes from `mednafen-state vram-dump <save> --out-bin`.

## Music / SFX selection (BGM lookup)

Documented under the [field VM](script-vm.md) → "BGM lookup table" section. The short version: the BGM ID is a PROT-relative offset, not a literal table lookup. `FUN_800243F0` resolves `bgm_id < 2000` to the scene-local PROT slot at `_DAT_80084540 + 6 + bgm_id`, and `bgm_id >= 2000` to the global pool at `_DAT_8007BC64 + bgm_id - 2000`. The "BGM table" *is* the [CDNAME.TXT](../formats/cdname.md) per-scene block layout.

## Sound bank loader (`FUN_8001FA88`)

The sound subsystem init / `.dpk` loader. Documented under [sound-driver path-string cluster](../formats/sound-driver.md). Loads the `bse.dat` master bank once at boot, then per-scene `.dpk` files via the path-based opener with `h:\main\bg\domepack\<name>.dpk`. Dev builds bypass the path-builder and load PROT index `0x37A` (`sound_data2`) plus `param_1 + 5` for per-scene variations directly.

## Streaming-asset loader (`FUN_8001FC00`)

Builds paths under the `sound\` prefix; the XA / `.pac` / `STR` consumer. Same dev/retail split as the field loader. Plays alongside `FUN_8001FA88` for "sound-but-not-bank" assets.

## Top-level extraction pipeline

`legaia-extract` (the binary in `crates/extract`) drives the offline preservation pipeline: verify → disc → PROT → categorize → streaming-format extract → TIM → PNG. See [`tooling/extraction.md`](../tooling/extraction.md) for the per-stage CLI invocations.

## See also

**Reference** -
[Asset descriptor](../formats/asset-descriptor.md) ·
[Asset-type dispatch](../formats/asset-type.md) ·
[DATA_FIELD streaming](../formats/data-field.md) ·
[Boot sequence](boot.md)
