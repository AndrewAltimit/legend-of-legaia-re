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
| [`anm.md`](docs/formats/anm.md) | ANM animation pack (player / field actors). Per-scene player ANM ships in each scene's first PROT slot as a type-0x05 ("MOVE") section (canonical ANM container, `marker_1 = 0x080C`; Baka Fighter variant at PROT 1203 `other5`). Per-(bone,frame) 8-byte entry → 3× signed-12-bit translation + 3× u8 rotation; frame 0 of an idle clip is the rest-pose assembly transform. Parser `legaia_asset::player_anm`. |
| [`monster-animation.md`](docs/formats/monster-animation.md) | Enemy battle animation: per-object rigid-transform keyframes inside the monster archive (PROT 867). Per-action packed stream at entry `+0x8c` (`[u8 parts][u8 frames][9-byte TRS records]`); action 0 = idle. Decoder `FUN_8004998c`. |
| [`character-mesh.md`](docs/formats/character-mesh.md) | Player-character mesh packs - two distinct packs, one per form. Field form (PROT 0874 §0, low-poly, parser `legaia_asset::character_pack`) and battle form (PROT 1204 `other5`, higher-detail, parser `legaia_asset::battle_char_pack`), both installed into `DAT_8007C018` (callsites `FUN_800513F0` + `FUN_800542C8`); Baka Fighter reuses the battle pack. Object-local vertices + ANM frame-0 rest pose, 256×256 4bpp atlas. |
| [`mdt.md`](docs/formats/mdt.md) | Move table (Tactical Arts). |
| [`move-power.md`](docs/formats/move-power.md) | Battle-action per-move power + behaviour table (26-byte stride, runtime VA `0x801F4F5C`, PROT 0898 file `0x26744`). Indexed by `map[actor+0x1df]` (128-byte map at `0x801F4E63`); whole record decoded (power roll modulus, strike-Y, homing/tracking, impact-effect + trail texpage + sound cue, on-contact/launch effect lists). Move-id space = the spell-table id space. Parser `legaia_asset::move_power`. |
| [`art-data.md`](docs/formats/art-data.md) | Art records: per-character ActionConstants, command sequences, power-byte encoding, Miracle/Super Art trigger tables. PROT entry `0x05C4`. |
| [`spell-table.md`](docs/formats/spell-table.md) | Static `SCUS_942.54` spell table: `DAT_800754C8` stats (`+3`=MP, `+0`=`'c'` capture class) / `DAT_800754D0` name pointers, 12-byte stride. Player Seru-magic block `0x81..=0x8b` pinned (Gimard=`0x81`); mirror at `engine-core::retail_magic`. Named monster attacks at `0x25..`; an enemy's cast resolves via the monster record's global magic-attack ids at `+0x21..=+0x23` (AI picker `FUN_801E9FD4` → actor `+0x1DF` → this table). Parser `legaia_asset::spell_names`. |
| [`item-table.md`](docs/formats/item-table.md) | Static `SCUS_942.54` item-name table `PTR_DAT_8007436C[id*3]` (256 ids, 12-byte stride, `+0`=name pointer). The id space a monster record's `drop_item` indexes; parser `legaia_asset::item_names`. |
| [`item-effect-table.md`](docs/formats/item-effect-table.md) | Static `SCUS_942.54` item-effect descriptor table `DAT_800752C0` (130 records, 4-byte stride), indexed item id → subtype (item-name `+1`) → `[class, tier, flags, 'A']`. Effect class + tier + all-party/field/battle usability flags (`FUN_8003043c`/`FUN_80030628`); literal restore amounts are overlay-resident, not here. Parser `legaia_asset::item_effect`. |
| [`equipment-table.md`](docs/formats/equipment-table.md) | Static `SCUS_942.54` equipment stat-bonus table `DAT_80074F68` (8-byte stride), indexed equippable item id → property `+1` byte. Per-equip attack/def-up/def-down (byte-exact vs gamedata) + agility/speed bonuses + equip-character mask + slot type + Ra-Seru flag (`FUN_801CF650`). Parser `legaia_asset::equip_stats`. |
| [`steal-table.md`](docs/formats/steal-table.md) | Static `SCUS_942.54` per-monster steal table `DAT_80077828 + monster_id*2` (1-based id, 2-byte stride, `[steal_chance_pct, steal_item_id]` — chance FIRST, item second, the reverse of the record's drop field order). What the Evil God Icon steals; NOT in the PROT 867 record. Pinned from a live player-steal capture + byte-exact vs the full published steal table. Parser `legaia_asset::steal_table`. |
| [`new-game-table.md`](docs/formats/new-game-table.md) | Static `SCUS_942.54` new-game starting-party template at `0x80078C4C` (4 records Vahn/Noa/Gala/Terra; 26-byte stride = `8×u16` stats + 10-byte name). Seeds the `0x80084708 + n*0x414` live records; opening scene = `town01`. Parser `legaia_asset::new_game`. |
| [`encounter.md`](docs/formats/encounter.md) | Encounter record installed at `actor[+0x94]`: `[3 reserved][count: u8][monster_ids: u8[count]]`. Reader at `FUN_801DA51C` body `0x801DA620..0x801DA678`. |
| [`man-relocation.md`](docs/formats/man-relocation.md) | Variable-length editing of a decompressed MAN: scene-transition (`0x3F` door) destinations are partition-2 records reached via the partition-2 record-offset table (runtime-pinned); resizing a destination name fixes the partition tables + `u24_at_28` + intra-record relative-jump deltas + the external descriptor size word. Engine `legaia_asset::man_edit`; powers the door randomizer. |
| [`str-fmv-table.md`](docs/formats/str-fmv-table.md) | In-RAM compact STR FMV file table at `0x801CAE40` (24-byte stride × 6: name + libcd BCD MSF + size). |
| [`scene-bundles.md`](docs/formats/scene-bundles.md) | Scene-asset bundle layout per game mode. |
| [`scene-v12-table.md`](docs/formats/scene-v12-table.md) | Per-scene runtime-fixup header + inline-record table + event-script prescript at offset `0x800` (97 PROT entries). |
| [`world-map-overlay.md`](docs/formats/world-map-overlay.md) | Slot 4 of each kingdom bundle (PROT 0085 / 0244 / 0391, type byte `0x05`). Container confirmed (15 / 16 / 16 sub-bodies, per-body header + 8-byte records, byte-verified vs live RAM at `0x8011A624`), but the record interpretation is **open** - the historical "world-map overlay outlines / coastline wireframe" reading is falsified. A runtime library of small object-local 3D meshes; a Drake warp capture pins the consumer - the world-map renderer (`0x801F78D4`) + SCUS cluster-A GTE prim path read the slot-4 records **in place** (no transcode). The per-record `[x,y,z,attr]` field semantic is the residual. |
| [`pochi.md`](docs/formats/pochi.md) | "Pochi-fill" placeholder slots - reserved-but-unused dev fillers. |
| [`mips-overlay.md`](docs/formats/mips-overlay.md) | Per-PROT MIPS-code-likelihood detection. |
| [`overlay-ptr-table.md`](docs/formats/overlay-ptr-table.md) | Sister of `mips-overlay`. |
| **Auxiliary** | |
| [`sfx-table.md`](docs/formats/sfx-table.md) | Static `SCUS_942.54` sound-effect descriptor table `DAT_8006F198 + id*8` (8-byte stride, 100 entries `0x00..=0x63`). Per cue: program/VAG, ADSR-region base, attr, voice count + sustained bit, mixer channel. Read by `FUN_800250D4` + cue-ring drainer `FUN_80016B6C`; ids `>= 0x200` use runtime bank `_DAT_8007B8D0`. Parser `legaia_asset::sfx_table`; feeds `SfxBank::from_descriptors`. |
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
| [`static-overlay-pipeline.md`](docs/tooling/static-overlay-pipeline.md) | Static complement to the dynamic captures: extract each clean-copy runtime overlay from `PROT.DAT` at its statically-recovered base (`asset overlay …`), identity attached from the PROT entry. Solves VA-aliasing identity structurally + reproducible from the disc; does NOT address runtime values. Committed map `crates/asset/data/static-overlays.toml`. |
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
| [`crates/mednafen`](crates/mednafen/README.md) | `mednafen-state` | Mednafen save-state parser (`MDFNSVST` gzip + section indexer) + watchpoint-equivalent automation: pairwise main-RAM diff, write-transition bisection, scenario manifest [`scripts/scenarios.toml`](scripts/scenarios.toml). `gpu` + `vram-dump` decode the 1 MiB VRAM blob (1024×512 PNG; engine VRAM oracle); `spu` exposes `PsxSpu` (24 voice snapshots, key-on/-off masks, reverb, 512 KiB SPU RAM — retail side of the audio-trace oracle). |
| [`crates/gamedata`](crates/gamedata/README.md) | `gamedata-tool` | Curated game-data tables (arts with command sequences + AP costs, magic, items, weapons, armor, accessories, enemies with drop / steal table, shops, casino, fishing) mined from public walkthroughs. Cross-validates against `legaia-art::tables`. Acts as ground-truth labels for the binary records being reverse-engineered. See [`docs/reference/gamedata.md`](docs/reference/gamedata.md). |
| [`crates/cheats`](crates/cheats/README.md) | `cheat-tool` | Parser + classifier for third-party GameShark / Pro-Action-Replay cheat databases (GameShark text dump + Mednafen `.cht`). Classifies codes by the RAM region they target; the pinned offsets (character record, inventory, battle actor, story flags) ground-truth the binary records. See [`docs/reference/cheats.md`](docs/reference/cheats.md). |
| [`crates/rando`](crates/rando/README.md) | `legaia-rando` | Randomizer / disc patcher for a user-supplied `.bin`. Same-size in-place PROT-entry + named-file edits (`disc::DiscPatcher` → `legaia_iso::write`), variable-length MAN relocation for doors, `rng::SplitMix64`, PPF 3.0 output. Feature modules: monster/equipment drops, encounters, chests, steals, arts combos, scene + house doors, shops, casino, starting items, item prices, unused content - via `apply`. No Sony bytes. Full reference in [`docs/tooling/randomizer.md`](docs/tooling/randomizer.md). |

**Track 2 - engine reimplementation (clean-room Rust)**

| Crate | Binary | Scope |
|---|---|---|
| [`crates/engine-core`](crates/engine-core/README.md) | - | World state, scene host, scene resources (runtime VRAM pre-pass `build_targeted` + `FIELD_SHARED_BLOCKS` keeping player TMD resident across transitions), dialog panel + option-picker + opt-in `inline_dialogue` runner (`step_inline_dialogue` ports dialog SM `FUN_80039B7C` through the real field VM), mode/menu/world dispatch, BGM director, camera controller, menu runtime + disk save/load (`save_full`/`load_full`, LGSF v2), shop/inn/level-up/tactical-arts session state, `apply_battle_loot` (XP + gold + level-ups), `input::Mapping`, `DefaultMapIdResolver`, `EffectCatalog`, `MemoryVfs`, `WorldMapController` (`SceneMode::WorldMap`). |
| [`crates/engine-render`](crates/engine-render/README.md) | - | winit 0.30 + wgpu 26; software PSX VRAM (1024×512 R16Uint, per-prim CBA/TSB + CLUT decode in fragment shader); text overlay via the `legaia-font` atlas. |
| [`crates/engine-audio`](crates/engine-audio/README.md) | - | cpal-backed audio mixer + clean-room SPU + SsAPI-shape SEQ sequencer; BGM cross-fade + volume ramp; `audio-webaudio` feature adds `WebAudioOut` (`ScriptProcessorNode`-based) for WASM targets. |
| [`crates/engine-vm`](crates/engine-vm/README.md) | - | Actor / field / effect / move / **motion** VMs + battle-action SM + 16-arm action validator + `battle_formulas` (damage / MP / accuracy / RNG) + **world-map entity SM** (`FUN_801DA51C`, 5-state encounter/interact port). Re-exports the field-VM disassembler from `legaia-asset` (`field_disasm`). |
| [`crates/engine-shell`](crates/engine-shell/) | `legaia-engine` | Top-level driver + `BootSession` + `AudioBgmDirector`; boots a CDNAME scene straight from `PROT.DAT`. Subcommands: `info`/`list-scenes`; `play` (tick N frames); `play-window` (960×720 wgpu + keyboard, shop/inn/level-up overlays; flags `--live-loop` Field↔Battle, `--player-battle` HUD + full overworld stage battle with the assembled PROT 1204 party under the orbit camera, `--vm-dialogue` inline-script field VM); `save`/`load`; `play-str` (MDEC video player); `config set --binding`. Parity oracles `vram-oracle` / `mode-trace` / `audio-trace`. |
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

Many integration tests touch a real disc and only run when `LEGAIA_DISC_BIN` points at a valid `.bin`:

```bash
LEGAIA_DISC_BIN="/path/to/Legend of Legaia (USA).bin" cargo test --workspace
```

Without the env var, every disc-gated test **skips and passes** - that's intentional, so CI works without redistributing Sony data. Don't change that gating. Find them with `grep -rl LEGAIA_DISC_BIN crates/*/tests`; each is named for what it covers. Two recurring shapes:

- **`crates/rando/tests/*_real.rs`** - disc-round-trip oracles: patch a feature (drops / encounters / chests / steals / arts / doors / shops / starting items / equipment / item prices / unused content) onto a scratch copy, re-decode off the patched image, assert the multiset/invariants are preserved + every touched sector stays EDC/ECC-valid + a fixed seed is byte-deterministic.
- **`crates/engine-core/tests/*_randomizer_runtime_e2e.rs`** - runtime oracles: patch the feature in memory, re-decode, then drive the *engine* grant kernel (`apply_battle_loot` / `apply_steal` / `buy_from_shop` / the field VM) to assert the runtime honors the patched value. Sidesteps the savestate RAM-cache trap; each keeps a baseline pass to stay non-vacuous.

Plus non-randomizer chains: `extract/validation_suite` (full pipeline), `engine-core/scene_chain_e2e` (every CDNAME scene's assets resolve), `engine-audio/real_bgm_chain` (SEQ+VAB through the mixer), `engine-shell/audio_trace` + `mednafen/real_spu_smoke` (SPU parity), `save/real_card_roundtrip` + `engine-core/end_to_end_gameplay_loop` (real memory-card saves; key on `~/.mednafen/sav/`, not `LEGAIA_DISC_BIN`).

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
- **Legaia SEQ has a u32 BE version field** (not PsyQ's u16) and its meta events carry **NO MIDI variable-length `length` field** — `0xFF 0x51` + 3 tempo bytes (no `0x03`), `0xFF 0x2F` ends track (no `0x00`). Reading a phantom length byte drops the first-body tempo override, pinning playback ~3x fast against the 240 BPM placeholder header. Meta events preserve running status. `ppqn = 480`; engine `Sequencer` clocks in exact integer SPU samples. Details: [`docs/formats/seq.md`](docs/formats/seq.md).
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
