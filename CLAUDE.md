# CLAUDE.md

Guidance for Claude Code when working in this repository.

This file is a **map**, not the manual. The technical content lives in `docs/` and the per-crate `README.md`s; this file points at the right page for whatever you're touching, plus the rules of engagement that apply across the whole repo.

## Project mission

Two coordinated tracks under one repo (`-re` = reverse-engineering, in both senses):

1. **Asset preservation + format docs.** Extract every asset on the disc, document every format with Ghidra-traced provenance, build round-trip parsers.
2. **Engine reimplementation.** Clean-room Rust port - render via wgpu, audio via the XA + VAB decoders, optional WASM target. End-user model: ship the engine, user supplies the disc image, engine extracts and runs.

Clean-room from format docs + decompiled-C reference (ScummVM / OpenRCT2 model), not a static recompilation of `SCUS_942.54`. See [`docs/subsystems/engine.md`](docs/subsystems/engine.md) for the clean-room boundaries.

**Sony IP (executable, ROM contents, asset bytes) is NEVER committed.** `extracted/` is gitignored, disc-dependent tests skip when `LEGAIA_DISC_BIN` is unset, no decompressed Sony bytes (text strings, sample data, decompiled-C dumps with literal data) get checked in. CI runs without disc data.

## Repository map

The committed docs are organised topic-first under `docs/` - public-facing technical reference, no progress tracker / session log / status tables. Operational state lives in git log + the agent-only memory directory at `~/.claude/projects/-home-mikunpc-Documents-repos-legend-of-legaia-re/memory/`.

### Top level

- [`README.md`](README.md) - public project overview, build instructions, license.
- [`docs/overview.md`](docs/overview.md) - elevator pitch + how the layers stack from disc to sub-asset.

### Formats - [`docs/formats/`](docs/formats/overview.md)

Per-format byte-level specs with Ghidra-traced provenance. Read the relevant page before writing a parser; don't guess from the data.

| Doc | Covers |
|---|---|
| [`overview.md`](docs/formats/overview.md) | Index page; confidence levels (Confirmed / Inferred / Unknown); format families. |
| **Disc + container layer** | |
| [`disc.md`](docs/formats/disc.md) | PSX Mode2/2352 layout, ISO9660 walk. |
| [`prot.md`](docs/formats/prot.md) | PROT.DAT TOC (`start_lba = toc[p+2]`, `size = toc[p+5] - toc[p+3] + 4`). |
| [`dmy.md`](docs/formats/dmy.md) | DMY.DAT - dev-fixture data, no real game content. |
| [`cdname.md`](docs/formats/cdname.md) | CDNAME.TXT name map (`#define name N` marks block start, names inherit forward). |
| **Compression + dispatch** | |
| [`lzs.md`](docs/formats/lzs.md) | Legaia LZS (4 KB ring buffer initialised to zeros - output magic-check is required). |
| [`asset-type.md`](docs/formats/asset-type.md) | 8-bit type byte → handler dispatch (TIM=0, TMD=2, MES=4, ANM=6, …). |
| [`asset-descriptor.md`](docs/formats/asset-descriptor.md) | Descriptor layout consumed by the asset dispatcher. |
| [`data-field.md`](docs/formats/data-field.md) | DATA_FIELD streaming format. |
| **Pack formats** (three distinct, don't confuse) | |
| [`pack.md`](docs/formats/pack.md) | `asset::pack` inside DATA_FIELD chunks. `u32 count` then `u32 word_offsets[count]`. |
| [`tim-pack.md`](docs/formats/tim-pack.md) | `prot::timpack` for some standalone PROT entries. `byte_offset = word_index*4 + 4`. |
| [`field-pack.md`](docs/formats/field-pack.md) | Magic `0x01059B84`. Legaia-specific TIM/TMD bundle. |
| [`battle-data-pack.md`](docs/formats/battle-data-pack.md) | Custom 16MB container for `battle_data` (PROT 0865) + `edstati3`. Streaming preamble + 12-byte record table + per-record LZS streams that decompress to `[32-byte header + Legaia TMD + texture pool]`. |
| [`npc-palette.md`](docs/formats/npc-palette.md) | Row-479 NPC CLUTs (`fb_x=0..256, fb_y=479`). Plain PSX TIMs in scene PROT entries; engine uploads them via the targeted-upload CLUT pass with merge-zeros semantics so multiple scene-pack TIMs targeting the same row can coexist (full slots 0..14 + partial slots 0..7). |
| [`effect.md`](docs/formats/effect.md) | Magic `0x02018B0C` (efect.dat). 2-pack wrapper: sprite anims + effect scripts. |
| **Sub-assets** | |
| [`tim.md`](docs/formats/tim.md) | PSX TIM. |
| [`tmd.md`](docs/formats/tmd.md) | Legaia TMD variant - magic `0x80000002`, custom primitive grouping (8-byte group header + `count × ilen*4` body), per-mode descriptor table at `DAT_8007326c`. |
| [`vab.md`](docs/formats/vab.md) | VAB sound bank. |
| [`mes.md`](docs/formats/mes.md) | MES dialog containers (Compact + Records variants). |
| [`anm.md`](docs/formats/anm.md) | ANM animation pack (player / field actors). Per-scene player ANM ships inside each scene's first PROT slot as a **type-0x05 ("MOVE")** section (87 bundles across the corpus; Baka Fighter variant at PROT 1203 `other5`). Despite the type-0x05 label, content is a canonical ANM container (`marker_1 = 0x080C` records). Per-record layout: 8-byte header + `b` frames × `(a & 0xFF)` bones × 8 bytes per (bone, frame) + 8-byte zero trailer; `record_size = 16 + 8 * (a & 0xFF) * b` falls out byte-exact across all 296 corpus records. Each 8-byte (bone, frame) entry decodes (via `FUN_8001BE80`) into 3 × signed 12-bit translation values (nibble-packed in bytes 0..4) + 3 × u8 rotation angles (bytes 5/6/7, each `<< 4` for a PSX 12-bit angle); engine composes per-bone Z→Y→X rotations via `FUN_8004638C`/`FUN_8004629C`/`FUN_800461A4`. Frame 0 of an idle clip is the rest-pose assembly transform that places each TMD object at its joint position. Parser `legaia_asset::player_anm`. |
| [`monster-animation.md`](docs/formats/monster-animation.md) | Enemy battle animation: per-object rigid-transform keyframes inside the monster archive (PROT 867). Per-action packed stream at entry `+0x8c` (`[u8 parts][u8 frames][9-byte TRS records]`); action 0 = idle. Decoder `FUN_8004998c`. |
| [`character-mesh.md`](docs/formats/character-mesh.md) | Player-character mesh packs - **two distinct packs, one per form.** **Field form** (PROT 0874 §0, low-poly walk/talk, parser `legaia_asset::character_pack`) and **battle form** (PROT 1204 `other5`, higher-detail, parser `legaia_asset::battle_char_pack`), both installed into `DAT_8007C018`; the Baka Fighter minigame reuses the battle pack. The empirical PROT-1204 provenance, the `DAT_8007C018[0..2]` install callsites (`FUN_800513F0` + `FUN_800542C8`), object-local vertices + ANM frame-0 rest pose, and the 256×256 4bpp atlas layout are detailed on the linked page. |
| [`mdt.md`](docs/formats/mdt.md) | Move table (Tactical Arts). |
| [`move-power.md`](docs/formats/move-power.md) | Battle-action per-move power + behaviour table (26-byte stride, runtime VA `0x801F4F5C`, PROT 0898 file `0x26744`). `param_1 = map[actor+0x1df]` via the 128-byte id→index map at `0x801F4E63`; the whole record decoded — `+0` power (roll modulus, used full/half/quarter), `+0x02` strike-Y offset, `+0x04`/`+0x06` move/phase counters, `+0x08`/`+0x09` homing speed + tracking flag, `+0x0a` impact-effect selector (table `0x801f53d4`), `+0x0b` trail texpage (`0x7700+id`), `+0x0d` sound cue (`FUN_8004fcc8`), `+0x0e` list-mode flag, `+0x12`/`+0x16` on-contact / launch effect-id lists (tables `0x801f6324`/`0x801f6418`). `+0x0c` = unused `C`/`E`/`G` designer tag (no runtime reader). SM reads the record via the held pointer `ctx+0x1014` (`FUN_801dea50` seeds, `FUN_801e09f8` ticks). Move-id space = the spell-table id space. Parser `legaia_asset::move_power`. |
| [`art-data.md`](docs/formats/art-data.md) | Art records: per-character ActionConstants, command sequences, power-byte encoding, Miracle/Super Art trigger tables. PROT entry `0x05C4`. |
| [`spell-table.md`](docs/formats/spell-table.md) | Static `SCUS_942.54` spell table: `DAT_800754C8` stats (`+3`=MP, `+0`=`'c'` capture class) / `DAT_800754D0` name pointers, 12-byte stride. Player Seru-magic block `0x81..=0x8b` pinned (Gimard=`0x81`); mirror at `engine-core::retail_magic`. Named monster attacks at `0x25..`; an enemy's cast resolves via the monster record's global magic-attack ids at `+0x21..=+0x23` (AI picker `FUN_801E9FD4` → actor `+0x1DF` → this table). Parser `legaia_asset::spell_names`. |
| [`item-table.md`](docs/formats/item-table.md) | Static `SCUS_942.54` item-name table `PTR_DAT_8007436C[id*3]` (256 ids, 12-byte stride, `+0`=name pointer). The id space a monster record's `drop_item` indexes; parser `legaia_asset::item_names`. |
| [`steal-table.md`](docs/formats/steal-table.md) | Static `SCUS_942.54` per-monster steal table `DAT_80077828 + monster_id*2` (1-based id, 2-byte stride, `[steal_chance_pct, steal_item_id]` — chance FIRST, item second, the reverse of the record's drop field order). What the Evil God Icon steals; NOT in the PROT 867 record. Pinned from a live player-steal capture + byte-exact vs the full published steal table. Parser `legaia_asset::steal_table`. |
| [`new-game-table.md`](docs/formats/new-game-table.md) | Static `SCUS_942.54` new-game starting-party template at `0x80078C4C` (4 records Vahn/Noa/Gala/Terra; 26-byte stride = `8×u16` stats + 10-byte name). Seeds the `0x80084708 + n*0x414` live records; opening scene = `town01`. Parser `legaia_asset::new_game`. |
| [`encounter.md`](docs/formats/encounter.md) | Encounter record installed at `actor[+0x94]`: `[3 reserved][count: u8][monster_ids: u8[count]]`. Reader at `FUN_801DA51C` body `0x801DA620..0x801DA678`. |
| [`man-relocation.md`](docs/formats/man-relocation.md) | Variable-length editing of a decompressed MAN: scene-transition (`0x3F` door) destinations are partition-2 records reached via the partition-2 record-offset table (runtime-pinned); resizing a destination name fixes the partition tables + `u24_at_28` + intra-record relative-jump deltas + the external descriptor size word. Engine `legaia_asset::man_edit`; powers the door randomizer. |
| [`str-fmv-table.md`](docs/formats/str-fmv-table.md) | In-RAM compact STR FMV file table at `0x801CAE40` (24-byte stride × 6: name + libcd BCD MSF + size). |
| [`scene-bundles.md`](docs/formats/scene-bundles.md) | Scene-asset bundle layout per game mode. |
| [`scene-v12-table.md`](docs/formats/scene-v12-table.md) | Per-scene runtime-fixup header + inline-record table + event-script prescript at offset `0x800` (97 PROT entries). |
| [`world-map-overlay.md`](docs/formats/world-map-overlay.md) | Slot 4 of each kingdom bundle (PROT 0085 / 0244 / 0391, type byte `0x05`). Container confirmed (15 / 16 / 16 sub-bodies, per-body header + 8-byte records, byte-verified vs live RAM at `0x8011A624`), but the record interpretation is **open** - the historical "world-map overlay outlines / coastline wireframe" reading is falsified. Most likely a runtime library of small object-local 3D meshes; consumer in Ghidra not yet pinned. |
| [`pochi.md`](docs/formats/pochi.md) | "Pochi-fill" placeholder slots - reserved-but-unused dev fillers. |
| [`mips-overlay.md`](docs/formats/mips-overlay.md) | Per-PROT MIPS-code-likelihood detection. |
| [`overlay-ptr-table.md`](docs/formats/overlay-ptr-table.md) | Sister of `mips-overlay`. |
| **Auxiliary** | |
| [`sfx-table.md`](docs/formats/sfx-table.md) | Static `SCUS_942.54` sound-effect descriptor table `DAT_8006F198 + id*8` (8-byte stride, 100 entries `0x00..=0x63`; the `< 0x200` runtime check is a bound, not the size). Per cue: `+0` program/VAG, `+1` tone/ADSR-region base (`+i` per voice), `+2` note-level attr, `+3` voice count (low 5 bits) + sustained bit `0x20`, `+4` mixer channel. Read by the per-actor SFX trigger `FUN_800250D4` + the cue-ring drainer `FUN_80016B6C` (programs voices via `FUN_80065034`); ids `>= 0x200` use the runtime bank `_DAT_8007B8D0`. Parser `legaia_asset::sfx_table`; byte-exact vs live save-state RAM; feeds `SfxBank::from_descriptors`. |
| [`sound-driver.md`](docs/formats/sound-driver.md) | `.dpk` / `.spk` / `.MAP` / `.PCH` (sound-driver outputs in `sound_data` blocks). |
| [`dialog-font.md`](docs/formats/dialog-font.md) | Glyph metadata at SCUS `0x80074050`; bitmaps in VRAM. |

### Subsystems - [`docs/subsystems/`](docs/subsystems/)

How the runtime engine works.

| Doc | Covers |
|---|---|
| [`engine.md`](docs/subsystems/engine.md) | Clean-room Rust port architecture and boundaries. |
| [`boot.md`](docs/subsystems/boot.md) | Boot sequence; PROT TOC into `0x801C70F0`. |
| [`asset-loader.md`](docs/subsystems/asset-loader.md) | LBA resolver + sub-asset chain. |
| [`renderer.md`](docs/subsystems/renderer.md) | TMD renderer at `FUN_8002735c` (60 GTE ops). |
| [`audio.md`](docs/subsystems/audio.md) | PsyQ libsnd / libspu stack; SsAPI sequencer; SPU DMA transfer engine. |
| [`script-vm.md`](docs/subsystems/script-vm.md) | Field/event VM at `FUN_801DE840` (overlay-resident, 43 opcodes). |
| [`tile-board.md`](docs/subsystems/tile-board.md) | Tile-board grid mode (puzzle / board minigame), NOT general town locomotion. `width×height` byte cell array (cell `2` = wall) + per-cell tile-actor rendering; installed inline in the field-VM script by op `0x49` (`_DAT_8007b450`); walk SM at `overlay_0897_801ef2b0`. |
| [`field-locomotion.md`](docs/subsystems/field-locomotion.md) | Player free-movement controller `FUN_801d01b0` (field overlay): camera-remapped held pad → direction + facing, per-frame speed, 2-unit stepping with per-axis collision `FUN_801cfe4c` against the per-scene walkability grid at `*(_DAT_1f8003ec)+0x4000` (4 sub-cell wall bits per 128-unit tile). Pinned by runtime write-watchpoint on `player+0x14/0x18`. |
| [`minigame-fishing.md`](docs/subsystems/minigame-fishing.md) | Fishing minigame: `DAT_801d926c` state machine (`FUN_801cf3bc`), tension-gauge `0x801d9168` reel tug-of-war (`FUN_801d4004`), catch scoring into the persistent counter `0x8008444c` (`FUN_801d5298`). |
| [`minigame-slot-machine.md`](docs/subsystems/minigame-slot-machine.md) | Casino slot machine gameplay: reel state machine (`FUN_801cf0d8`), dual RNG (LCG `FUN_801d30cc` + BIOS-rand feature rolls), payout/jackpot eval (`FUN_801d13e8`); cash-out commits the overlay-local balance into coin bank `0x800845A4`. Distinct from the prize exchange. |
| [`minigame-baka-fighter.md`](docs/subsystems/minigame-baka-fighter.md) | Baka Fighter duel minigame: round SM (`FUN_801d3468`), rock-paper-scissors exchange resolver (`FUN_801d3a14`), stat/combo damage, pad-vs-AI move pick; reuses the PROT 1204 battle-form party meshes. |
| [`minigame-dance.md`](docs/subsystems/minigame-dance.md) | Noa dance rhythm minigame: beat-clock state machine (`FUN_801cf470`), timing-window judge (`FUN_801d1960`, accuracy-weighted), step chart at `0x801d509c`, groove gauge `DAT_801d544c` as difficulty/multiplier. |
| [`minigame-muscle-dome.md`](docs/subsystems/minigame-muscle-dome.md) | Muscle Dome card-battle arena: match SM (`FUN_801d0748`, phase byte `ctx+6`), 4-slot hand deal/commit under a point budget into the actor `+0x1df` action queue, resolution via the shared battle-action path. Own overlay, not the hub family. |
| [`actor-vm.md`](docs/subsystems/actor-vm.md) | Actor / sprite VM at `FUN_801D6628` (13 opcodes). |
| [`effect-vm.md`](docs/subsystems/effect-vm.md) | Effect-bundle pool; spawn API. |
| [`move-vm.md`](docs/subsystems/move-vm.md) | Move-table opcode VM at `FUN_80023070` (71 ops, JT `0x80010778`); op `0x2F` escapes to overlay extension. |
| [`motion-vm.md`](docs/subsystems/motion-vm.md) | Per-actor motion VM at `FUN_8003774C` - pursue / patrol / face-target. Used by NPC pathing + camera follow scripts. |
| [`cutscene.md`](docs/subsystems/cutscene.md) | STR game modes 26/27; MDEC decoder algorithm (VLC → IDCT → BT.601 YCbCr→RGBA); XA audio sync; `play-str` loop. |
| [`battle.md`](docs/subsystems/battle.md) | Battle scene loader; actor pointer table. |
| [`battle-action.md`](docs/subsystems/battle-action.md) | Battle action state machine at `FUN_801E295C`. |
| [`battle-formulas.md`](docs/subsystems/battle-formulas.md) | Damage / MP-cost / accuracy / RNG arithmetic kernels. Mirror lives at `engine-vm::battle_formulas`. |
| [`world-map.md`](docs/subsystems/world-map.md) | World map controller (`FUN_801E76D4`); top-view debug toggle; camera scroll globals; dev menu renderer (`FUN_801EAD98`); render pipeline + bulk continent terrain emit mechanism. |
| [`world-overview-viewer.md`](docs/subsystems/world-overview-viewer.md) | The static-site `/world-overview/` WebGL viewer: AABB layout, distance-cue fog pass (per-Z scalar LUT + per-kingdom haze), MAN `0x7F`-sentinel bulk-terrain resolver, ocean tile + 13-frame CLUT animation, camera anchors. |
| [`save-screen.md`](docs/subsystems/save-screen.md) | Save-slot select + write flow (`FUN_801DC6B4`); lives in menu overlay; entry-context pointer table; save-block existence scan at `DAT_80084140`. |

### Tooling - [`docs/tooling/`](docs/tooling/)

| Doc | Covers |
|---|---|
| [`extraction.md`](docs/tooling/extraction.md) | Per-stage CLIs (`disc-extract`, `prot-extract`, `lzs-decode`, `legaia-extract`, …). |
| [`ghidra.md`](docs/tooling/ghidra.md) | Compose-exec invocation, the LUI+ADDIU workaround, full script catalogue. |
| [`overlay-capture.md`](docs/tooling/overlay-capture.md) | Mednafen save-state slicing; one-shot pipeline. |
| [`mednafen-automation.md`](docs/tooling/mednafen-automation.md) | Save-state diff / bisect / scenario manifest; watchpoint-equivalent observation across `.mc{0..9}` snapshots. |
| [`pcsx-redux-automation.md`](docs/tooling/pcsx-redux-automation.md) | Closed-loop Lua probes layered on PCSX-Redux's breakpoint debugger. Save-state load → arm probes → capture N VSyncs → CSV / snapshot. Catalogue + authoring pattern. |
| [`port-catalog.md`](docs/tooling/port-catalog.md) | Per-function status catalog: `dumped` (Ghidra) × `documented` (`docs/`) × `ported` (`// PORT: FUN_<addr>` tag in `crates/`) × `ignored` (PsyQ infra in `scripts/port-catalog-ignore.toml`). BFS-from-roots feature views in `scripts/features.toml`. `// REF:` sibling tag for cross-references. `--dashboard` mode emits a single regenerable open-work page. Drift checker `scripts/check-port-tags.py` (warn-only in pre-commit). |
| [`determinism-replay.md`](docs/tooling/determinism-replay.md) | `j-replay-v1` TOML record/replay format + `legaia-engine record` / `replay` subcommands + disc-free determinism cargo-test. Same input file run twice → bit-identical state-trace bytes; pad transitions captured from `play-window` keyboard handler. |
| [`randomizer.md`](docs/tooling/randomizer.md) | Disc patcher for a user-supplied `.bin` (monster drops; encounters/treasure as they land). Built on three new capabilities: `legaia_lzs::compress` (LZS *encoder*; greedy LZSS, `decompress(compress(x))==x`), `legaia_iso::write` (Mode 2/2352 EDC/ECC re-encode + `patch_file_logical`), and `legaia_rando::disc::DiscPatcher` (PROT-entry → LBA same-size in-place edit). No Sony bytes committed; disc-gated tests. |
| [`doc-density.md`](docs/tooling/doc-density.md) | `scripts/check-doc-density.py` legibility linter: flags >800-char lines and >150-word markdown table cells across `docs/` + crate READMEs. Exits non-zero on violations; wired warn-only into pre-commit (mirrors `check-port-tags.py`). |

### Reference - [`docs/reference/`](docs/reference/)

| Doc | Covers |
|---|---|
| [`functions.md`](docs/reference/functions.md) | Notable Ghidra-traced function entry points (the canonical directory). |
| [`memory-map.md`](docs/reference/memory-map.md) | RAM map + key globals. |
| [`builds.md`](docs/reference/builds.md) | TCRF region data; known builds. |
| [`cheats.md`](docs/reference/cheats.md) | GameShark / Mednafen cheat database parser + classifier; pinned RAM offsets for character record, inventory, battle actor, story flags. |
| [`gamedata.md`](docs/reference/gamedata.md) | Curated arts/magic/items/weapons/armor/accessories/enemies/shops/casino/fishing tables mined from public walkthroughs. Ground-truth labels for binary records under reverse engineering. |
| [`open-rev-eng-threads.md`](docs/reference/open-rev-eng-threads.md) | Index of still-open RE hunts + falsified hypotheses worth not re-walking. Question-level companion to `port-catalog.py --dashboard`. |

### Crates - [`crates/`](crates/)

Each crate has a one-page `README.md` describing its scope, format coverage, and how it composes into the pipeline. Crate naming: package `legaia-foo`, lib `legaia_foo`. Internal deps go through workspace path entries (`legaia-asset = { path = "../asset" }`).

**Track 1 - preservation (asset → PNG / WAV / OBJ / JSON)**

| Crate | Binary | Scope |
|---|---|---|
| [`crates/iso`](crates/iso/README.md) | `disc-extract` | PSX Mode2/2352 disc reader, ISO9660 walker, **sector write-back** (`write` module: EDC/ECC re-encode + `patch_file_logical`; `iso9660::find_file_in_image`). |
| [`crates/prot`](crates/prot/README.md) | `prot-extract` | PROT.DAT / DMY.DAT TOC, CDNAME map, standalone TIM-pack. |
| [`crates/lzs`](crates/lzs/README.md) | `lzs-decode` | Legaia LZS decoder (reversed from `FUN_8001a55c`) + `compress` re-packer (greedy LZSS the retail decoder accepts; for editing assets). |
| [`crates/asset`](crates/asset/README.md) | `asset` | Dispatcher, DATA_FIELD streaming, pack format, scene-bundle + effect-bundle + multi-bank-VAB detectors; `categorize` module classifies every PROT entry by format class (disc-gated `categorize_coverage` test asserts ≥99% of corpus bytes are covered). `field_disasm` is the side-effect-free field-VM bytecode disassembler (width/format decoder + `LinearWalker`; `legaia-engine-vm` re-exports it for the executing VM). |
| [`crates/tmd`](crates/tmd/README.md) | `tmd` | Legaia TMD parser + primitive walker + OBJ-with-faces export. |
| [`crates/tim`](crates/tim/README.md) | `tim` | PSX TIM parser + PNG exporter. |
| [`crates/xa`](crates/xa/README.md) | `xa` | XA-ADPCM decoder + WAV exporter. |
| [`crates/vab`](crates/vab/README.md) | `vab` | VAB sound bank extractor + SPU-ADPCM decoder. |
| [`crates/seq`](crates/seq/README.md) | `seq` | PsyQ SEQ parser + CLI inspector. |
| [`crates/mdt`](crates/mdt/README.md) | `mdt` | Move table (Tactical Arts) parser. |
| [`crates/art`](crates/art/README.md) | `art` | Tactical Arts data: ActionConstants, per-character art tables, Miracle/Super Art trigger matchers, art-record parser, SCUS arts-name table decoder (`arts_table` - name + AP + command directions). |
| [`crates/mes`](crates/mes/README.md) | `mes` | MES dialog container parser (Compact + Records). |
| [`crates/anm`](crates/anm/README.md) | `anm` | ANM animation container parser. |
| [`crates/save`](crates/save/README.md) | `save-tool` | Per-character record schema (typed accessors + round-trip parse/write for the 0x414-byte record) plus PSX memory-card walker; `Party::from_retail_sc_block` lifts a real SC block into a typed [`Party`]; `SaveExt` / `SaveFile` (`LGSF v2`) for full engine save round-trips (party + story flags + money + inventory + per-character ext + saved chains). |
| [`crates/font`](crates/font/README.md) | `font-extract` | Proportional dialog font: extracts width table + 4bpp atlas from `SCUS_942.54` + a mednafen save state, exposes layout API for engine consumers. |
| [`crates/extract`](crates/extract/README.md) | `legaia-extract` | Top-level pipeline driver: disc → PROT → categorize → streaming sub-asset extract → PNG. |
| [`crates/mdec`](crates/mdec/README.md) | `mdec` | PSX MDEC clean-room decoder — Legaia movies are the **Iki** bitstream (LZSS-compressed per-block qscale/DC table + AC-only entropy stream, 16-bit-LE MSB-first, column-major macroblocks), not STRv2. Frame → RGBA8: PSX AC VLC table, 8-point IDCT, YCbCr→RGB; `StrFrameAssembler` for multi-sector STR video frames. |
| [`crates/mednafen`](crates/mednafen/README.md) | `mednafen-state` | Mednafen save-state parser (`MDFNSVST` gzip + targeted-scan section indexer) + watchpoint-equivalent automation toolkit: pairwise main-RAM diff with PSX-virtual-address regions, sequence bisection for write-transition detection, declarative scenario manifest at [`scripts/scenarios.toml`](scripts/scenarios.toml). `gpu` module + `vram-dump` subcommand decode the GPU section's 1 MiB VRAM blob as a 1024x512 PNG plus optional raw BGR555 bin, useful as a ground-truth oracle for engine-side VRAM state. `spu` module exposes `PsxSpu` over the SPU section: 24 per-voice snapshots (start_addr, loop_addr, pitch, ADSR phase, sweep volumes), key-on/-off masks, reverb mode, 512 KiB SPU RAM — the retail side of the audio-trace parity oracle in `engine-shell`. |
| [`crates/gamedata`](crates/gamedata/README.md) | `gamedata-tool` | Curated game-data tables (arts with command sequences + AP costs, magic, items, weapons, armor, accessories, enemies with drop / steal table, shops, casino, fishing) mined from public walkthroughs. Cross-validates against `legaia-art::tables`. Acts as ground-truth labels for the binary records being reverse-engineered. See [`docs/reference/gamedata.md`](docs/reference/gamedata.md). |
| [`crates/cheats`](crates/cheats/README.md) | `cheat-tool` | Parser + classifier for third-party GameShark / Pro-Action-Replay cheat databases (GameShark text dump + Mednafen `.cht`). Classifies codes by the RAM region they target; the pinned offsets (character record, inventory, battle actor, story flags) ground-truth the binary records. See [`docs/reference/cheats.md`](docs/reference/cheats.md). |
| [`crates/rando`](crates/rando/README.md) | `legaia-rando` | Randomizer / disc patcher for a user-supplied `.bin`. Same-size in-place PROT-entry + named-file edits (`disc::DiscPatcher` -> `legaia_iso::write`), variable-length MAN relocation for doors, version-stable `rng::SplitMix64`, and PPF 3.0 output. Feature modules cover monster drops, equipment-as-drops, encounters, chests, steals, arts button-combos, scene + house doors, town shops, casino exchange, starting items (+ Door-of-Wind / all-warps), item prices, unused content, and Seru-Bell name injection - orchestrated through `apply`. Ships only code; no Sony bytes. Full per-feature provenance + the CLI flag reference live on the linked page and in [`docs/tooling/randomizer.md`](docs/tooling/randomizer.md). |

**Track 2 - engine reimplementation (clean-room Rust)**

| Crate | Binary | Scope |
|---|---|---|
| [`crates/engine-core`](crates/engine-core/README.md) | - | World state, scene host, scene resources (runtime VRAM pre-pass with `build_targeted` + `FIELD_SHARED_BLOCKS` = `init_data` + `player_data` keeping player TMD resident across field transitions), dialog panel (+ option-picker menus and the opt-in `inline_dialogue` runner — `World::step_inline_dialogue` ports the dialog SM `FUN_80039B7C` to drive an inline interaction script through the real field VM so branch handlers execute), mode/menu/world dispatch, BGM director, **camera controller**, **menu runtime + disk save/load**, `save_full`/`load_full` (LGSF v2 round-trip: party + story flags + money + inventory + per-char ext + saved chains), **shop/inn/level-up/tactical-arts session state**, **`apply_battle_loot`** (formation → XP + gold + level-ups), `input::Mapping` (TOML key-binding persistence), `DefaultMapIdResolver`, `EffectCatalog`, `MemoryVfs` (WASM in-memory Vfs), **`WorldMapController`** (camera scroll/azimuth/zoom + top-view debug toggle, `SceneMode::WorldMap`). |
| [`crates/engine-render`](crates/engine-render/README.md) | - | winit 0.30 + wgpu 26; software PSX VRAM (1024×512 R16Uint, per-prim CBA/TSB + CLUT decode in fragment shader); text overlay via the `legaia-font` atlas. |
| [`crates/engine-audio`](crates/engine-audio/README.md) | - | cpal-backed audio mixer + clean-room SPU + SsAPI-shape SEQ sequencer; BGM cross-fade + volume ramp; `audio-webaudio` feature adds `WebAudioOut` (`ScriptProcessorNode`-based) for WASM targets. |
| [`crates/engine-vm`](crates/engine-vm/README.md) | - | Actor / field / effect / move / **motion** VMs + battle-action SM + 16-arm action validator + `battle_formulas` (damage / MP / accuracy / RNG) + **world-map entity SM** (`FUN_801DA51C`, 5-state encounter/interact port). Re-exports the field-VM disassembler from `legaia-asset` (`field_disasm`). |
| [`crates/engine-shell`](crates/engine-shell/) | `legaia-engine` | Top-level driver + `BootSession` + `AudioBgmDirector`. Boots a CDNAME scene straight from `PROT.DAT`. `info` / `list-scenes` for inspection; `play` ticks the engine for N frames; `play-window` opens a 960×720 wgpu window with keyboard input and renders shop + inn overlay via `shop_draws_for` (cost prompt + Yes/No cursor) and level-up banner via `level_up_draws_for` (the `--live-loop` flag drives the in-`tick` Field↔Battle round trip; `--player-battle` makes battles player-driven and draws the battle HP + command-menu HUD; an overworld stage battle renders the full scene — PROT 88 dome backdrop (front + 180° mirror) + flat grass grid + sky + the real assembled PROT 1204 party (Vahn/Noa/Gala, posed by the PROT 1203 battle-ANM frame-0 rest pose + decoded per-char palettes) + animated monsters, under the exact-RE'd orbit camera (live-confirmed by `scripts/pcsx-redux/autorun_battle_render_capture.lua`); `--vm-dialogue` routes field dialogue through the inline-script field-VM runner so branch handlers execute, with navigable option menus); `save` / `load` exercise the runtime save flow; `play-str` decodes a raw PSX STR file (MDEC video) into a windowed player; `config set --binding` edits `input::Mapping`. Parity oracles: `vram-oracle` (byte-exact VRAM diff against a runtime save), `mode-trace` (per-frame `(scene_mode, active_scene)` diff), and `audio-trace` (per-frame voice-activity diff against the save state's SPU section). |
| [`crates/asset-viewer`](crates/asset-viewer/README.md) | `asset-viewer` | Combined viewer: TIM, TMD, VAB, SEQ, stage geometry, PROT browser, scene-bundle presets, dialog box, field-VM scene runner with dialog rendering, battle-scene SM driver. |
| [`crates/web-viewer`](crates/web-viewer/README.md) | - | WASM target. Disc browser + TIM thumbnails + software TMD rasteriser running in the browser, plus per-entry MES/SEQ/VAB inspector via `current_entry_info_json`. `rom_patcher` runs the `legaia-rando` randomizer client-side (`patch_rom` → patched-image bytes + summary) for the in-browser ROM-patcher page; nothing is uploaded. |

### Ghidra-side scripts - [`ghidra/scripts/`](ghidra/scripts/)

Jython analysis scripts that run inside the `blacktop/ghidra:latest` container. The script catalogue lives in [`docs/tooling/ghidra.md`](docs/tooling/ghidra.md#script-catalogue). Per-function decompiled-C dumps land in `ghidra/scripts/funcs/<addr>.txt` (gitignored - they're Sony-derived).

## Common commands

```bash
cargo build --release                                    # all binaries → target/release/
cargo fmt --all -- --check                               # CI gate
cargo clippy --all-targets --workspace -- -D warnings    # CI gate (warnings = failure)
cargo test --workspace --release                         # CI runs --release
cargo test -p legaia-asset                               # single-crate
cargo test --workspace test_name                         # single test by name
```

Top-level pipeline (recommended for end-to-end runs):

```bash
./target/release/legaia-extract "/path/to/Legend of Legaia (USA).bin" --out extracted
```

`--skip-png` / `--skip-verify` skip the slow steps. See [`docs/tooling/extraction.md`](docs/tooling/extraction.md) for per-stage invocations.

### Disc-gated tests

Several integration tests touch a real disc and only run when `LEGAIA_DISC_BIN` points at a valid `.bin`:

- `crates/iso/tests/disc_pipeline.rs` - disc walk, file count, key file SHA-256s.
- `crates/iso/tests/ecc_real.rs` - the Mode 2/2352 EDC/ECC encoder reproduces real PROT.DAT sectors' parity bit-for-bit; a one-byte patch + restore round-trips a real sector exactly.
- `crates/asset/tests/lzs_compress_roundtrip_real.rs` - `legaia_lzs::compress` round-trips + compresses real monster records (PROT 867) and LZS-container sections.
- `crates/rando/tests/disc_patch_real.rs` - patch a real monster's drop onto a scratch copy of the disc; it re-decodes off the patched image (drop applied, neighbours untouched, sectors EDC/ECC-valid) through disc → ISO → PROT → LZS.
- `crates/rando/tests/rando_cli_real.rs` - full-archive drop shuffle from a seed, applied to a scratch copy: every monster reads its planned drop (slots too full to re-pack stay unchanged), the diff serializes to a PPF that reproduces the patched image, and a fixed seed is byte-deterministic.
- `crates/rando/tests/encounter_patch_real.rs` - whole-disc encounter shuffle on a scratch copy: re-decodes every patched scene MAN off the disc and asserts formation counts + monster-id multiset preserved, ids stay in the scene's pool, sectors EDC/ECC-valid, deterministic for a fixed seed; plus `scripted_formations_survive_the_shuffle` asserts every non-random (scripted/boss) formation is byte-identical after a shuffle — >10 across the corpus, including the Tetsu fight (id `0x4F`).
- `crates/rando/tests/chest_patch_real.rs` - whole-disc chest shuffle on a scratch copy: re-decodes every patched scene MAN and asserts the field-VM `0x39` give-item site offsets are unchanged, the chest-item multiset is preserved, sectors stay valid, and a fixed seed is deterministic; plus a targeted keikoku-chest patch asserting the give operand AND every `0xC2` announcement item-name token both carry the new id; plus a keep-static assertion that every chest holding a curated quest/key item keeps that exact item at its site and no static item migrates.
- `crates/rando/tests/steal_patch_real.rs` - whole-disc steal shuffle on a scratch copy: re-reads the patched `SCUS_942.54` steal table (`DAT_80077828`) and asserts the steal-item multiset is preserved, every monster's steal chance byte is untouched (item-only edit), the touched SCUS sector stays EDC/ECC-valid, and a fixed seed is byte-deterministic.
- `crates/rando/tests/arts_patch_real.rs` - arts-combo shuffle + random on a scratch copy: re-decodes the patched combos and asserts every art keeps its input count, each character's combos stay unique, the Miracle Arts are untouched, a shuffle preserves the global per-length set of distinct combos, sectors stay EDC/ECC-valid, and a fixed seed is byte-deterministic; **plus the MATCHER GUARD** - decompresses each character's player-file `record0` (Vahn 0861/Noa 0864/Gala 0865) and asserts every art's display combo is present as a matcher record and the records actually changed (the display/trigger desync that caught both earlier attempts).
- `crates/rando/tests/door_enumerate_real.rs` - whole-disc scene-transition census: 160 `0x3F` door sites across 48 scenes, every destination a clean CDNAME label, the pinned town01 → map01 exit present, the overworld hubs fanning out.
- `crates/rando/tests/door_patch_real.rs` - whole-disc door shuffle (one-way + coupled) on a scratch copy: re-decodes every patched scene MAN off the patched image and asserts the destination multiset preserved (clean shuffle) / names valid (with skips), every touched sector EDC/ECC-valid, image size unchanged, and a fixed seed deterministic.
- `crates/rando/tests/house_door_patch_real.rs` - whole-disc intra-town (house) door shuffle on a scratch copy: re-decodes every patched scene MAN and asserts the per-scene `0x23 MOVE_TO` target-tile multiset is preserved (so every target stays a valid scene tile), sectors EDC/ECC-valid, image size unchanged, and a fixed seed deterministic.
- `crates/rando/tests/starting_items_patch_real.rs` - starting-item randomize on a scratch copy: re-decodes the rewritten `FUN_80034A6C` seed off the patched `SCUS_942.54`, asserts the seeded items match the plan, every id is an in-pool consumable, the surrounding function bytes are untouched, the image size is unchanged, the touched SCUS sector stays EDC/ECC-valid, and a fixed seed is byte-deterministic; plus a Door-of-Wind + all-warps case asserting the warp preset lands in its OWN region (`WARP_SEED_VA=0x80034adc`) without reducing the item count (5 items + warps coexist), the inventory region carries no warp stores, and the live `$v0` instruction flanking the warp region is byte-identical (so `DAT_80073ef8` isn't clobbered).
- `crates/rando/tests/equipment_drops_real.rs` - equipment-as-drops on a scratch copy: builds the equipment pool from the real `SCUS_942.54` (gamedata-name ↔ item-table-id match), asserts the pool classifies weapons/armor/accessories (and excludes the stray in-range consumable), plans an every-monster equipment drop, re-decodes every monster's drop off the patched `battle_data`, and asserts each is a pool equipment id at a tiered 1..=3% chance; a fixed seed is byte-deterministic.
- `crates/rando/tests/item_price_real.rs` - shop item-price edits + sellable-pool filtering: the 13 chest-found equipment items ship at price 0 and get the reviewed values (idempotent), the sellable pool (item price >0) includes them and excludes known quest/key ids, and a shop `Random` pass only ever stocks priced (non-quest) items.
- `crates/rando/tests/shop_patch_real.rs` - town-shop + casino randomize on a scratch copy: enumerates every town shop (asserts the Rim Elm Variety Store + its 10 known ids, every shop name printable + every id named), a town-shop shuffle preserves the global shop-item multiset + per-shop counts/names and is byte-deterministic, and a casino shuffle preserves the (item, coin-price) prize multiset + per-block counts and is byte-deterministic.
- `crates/engine-core/tests/chest_randomizer_runtime_e2e.rs` - patches keikoku's Phoenix chest in memory, re-decodes the MAN off the patched image, and drives that chest's inline interaction script through the real field VM to assert the runtime grants the patched id (not the original) - a deterministic runtime oracle that sidesteps the savestate RAM-cache trap.
- `crates/engine-core/tests/monster_drop_randomizer_runtime_e2e.rs` - the drop-randomizer counterpart: patches one monster's `+0x48` drop item in memory, re-decodes the record off the patched `battle_data` archive, builds the engine `MonsterCatalog`, and drives a one-monster formation through `apply_battle_loot` (the victory-spoils path) to assert the runtime grants the patched drop (not the original). RNG seeded so the drop roll lands; same savestate-cache-trap rationale as the chest oracle.
- `crates/engine-core/tests/encounter_randomizer_runtime_e2e.rs` - the encounter-randomizer counterpart: patches one scene formation's slot-0 monster id in memory, re-decodes the scene MAN off the patched image, builds the engine encounter table + per-row formation defs from those bytes (`scene_encounter_from_man`), forces that row into a battle through the live-loop encounter path (`FUN_801DA51C` select-by-index → `enter_battle_from_formation`), and asserts the spawned enemy actor's `battle_monster_id` is the patched id (not the original). Same savestate-cache-trap rationale as the chest + drop oracles.
- `crates/engine-core/tests/steal_randomizer_runtime_e2e.rs` - the steal-randomizer counterpart: patches one monster's steal item byte in `SCUS_942.54` (the static `DAT_80077828` table) in memory, re-decodes the steal table off the patched image, and drives the engine steal-grant kernel (`World::apply_steal`, the steal sibling of `apply_battle_loot`) to assert the runtime steals the patched id (not the original). RNG seeded so the steal roll lands; the steal chance is left untouched; same savestate-cache-trap rationale (the steal table is static rodata resident in RAM the moment the executable loads).
- `crates/engine-core/tests/arts_randomizer_runtime_e2e.rs` - the arts-combo-randomizer counterpart: shuffles the arts combos in memory (in-place glyph-byte edits), re-decodes them off the patched `SCUS_942.54` image, and drives the real combo-recognition kernel (`battle_arts::chain_matches_record`) to assert every changed art fires on the NEW combo bytes and no longer on the old one. A baseline over the unpatched combos keeps it non-vacuous; same savestate-cache-trap rationale (the arts table is static rodata resident in RAM the moment the executable loads).
- `crates/engine-core/tests/door_randomizer_runtime_e2e.rs` - the door-randomizer counterpart: patches Rim Elm's (town01) single exit — the `0x3F` named-scene-change at MAN op `0x6f95`, originally → map01 — to a differently-named scene (exercising the variable-length resize) in memory, re-decodes the patched MAN off the patched image, and drives the patched op through the real field VM (`World::load_field_script` + `tick`) to assert the runtime warps to the patched destination (not the original). A baseline pass over the unpatched exit keeps it non-vacuous; same savestate-cache-trap rationale (the scene MAN is resident in RAM the moment you're in the scene).
- `crates/engine-core/tests/starting_items_randomizer_runtime_e2e.rs` - the starting-item-randomizer counterpart: confirms a New Game off the unpatched disc seeds Healing Leaf ×5 (baseline), randomizes the seed on a scratch copy, re-decodes it off the patched image (`StartingInventory::from_scus`), seeds a fresh world via `World::seed_starting_inventory`, and asserts the bag holds exactly the patched items (never the vanilla Healing Leaf ×5). A second case covers the `--door-of-wind` toggle: a New Game off the patched disc seeds Door of Wind into the bag alongside the preserved Healing Leaf base. Same savestate-cache-trap rationale (the seed is executable code resident in RAM the moment the executable loads). (The all-warps preset has no clean-room engine consumer yet, so it stays validated at the disc-round-trip level.)
- `crates/engine-core/tests/shop_randomizer_runtime_e2e.rs` - the shop + casino counterpart: patches one town-shop slot's item id (in the scene MAN, op `0x49`) and one casino prize id (PROT 899 table) on scratch copies, re-decodes the patched stock off the patched image, builds a `ShopSession` and drives `World::buy_from_shop` (the buy-grant kernel shared with the menu runtime's `ShopConfirm` commit) to assert the runtime sells/grants the patched id (not the original). Baseline passes over the unpatched stock keep both non-vacuous; same savestate-cache-trap rationale (the shop record is resident in RAM the moment the shop opens).
- `crates/engine-core/tests/equipment_drops_runtime_e2e.rs` - the equipment-drops counterpart: builds the equipment pool from `SCUS_942.54`, runs `randomize_equipment_drops` on a scratch copy, re-decodes a planned monster's record off the patched `battle_data`, and drives `apply_battle_loot` (RNG seeded so the tiered 1..=3% roll lands) to assert the runtime grants the planned **equipment** id from the pool (baseline grants the original consumable).
- `crates/rando/tests/unused_content_real.rs` - pins the unused-content facts the toggles rely on: Evil Bat ids 176/177/178 are byte-identical clones of the in-use id 140 and "Comm" (id 78) is a populated standalone record (not a clone); item `0x6B` is named ("Something Good") vs `0xFD` unnamed (so `--unused-items` widens the pool by exactly one); the `--unused-enemies` toggle injects an unused id into the encounter Random pool only when enabled (deterministic); and the "Seru Bell" injection names only `0xFD` (the other empty-name ids stay blank), same-size, the touched SCUS sectors stay EDC/ECC-valid, idempotent.
- `crates/engine-core/tests/unused_enemy_randomizer_runtime_e2e.rs` - the `--unused-enemies` runtime oracle: runs the toggle path (`SceneEncounters::randomize_with_extra` with `UNUSED_ENEMY_IDS`) until it places an unused id at a formation slot, writes the re-packed MAN to a scratch disc, re-decodes off the patched image, forces that row into a battle through the live-loop encounter path, and asserts the spawned enemy actor's `battle_monster_id` is an unused-enemy id (baseline spawns the vanilla monster). Same savestate-cache-trap rationale as the other encounter oracle.
- `crates/engine-core/tests/unused_item_randomizer_runtime_e2e.rs` - the `--unused-items` runtime oracle: applies the name injection and asserts the engine item-name table resolves `0xFD` to "Seru Bell" (the other empty-name ids stay blank, Something Good `0x6B` is named) - the display side - then patches a monster's drop to `0xFD`, re-decodes off the patched `battle_data`, drives `apply_battle_loot`, and asserts the bag receives the unused accessory - the grant side (baseline grants the original drop).
- `crates/extract/tests/validation_suite.rs` - full pipeline, PROT entry count, sub-asset totals, TIM round-trip.
- `crates/engine-core/tests/scene_chain_e2e.rs` - every CDNAME scene's MES + SEQ + TMD assets resolve through `SceneHost`; validates `bgm_seq_bytes` slices through the chunk-header wrapper for `scene_vab_stream` entries.
- `crates/engine-core/tests/battle_real_data_chain.rs` - locate the retail effect bundle and drive the battle SM against it.
- `crates/engine-audio/tests/real_bgm_chain.rs` - pull a real `music_01` SEQ + VAB pair through the sequencer and SPU mixer.
- `crates/engine-shell/tests/audio_trace.rs` - audio-trace parity oracle: for every scenario with both `expected_active_scene` and an on-disk `.mc{slot}` save, build the engine's per-frame voice-activity trace and diff against the retail SPU snapshot lifted via `legaia_mednafen::PsxSpu`. Convergence rule: at least one engine frame is a superset of retail's active-voice mask.
- `crates/mednafen/tests/real_spu_smoke.rs` - smoke-checks `legaia_mednafen::PsxSpu` against a real save state: all 24 voice records resolve, 512 KiB SPU RAM is exact, every global register (master sweep, voice-on/-off, reverb, SPU control) is present.
- `crates/save/tests/real_card_roundtrip.rs` - walk a real PSX memory card (mednafen `.mcr`) and verify the save-block layout. Looks at `~/.mednafen/sav/`; doesn't gate on `LEGAIA_DISC_BIN`.
- `crates/engine-core/tests/end_to_end_gameplay_loop.rs` - drives the full minimum-viable gameplay loop: boot save → install encounter session → drive battle action SM to MonsterWipe → apply XP / gold / level-up → round-trip through `SaveFile`. Has a synthetic variant that always runs in CI plus a `real_psx_memory_card_save_drives_full_loop` variant that boots the loop from a real Legaia memory-card save block (skips when `~/.mednafen/sav/` has no Legaia card).

```bash
LEGAIA_DISC_BIN="/path/to/Legend of Legaia (USA).bin" cargo test --workspace
```

Without the env var, every disc-gated test **skips and passes** - that's intentional, so CI works without redistributing Sony data. Don't change that gating.

## Conventions

- **Don't redistribute or commit any Sony-owned bytes** (executables, asset data, decompressed output). `extracted/` and `ghidra/projects/` are gitignored. CI runs without disc data.
- **Disc-dependent tests behind the same `LEGAIA_DISC_BIN` skip-pattern.** Tests must pass when the env var is unset.
- **Prefer adding a CLI subcommand to the existing per-crate binary** over a new binary unless the new tool spans crates. The pattern is `clap` derive + an enum of subcommands at the top of each `bin/<name>.rs`.
- **CI is strict.** `cargo clippy --all-targets --workspace -- -D warnings` and `cargo fmt --all -- --check` both before pushing. A pre-commit hook is shipped - run `scripts/install-hooks.sh` once per clone and the same gates run on every `git commit`. Set `LEGAIA_SKIP_PRECOMMIT=1` to bypass in emergencies.

## Cross-cutting facts that catch people out

These bite repeatedly across subsystems. Skim before chasing a "why is X broken / missing" thread.

- **"No static caller in `SCUS_942.54`" ≠ "dead in retail".** Most game logic lives in RAM overlays loaded at `0x801C0000+` (the field/event VM, the dialog renderer, the actor / battle / menu VMs). Treat zero static callers as "needs overlay sweep". Capture pipeline: [`docs/tooling/overlay-capture.md`](docs/tooling/overlay-capture.md).
- **MIPS LUI+ADDIU pairs are not auto-resolved by Ghidra's reference manager.** Direct xref queries return zero hits even when the address is heavily used. Use `ghidra/scripts/find_lui_writers.py` (edit `LO`/`HI` to your target range). Details: [`docs/tooling/ghidra.md`](docs/tooling/ghidra.md).
- **CDNAME labels can mislead.** `vab_01` doesn't contain VAB headers (real banks live in `battle_data` / `level_up`); `move_program_no` doesn't match the consumer-expected layout. Verify with the loader-call constant or the file's magic bytes. Details: [`docs/formats/cdname.md`](docs/formats/cdname.md).
- **LZS "decompresses without error" is not a validity signal.** The 4 KB ring buffer initialises to zeros, so most random inputs decode to plausible-looking output. Always magic-check the *decoded* bytes. Details: [`docs/formats/lzs.md`](docs/formats/lzs.md).
- **Legaia SEQ has a u32 BE version field**, not the u16 BE shape from PsyQ docs. Real game data is `pQES + u32 BE version + u16 BE ppqn + ...`; `legaia_seq::parse_header` accepts both shapes. Meta events preserve running status (a strict-MIDI `running_status = None` on `0xFF` would break the next event). **PSX SEQ meta events carry NO MIDI variable-length `length` field** - the type byte is followed by a *fixed* count: `0xFF 0x51` + 3 tempo bytes (no `0x03` prefix), `0xFF 0x2F` ends the track (no `0x00`). Reading a phantom length byte mis-decodes the tempo: retail tracks ship a 240 BPM placeholder header tempo that the first body `0xFF 0x51` immediately overrides to the real musical tempo, so dropping that override pins playback ~3x too fast. Every retail SEQ has `ppqn = 480`. The engine `Sequencer` clocks timing as an exact integer in SPU samples (no per-tick float / no drift). Details: [`docs/formats/seq.md`](docs/formats/seq.md).
- **SEQ data in `scene_vab_stream` entries lives at non-zero offsets.** Most retail BGM is wrapped: `[u32 chunk_header][VAB][chunk1_header][SEQ]`. Use `SceneAssets::seq_in_stream_entries` and `bgm_seq_offset` to slice past the wrapper. The `scene_chain_e2e` test exercises this end-to-end.
- **Three pack formats coexist.** `asset::pack` (DATA_FIELD chunks), `prot::timpack` (standalone PROT entries), and field-pack / effect-bundle (Legaia-specific magic-prefixed bundles). Don't apply the wrong header math. See the four format pages linked under "Pack formats" above.
- **Legaia TMDs are a custom variant.** Magic `0x80000002`, custom 8-byte group header, per-mode descriptor table at `DAT_8007326c`. Details: [`docs/formats/tmd.md`](docs/formats/tmd.md).
- **Ghidra promotes intra-function labels to fake `FUN_xxxxxxxx` calls.** When you see `iVar = FUN_801xxxxx(); return iVar;` in a giant dispatcher's C decomp, cross-check `grep -n "0x<addr>" overlay_<dump>.txt` - if the address appears as a `j` target inside that same function's disassembly, it's a label, not a call. Each such "label-call" is really `addiu s8, s8, N; j epilogue` (the standard PC-delta exit idiom). Catalogued for FUN_801de840 in [`docs/subsystems/script-vm.md`](docs/subsystems/script-vm.md#intra-function-label-catalogue) - applies to the dispatcher pattern in any large MIPS function, not just the field VM.

## Ghidra container quick reference

`docker-compose.yml` defines a single `ghidra` service (`blacktop/ghidra:latest`):

- `./extracted:/data:ro` - disc-extracted files (read-only into Ghidra).
- `./ghidra/projects:/projects` - Ghidra project DB (gitignored; local only).
- `./ghidra/scripts:/scripts` - analysis scripts (read-write so dumps land back on host).

Workflow: `docker compose up -d ghidra` once, then `docker compose exec ghidra /ghidra/support/analyzeHeadless ...` per query. Don't restart the service per command. Full setup + per-query invocations: [`docs/tooling/ghidra.md`](docs/tooling/ghidra.md).

To add a new function dump, edit the `TARGETS` list in `ghidra/scripts/dump_funcs.py` and run the post-script - output lands in `ghidra/scripts/funcs/<addr>.txt`. Then update [`docs/reference/functions.md`](docs/reference/functions.md) if the entry point is notable.

For overlay-specific dumps use per-overlay scripts (e.g. `dump_shop_overlay.py`, `dump_levelup_overlay.py`, `dump_cutscene_overlay.py`, `dump_str_fmv_overlay.py`) following the `dump_pending_helpers.py` pattern: `in_program()` guard skips addresses not in the current program, and `out_path_for()` prefixes output as `overlay_<label>_<addr>.txt`. Run with `-process overlay_<label>.bin -noanalysis -postScript /scripts/dump_<label>.py`.

Jython 2.7 (Ghidra-bundled) chokes on Unicode in source unless an encoding declaration is added - keep `ghidra/scripts/*.py` ASCII-only.

## Writing rules for committed docs

(also enforced by the auto-memory `feedback_no_rot_counts.md`)

- Present tense. State what the format / function / subsystem **is**, not when it was figured out.
- No session numbers, dates, "ported in session N" markers, before-vs-after counts.
- No rot-prone counts of project state (tests, crates, function-coverage percentages).
- Stable invariants of the disc itself (PROT entry counts, opcode counts) are fine.
- Provenance citations: `see ghidra/scripts/funcs/<addr>.txt` and `FUN_801XXXXXX in PROT entry NNNN_<name>`.
- Operational state (progress, dates, session logs, status tables) lives in git log + agent memory, not in committed docs.
