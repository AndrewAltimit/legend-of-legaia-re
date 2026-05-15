# Asset loader

How the runtime stitches per-scene assets together. Each scene-type has its own loader function with the same dev/retail split: dev branch loads via PROT indices, retail branch resolves dev paths through `FUN_8003E6BC` (the path-based opener) into the same indices via [CDNAME.TXT](../formats/cdname.md).

## Battle bundle (`FUN_800520F0`)

The 11-case battle scene loader. Case 6 loads the `befect_data` bundle (PROT 0x369–0x36B). Case 0xE initialises the runtime [effect 2-pack wrapper](../formats/effect.md) via `FUN_801DE914`. Case 0xFF dispatches `0x801F17F8` (the side-band streaming-effect handler that streams `summon.dat` and `readef.dat`).

Two cases call `FUN_8003E104(monster_idx, slot, dst_buf)` to populate slots 7 and 8 with the active battle's monster sound banks - the per-monster body of `h:\mpack\monster.snd`. Each monster has a `(start_lba, end_lba+1)` entry pair in the TOC at `0x801C8980 - 0x10`. See [`subsystems/audio.md`](audio.md) → "Monster sound bank" for the full loader contract.

The asset-viewer's `--bundle battle` mode mirrors this loader's PROT 865–890 set so character meshes have the right CLUT bindings.

### Per-PROT walker (`FUN_8001FE70`)

One battle-scene state (in `FUN_800513F0`, around `0x80051a50`) calls `FUN_8001FA88(scene_base + slot, 0, dst_buf)` to load a per-scene PROT entry into the working buffer, then `FUN_8001FE70(dst_buf)` to walk its chunk list. The walker is the dispatch path for the [`scene_tmd_stream`](../formats/scene-bundles.md) layout - leading TMD body followed by streaming chunks - and is *different* from the standard `FUN_8002541C` streaming walker:

- First chunk: read `chunk0_header = u32 LE` at offset 0. Low 24 bits = TMD body size. Round up to 32-byte alignment, allocate a buffer of that size at `_DAT_8007B864`, copy the TMD body in via `FUN_8003D26C`.
- Loop: advance by `(prev_size & ~3) + 4` to the next chunk header. Read `header`; if `header & 0xFFFFFF == 0`, exit (terminator). Otherwise:
  - If `(header >> 24) == 0x01` -> call `FUN_800198E0(payload_ptr)` (LoadImage).
  - If `(header >> 24) == 0x02` -> exit (explicit terminator).
  - Other types are skipped silently (the loop advances without uploading).
- Returns once the terminator is hit. The `FUN_80026B4C` TMD register call that follows hooks the parsed TMD into the per-scene mesh pointer table at `0x8007C018 + idx*4`.

This is the path that uploads field NPC palettes to VRAM row 479 - they're plain PSX TIMs wrapped in type-0x01 chunks, dispatched only during battle init. See [`docs/formats/npc-palette.md`](../formats/npc-palette.md) for the cross-save corroboration and [`docs/formats/scene-bundles.md`](../formats/scene-bundles.md#streaming-tail---fun_8001fe70-walker) for the type-byte table.

## Field / town scene loader (`FUN_8001F7C0` + `FUN_800255B8`)

The town/field scene-init chain. Builds paths under `DATA\FIELD\` and `h:\PROT\FIELD\<scene>\`. Each scene reserves **six file types** in CDNAME's per-scene block, and the loader walks the [scene asset table](../formats/scene-bundles.md) at the leading PROT entry to pull each file in turn.

The on-disc form of the scene asset table is the canonical 7-typed-asset bundle (`07 00 00 00` lead). The descriptor offsets past the first are NOT file-relative - they index into the loader's runtime decompression buffer (e.g. `0031_izumi.BIN` is 96 KB but `desc[2].data_offset = 141 KB` lands inside the *decompressed* working buffer, not the source file).

Per-scene reference resolution is partial - many scene-bundle entries cross-reference each other through indices in the descriptor table that the loader stitches at runtime. The asset chain for any given scene is "load the scene asset table, decode each descriptor, then load each typed sub-asset using the dispatcher" but the precise indexing scheme is still under investigation for non-battle scenes.

### WARP opcode → scene transition flow (map_id)

The field VM opcode `0x3E` with `op0 >= 100` triggers a scene warp: `map_id = op0 - 100`. This is stored in `_DAT_8007ba34` and the game mode switches to `0xe` (SCENE_TRANSITION), which calls `FUN_80025980`.

`FUN_80025980` loads a **code overlay** at PROT index `map_id + 0x4d` (or `map_id + 0x4f` when `map_id >= 6`). Only 7 distinct warp destinations exist (map_id 0–6), each loading a different scene-type overlay at PROT 0x4D–0x55. These overlays contain scene-specific entry functions at overlay-resident addresses (`0x801CF070`, `0x801CE8A0`, etc.).

The **scene name** (stored at `DAT_80084548`) is pre-set before `FUN_80025980` executes. The overlay entry function reads this buffer and passes it to `FUN_8001F7C0` and `FUN_80020118`. The mechanism that writes the scene name before the WARP fires is in code paths not yet fully traced (likely a pre-transition handler in the field/town overlay).

Key globals:
- `DAT_80084548` - scene name string (max 8 chars; e.g. `"izumi"`, `"town01"`)  
- `DAT_80084540` - current scene's PROT base index (short)  
- `DAT_8007b768` - pending destination PROT index; `0xffff` = none  
- `DAT_8007ba34` - pending warp map_id (0–6); read by `FUN_80025980`  

The `DefaultMapIdResolver` in `engine-core::scene` uses CDNAME blocks in ascending PROT-index order as a positional approximation. The actual retail warp only supports 7 destinations, not the full CDNAME scene list, and the scene name is determined by a pre-WARP state-machine path not yet traced.

## Asset descriptor walker (`FUN_80020224`)

Walks the [asset descriptor format](../formats/asset-descriptor.md) and calls the asset-type dispatcher per descriptor. Its sole runtime caller in retail is the town overlay's `FUN_801D6704` (MAIN_INIT) at `0x801D6B0C` with `a0 = 0`. The result is stored at `0x80087AF8`. So the walker IS exercised by retail gameplay, just not from a static call site inside `SCUS_942.54`.

## CLUT-data scattering

Many character meshes reference CLUT rows that live in **different PROT entries** from their TMD source. The runtime asset chain stitches them together - the loader puts the relevant TIMs into VRAM before the TMD is rendered.

Engines that drive a clean-room scene loop call [`SceneResources::build_targeted`](../../crates/engine-core/src/scene_resources.rs) once per scene transition. The builder:

1. Parses every TMD in the scene's CDNAME block (plus the optional shared blocks - see below).
2. Collects the union of all prim-target rectangles (CLUT rows + texture-page UV bboxes the meshes will sample).
3. Walks every TIM and decides per-block whether to upload it, suppressing the image block when it would land on a CLUT row another mesh references and vice-versa.

This matches what the retail field loader does (DMA only the texture bytes the current scene's meshes need) and avoids the 4bpp-vs-256-wide CLUT collisions that previously dropped 80%+ of textured prims through the prim filter. See [`renderer.md`](renderer.md#engine-side-targeted-upload--shared-blocks) for the engine-side wiring.

### Field-shared CDNAME blocks

`FIELD_SHARED_BLOCKS = ["init_data", "player_data"]` is the set of CDNAME blocks the retail field engine keeps resident in VRAM across scene transitions. `player_data` (PROT 876) is a streaming-format file with three chunks — a VAB header, a 256x256 TIM_LIST atlas (uploaded to VRAM `fb=(768, 0)` with CLUT at `(0, 500)`), and a small SEQ-magic trailer; it carries **no TMDs** (the character-mesh TMDs that populate `DAT_8007C018[0..4]` come from PROT 0874 — see [`world-map-overlay.md` § Disc-side source of `[0..4]`](../formats/world-map-overlay.md#disc-side-source-of-04)). `init_data` (PROT 0) holds shared UI / sprite tiles. `SceneHost::enter_field_scene` passes both blocks to `build_targeted` so the player atlas survives every town / dungeon transition without being re-uploaded per scene.

### Field vs battle dispatch (`SceneLoadKind`)

`SceneResources::build_targeted_with_options(scene, shared, BuildOptions { kind })` lets callers pick the dispatch path the build mimics:

- `SceneLoadKind::Battle` (legacy default of `build_targeted`): uploads every TIM the scanner finds AND parses every TMD the scanner finds. Includes the leading TMD + type-0x01 TIM chunks inside `scene_tmd_stream` PROT entries (which `FUN_8001FE70` walks during battle init) and every TMD + TIM embedded inside `battle_data` records (which the `FUN_8001E890` chain loads at boot for battle init). Town01 keep ratio: 99.3% under disc-gated test.
- `SceneLoadKind::Field`: skips both sources. Mirrors retail field/town scene-load:
  - `scene_tmd_stream` entries are excluded entirely - their leading TMDs go to the battle character TMD register (`_DAT_8007B864`), never rendered from a field scene, and their type-0x01 TIM chunks upload the same mesh's textures. Retail field saves carry row 479 fb_x=0..256 = zero.
  - `battle_data` records are skipped at parse + upload time - the pack is battle-init resident, not part of field VRAM.

`SceneHost::enter_field_scene` uses `SceneLoadKind::Field` so the engine port matches the retail dispatch boundary. The `BuildOptions::default` remains `Battle` so legacy `build_targeted` calls keep their previous semantics.

### Battle-boot pre-load (`build_battle_boot_vram`)

`SceneResources::build_battle_boot_vram(battle_data_scenes)` builds a VRAM blob from the `battle_data` CDNAME block (`BATTLE_BOOT_BLOCKS = ["battle_data"]`). It walks every record's LZS stream, uploads any standard-PSX-TIM textures it finds, and invokes the descriptor-driven CLUT pass via `battle_data_pack::clut_uploads`. The retail engine performs an equivalent pre-pass via `FUN_8001E890` at boot or first-battle entry so battle-init has the character meshes resident before the scene-specific `FUN_8001FE70` walk fires.

Today the CLUT pass is a documented no-op until the `battle_data` post-TMD descriptor at `u32[3..0x20]` is pinned (see [`battle-data-pack.md`](../formats/battle-data-pack.md)). Engines that want the API in place can call `build_battle_boot_vram` to walk the pack and accept any TIM-shaped textures it does carry; once descriptor decoding lands, the same call also surfaces the per-record `(fb_x, fb_y)` CLUT placements without further wiring changes. The returned VRAM is intentionally separate from the scene's field VRAM - battle init merges it with the scene-specific upload pass; field rendering does not.

The legacy [`SceneResources::build`](../../crates/engine-core/src/scene_resources.rs) (no shared blocks, unfiltered upload) is preserved for tests + diagnostic surfaces; engines should prefer `build_targeted` for production scene loads. The asset-viewer's `--vram-extra-dir` flag remains the manual workaround for browsing extracted `tim_scan/` dirs that aren't tied to a CDNAME scene.

To find which PROT entry provides a missing CLUT row, run:

```bash
asset clut-finder <vram_x> <vram_y> --extracted-root extracted
```

This walks `extracted/tim_scan/<entry>/*.tim` and reports every TIM whose CLUT (or image, with `--clut-only` off) covers the requested VRAM cell. The output gives `(entry, tim, kind, fbx, fby, w, h)` rows so the user can pick the matching entry directory and pass it via `--vram-extra-dir`. This is the principled discovery step before adding a viewer overlay.

For scene-level diagnostics that don't need a pre-extracted tree, `legaia-engine clut-trace --scene <name> --disc <bin>` walks every dropping `MissingClut` prim in a CDNAME scene, identifies the unique missing CLUT rows (by `(cba, depth)`), and reports the suppliers found across the whole PROT corpus by **rectangle containment** (PSX TIMs commonly pack 16 distinct 16-entry 4bpp palettes into one 256-wide CLUT block, so a CBA's 16-pixel slot sits inside a wider supplier rect). Pair with `--runtime-vram <bin>` (mednafen save state captured via `mednafen-state vram-dump --out-bin`) to mark which missing rows are populated at runtime (engine loader gap) vs absent everywhere (mesh references unreachable CLUT - likely needs a sub-pack walker port).

### Row-479 NPC CLUTs: scene_tmd_stream type-0x01, not battle_data

The four town01 NPC TMDs at field intersections sample CLUT row y=479 slots x=128..240 (`CBA = 0x77C8..0x77CF`). An earlier hypothesis was that those palettes lived inside `battle_data` (PROT 865..869) and would land in VRAM via a boot pre-load - the [byte-match corpus](../formats/battle-data-pack.md#what-s-not-in-the-pack-the-npc-palettes-at-row-479) refutes that. The actual source is the matching `scene_tmd_stream` entries in *town01's own CDNAME block*, wrapped in type-0x01 chunk headers that `FUN_8001FE70` dispatches during battle init (see [`npc-palette.md`](../formats/npc-palette.md)). Retail field saves carry row 479 = zero because retail field-mode rendering never has those CLUTs resident either.

`SceneResources::build_targeted_with_options(.. SceneLoadKind::Field)` matches this dispatch boundary: it excludes both the leading TMD and the type-0x01 TIMs from every `scene_tmd_stream` entry, so the field-mode TMD pool drops the battle character meshes that retail wouldn't render either. The 388-prim "MissingClut" measurement that surfaced under the previous battle-mode default disappears - those prims belonged to meshes that simply aren't loaded in field mode.

The [`battle_data` pack format](../formats/battle-data-pack.md) parser remains the entry point for battle-init: `legaia_asset::battle_data_pack` decompresses every record's LZS stream and exposes the embedded Legaia TMDs + 32-byte layout header. The post-TMD texture/CLUT pool layout is partially TBD - the descriptor at `u32[3..0x20]` points at specific palette positions but the encoding isn't pinned. `build_battle_boot_vram` wires the API in place so once descriptor decoding lands (see [`battle-data-pack.md`](../formats/battle-data-pack.md#open-layout-questions)), battle scenes pick up the per-record `(fb_x, fb_y)` CLUTs without further integration work.

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
