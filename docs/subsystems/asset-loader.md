# Asset loader

How the runtime stitches per-scene assets together. Each scene-type has its own loader function with the same dev/retail split: dev branch loads via PROT indices, retail branch resolves dev paths through `FUN_8003E6BC` (the path-based opener) into the same indices via [CDNAME.TXT](../formats/cdname.md).

## Battle bundle (`FUN_800520F0`)

The 11-case battle scene loader. Case 6 loads the `befect_data` bundle (PROT 0x369–0x36B). Case 0xE initialises the runtime [effect 2-pack wrapper](../formats/effect.md) via `FUN_801DE914`. Case 0xFF dispatches `0x801F17F8` (the side-band streaming-effect handler that streams `summon.dat` and `readef.dat`).

Two cases call `FUN_8003E104(monster_idx, slot, dst_buf)` to populate slots 7 and 8 with the active battle's monster sound banks — the per-monster body of `h:\mpack\monster.snd`. Each monster has a `(start_lba, end_lba+1)` entry pair in the TOC at `0x801C8980 - 0x10`. See [`subsystems/audio.md`](audio.md) → "Monster sound bank" for the full loader contract.

The asset-viewer's `--bundle battle` mode mirrors this loader's PROT 865–890 set so character meshes have the right CLUT bindings.

## Field / town scene loader (`FUN_8001F7C0` + `FUN_800255B8`)

The town/field scene-init chain. Builds paths under `DATA\FIELD\` and `h:\PROT\FIELD\<scene>\`. Each scene reserves **six file types** in CDNAME's per-scene block, and the loader walks the [scene asset table](../formats/scene-bundles.md) at the leading PROT entry to pull each file in turn.

The on-disc form of the scene asset table is the canonical 7-typed-asset bundle (`07 00 00 00` lead). The descriptor offsets past the first are NOT file-relative — they index into the loader's runtime decompression buffer (e.g. `0031_izumi.BIN` is 96 KB but `desc[2].data_offset = 141 KB` lands inside the *decompressed* working buffer, not the source file).

Per-scene reference resolution is partial — many scene-bundle entries cross-reference each other through indices in the descriptor table that the loader stitches at runtime. The asset chain for any given scene is "load the scene asset table, decode each descriptor, then load each typed sub-asset using the dispatcher" but the precise indexing scheme is still under investigation for non-battle scenes.

## Asset descriptor walker (`FUN_80020224`)

Walks the [asset descriptor format](../formats/asset-descriptor.md) and calls the asset-type dispatcher per descriptor. Its sole runtime caller in retail is the town overlay's `FUN_801D6704` (MAIN_INIT) at `0x801D6B0C` with `a0 = 0`. The result is stored at `0x80087AF8`. So the walker IS exercised by retail gameplay, just not from a static call site inside `SCUS_942.54`.

## CLUT-data scattering

Many character meshes reference CLUT rows that live in **different PROT entries** from their TMD source. The runtime asset chain stitches them together — the loader puts the relevant TIMs into VRAM before the TMD is rendered.

The asset-viewer's `--vram-extra-dir` flag is the workaround until the chain is fully traced for every scene type. Battle is fully traced; field / town / level-up rely on the workaround.

## Music / SFX selection (BGM lookup)

Documented under the [field VM](script-vm.md) → "BGM lookup table" section. The short version: the BGM ID is a PROT-relative offset, not a literal table lookup. `FUN_800243F0` resolves `bgm_id < 2000` to the scene-local PROT slot at `_DAT_80084540 + 6 + bgm_id`, and `bgm_id >= 2000` to the global pool at `_DAT_8007BC64 + bgm_id - 2000`. The "BGM table" *is* the [CDNAME.TXT](../formats/cdname.md) per-scene block layout.

## Sound bank loader (`FUN_8001FA88`)

The sound subsystem init / `.dpk` loader. Documented under [sound-driver path-string cluster](../formats/sound-driver.md). Loads the `bse.dat` master bank once at boot, then per-scene `.dpk` files via the path-based opener with `h:\main\bg\domepack\<name>.dpk`. Dev builds bypass the path-builder and load PROT index `0x37A` (`sound_data2`) plus `param_1 + 5` for per-scene variations directly.

## Streaming-asset loader (`FUN_8001FC00`)

Builds paths under the `sound\` prefix; the XA / `.pac` / `STR` consumer. Same dev/retail split as the field loader. Plays alongside `FUN_8001FA88` for "sound-but-not-bank" assets.

## Top-level extraction pipeline

`legaia-extract` (the binary in `crates/extract`) drives the offline preservation pipeline: verify → disc → PROT → categorize → streaming-format extract → TIM → PNG. See [`tooling/extraction.md`](../tooling/extraction.md) for the per-stage CLI invocations.
