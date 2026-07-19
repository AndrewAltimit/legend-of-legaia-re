# CLAUDE.md

Guidance for Claude Code when working in this repository.

This file is a **map**, not the manual. The technical content lives in `docs/` and the per-crate `README.md`s; this file points at the right page for whatever you're touching, plus the rules of engagement that apply across the whole repo.

Keep it that way when you edit it. A table row here is a one-line "what this covers" plus the link - if you find yourself writing a spec into a cell, the spec belongs on the linked page instead.

## Project mission

Two coordinated tracks under one repo (`-re` = reverse-engineering, in both senses):

1. **Asset preservation + format docs.** Extract every asset on the disc, document every format with Ghidra-traced provenance, build round-trip parsers.
2. **Engine reimplementation.** Clean-room Rust port - render via wgpu, audio via the XA + VAB decoders, optional WASM target. End-user model: ship the engine, user supplies the disc image, engine extracts and runs.

Clean-room from format docs + decompiled-C reference (ScummVM / OpenRCT2 model), not a static recompilation of `SCUS_942.54`. See [`docs/subsystems/engine.md`](docs/subsystems/engine.md) for the clean-room boundaries.

**"Port" does not mean 1:1.** Retail behaviour is the baseline and the default for game logic and simulation - that is what the parity oracles measure, and it is why the enhancement toggles default off.

On top of that baseline the engine ships a deliberate, opt-in enhancement layer: dynamic lighting (`--dynamic-lighting`, default off and pixel-identical when off), precise free-angle movement (`options::precise_movement`, default off), the debug orbit camera, and VR.

The render path splits in two, and the halves point opposite ways. **Shading defaults to retail**: the game's textured / colour mesh paths draw the TMD's baked colour word through the GTE depth cue and apply no light source at all (the synthetic Lambert lives only in `MESH_SHADER_SRC`, the asset-viewer's bare-geometry preview - a viewer aid, not a claim about retail). **Rasterisation defaults to clean**: `Renderer::set_psx_mode` is opt-in and gates vertex jitter + 15-bit dither only. Affine UVs are not gated - they are unconditional, and they are the faithful behaviour.

Modding (`crates/rando`) and translation are designed, shipped tracks, not side-effects. What the project still is *not*: a rebalance of the game's design, and not a static recompile.

**Sony IP (executable, ROM contents, asset bytes) is NEVER committed.** `extracted/` is gitignored, disc-dependent tests skip when `LEGAIA_DISC_BIN` is unset, no decompressed Sony bytes (text strings, sample data, decompiled-C dumps with literal data) get checked in. CI runs without disc data.

## Repository map

The committed docs are organised topic-first under `docs/` - public-facing technical reference, no progress tracker / session log / status tables. Operational state lives in git log + the agent-only memory directory at `~/.claude/projects/-home-mikunpc-Documents-repos-legend-of-legaia-re/memory/`.

### Top level

- [`README.md`](README.md) - public project overview, build instructions, license.
- [`docs/overview.md`](docs/overview.md) - elevator pitch + how the layers stack from disc to sub-asset.
- [`docs/guides/`](docs/guides/getting-started.md) - task-oriented user guides: getting-started, extracting-assets, playing-and-viewing, modding-and-translation.

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
| [`battle-data-pack.md`](docs/formats/battle-data-pack.md) | Player battle files `data\battle\PLAYER1..4` (extraction 863..866; Vahn/Noa/Gala/Terra): header + LZS `record[0]` + equip-slot descriptor table + per-slot mesh/texture streams. Also the in-battle **pose** source for the assembled meshes - **not** PROT 1203. NB the doc records why the old "16 MB container at 0865" reading was wrong. |
| [`npc-palette.md`](docs/formats/npc-palette.md) | Row-479 NPC CLUTs (`fb_x=0..256, fb_y=479`) - plain PSX TIMs in scene PROT entries. The doc covers the merge-zeros upload semantics that let several scene-pack TIMs share the row. |
| [`effect.md`](docs/formats/effect.md) | Magic `0x02018B0C` bundle + the `efect.dat` runtime 2-pack (extraction 0873): sprite anims + effect scripts. Carries the verified `befect_data` map (`etim`/`etmd`/`vdf`/`efect` = extraction 0870..0873). |
| [`summon-readef.md`](docs/formats/summon-readef.md) | `summon.dat` / `readef.DAT` battle side-band streaming slots (extraction PROT 893 / 894): per-special-attack CLUT rows, 4bpp texture pages, summon-creature actor records, player art-anim "ME" archives. Doc carries the action-id → slot mapping. Parser `legaia_asset::summon_readef`. |
| **Sub-assets** | |
| [`tim.md`](docs/formats/tim.md) | PSX TIM. |
| [`tmd.md`](docs/formats/tmd.md) | Legaia TMD variant - magic `0x80000002`, custom primitive grouping (8-byte group header + `count × ilen*4` body), per-mode descriptor table at `DAT_8007326c`. |
| [`vab.md`](docs/formats/vab.md) | VAB sound bank. |
| [`mes.md`](docs/formats/mes.md) | MES dialog containers (Compact + Records variants). |
| [`anm.md`](docs/formats/anm.md) | ANM animation pack (player / field actors). Two frame-stream families the doc keeps apart: the **party locomotion bundle** (PROT 0874 §1) and the **per-scene NPC/scene-actor bundle** (each scene's first PROT slot). Per-(bone,frame) 8-byte entry. Parser `legaia_asset::player_anm`. |
| [`monster-animation.md`](docs/formats/monster-animation.md) | Enemy battle animation: per-object rigid-transform keyframes inside the monster archive (PROT 867). Per-action packed stream at entry `+0x8c`; the entry's first byte is an action **tag**, not an index - the doc tabulates the tag space. Decoder `FUN_8004998c`. |
| [`character-mesh.md`](docs/formats/character-mesh.md) | Player-character meshes. Field form = PROT 0874 §0 (parser `legaia_asset::character_pack`). Battle form is **assembled per character** from the player battle files' equipment-id sections, not loaded whole (port `legaia_asset::battle_char_assembly`); PROT 1204 is the Baka Fighter / default-equipment sibling pack. Doc has the splice chain + where each rest pose comes from. |
| [`mdt.md`](docs/formats/mdt.md) | Move table (Tactical Arts). |
| [`move-power.md`](docs/formats/move-power.md) | Battle-action per-move power + behaviour table (26-byte stride, runtime VA `0x801F4F5C`, PROT 0898 file `0x26744`), indexed by `map[actor+0x1df]`. Whole record decoded in the doc. Move-id space = the spell-table id space. Parser `legaia_asset::move_power`. |
| [`art-data.md`](docs/formats/art-data.md) | Art records: per-character ActionConstants, command sequences, power-byte encoding, Miracle/Super Art trigger tables. PROT entry `0x05C4`. |
| [`spell-table.md`](docs/formats/spell-table.md) | Static `SCUS_942.54` spell table: `DAT_800754C8` stats / `DAT_800754D0` name pointers, 12-byte stride. Player Seru-magic block `0x81..=0x8b`; mirror at `engine-core::retail_magic`. Doc covers how an enemy's cast resolves into the same id space. Parser `legaia_asset::spell_names`. |
| [`item-table.md`](docs/formats/item-table.md) | Static `SCUS_942.54` item-name table `PTR_DAT_8007436C[id*3]` (256 ids, 12-byte stride, `+0`=name pointer). The id space a monster record's `drop_item` indexes; parser `legaia_asset::item_names`. |
| [`item-effect-table.md`](docs/formats/item-effect-table.md) | Static `SCUS_942.54` item-effect descriptor table `DAT_800752C0` (130 records, 4-byte stride): effect class, tier, usability flags. **Literal restore amounts are not here** - they're overlay-resident. Parser `legaia_asset::item_effect`. |
| [`equipment-table.md`](docs/formats/equipment-table.md) | Static `SCUS_942.54` equipment stat-bonus table `DAT_80074F68` (8-byte stride): per-equip attack/defence/agility bonuses, equip-character mask, slot type, Ra-Seru flag. Parser `legaia_asset::equip_stats`. |
| [`accessory-passive-table.md`](docs/formats/accessory-passive-table.md) | Accessory ("Goods") passive effects over a 64-slot index space → bit `index` in the per-character ability bitfield `char+0xF4`. Name/description/scope table at `0x8007625C`. Quest items alias their purchasable twins. Parser `legaia_asset::accessory_passive`; engine side `engine-core::accessory_passives`. |
| [`steal-table.md`](docs/formats/steal-table.md) | Static `SCUS_942.54` per-monster steal table `DAT_80077828 + monster_id*2` (1-based id, 2-byte stride). **Field order is `[chance, item]`** - the reverse of the drop fields in the monster record, and it is NOT in the PROT 867 record at all. Parser `legaia_asset::steal_table`. |
| [`new-game-table.md`](docs/formats/new-game-table.md) | Static `SCUS_942.54` new-game starting-party template at `0x80078C4C` (4 records, 26-byte stride). Seeds the `0x80084708 + n*0x414` live records; opening scene = `town01`. Parser `legaia_asset::new_game`. |
| [`encounter.md`](docs/formats/encounter.md) | Encounter record installed at `actor[+0x94]`: `[3 reserved][count: u8][monster_ids: u8[count]]`. Reader at `FUN_801DA51C` body `0x801DA620..0x801DA678`. |
| [`man-relocation.md`](docs/formats/man-relocation.md) | Variable-length editing of a decompressed MAN - resizing a record means fixing the partition tables, `u24_at_28`, intra-record jump deltas and the external descriptor size word, all of which the doc enumerates. Engine `legaia_asset::man_edit`; powers the door randomizer and the localization dialog rewriter. |
| [`str-fmv-table.md`](docs/formats/str-fmv-table.md) | FMV dispatch table at `0x801D0A6C` (23 × 32-byte slots; nine retail `fmv_id 0..=8` = every disc movie, `MV3.STR` split by frame range; parser `legaia_asset::fmv_dispatch`). Per-scene trigger assignment is disc-sourced, not in the table - literal `fmv_id` operands in the scene MAN scripts. |
| [`scene-bundles.md`](docs/formats/scene-bundles.md) | Scene-asset bundle layout per game mode. |
| [`scene-v12-table.md`](docs/formats/scene-v12-table.md) | Per-scene runtime-fixup header + inline-record table + event-script prescript at offset `0x800` (97 PROT entries). |
| [`world-map-overlay.md`](docs/formats/world-map-overlay.md) | Slot 4 of each kingdom bundle (PROT 0085 / 0244 / 0391, type byte `0x05`): a runtime library of small object-local 3D meshes. Each 8-byte record is a **GTE vertex** `(i16 x, y, z, attr)`, read in place by the renderer with no transcode; `attr` is not a coordinate and is render-unused. The "coastline wireframe" reading is **falsified** - see the doc before re-opening that thread. |
| [`pochi.md`](docs/formats/pochi.md) | "Pochi-fill" placeholder slots - reserved-but-unused dev fillers. **The bytes behind the `pochipochi...` prefix are not zeros**: they're stale mastering scratch that often parses as a *complete, valid* TIM. Any "scan the block for TIMs" sweep must skip `Class::PochiFiller` or the stale page uploads over the ground-tile atlas and terrain samples character texels. |
| [`mips-overlay.md`](docs/formats/mips-overlay.md) | Per-PROT MIPS-code-likelihood detection. |
| [`overlay-ptr-table.md`](docs/formats/overlay-ptr-table.md) | Sister of `mips-overlay`. |
| **Auxiliary** | |
| [`sfx-table.md`](docs/formats/sfx-table.md) | Static `SCUS_942.54` sound-effect descriptor table `DAT_8006F198 + id*8` (8-byte stride, 100 entries `0x00..=0x63`): program/VAG, ADSR base, voice count, mixer channel. Ids `>= 0x200` come from a runtime bank instead. Parser `legaia_asset::sfx_table`. |
| [`sound-driver.md`](docs/formats/sound-driver.md) | `.dpk` / `.spk` / `.MAP` / `.PCH` (sound-driver outputs in `sound_data` blocks). |
| [`dialog-font.md`](docs/formats/dialog-font.md) | Glyph metadata at SCUS `0x80074050`; bitmaps in VRAM. |

### Subsystems - [`docs/subsystems/`](docs/subsystems/)

How the runtime engine works.

| Doc | Covers |
|---|---|
| [`engine.md`](docs/subsystems/engine.md) | Clean-room Rust port architecture and boundaries. |
| [`boot.md`](docs/subsystems/boot.md) | Boot sequence; PROT TOC into `0x801C70F0`. |
| [`asset-loader.md`](docs/subsystems/asset-loader.md) | LBA resolver + sub-asset chain. |
| [`renderer.md`](docs/subsystems/renderer.md) | TMD renderer at `FUN_8002735c` (60 GTE ops). Scene clip volume: every camera draws the whole scene (`SCENE_FAR`), no distance/frustum culling in the port. |
| [`vr-mode.md`](docs/subsystems/vr-mode.md) | WebXR `immersive-vr` on the static site's 3D pages (world overview / field-scene viewer / play). The flat renderer stays the geometry source - only the framebuffer and the view-projection fork per eye. Doc covers the per-page scale, the retail screen-X mirror the shader depends on, and the two-mode (`VR:`) pages incl. first-person "what Vahn sees". |
| [`audio.md`](docs/subsystems/audio.md) | PsyQ libsnd / libspu stack; SsAPI sequencer; SPU DMA transfer engine. |
| [`script-vm.md`](docs/subsystems/script-vm.md) | Field/event VM at `FUN_801DE840` (overlay-resident, 43 opcodes). |
| [`tile-board.md`](docs/subsystems/tile-board.md) | Tile-board grid mode (puzzle / board minigame), NOT general town locomotion. `width×height` byte cell array (cell `2` = wall) + per-cell tile-actor rendering; installed inline in the field-VM script by op `0x49` (`_DAT_8007b450`); walk SM at `overlay_0897_801ef2b0`. |
| [`field-locomotion.md`](docs/subsystems/field-locomotion.md) | Player free-movement controller `FUN_801d01b0` (field overlay): camera-remapped held pad → direction + facing, per-frame speed, 2-unit stepping with per-axis collision `FUN_801cfe4c` against the per-scene walkability grid at `*(_DAT_1f8003ec)+0x4000` (4 sub-cell wall bits per 128-unit tile). Pinned by runtime write-watchpoint on `player+0x14/0x18`. |
| [`minigame-fishing.md`](docs/subsystems/minigame-fishing.md) | Fishing minigame: state machine (`FUN_801cf3bc`), tension-gauge reel tug-of-war (`FUN_801d4004`), catch scoring into the persistent counter `0x8008444c`. Also the point-exchange prize counter (parser `legaia_asset::fishing_exchange`) + per-venue rod×cast-band species-spawn tables. |
| [`minigame-slot-machine.md`](docs/subsystems/minigame-slot-machine.md) | Casino slot machine: reel state machine (`FUN_801cf0d8`), dual RNG, **five**-payline payout eval (`FUN_801d13e8` - three straight + two diagonal). The machine is a **3D scene** projected through the GTE via the SCUS wrappers, not 2D sprites - reels are textured cylinders, paylines are projected 3D lines. Doc also covers the bonus round's numeral payout and the still-unpinned cabinet emitter. Parser `legaia_asset::minigame_slot_scene`. |
| [`minigame-baka-fighter.md`](docs/subsystems/minigame-baka-fighter.md) | Baka Fighter duel minigame: round SM (`FUN_801d3468`), rock-paper-scissors exchange resolver (`FUN_801d3a14`), stat/combo damage, pad-vs-AI move pick; reuses the PROT 1204 battle-form party meshes. |
| [`minigame-dance.md`](docs/subsystems/minigame-dance.md) | Noa dance rhythm minigame: beat-clock state machine (`FUN_801cf470`), timing-window judge (`FUN_801d1960`, accuracy-weighted), step chart at `0x801d509c`, groove gauge `DAT_801d544c` as difficulty/multiplier. |
| [`minigame-muscle-dome.md`](docs/subsystems/minigame-muscle-dome.md) | Muscle Dome card-battle arena: match SM (`FUN_801d0748`, phase byte `ctx+6`), 4-slot hand deal/commit under a point budget into the actor `+0x1df` action queue, resolution via the shared battle-action path. Own overlay, not the hub family. |
| [`actor-vm.md`](docs/subsystems/actor-vm.md) | Actor / sprite VM at `FUN_801D6628` (13 opcodes). |
| [`effect-vm.md`](docs/subsystems/effect-vm.md) | Effect-bundle pool; spawn API. |
| [`move-vm.md`](docs/subsystems/move-vm.md) | Move-table opcode VM at `FUN_80023070` (71 ops, JT `0x80010778`); op `0x2F` escapes to overlay extension. |
| [`motion-vm.md`](docs/subsystems/motion-vm.md) | The two per-actor motion VMs. `FUN_8003774C` - pursue / patrol / face-target (NPC pathing + camera follow). `FUN_80038158` - scripted motion + story-flag writes into `DAT_80085758`; its bytecode is MAN tail-section 1 (`legaia_asset::man_motion`). |
| [`cutscene.md`](docs/subsystems/cutscene.md) | STR game modes 26/27; MDEC decoder algorithm (VLC → IDCT → BT.601 YCbCr→RGBA); XA audio sync; `play-str` loop. |
| [`battle.md`](docs/subsystems/battle.md) | Battle scene loader; actor pointer table. |
| [`battle-action.md`](docs/subsystems/battle-action.md) | Battle action state machine at `FUN_801E295C`. |
| [`battle-formulas.md`](docs/subsystems/battle-formulas.md) | Damage / MP-cost / accuracy / escape / RNG arithmetic kernels. Mirror lives at `engine-vm::battle_formulas`. |
| [`arts-command-gauge.md`](docs/subsystems/arts-command-gauge.md) | Arts AP gauge + weapon-specialty arm width. Per-command cost at `DAT_801C9360[char][cmd]+0x74` (arm = cmd `0x0C`); the off-class penalty escalates, it is not a flat ×2. **Not a runtime comparison** - the cost is per-(character, weapon) *disc data* copied verbatim into the runtime struct at battle load, which is what makes it a randomizer target. |
| [`world-map.md`](docs/subsystems/world-map.md) | World map controller (`FUN_801E76D4`); top-view debug toggle; camera scroll globals; dev menu renderer (`FUN_801EAD98`); render pipeline + bulk continent terrain emit. Ocean CLUT cycling comes from the kingdom slot-5 CLUT-walk table (`legaia_asset::clut_walk`) - **not** the script-driven CLUT-cell family, which only carries the row-498 park one-shots. |
| [`world-overview-viewer.md`](docs/subsystems/world-overview-viewer.md) | The static-site `/world-overview/` WebGL viewer: AABB layout, distance-cue fog pass (per-Z scalar LUT + per-kingdom haze), MAN `0x7F`-sentinel bulk-terrain resolver, ocean tile + 13-frame CLUT animation, camera anchors. |
| [`save-screen.md`](docs/subsystems/save-screen.md) | Save-slot select + write flow (`FUN_801DC6B4`); lives in menu overlay; entry-context pointer table; save-block existence scan at `DAT_80084140`. |
| [`field-menu.md`](docs/subsystems/field-menu.md) | Pause-menu **window descriptor table** (52 records at menu-overlay VA `0x801E473C` / PROT 0899 file `0x15F24`; parser `legaia_asset::menu_windows`), the per-character status/party panel renderer `FUN_801D33D8`, and the **options screen**. Panels are content-only draws - the frame is caller-drawn. Engine port lives in `engine-ui` (`status_screen_draws_for` + siblings, re-exported by `engine-render`) with window rects disc-parsed at boot. |

### Tooling - [`docs/tooling/`](docs/tooling/)

| Doc | Covers |
|---|---|
| [`extraction.md`](docs/tooling/extraction.md) | Per-stage CLIs (`disc-extract`, `prot-extract`, `lzs-decode`, `legaia-extract`, …). |
| [`ghidra.md`](docs/tooling/ghidra.md) | Compose-exec invocation, the LUI+ADDIU workaround, full script catalogue. |
| [`overlay-capture.md`](docs/tooling/overlay-capture.md) | Mednafen save-state slicing; one-shot pipeline. |
| [`static-overlay-pipeline.md`](docs/tooling/static-overlay-pipeline.md) | Static complement to the dynamic captures: extract each clean-copy runtime overlay from `PROT.DAT` at its statically-recovered base (`asset overlay …`), identity attached from the PROT entry. Solves VA-aliasing identity structurally + reproducible from the disc; does NOT address runtime values. Committed map `crates/asset/data/static-overlays.toml`. |
| [`mednafen-automation.md`](docs/tooling/mednafen-automation.md) | Save-state diff / bisect / scenario manifest; watchpoint-equivalent observation across `.mc{0..9}` snapshots. |
| [`pcsx-redux-automation.md`](docs/tooling/pcsx-redux-automation.md) | Closed-loop Lua probes layered on PCSX-Redux's breakpoint debugger. Save-state load → arm probes → capture N VSyncs → CSV / snapshot. Catalogue + authoring pattern. |
| [`port-catalog.md`](docs/tooling/port-catalog.md) | Per-function status catalog: `dumped` (Ghidra) × `documented` (`docs/`) × `ported` (`// PORT: FUN_<addr>` tag in `crates/`) × `ignored` (PsyQ infra in `scripts/ci/port-catalog-ignore.toml`). BFS-from-roots feature views in `scripts/ci/features.toml`. `// REF:` sibling tag for cross-references. `--dashboard` mode emits a single regenerable open-work page. Drift checker `scripts/ci/check-port-tags.py` (warn-only in pre-commit). |
| [`recomp-differential.md`](docs/tooling/recomp-differential.md) | Frame-tagged differential oracle vs the static recomp: `scripts/recomp/` TCP probe client + trace capture, `legaia-engine sim-trace` engine emitter, `trace_diff.py` per-channel first-divergence report. Traces are Sony-derived - never committed. |
| [`determinism-replay.md`](docs/tooling/determinism-replay.md) | `j-replay-v1` TOML record/replay format + `legaia-engine record` / `replay` subcommands + disc-free determinism cargo-test. Same input file run twice → bit-identical state-trace bytes; pad transitions captured from `play-window` keyboard handler. |
| [`randomizer.md`](docs/tooling/randomizer.md) | Disc patcher for a user-supplied `.bin`. Built on three capabilities: `legaia_lzs::compress` (LZS *encoder*), `legaia_iso::write` (Mode 2/2352 EDC/ECC re-encode), and `legaia_rando::disc::DiscPatcher` (PROT-entry → LBA same-size in-place edit). The full feature + code-injection reference, incl. where injected routines may live, is on this page. No Sony bytes committed; disc-gated tests. |
| [`releases.md`](docs/tooling/releases.md) | Tagged-release binary pipeline: pushing a `v*` tag builds the workspace binaries and attaches per-target archives + `SHA256SUMS` to that tag's GitHub release (no crates.io). The runner is **arm64**, so both non-native targets are cross-compiles - the doc covers the mingw-w64 / `cargo-zigbuild` setup and why the x86_64 GUI bins need a hand-unpacked amd64 ALSA sysroot. Archives carry binaries + licenses only, never game data. |
| [`doc-density.md`](docs/tooling/doc-density.md) | The two doc gates, both hard pre-commit checks on the staged set (bypass with `LEGAIA_SKIP_PRECOMMIT=1`), both scoping `docs/` + crate READMEs + top-level `*.md`. `check-doc-density.py` flags >800-char lines and >150-word table cells. `check-md-links.py` resolves relative links + `#anchors` - a dead fragment silently jumps to the top of the page, so nothing but a checker catches it; when a link and a heading disagree, fix the link. |
| [`translation.md`](docs/tooling/translation.md) | `legaia-rando translate export/init/strip/merge/stats/diff-disc/lift-official/fit-report/import` - community language packs. Disc text → editable YAML (SCUS name tables + the `0x1F`-segment dialog corpus: scene-bundle MANs + raw event-script carriers) → same-size in-place reimport (`translation` module; markup codec is byte-exact, per-character encodability errors; non-Latin scripts need a font patch, out of scope). Exported packs carry game text - gitignored (`/translations/`, `legaia_*.yaml`), never commit. |
| [`pal-localizations.md`](docs/tooling/pal-localizations.md) | Official PAL discs (`SCES_019.44`/`.45`/`.46` = FR/DE/IT). PROT.DAT is 1:1 with USA (1233 entries, same block boundaries). `translate lift-official` re-keys the official text onto USA coordinates; `diff-disc`/`fit-report` are the text-free alignment measurements. Key constraint: USA scene MANs are sector-aligned with **zero** compressed slack, so in-place growth fits only a small share of lines - the doc has the accent/CP437 layout and the fit numbers. |

### Reference - [`docs/reference/`](docs/reference/)

| Doc | Covers |
|---|---|
| [`functions.md`](docs/reference/functions.md) | Notable Ghidra-traced function entry points (the canonical directory). |
| [`memory-map.md`](docs/reference/memory-map.md) | RAM map + key globals. |
| [`builds.md`](docs/reference/builds.md) | TCRF region data; known builds. |
| [`cheats.md`](docs/reference/cheats.md) | GameShark / Mednafen cheat database parser + classifier; pinned RAM offsets for character record, inventory, battle actor, story flags. |
| [`gamedata.md`](docs/reference/gamedata.md) | Curated arts/magic/items/weapons/armor/accessories/enemies/shops/casino/fishing tables mined from public walkthroughs. Ground-truth labels for binary records under reverse engineering. |
| [`music-tracks.md`](docs/reference/music-tracks.md) | Music-track disambiguation: every BGM cue across its four naming spaces (debug sound-test ID + title / in-game context / official OST title / proposed relocalization). Curated reference (Stann0x), structurally joined to the disc - the `music_01` bank (extraction 990..=1071) is the sound-test order, global BGM id `2000+i` = track `i`. Resolver `engine-core::music_labels`. |
| [`open-rev-eng-threads.md`](docs/reference/open-rev-eng-threads.md) | Index of still-open RE hunts + falsified hypotheses worth not re-walking. Question-level companion to `port-catalog.py --dashboard`. |

### Crates - [`crates/`](crates/)

Each crate has a one-page `README.md` describing its scope, format coverage, and how it composes into the pipeline. Crate naming: package `legaia-foo`, lib `legaia_foo`. Internal deps go through workspace path entries (`legaia-asset = { path = "../asset" }`).

**Track 1 - preservation (asset → PNG / WAV / OBJ / JSON)**

| Crate | Binary | Scope |
|---|---|---|
| [`crates/bytes`](crates/bytes/README.md) | - | Checked little-endian byte readers shared by every parser crate. The leaf dependency under the format stack. |
| [`crates/iso`](crates/iso/README.md) | `disc-extract` | PSX Mode2/2352 disc reader, ISO9660 walker, **sector write-back** (`write` module: EDC/ECC re-encode + `patch_file_logical`; `iso9660::find_file_in_image`). |
| [`crates/prot`](crates/prot/README.md) | `prot-extract` | PROT.DAT / DMY.DAT TOC, CDNAME map, standalone TIM-pack. |
| [`crates/lzs`](crates/lzs/README.md) | `lzs-decode` | Legaia LZS decoder (reversed from `FUN_8001a55c`) + `compress` re-packer (greedy LZSS the retail decoder accepts; for editing assets). |
| [`crates/asset`](crates/asset/README.md) | `asset` | The format hub: dispatcher, DATA_FIELD streaming, pack format, scene-bundle / effect-bundle / multi-bank-VAB detectors. `categorize` classifies every PROT entry by format class. `field_disasm` is the side-effect-free field-VM disassembler (`legaia-engine-vm` re-exports it for the executing VM). `inn_costs` locates the scripted gold charges - **retail has no inn cost table**. |
| [`crates/tmd`](crates/tmd/README.md) | `tmd` | Legaia TMD parser + primitive walker + OBJ-with-faces export. |
| [`crates/tim`](crates/tim/README.md) | `tim` | PSX TIM parser + PNG exporter. |
| [`crates/xa`](crates/xa/README.md) | `xa` | XA-ADPCM decoder + WAV exporter. |
| [`crates/vab`](crates/vab/README.md) | `vab` | VAB sound bank extractor + SPU-ADPCM decoder. |
| [`crates/seq`](crates/seq/README.md) | `seq` | PsyQ SEQ parser + CLI inspector. |
| [`crates/mdt`](crates/mdt/README.md) | `mdt` | Move table (Tactical Arts) parser. |
| [`crates/art`](crates/art/README.md) | `art` | Tactical Arts data: ActionConstants, per-character art tables, Miracle/Super Art trigger matchers, art-record parser, the SCUS arts-name table decoder (`arts_table`), and the arts-**voice** cue tables (`arts_voice` - the shout is CD-XA, per-character file + candidate-channel pool). |
| [`crates/mes`](crates/mes/README.md) | `mes` | MES dialog container parser (Compact + Records). |
| [`crates/anm`](crates/anm/README.md) | `anm` | ANM animation container parser. |
| [`crates/save`](crates/save/README.md) | `save-tool` | Per-character record schema (typed accessors + round-trip parse/write for the 0x414-byte record) plus a PSX memory-card walker. `Party::from_retail_sc_block` lifts a real SC block into a typed `Party`; `SaveExt` / `SaveFile` (LGSF) cover full engine save round-trips. |
| [`crates/font`](crates/font/README.md) | `font-extract` | Proportional dialog font: extracts width table + 4bpp atlas from `SCUS_942.54` + a mednafen save state, exposes a layout API for engine consumers. |
| [`crates/extract`](crates/extract/README.md) | `legaia-extract` | Top-level pipeline driver: disc → PROT → categorize → streaming sub-asset extract → PNG. |
| [`crates/mdec`](crates/mdec/README.md) | `mdec` | PSX MDEC clean-room decoder. Legaia movies are the **Iki** bitstream, **not STRv2** - that's the thing to know before debugging a garbled frame. Frame → RGBA8 via the PSX AC VLC table, 8-point IDCT, YCbCr→RGB; `StrFrameAssembler` handles multi-sector STR video frames. |
| [`crates/mednafen`](crates/mednafen/README.md) | `mednafen-state` | Mednafen save-state parser + watchpoint-equivalent automation (pairwise main-RAM diff, write-transition bisection, scenario manifest [`scripts/scenarios.toml`](scripts/scenarios.toml)). `gpu`/`vram-dump` decode the VRAM blob and `spu` exposes `PsxSpu` - the retail sides of the engine's VRAM and audio parity oracles. |
| [`crates/pcsxr`](crates/pcsxr/README.md) | - | PCSX-Redux save-state (`.sstate`) main-RAM reader, exposing `main_ram()` + VA readers + `scene_name()`/`game_mode()`/`player_pos()`. The bridge that feeds the cataloged playthrough anchors (`s1..s5`) into the engine's disc-gated field/opening oracles. |
| [`crates/gamedata`](crates/gamedata/README.md) | `gamedata-tool` | Curated game-data tables (arts, magic, items, weapons, armor, accessories, enemies, shops, casino, fishing, music tracks) mined from public walkthroughs; music table contributed by Stann0x. These are **ground-truth labels** for the binary records under RE, not disc data. See [`docs/reference/gamedata.md`](docs/reference/gamedata.md). |
| [`crates/cheats`](crates/cheats/README.md) | `cheat-tool` | Parser + classifier for third-party GameShark / Pro-Action-Replay cheat databases. Classifies codes by the RAM region they target; the pinned offsets (character record, inventory, battle actor, story flags) ground-truth the binary records. See [`docs/reference/cheats.md`](docs/reference/cheats.md). |
| [`crates/rando`](crates/rando/README.md) | `legaia-rando` | Randomizer / disc patcher for a user-supplied `.bin`: same-size in-place PROT-entry + named-file edits (`disc::DiscPatcher`), variable-length MAN relocation, `rng::SplitMix64`, PPF 3.0 output. Shuffles drops, encounters, chests, steals, arts, doors, shops, casino, starting items/level, prices, equipment and battle tuning. Several features are hand-assembled MIPS code hooks detoured into SCUS/overlay dead space; the [crate README](crates/rando/README.md) carries the two traps that bite there (the R3000 load-delay slot, and **"zero is not dead"**). Full reference: [`docs/tooling/randomizer.md`](docs/tooling/randomizer.md). No Sony bytes. |

**Track 2 - engine reimplementation (clean-room Rust)**

| Crate | Binary | Scope |
|---|---|---|
| [`crates/engine-core`](crates/engine-core/README.md) | - | The simulation half of the engine, renderer-free: world state, scene host + scene resources, dialog panel / option picker / `inline_dialogue` runner, mode-menu-world dispatch, BGM director, camera controller, menu runtime, disk save/load (LGSF), shop / inn / level-up / tactical-arts session state, battle loot grants, and the minigame rules engines (`dance`, `baka_fighter`, `muscle_dome`). Module-by-module map in the [crate README](crates/engine-core/README.md). |
| [`crates/engine-ui`](crates/engine-ui/README.md) | - | Renderer-agnostic UI draw-list builders (`TextDraw`/`SpriteDraw`, `ui_overlay`, `ui_menu/`, `ui_title_save/`). The wgpu-free leaf shared by the native renderer and the browser play page - the crate `web-viewer` can depend on where `engine-render` can't (hard wgpu link). |
| [`crates/engine-render`](crates/engine-render/README.md) | - | winit 0.30 + wgpu 26; software PSX VRAM (1024×512 R16Uint, per-prim CBA/TSB + CLUT decode in fragment shader); text overlay via the `legaia-font` atlas. |
| [`crates/engine-audio`](crates/engine-audio/README.md) | - | cpal-backed audio mixer + clean-room SPU + SsAPI-shape SEQ sequencer; BGM cross-fade + volume ramp; `audio-webaudio` feature adds `WebAudioOut` (`ScriptProcessorNode`-based) for WASM targets. |
| [`crates/engine-vm`](crates/engine-vm/README.md) | `field-disasm` | Actor / field / effect / move / **motion** VMs + battle-action SM + `battle_formulas` (damage / MP / accuracy / RNG / escape) + **world-map entity SM** (`FUN_801DA51C`, 5-state encounter/interact port). Re-exports the field-VM disassembler from `legaia-asset` (`field_disasm`). |
| [`crates/engine-shell`](crates/engine-shell/) | `legaia-engine` | Top-level driver + `BootSession` + `AudioBgmDirector`; boots a CDNAME scene straight from `PROT.DAT`. Subcommands: `info`/`list-scenes`, `play` (tick N frames), `play-window` (the real 960×720 wgpu window - shop/inn/level-up overlays, `--live-loop`, `--player-battle`, `--party`), `save`/`load`, `play-str` (MDEC video player), `config set --binding`. Parity oracles `vram-oracle` / `mode-trace` / `audio-trace`. |
| [`crates/asset-viewer`](crates/asset-viewer/README.md) | `asset-viewer` | Combined viewer: TIM, TMD, VAB, SEQ, stage geometry, PROT browser, scene-bundle presets, dialog box, field-VM scene runner with dialog rendering, battle-scene SM driver. |
| [`crates/web-viewer`](crates/web-viewer/README.md) | - | WASM target: disc browser, TIM thumbnails, a software TMD rasteriser and a per-entry MES/SEQ/VAB inspector, all in the browser. `field_scene` assembles a scene's **full map** via the shared `engine-core::field_env` kernel; `rom_patcher` runs the randomizer client-side (**nothing is uploaded** - the user's disc never leaves the tab, and the summary is spoiler-safe); `scene_export` bakes `.glb` downloads. |

### Ghidra-side scripts - [`ghidra/scripts/`](ghidra/scripts/)

Jython analysis scripts that run inside the `blacktop/ghidra:latest` container. The script catalogue lives in [`docs/tooling/ghidra.md`](docs/tooling/ghidra.md#script-catalogue). Per-function decompiled-C dumps land in `ghidra/scripts/funcs/<addr>.txt` (gitignored - they're Sony-derived).

### Host-side scripts - [`scripts/`](scripts/README.md)

Helper scripts that run on the host (not in the Ghidra container), mapped in [`scripts/README.md`](scripts/README.md): `ci/` (the pre-commit + CI gates and build/install helpers), `ghidra-analysis/` (overlay extraction + MIPS/GTE disassembly), `asset-investigation/` (TIM/TMD/slot-4/scene RE one-offs), plus `pcsx-redux/` + `mednafen/` capture automation. `scripts/scenarios.toml` (the capture-scenario manifest) and `manage-states.py` stay at the top level as operational entry points.

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

- **`crates/rando/tests/*_real.rs`** - disc-round-trip oracles: patch a feature (drops / encounters / chests / steals / arts / doors / shops / starting items / starting level / equipment / item prices / unused content / weapon specialty / monster stats / move power / element affinity / spell costs) onto a scratch copy, re-decode off the patched image, assert the multiset/invariants are preserved + every touched sector stays EDC/ECC-valid + a fixed seed is byte-deterministic.
- **`crates/engine-core/tests/*_randomizer_runtime_e2e.rs`** - runtime oracles: patch the feature in memory, re-decode, then drive the *engine* grant kernel (`apply_battle_loot` / `apply_steal` / `buy_from_shop` / the field VM) to assert the runtime honors the patched value. Sidesteps the savestate RAM-cache trap; each keeps a baseline pass to stay non-vacuous.

Plus non-randomizer chains: `extract/validation_suite` (full pipeline), `engine-core/scene_chain_e2e` (every CDNAME scene's assets resolve), `engine-audio/real_bgm_chain` (SEQ+VAB through the mixer), `engine-shell/audio_trace` + `mednafen/real_spu_smoke` (SPU parity), `save/real_card_roundtrip` + `engine-core/end_to_end_gameplay_loop` (real memory-card saves; key on `~/.mednafen/sav/`, not `LEGAIA_DISC_BIN`).

## Conventions

- **Don't redistribute or commit any Sony-owned bytes** (executables, asset data, decompressed output). `extracted/` and `ghidra/projects/` are gitignored. CI runs without disc data.
- **Disc-dependent tests behind the same `LEGAIA_DISC_BIN` skip-pattern.** Tests must pass when the env var is unset.
- **Prefer adding a CLI subcommand to the existing per-crate binary** over a new binary unless the new tool spans crates. The pattern is `clap` derive + an enum of subcommands at the top of each `bin/<name>.rs`.
- **CI is strict.** `cargo clippy --all-targets --workspace -- -D warnings` and `cargo fmt --all -- --check` both before pushing. A pre-commit hook is shipped - run `scripts/ci/install-hooks.sh` once per clone and the same gates run on every `git commit`. Set `LEGAIA_SKIP_PRECOMMIT=1` to bypass in emergencies.

## Cross-cutting facts that catch people out

These bite repeatedly across subsystems. Skim before chasing a "why is X broken / missing" thread.

- **"No static caller in `SCUS_942.54`" ≠ "dead in retail".** Most game logic lives in RAM overlays loaded at `0x801C0000+` (the field/event VM, the dialog renderer, the actor / battle / menu VMs). Treat zero static callers as "needs overlay sweep". Capture pipeline: [`docs/tooling/overlay-capture.md`](docs/tooling/overlay-capture.md).
- **MIPS LUI+ADDIU pairs are not auto-resolved by Ghidra's reference manager.** Direct xref queries return zero hits even when the address is heavily used. Use `ghidra/scripts/find_lui_writers.py` (edit `LO`/`HI` to your target range). Details: [`docs/tooling/ghidra.md`](docs/tooling/ghidra.md).
- **CDNAME `#define` numbers are raw in-RAM TOC indices, so every extraction filename label is shifted +2.** The named content for `#define name N` lives at extraction entry `N − 2` (`legaia_prot::cdname::block_for_extraction_index`); the historical "CDNAME labels mislead" cases (`vab_01` without VAB headers, `move_program_no` not matching the move-table layout) dissolve under the shift. When attributing an entry, verify with the loader-call constant or magic bytes and say which index space you mean. Details: [`docs/formats/cdname.md`](docs/formats/cdname.md#numbering-space).
- **LZS "decompresses without error" is not a validity signal.** The 4 KB ring buffer initialises to zeros, so most random inputs decode to plausible-looking output. Always magic-check the *decoded* bytes. Details: [`docs/formats/lzs.md`](docs/formats/lzs.md).
- **Legaia SEQ has a u32 BE version field** (not PsyQ's u16) and its meta events carry **NO MIDI variable-length `length` field** - `0xFF 0x51` + 3 tempo bytes (no `0x03`), `0xFF 0x2F` ends track (no `0x00`). Reading a phantom length byte drops the first-body tempo override, pinning playback ~3x fast against the 240 BPM placeholder header. Meta events preserve running status. `ppqn = 480`; engine `Sequencer` clocks in exact integer SPU samples. Details: [`docs/formats/seq.md`](docs/formats/seq.md).
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

- Present tense. State what the format / function / subsystem **is**, not when it was figured out.
- No session numbers, dates, "ported in session N" markers, before-vs-after counts.
- No rot-prone counts of project state (tests, crates, function-coverage percentages).
- Stable invariants of the disc itself (PROT entry counts, opcode counts) are fine.
- Provenance citations: `see ghidra/scripts/funcs/<addr>.txt` and `FUN_801XXXXXX in PROT entry NNNN_<name>`.
- Operational state (progress, dates, session logs, status tables) lives in git log + agent memory, not in committed docs. Don't cite agent-memory filenames from a committed doc either - they aren't public files.
- Keep prose out of table cells. A cell is a one-line "what this covers"; if it needs a paragraph, the paragraph goes in a section on the linked page. `scripts/ci/check-doc-density.py` enforces this (>800-char lines, >150-word cells) across `docs/`, crate READMEs, **and this file** - it is in scope precisely because it decayed furthest while exempt. Passing is a floor, not a target: prose that lands at 799 chars was written for the linter, not a reader.
