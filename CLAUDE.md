# CLAUDE.md

Guidance for Claude Code when working in this repository.

This file is a **map**, not the manual. The technical content lives in `docs/` and the per-crate `README.md`s; this file points at the right page for whatever you're touching, plus the rules of engagement that apply across the whole repo.

Keep it that way when you edit it. A table row here is a one-line "what this covers" plus the link - if you find yourself writing a spec into a cell, the spec belongs on the linked page instead.

## Project mission

Two coordinated tracks under one repo (`-re` = reverse-engineering, in both senses):

1. **Asset preservation + format docs.** Extract every asset on the disc, document every format with Ghidra-traced provenance, build round-trip parsers.
2. **Engine reimplementation.** From-scratch Rust port - render via wgpu, audio via the XA + VAB decoders, optional WASM target. End-user model: ship the engine, user supplies the disc image, engine extracts and runs.

The public framing is **a playable port and modding hub standing on Ghidra-traced reverse engineering** - fresh Rust from format docs + decompiled-C reference (ScummVM / OpenRCT2 model), not a decompilation project and not a static recompilation of `SCUS_942.54`. Don't call it "clean-room" in committed prose (the same people read the dumps and write the Rust); the enforced boundary is in [`docs/subsystems/engine.md`](docs/subsystems/engine.md).

**"Port" does not mean 1:1.** Retail behaviour is the measured ground truth - the traced dumps and parity oracles pin it exactly, and a retail-faithful mode stays available and testable - but the port is not bound by it. New features, mechanics, rendering and audio are in scope; enhancements ship enabled by default where they are clearly the better experience, with retail one toggle away.

The enhancement layer today: enhanced lighting (moods, lamp lights, emissives, halos from `engine-ui::scene_lighting`; default-on on both hosts, `I` / `--no-dynamic-lighting` disables, `F8` cycles time of day), volumetric ground fog (default-on, `F9` / `--no-volumetric-fog` disables), the camera-occlusion fade (see-through walls around the player, default-on on both hosts, `--no-occlusion-fade` / `F4` disables), precise free-angle movement (`options::precise_movement`), the debug orbit camera, and VR - all toggles that leave the faithful mode bit-identical when off. Per-knob current defaults live in [`docs/subsystems/engine.md`](docs/subsystems/engine.md#fidelity-and-enhancements); a knob still defaulting to retail marks an enhanced side that is maturing, not a policy of restraint.

The render path splits in two, and the halves point opposite ways. **Shading defaults to retail** (in the renderer; the play hosts layer enhanced lighting over it): the game's textured / colour mesh paths draw the TMD's baked colour word through the GTE depth cue with no light source - except the light-source rows (TMD group flags `0x10..=0x17`), which the field dispatcher sends to its `NCCS` / `NCCT` handlers and which shade through the GTE light ([`shading.md`](docs/subsystems/shading.md)). **Rasterisation defaults to clean**: `Renderer::set_psx_mode` is opt-in and gates vertex jitter + 15-bit dither only. Affine UVs are not gated - they are unconditional, and they are the faithful behaviour.

The shading half holds on **both** hosts. A synthetic Lambert survives only in two viewer aids (the asset-viewer's bare-geometry `MESH_SHADER_SRC` and the site's lit bestiary preview), neither a claim about retail. The trap: an unbound colour attribute defaults to white, and white is `texel * 255/128`, so a missing colour stream reads as "too bright", not "unlit" ([`renderer.md`](docs/subsystems/renderer.md#lighting)).

Modding (`crates/patcher`) and translation are designed, shipped tracks, not side-effects - and what the patcher proves out against retail (randomizer logic, softlock fixes, tuning sliders) is expected to graduate into engine toggles. What the project still is *not*: a static recompile, and never a build that silently loses the retail-faithful mode.

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
| [`prot.md`](docs/formats/prot.md) | PROT.DAT TOC, 1233 entries (`start_lba = toc[p+2]`, `size_sectors = toc[p+3] - toc[p+2]` - the gap to the next entry). |
| [`dmy.md`](docs/formats/dmy.md) | DMY.DAT - dev-fixture data, no real game content. |
| [`cdname.md`](docs/formats/cdname.md) | CDNAME.TXT name map (`#define name N` marks block start, names inherit forward). |
| **Compression + dispatch** | |
| [`lzs.md`](docs/formats/lzs.md) | Legaia LZS (4 KB ring buffer initialised to zeros - output magic-check is required). |
| [`asset-type.md`](docs/formats/asset-type.md) | 8-bit type byte → handler dispatch (TIM=0, TMD=2, MES=4, ANM=6, …). |
| [`asset-descriptor.md`](docs/formats/asset-descriptor.md) | Descriptor layout consumed by the asset dispatcher. |
| [`data-field.md`](docs/formats/data-field.md) | DATA_FIELD streaming format. |
| **Pack + bundle formats** (the first three are distinct layouts - don't confuse their header math) | |
| [`pack.md`](docs/formats/pack.md) | `asset::pack` inside DATA_FIELD chunks. `u32 count` then `u32 word_offsets[count]`. |
| [`tim-pack.md`](docs/formats/tim-pack.md) | `prot::timpack` - not a container of its own, but the reader for an `asset::pack` sitting behind a `TIM_LIST` chunk header. `byte_offset = word_index*4 + 4`, and the `marker == 0x01` it keys on is that header's type byte. |
| [`field-pack.md`](docs/formats/field-pack.md) | **Not a format.** The "magic" `0x01059B84` is a DATA_FIELD chunk header `(TIM_LIST << 24) \| payload_len` wrapping an `asset::pack` of one scene's TIMs, at raw-TOC `+4` of its CDNAME block; the page records how the format reading arose and what consumes the entry. |
| [`battle-data-pack.md`](docs/formats/battle-data-pack.md) | Player battle files `data\battle\PLAYER1..4` (extraction 863..866): header, LZS `record[0]`, equip-slot descriptor table, per-slot mesh / texture streams. The in-battle **pose** source - not PROT 1203. |
| [`place-names.md`](docs/formats/place-names.md) | The **three** carriers one place name has, each driving one display: the SCUS quick-travel cells, the 29-record world-map label table trailing every kingdom MAN (`DAT_80073EE0`), and each scene MAN's section-2 banner name (`_DAT_801C6EA0`). Parser `legaia_asset::place_names`. |
| [`ringside-still.md`](docs/formats/ringside-still.md) | Headerless 16bpp stills (extraction 1221 / 1222): raw BGR555 uploaded to VRAM `(384, 0)`, the pair picked from the lead character's live HP - the party's ringside cheer / dejected reaction in the contest hub. |
| [`npc-palette.md`](docs/formats/npc-palette.md) | Row-479 NPC CLUTs (`fb_x=0..256, fb_y=479`) - plain PSX TIMs in scene PROT entries. The doc covers the merge-zeros upload semantics that let several scene-pack TIMs share the row. |
| [`effect.md`](docs/formats/effect.md) | Magic `0x02018B0C` bundle + the `efect.dat` runtime 2-pack (extraction 0873): sprite anims + effect scripts. Carries the verified `befect_data` map (`etim`/`etmd`/`vdf`/`efect` = extraction 0870..0873). |
| [`summon-readef.md`](docs/formats/summon-readef.md) | `summon.dat` / `readef.DAT` battle side-band streaming slots (extraction PROT 893 / 894): per-special-attack CLUT rows, 4bpp texture pages, summon-creature actor records, player art-anim "ME" archives. Doc carries the action-id → slot mapping. Parser `legaia_asset::summon_readef`. |
| **Sub-assets** | |
| [`tim.md`](docs/formats/tim.md) | PSX TIM. |
| [`tmd.md`](docs/formats/tmd.md) | Legaia TMD variant - magic `0x80000002`, custom primitive grouping (8-byte group header + `count × ilen*4` body), per-mode descriptor table at `DAT_8007326c`. |
| [`vab.md`](docs/formats/vab.md) | VAB sound bank; carried as **two** chunks of its DATA_FIELD stream (header part, then the VAG bodies). Resolve the body origin off the stream - a parse's own `+4` skew and the upload's re-slice cancel on all but six carriers. |
| [`seq.md`](docs/formats/seq.md) | PsyQ-derived SEQ sequence (`pQES` magic). Legaia's header + meta-event encoding diverge - see the cross-cutting bullet below. Parser `crates/seq`. |
| [`xa.md`](docs/formats/xa.md) | CD-XA Mode 2 Form 2 audio sectors (`XA1.XA`…) carrying cutscene voice / streamed audio: 2352-byte raw layout, 18 sound groups × 128 B per sector. Decoder `crates/xa`. |
| [`mes.md`](docs/formats/mes.md) | MES dialog containers (Compact + Records variants). |
| [`anm.md`](docs/formats/anm.md) | ANM animation pack (player / field actors). Two frame-stream families the doc keeps apart: the **party locomotion bundle** (PROT 0874 §1) and the **per-scene NPC/scene-actor bundle** (each scene's first PROT slot). Per-(bone,frame) 8-byte entry. Parser `legaia_asset::player_anm`. |
| [`monster-animation.md`](docs/formats/monster-animation.md) | Enemy battle animation: per-object rigid-transform keyframes inside the monster archive (PROT 867). Per-action packed stream at entry `+0x8c`; the entry's first byte is an action **tag**, not an index - the doc tabulates the tag space. Decoder `FUN_8004998c`. |
| [`character-mesh.md`](docs/formats/character-mesh.md) | Player-character meshes. Field form = PROT 0874 §0 (`legaia_asset::character_pack`); battle form is **assembled per character** from equipment sections (`legaia_asset::battle_char_assembly`), not loaded whole. |
| [`mdt.md`](docs/formats/mdt.md) | Move table (Tactical Arts). |
| [`save-icon.md`](docs/formats/save-icon.md) | Save-slot portrait sheet in the menu overlay (PROT 899 `0x1F908`): sixteen 16x16 tiles row-interleaved across a 256x16 strip, one 16-colour palette per tile. Tile N = the memory-card icon for save N+1 (`FUN_801E1934`); tile 15 is blank padding nothing selects. Parser `legaia_asset::save_icon`. |
| [`window-script.md`](docs/formats/window-script.md) | The window-script VM's bytecode programs (fixed 4-byte `[opcode][window id][operand]` instructions): a program table resident in the menu overlay's own data segment (PROT 0899), driving the shop / menu window choreography. Parser + `jal`-site scanner `legaia_asset::widget_script`. |
| [`move-power.md`](docs/formats/move-power.md) | Battle-action per-move power + behaviour table (26-byte stride, runtime VA `0x801F4F5C`, PROT 0898 file `0x26744`), indexed by `map[actor+0x1df]`. Whole record decoded in the doc. Move-id space = the spell-table id space. Parser `legaia_asset::move_power`. |
| [`battle-attack-camera-table.md`](docs/formats/battle-attack-camera-table.md) | Per-art attack-camera tracks (`0x801F4E10`, PROT 0898 file `0x265F8`): 20 rows x 2 signed halfwords, addressed `row*4 + phase_cursor*2`. The data `FUN_801D71B8`'s per-art arms fold into the swing framing. Parser `legaia_asset::battle_attack_camera_table`. |
| [`art-data.md`](docs/formats/art-data.md) | Art records: per-character ActionConstants, command sequences, power-byte encoding, Miracle/Super Art trigger tables. PROT entry `0x05C4`. |
| [`spell-table.md`](docs/formats/spell-table.md) | Static `SCUS_942.54` spell table: `DAT_800754C8` stats / `DAT_800754D0` name pointers, 12-byte stride. Player Seru-magic block `0x81..=0x8b`; mirror at `engine-core::retail_magic`. Doc covers how an enemy's cast resolves into the same id space. Parser `legaia_asset::spell_names`. |
| [`item-table.md`](docs/formats/item-table.md) | Static `SCUS_942.54` item-name table `PTR_DAT_8007436C[id*3]` (256 ids, 12-byte stride, `+0`=name pointer). The id space a monster record's `drop_item` indexes; parser `legaia_asset::item_names`. |
| [`item-effect-table.md`](docs/formats/item-effect-table.md) | Static `SCUS_942.54` item-effect descriptor table `DAT_800752C0` (130 records, 4-byte stride): effect class, tier, usability flags. **Literal restore amounts are not here** - they're overlay-resident. Parser `legaia_asset::item_effect`. |
| [`equipment-table.md`](docs/formats/equipment-table.md) | Static `SCUS_942.54` equipment stat-bonus table `DAT_80074F68` (8-byte stride): per-equip attack/defence/agility bonuses, equip-character mask, slot type, Ra-Seru flag. Parser `legaia_asset::equip_stats`. |
| [`accessory-passive-table.md`](docs/formats/accessory-passive-table.md) | Accessory ("Goods") passive effects over a 64-slot index space → bit `index` in the per-character ability bitfield `char+0xF4`. Name/description/scope table at `0x8007625C`. Quest items alias their purchasable twins. Parser `legaia_asset::accessory_passive`; engine side `engine-core::accessory_passives`. |
| [`steal-table.md`](docs/formats/steal-table.md) | Static `SCUS_942.54` per-monster steal table `DAT_80077828 + monster_id*2` (1-based id, 2-byte stride). **Field order is `[chance, item]`** - the reverse of the drop fields in the monster record, and it is NOT in the PROT 867 record at all. Parser `legaia_asset::steal_table`. |
| [`new-game-table.md`](docs/formats/new-game-table.md) | Static `SCUS_942.54` new-game starting-party template at `0x80078C4C` (4 records, 26-byte stride). Seeds the `0x80084708 + n*0x414` live records; opening scene = `town01`. Parser `legaia_asset::new_game`. |
| [`save-record.md`](docs/formats/save-record.md) | The runtime per-character record the new-game template seeds: `0x414` bytes, 4 contiguous at `0x80084708 + slot*0x414`; display name at `+0x2A7`. Typed accessors + round-trip in `legaia_save`. |
| [`encounter.md`](docs/formats/encounter.md) | Encounter record at `actor[+0x94]` (`[3 reserved][count][monster_ids]`, reader `FUN_801DA51C`), the MAN section-3 camera-region table, and the camera's visible-tile window at `0x1F8003E8..EB`. |
| [`man-relocation.md`](docs/formats/man-relocation.md) | Variable-length editing of a decompressed MAN - resizing a record means fixing the partition tables, `u24_at_28`, intra-record jump deltas and the external descriptor size word, all of which the doc enumerates. Engine `legaia_asset::man_edit`; powers the door randomizer and the localization dialog rewriter. |
| [`str-fmv-table.md`](docs/formats/str-fmv-table.md) | FMV dispatch table at `0x801D0A6C` (23 × 32-byte slots; nine retail `fmv_id 0..=8` = every disc movie, `MV3.STR` split by frame range; parser `legaia_asset::fmv_dispatch`). Per-scene trigger assignment is disc-sourced, not in the table - literal `fmv_id` operands in the scene MAN scripts. |
| [`scene-bundles.md`](docs/formats/scene-bundles.md) | Scene-asset bundle layout per game mode. Nothing bounds the descriptor count - anchor on descriptor 0 at `8 + count*8`. |
| [`slot-b-module-layout.md`](docs/formats/slot-b-module-layout.md) | File layout of the 64 slot-B cast / summon images (`0903..=0966`): head table, code, spawn-record band, inherited tail. Parser `legaia_asset::slot_b_module`. |
| [`scene-v12-table.md`](docs/formats/scene-v12-table.md) | Per-scene `.PCH` walk-on trigger sidecar: runtime-fixup header + inline-record table, exactly one `0x800` sector in all 97 cases. The event-script prescript is the **next PROT entry**, not a field at `+0x800` - that reading was the over-reading entry size appending the neighbour. |
| [`field-map.md`](docs/formats/field-map.md) | Per-scene `DATA\FIELD\<scene>.MAP` - the fixed `0x12000`-byte slot 0 of every scene block (101 entries). Four regions summing to the footprint exactly; detected on the trigger block's sub-table chain. Per-field semantics in [`field-locomotion.md`](docs/subsystems/field-locomotion.md). |
| [`world-map-overlay.md`](docs/formats/world-map-overlay.md) | Slot 4 of each kingdom bundle (PROT 0086 / 0245 / 0392): the world-map scene's **ANM animation bank**. The vertex-pool and coastline-wireframe readings are falsified - read the doc before re-opening either. |
| [`pochi.md`](docs/formats/pochi.md) | "Pochi-fill" placeholder slots: one 2048-byte sector of fill each, never a parseable TIM. The "stale scratch TIM" reading is falsified. |
| [`mips-overlay.md`](docs/formats/mips-overlay.md) | Per-PROT MIPS-code-likelihood detection. |
| [`overlay-ptr-table.md`](docs/formats/overlay-ptr-table.md) | Sister of `mips-overlay`. |
| **Auxiliary** | |
| [`sfx-table.md`](docs/formats/sfx-table.md) | Static `SCUS_942.54` sound-effect descriptor table `DAT_8006F198 + id*8` (100 entries); the `+4` category selects the cue's VAB slot, ids `>= 0x200` come from a runtime bank. Parser `legaia_asset::sfx_table`. |
| [`bse-dat.md`](docs/formats/bse-dat.md) | `bse.dat` = the **battle** occupant of the runtime SFX descriptor bank (cue ids `>= 0x200`), loaded per battle by `FUN_800513F0`. Parser `legaia_asset::bse_bank`. |
| [`sound-driver.md`](docs/formats/sound-driver.md) | `.dpk` / `.spk` / `.MAP` / `.PCH` (sound-driver outputs in `sound_data` blocks). |
| [`dialog-font.md`](docs/formats/dialog-font.md) | Proportional dialog font: 256-byte width table at SCUS `0x80073F1C`, 38-entry `0xCE` escape table at `0x80074050`, glyph bitmaps in VRAM at `(896, 0)`. |
| [`navmesh.md`](docs/formats/navmesh.md) | Negative finding: the `0x80108EA4` cluster that differs across area-load saves is **not** a 24-byte-stride navmesh - it's a per-scene GPU-primitive scratch buffer the renderer refills on scene entry. Recorded so the path isn't re-walked. |

### Subsystems - [`docs/subsystems/`](docs/subsystems/)

How the runtime engine works.

| Doc | Covers |
|---|---|
| [`engine.md`](docs/subsystems/engine.md) | The port as it is: what works, crate layering, the hosts on one engine, the fidelity-vs-enhancement model with current defaults, the legal boundary. |
| [`boot.md`](docs/subsystems/boot.md) | Boot sequence; PROT TOC into `0x801C70F0`. |
| [`asset-loader.md`](docs/subsystems/asset-loader.md) | LBA resolver + sub-asset chain. |
| [`renderer.md`](docs/subsystems/renderer.md) | TMD renderer at `FUN_8002735c` (60 GTE ops). No culling of the port's own: what drops geometry is retail's (visible-tile crop at retail framing, actor cull, per-prim rejects); the clip volume holds the whole scene (`SCENE_FAR`). |
| [`shading.md`](docs/subsystems/shading.md) | How a pixel gets its colour: texel index -> CLUT read from **VRAM** (not the file) -> `texel * colour / 128` -> depth cue -> blend -> dither. No light source. Why exported palettes look wrong. |
| [`vr-mode.md`](docs/subsystems/vr-mode.md) | WebXR `immersive-vr` on the site's 3D pages: the flat renderer stays the geometry source, only the framebuffer and view-projection fork per eye. |
| [`audio.md`](docs/subsystems/audio.md) | PsyQ libsnd / libspu stack, SsAPI sequencer, SPU DMA engine; the cold scene-entry sequence every host owes, and how BGM parity is measured against a save state. |
| [`script-vm.md`](docs/subsystems/script-vm.md) | Field/event VM at `FUN_801DE840` (overlay-resident, 43 opcodes). Op `0x46` is `VIEW_WINDOW` (camera tile window, not fog); talking to an NPC is a button press, not an opcode. |
| [`script-vm-menuctrl.md`](docs/subsystems/script-vm-menuctrl.md) | Op `0x4C` `MENU_CTRL`, whose outer high nibble selects 16 sub-dispatchers; split out of `script-vm.md` for length. |
| [`tile-board.md`](docs/subsystems/tile-board.md) | Tile-board grid mode (puzzle / board minigame), NOT general town locomotion. `width×height` byte cell array (cell `2` = wall) + per-cell tile-actor rendering; installed inline in the field-VM script by op `0x49` (`_DAT_8007b450`); walk SM at `overlay_0897_801ef2b0`. |
| [`field-locomotion.md`](docs/subsystems/field-locomotion.md) | Player free-movement controller `FUN_801d01b0` (field overlay): camera-remapped held pad → direction + facing, per-frame speed, 2-unit stepping with per-axis collision `FUN_801cfe4c` against the per-scene walkability grid at `*(_DAT_1f8003ec)+0x4000` (4 sub-cell wall bits per 128-unit tile). Pinned by runtime write-watchpoint on `player+0x14/0x18`. |
| [`field-ambient-fx.md`](docs/subsystems/field-ambient-fx.md) | Field ambient animation: the bundle type-6 CLUT-walk table (12 carriers, not kingdom-only), the scene-entry ambient move-VM effect tree (MAN P1 effect scripts → `FUN_800252EC` → `FUN_80021B04`), and the mode-3 CLUT-cell HSV cycler `FUN_80019D50` behind jou's pulsating flesh + lightning. Engine `engine-core::world::ambient` + `clut_cell_fx`. |
| [`minigame-fishing.md`](docs/subsystems/minigame-fishing.md) | Fishing: state machine (`FUN_801cf3bc`), lure and bite tick, tension-gauge reel (`FUN_801d4004`), catch scoring, point exchange and per-venue spawn tables. All three hosts run one `PondSession`. |
| [`minigame-slot-machine.md`](docs/subsystems/minigame-slot-machine.md) | Casino slot machine: reel SM (`FUN_801cf0d8`), dual RNG, **five**-payline payout (`FUN_801d13e8`). A **3D scene** through the GTE, not 2D sprites. Parser `legaia_asset::minigame_slot_scene`. |
| [`minigame-baka-fighter.md`](docs/subsystems/minigame-baka-fighter.md) | Baka Fighter duel minigame: round SM (`FUN_801d3468`), rock-paper-scissors exchange resolver (`FUN_801d3a14`), stat/combo damage, pad-vs-AI move pick; reuses the PROT 1204 battle-form party meshes. |
| [`minigame-dance.md`](docs/subsystems/minigame-dance.md) | Noa dance rhythm minigame: beat-clock state machine (`FUN_801cf470`), timing-window judge (`FUN_801d1960`, accuracy-weighted), step chart at `0x801d509c`, groove gauge `DAT_801d544c` as difficulty/multiplier. |
| [`minigame-muscle-dome.md`](docs/subsystems/minigame-muscle-dome.md) | Muscle Dome arena ladder: rounds run on the general battle round SM (`FUN_801d0748`), the contest hub (`FUN_801cf870`) in PROT 0977. Not a card battle. |
| [`actor-vm.md`](docs/subsystems/actor-vm.md) | Actor / sprite VM at `FUN_801D6628` (13 opcodes) - the menu overlay's window-widget script interpreter; programs in [`window-script.md`](docs/formats/window-script.md). |
| [`effect-vm.md`](docs/subsystems/effect-vm.md) | Effect-bundle pool; spawn API; the three render-mode-4 emitters, all three carrying shipped content (the move VM selects them). |
| [`move-vm.md`](docs/subsystems/move-vm.md) | Move-table opcode VM at `FUN_80023070` (71 ops, JT `0x80010778`); op `0x2F` escapes to overlay extension. |
| [`move-vm-overlay-ext.md`](docs/subsystems/move-vm-overlay-ext.md) | The move VM's `0x2F` `OVERLAY_EXT` opcode and its 61 overlay-resident sub-opcodes (`FUN_801D362C`, dispatch JT `0x801CE868`). Split out of `move-vm.md`. |
| [`motion-vm.md`](docs/subsystems/motion-vm.md) | The two per-actor motion VMs: `FUN_8003774C` (pursue / patrol / face-target) and `FUN_80038158` (scripted motion + story-flag writes, bytecode in MAN tail-section 1). |
| [`vm-inventory.md`](docs/subsystems/vm-inventory.md) | Census of every VM-shaped subsystem (a bytecode dispatcher or a per-entity state-byte `switch`): what is decoded, ported, and reached by a live caller. "Five VMs" is an orientation, not the full list. |
| [`cutscene.md`](docs/subsystems/cutscene.md) | STR movies (game modes 26/27, Iki-bitstream MDEC decode, XA sync, `play-str`), the in-engine scripted opening / ending timelines on the field VM, and the field-to-battle transition. |
| [`battle.md`](docs/subsystems/battle.md) | Battle hub: scene loader `FUN_800520F0`, context struct, actor record, stage seats, monster record + archive (PROT 867), heap budget, per-frame actor passes; map of the sibling battle pages. |
| [`battle-command-flow.md`](docs/subsystems/battle-command-flow.md) | Command-flow SM `FUN_801D0748` (`ctx[+0x06]`): round prompt, D-pad command ring, commit log + confirm, target picker; the tutorial (967) and Cort boss-stage (968 / 969) overlays. |
| [`battle-action.md`](docs/subsystems/battle-action.md) | Battle action state machine `FUN_801E295C` (`ctx[+0x07]`): phase diagram, state table + per-state detail, cast cameras, turn cursor, effect script, port. |
| [`battle-action-exit-gates.md`](docs/subsystems/battle-action-exit-gates.md) | The state-`0x51` HP-bar settle gate and the state-`0x19` approach park: the two retail battle softlock classes and their root causes. |
| [`battle-action-helpers.md`](docs/subsystems/battle-action-helpers.md) | Helpers the action SM calls: range metric, escape roll, AI delegation + auto-combo, summon / cast dispatch, pose driver, voice cues, status word, overlay PRNG. |
| [`battle-action-queue.md`](docs/subsystems/battle-action-queue.md) | The action queue at `actor[+0x1DF..]`: byte alphabet, queue builder `FUN_801EED1C`, Tactical / Miracle / Super Arts, the no-input attack, damage per hit event, action validator `FUN_8003FB10`. |
| [`battle-stage-camera.md`](docs/subsystems/battle-stage-camera.md) | Battle stage (procedural ground grid, twice-drawn backdrop shell, stage-stream pick), the phase-scripted camera (`FUN_801D5854` framing cases, entry sweep) and the field-to-battle intro. |
| [`battle-actor-rendering.md`](docs/subsystems/battle-actor-rendering.md) | Battle actor rendering: assembled party meshes, monster mesh, staged-anim channel, tint passes, weapon trails, slow motion and after-image ghosts; both hosts' render bridges. |
| [`battle-hud.md`](docs/subsystems/battle-hud.md) | Battle HUD and screen chrome: placement table `0x80076C10`, widget-class table, per-phase sub-draw script, banners, badges, item window. Port `engine-core::battle_hud` + `engine-ui`. |
| [`battle-round-loop.md`](docs/subsystems/battle-round-loop.md) | One fight end to end: encounter roll + scripted `3E FF` entry, round bands, results sequencer, party wipe, `+0x16E` status bits, monster AI, Seru capture, the port's Field / Battle loop. |
| [`battle-formulas.md`](docs/subsystems/battle-formulas.md) | Damage / MP-cost / accuracy / escape / RNG kernels. A physical hit is *Offense Value - Defense Value*, with equipment folded in at swing time. Mirror `engine-battle-vm::battle_formulas` (re-exported by `engine-vm`). |
| [`cast-module.md`](docs/subsystems/cast-module.md) | Slot-B cast / summon modules (PROT 0903..0966 at `0x801F69D8`): how an action reaches one through PROT 0898's three entry tables, image anatomy, `ctx+0x279` phase machine, tick ABI, damage shapes, camera, port. |
| [`inventory.md`](docs/subsystems/inventory.md) | One 256-slot bag at `0x80085958`, bounded by an **active window** (`gp[+0x2D2..+0x2D6]`) that collapses to one 128-slot half for a lone character. The "72-slot" figure is a cheat's display page. |
| [`arts-command-gauge.md`](docs/subsystems/arts-command-gauge.md) | Arts AP gauge + weapon-specialty arm width. Per-command cost is per-(character, weapon) **disc data** copied at battle load, not a runtime comparison - which makes it a randomizer target. |
| [`world-map.md`](docs/subsystems/world-map.md) | The overworld as a field-run scene: walk, camera, entrances and encounters, heightfield ground, PROT 0901 draw kernels, the L1 map display (PROT 0981), and the debug top view + dev menu behind `FUN_801E76D4`. Ocean CLUT cycling comes from the kingdom slot-5 CLUT-walk table. |
| [`world-overview-viewer.md`](docs/subsystems/world-overview-viewer.md) | The static-site `/world-overview/` WebGL viewer: AABB layout, distance-cue fog pass (per-Z scalar LUT + per-kingdom haze), MAN `0x7F`-sentinel bulk-terrain resolver, ocean tile + 13-frame CLUT animation, camera anchors. |
| [`save-screen.md`](docs/subsystems/save-screen.md) | Save-slot select + write flow (`FUN_801DC6B4`, menu overlay), the live game-state window at `DAT_80084140` the save block is composed from, its checksum, and the record screen behind the pause Status row. |
| [`field-menu.md`](docs/subsystems/field-menu.md) | Pause menu: the 52-record **window descriptor table** (`legaia_asset::menu_windows`), status / party panels, equip stat-compare panels, options screen. Panels are content-only draws; port in `engine-ui`. |
| [`shop.md`](docs/subsystems/shop.md) | Town shop buy / sell / quantity / confirm flow. UI lives in the **menu overlay** (no separate shop overlay); per-scene stock is inline in the scene MAN's field-VM script, prices from the static item table. Port = `engine-core` shop session. |
| [`inn.md`](docs/subsystems/inn.md) | Inn HP/MP restore. **No inn overlay and no cost table** - each inn is an ordinary field-VM dialogue and the price is a script literal in the scene MAN. Port `engine-core::inn`; cost scanned from the MAN at scene load. |
| [`level-up.md`](docs/subsystems/level-up.md) | Post-battle XP distribution, the per-level stat-gain table, and the banner. Retail applies XP + growth + level bump in overlay `FUN_801E9504`; port `engine-battle::levelup::LevelUpTracker`. |

### Tooling - [`docs/tooling/`](docs/tooling/)

| Doc | Covers |
|---|---|
| [`extraction.md`](docs/tooling/extraction.md) | Per-stage CLIs (`disc-extract`, `prot-extract`, `lzs-decode`, `legaia-extract`, …). |
| [`ghidra.md`](docs/tooling/ghidra.md) | Compose-exec invocation, the LUI+ADDIU workaround, full script catalogue. |
| [`overlay-capture.md`](docs/tooling/overlay-capture.md) | Mednafen save-state slicing; one-shot pipeline. |
| [`static-overlay-pipeline.md`](docs/tooling/static-overlay-pipeline.md) | Extract each clean-copy runtime overlay from `PROT.DAT` at its statically-recovered base (`asset overlay …`) - identity from the PROT entry, reproducible from the disc. Map `crates/asset/data/static-overlays.toml`. |
| [`mednafen-automation.md`](docs/tooling/mednafen-automation.md) | Save-state diff / bisect / scenario manifest; watchpoint-equivalent observation across `.mc{0..9}` snapshots. |
| [`pcsx-redux-automation.md`](docs/tooling/pcsx-redux-automation.md) | Closed-loop Lua probes layered on PCSX-Redux's breakpoint debugger. Save-state load → arm probes → capture N VSyncs → CSV / snapshot. Catalogue + authoring pattern. |
| [`playthrough-coverage.md`](docs/tooling/playthrough-coverage.md) | Trace-driven documentation worklist: play a scripted opening segment under PCSX-Redux with a breakpoint on every not-yet-understood function, and let the hits pick what to document next. The segment/triage instrument for the probe harness. |
| [`spine-flag-writers-capture.md`](docs/tooling/spine-flag-writers-capture.md) | Runbook to capture the chapter-1 spine story-flag writers that live in un-imported overlays (system flags `0x142` dolk-dungeon-clear, `0x482` Drake mist walls) in one play-forward with all watches armed. |
| [`super-art-queue-capture.md`](docs/tooling/super-art-queue-capture.md) | Runbook + Lua capture pinning the byte-exact Super / Miracle Art action queue (`actor[+0x1DF..+0x1F2]`) each triggered combo expands to; corrects the earlier `ctx[+0x274]` hypothesis. |
| [`port-catalog.md`](docs/tooling/port-catalog.md) | Per-function status catalog (`dumped` × `documented` × `ported` × `ignored`), the `// PORT:` / `// REF:` / `REPLACED-BY:` tags, feature views and `--dashboard`. Drift checker `scripts/ci/check-port-tags.py`. |
| [`disc-coverage.md`](docs/tooling/disc-coverage.md) | Coverage with the denominator taken from the **disc**: each code image's bytes covered by dumped functions, plus PROT format recognition. Gate `scripts/ci/disc-coverage.py`. |
| [`byte-accounting.md`](docs/tooling/byte-accounting.md) | Per PROT entry, which bytes a parser actually consumes and what shape the rest has (`asset account`, `legaia_asset::byte_account`). Gate `scripts/ci/byte-account-coverage.py`. |
| [`field-op-census.md`](docs/tooling/field-op-census.md) | Disc-wide field-VM opcode census over every scene MAN and event-script carrier (`legaia_asset::field_disasm::census`, `asset field-op-census`): does any shipped scene issue op X, and where. Its **zeros** are the point - they separate "no fixture drives this arm" from "the disc has nothing to drive it". |
| [`worklist-classification.md`](docs/tooling/worklist-classification.md) | Classifies every `--missing-ports` row by whether it is a portable function entry at all (`REAL` / `INTERIOR` / `SHARED_TAIL` / `DUPLICATE` / `VA_ALIASED` / …), so the worklist reads as work rather than as addresses. |
| [`address-reference-scan.md`](docs/tooling/address-reference-scan.md) | Who references an address, in all five forms at once (literal word, `lui` pair, `jal`, `j`, branch) across SCUS, overlays and raw PROT - turns "no caller found" into "no reference exists". |
| [`live-audit-triage.md`](docs/tooling/live-audit-triage.md) | How a `port-catalog.py --live-audit` "undisclosed inert port" row is settled: verdict vocabulary, the audit's false-result shapes, how a `NOT WIRED:` reason goes wrong, and the settled rows by address. Its `REPLACE` verdict is where a row's `REPLACED-BY:` text comes from. |
| [`stale-not-wired-triage.md`](docs/tooling/stale-not-wired-triage.md) | The same audit's "tagged `NOT WIRED` but analysed live" section: stale tag or false edge, the six name-resolution mechanisms that invent edges, and the fix each takes. |
| [`reach-triage.md`](docs/tooling/reach-triage.md) | Per-address verdicts for `replay-port-coverage.py`'s **live but never entered** set - the runtime gap the static audits miss. |
| [`full-game-ladder.md`](docs/tooling/full-game-ladder.md) | Segmented New Game to credits ladder: story milestones anchored to retail card saves / states, per-segment tiers `loads` / `enters` / `progresses` / `pad`, ratcheted in `scripts/replays/full_game_baseline.toml`. A stall names the parked op, the failing door gates and the unset flags with their disc setters. |
| [`retail-compare.md`](docs/tooling/retail-compare.md) | Seed the engine from every walkable and battle library state and score it against the state's own RAM and VRAM, worst first; ratchet `scripts/ci/retail-compare-baseline.json`. |
| [`soak-harness.md`](docs/tooling/soak-harness.md) | Seeded, game-shaped random pad input over every playable scene and minigame (`engine-shell/tests/soak_harness.rs`) with panic / softlock / script-stall / battle-loop / value detectors and reduced `j-replay-v1` repros; fixed findings stay as hard-fail regressions in `scripts/replays/soak/fixed/`. |
| [`call-target-integrity.md`](docs/tooling/call-target-integrity.md) | Why a decoded `jal` target is a property of the bytes and not of the load base, and the one dump window (`overlay_0896` below `0x801CE818`) whose targets are therefore untrustworthy. |
| [`dump-corpus-integrity.md`](docs/tooling/dump-corpus-integrity.md) | The sibling failure: a dump's *printed addresses* are a property of the load base. A filename prefix is not evidence of base correctness - only the header tag is, and even a tagged dump may have gaps. Sweep `check-dump-base-integrity.py`. |
| [`port-provenance.md`](docs/tooling/port-provenance.md) | Does a `// PORT:` address name the routine the Rust code implements? Nothing gates it; ranked worklist `scripts/ci/check-port-provenance.py` (warn-only). |
| [`phantom-print-index.md`](docs/tooling/phantom-print-index.md) | The above applied address-by-address to the `0x801C****` / `0x801D****` printed band: per-dump-program re-key deltas, and for every such address the image + VA its bytes really occupy and whether that VA is a function entry. Almost none is. |
| [`recomp-differential.md`](docs/tooling/recomp-differential.md) | Frame-tagged differential oracle vs the static recomp: `scripts/recomp/` TCP probe client + trace capture, `legaia-engine sim-trace` engine emitter, `trace_diff.py` per-channel first-divergence report. Traces are Sony-derived - never committed. |
| [`determinism-replay.md`](docs/tooling/determinism-replay.md) | `j-replay-v1` TOML record/replay format + `legaia-engine record` / `replay` subcommands + disc-free determinism cargo-test. Same input file run twice → bit-identical state-trace bytes; pad transitions captured from `play-window` keyboard handler. |
| [`vrchat-world-export.md`](docs/tooling/vrchat-world-export.md) | `legaia-engine export-glb`: per-scene textured world / NPC / animated-prop `.glb`s + placement manifest for Unity/VRChat world building, `--items` for every equipment item, `--party` for Vahn/Noa/Gala's field forms; Unity kit in `scripts/vrchat-world/`. Output is Sony-derived, gitignored. |
| [`randomizer.md`](docs/tooling/randomizer.md) | `legaia-patcher` for a user-supplied `.bin`: what each randomizer, slider and mod does, with the full flag reference. Entry page for the two below. |
| [`randomizer-internals.md`](docs/tooling/randomizer-internals.md) | The patcher's write path (LZS re-pack, in-place PROT edit, EDC/ECC re-encode, PPF), the injected-code arenas, each MIPS hook's design, and the test catalogue. |
| [`randomizer-delilas.md`](docs/tooling/randomizer-delilas.md) | The Delilas Challenge dome course, the custom items and the Delilas party swap (model, moveset, voice and cast-route rebuilds). |
| [`releases.md`](docs/tooling/releases.md) | Tagged-release pipeline: a `v*` tag builds per-target archives + `SHA256SUMS` onto its GitHub release. The runner is **arm64**, so x86_64 builds are cross-compiles. Never game data. |
| [`shell-observer-traps.md`](docs/tooling/shell-observer-traps.md) | Three shell defects where the observer sits inside the thing it observes: pipe exit status read off the wrong stage, `pkill`/`pgrep -f` matching the caller's own command line, `grep`'s no-match exit 1 read as failure. Gate `check-shell-observer-traps.py`; helpers `scripts/lib/proc.sh`. |
| [`host-drift.md`](docs/tooling/host-drift.md) | Three hosts on one engine (native play-window, browser play page, minigames page): the gate tiers that catch a feature wired into only one, and the shapes a one-host reading gets backwards. |
| [`shipped-bundle-freshness.md`](docs/tooling/shipped-bundle-freshness.md) | A local `site/wasm/` bundle goes stale invisibly; content-addressed stamp + `check-wasm-freshness.py`. |
| [`site-shell.md`](docs/tooling/site-shell.md) | The static site's app shell across breakpoints, the silent-shadowing traps and their gates (CSS `@media` order, split case folds, module syntax in classic scripts), shared `site-util.js` helpers, and page delivery rules. |
| [`doc-density.md`](docs/tooling/doc-density.md) | The two hard doc gates: `check-doc-density.py` (>800-char lines, >150-word cells) and `check-md-links.py` (relative links + `#anchors`). Bypass `LEGAIA_SKIP_PRECOMMIT=1`. |
| [`translation/`](docs/tooling/translation/index.md) | `legaia-patcher translate` - community language packs: disc text → editable YAML → in-place reimport; pages for pack format, space and budgets, dialog import, UI strings, textures and fonts. Beginner guide: [`guides/translating.md`](docs/guides/translating.md). Exported packs carry game text - gitignored, never commit. |
| [`pal-localizations.md`](docs/tooling/pal-localizations.md) | Official PAL discs (FR/DE/IT): PROT.DAT is 1:1 with USA; `translate lift-official` re-keys their text onto USA coordinates. USA scene MANs have zero compressed slack. |

### Reference - [`docs/reference/`](docs/reference/)

| Doc | Covers |
|---|---|
| [`functions.md`](docs/reference/functions.md) | Notable Ghidra-traced function entry points (the canonical directory) - index page over the per-subsystem tables in `docs/reference/functions/`. |
| [`memory-map.md`](docs/reference/memory-map.md) | RAM map + key globals. |
| [`builds.md`](docs/reference/builds.md) | Region data; known builds. |
| [`cheats.md`](docs/reference/cheats.md) | GameShark / Mednafen cheat database parser + classifier; pinned RAM offsets for character record, inventory, battle actor, story flags. |
| [`gamedata.md`](docs/reference/gamedata.md) | Curated arts/magic/items/weapons/armor/accessories/enemies/shops/casino/fishing tables mined from public walkthroughs. Ground-truth labels for binary records under reverse engineering. |
| [`music-tracks.md`](docs/reference/music-tracks.md) | Every BGM cue across its four naming spaces (sound-test id, in-game context, OST title, relocalization), joined to the disc. The `music_01` bank is **piecewise** in extraction space. Resolver `engine-core::music_labels`. |
| [`scene-names.md`](docs/reference/scene-names.md) | The one scene-name table (`data/gamedata/scenes.toml`, `legaia_gamedata::scene_names`): display name, disc banner, in-game reading - and the labels that mislead. |
| [`open-rev-eng-threads.md`](docs/reference/open-rev-eng-threads.md) | The live RE hunts (`open` / `partial` / `mostly resolved`), plus what an evidence grade means. Question-level companion to `port-catalog.py --dashboard`. |
| [`re-settled-threads.md`](docs/reference/re-settled-threads.md) | Answered RE questions, each graded `disassembly` / `capture` / `decompiled-C` / `inference` by what its own evidence rests on. Index page over the per-area pages in `docs/reference/re-settled-threads/`. `decompiled-C` = the re-audit bucket. |
| [`re-do-not-re-walk.md`](docs/reference/re-do-not-re-walk.md) | Falsified hypotheses with their reasoning intact - the plausible readings of the bytes that turned out wrong. |
| [`overlay-va-aliases.md`](docs/reference/overlay-va-aliases.md) | Phantom virtual addresses in `ghidra/scripts/funcs/`: real bytes and real disassembly under a VA that belongs to no runtime image. |

### Crates - [`crates/`](crates/)

Each crate has a one-page `README.md` describing its scope, format coverage, and how it composes into the pipeline. Crate naming: package `legaia-foo`, lib `legaia_foo`. Every dependency, internal or external, is declared once in the root `[workspace.dependencies]` and taken as `legaia-asset.workspace = true` - never a per-crate `path =` or version pin. A crate's command-line binary sits behind its default-on `cli` feature (`required-features = ["cli"]`, `clap` / `libc` optional); the workspace table lists those crates with `default-features = false`, which keeps CLI dependencies out of library builds and the wasm bundle.

**Track 1 - preservation (asset → PNG / WAV / OBJ / JSON)**

| Crate | Binary | Scope |
|---|---|---|
| [`crates/bytes`](crates/bytes/README.md) | - | Checked little-endian byte readers. The leaf under the format stack, but **not** yet universal: `legaia-asset`, the engine and patcher crates and the crates split out of them depend on it, while the older per-format crates hand-roll their reads. |
| [`crates/iso`](crates/iso/README.md) | `disc-extract` | PSX Mode2/2352 disc reader, ISO9660 walker, **sector write-back** (`write` module: EDC/ECC re-encode + `patch_file_logical`; `iso9660::find_file_in_image`). |
| [`crates/prot`](crates/prot/README.md) | `prot-extract` | PROT.DAT / DMY.DAT TOC, CDNAME map, standalone TIM-pack. |
| [`crates/lzs`](crates/lzs/README.md) | `lzs-decode` | Legaia LZS decoder (reversed from `FUN_8001a55c`) + `compress` re-packer (greedy LZSS the retail decoder accepts; for editing assets). |
| [`crates/asset`](crates/asset/README.md) | `asset` | The format hub: dispatcher, DATA_FIELD streaming, pack format, bundle detectors, `categorize`, and the side-effect-free field-VM disassembler `field_disasm`. |
| [`crates/game-tables`](crates/game-tables/README.md) | - | The static data tables parsed straight out of `SCUS_942.54` and the overlay images (items, equipment, spells, level-up, steal, element affinity, mode / FMV / SFX / XA tables). Re-exported by `legaia-asset`. |
| [`crates/battle-models`](crates/battle-models/README.md) | - | Battle model formats and their glTF export: the monster archive, player battle files + equipment assembly, `"ME"` archives, face animation, battle textures / palettes, the summon side-band, the `.glb` builders. Re-exported by `legaia-asset`. |
| [`crates/overlay-images`](crates/overlay-images/README.md) | - | The code-overlay image formats: MIPS-overlay and pointer-table detectors, the static-overlay extraction map, the slot-B cast / summon module layout and spawn records, and the tables resident in the battle and menu overlays (move power, window descriptors, window scripts). Re-exported by `legaia-asset`. |
| [`crates/tmd`](crates/tmd/README.md) | `tmd` | Legaia TMD parser + primitive walker + OBJ-with-faces export. |
| [`crates/tim`](crates/tim/README.md) | `tim` | PSX TIM parser + PNG exporter + PNG-to-TIM encoder (texture replacement). |
| [`crates/xa`](crates/xa/README.md) | `xa` | XA-ADPCM decoder + WAV exporter. |
| [`crates/vab`](crates/vab/README.md) | `vab` | VAB sound bank extractor + SPU-ADPCM decoder. Three entry points, and picking the wrong one is a silent defect: `parse` for a bank at a known offset, `parse_in_stream` for one inside a DATA_FIELD stream, `vag_body_origin_at` for the body chunk. |
| [`crates/seq`](crates/seq/README.md) | `seq` | PsyQ SEQ parser + CLI inspector. |
| [`crates/mdt`](crates/mdt/README.md) | `mdt` | Move table (Tactical Arts) parser. |
| [`crates/art`](crates/art/README.md) | `art` | Tactical Arts data: ActionConstants, per-character art tables, Miracle / Super Art trigger matchers, the arts name and voice tables, and the byte-exact retail arts tokenizer. |
| [`crates/mes`](crates/mes/README.md) | `mes` | MES dialog container parser (Compact + Records). |
| [`crates/anm`](crates/anm/README.md) | `anm` | ANM animation container parser. |
| [`crates/save`](crates/save/README.md) | `save-tool` | Per-character record schema (typed accessors + round-trip parse/write for the 0x414-byte record) plus a PSX memory-card walker. `Party::from_retail_sc_block` lifts a real SC block into a typed `Party`; `SaveExt` / `SaveFile` (LGSF) cover full engine save round-trips. |
| [`crates/font`](crates/font/README.md) | `font-extract` | Proportional dialog font: extracts width table + 4bpp atlas from `SCUS_942.54` + a mednafen save state, exposes a layout API for engine consumers. |
| [`crates/extract`](crates/extract/README.md) | `legaia-extract` | Top-level pipeline driver: disc → PROT → categorize → streaming sub-asset extract → PNG. |
| [`crates/mdec`](crates/mdec/README.md) | `mdec` | PSX MDEC from-scratch decoder. Legaia movies are the **Iki** bitstream, **not STRv2** - that's the thing to know before debugging a garbled frame. Frame → RGBA8 via the PSX AC VLC table, 8-point IDCT, YCbCr→RGB; `StrFrameAssembler` handles multi-sector STR video frames. |
| [`crates/mednafen`](crates/mednafen/README.md) | `mednafen-state` | Mednafen save-state parser + watchpoint-equivalent automation (pairwise main-RAM diff, write-transition bisection, scenario manifest [`scripts/scenarios.toml`](scripts/scenarios.toml)). `gpu`/`vram-dump` decode the VRAM blob and `spu` exposes `PsxSpu` - the retail sides of the engine's VRAM and audio parity oracles. |
| [`crates/pcsxr`](crates/pcsxr/README.md) | `pcsxr-state` | PCSX-Redux save-state (`.sstate`) main-RAM reader, exposing `main_ram()` + VA readers + `scene_name()`/`game_mode()`/`player_pos()`; the CLI mirrors `mednafen-state`'s `extract` so scripts treat both emulators' states interchangeably. The bridge that feeds the cataloged playthrough anchors (`s1..s5`) into the engine's disc-gated field/opening oracles. |
| [`crates/gamedata`](crates/gamedata/README.md) | `gamedata-tool` | Curated game-data tables (arts, magic, items, weapons, armor, accessories, enemies, shops, casino, fishing, music tracks, scene names) mined from public walkthroughs; the music and scene-name tables contributed by Stann0x. These are **ground-truth labels** for the binary records under RE, not disc data. See [`docs/reference/gamedata.md`](docs/reference/gamedata.md). |
| [`crates/cheats`](crates/cheats/README.md) | `cheat-tool` | Parser + classifier for third-party GameShark / Pro-Action-Replay cheat databases. Classifies codes by the RAM region they target; the pinned offsets (character record, inventory, battle actor, story flags) ground-truth the binary records. See [`docs/reference/cheats.md`](docs/reference/cheats.md). |
| [`crates/patcher`](crates/patcher/README.md) | `legaia-patcher` | Disc-patching toolkit for a user-supplied `.bin` (in-place PROT edits, MAN relocation, PPF output): randomizer, translation packs, manual edits, MIPS code hooks. Reference: [`randomizer.md`](docs/tooling/randomizer.md). No Sony bytes. |
| [`crates/disc-patch`](crates/disc-patch/README.md) | - | The patcher's foundation: `DiscPatcher` same-size PROT edits with EDC/ECC re-encode, PPF 3.0 writer, the SCUS / overlay free-space ledger, MAN re-pack budgets, the seeded `rng`. Re-exported by `legaia-patcher`. |
| [`crates/translate`](crates/translate/README.md) | - | The language-pack pipeline behind `legaia-patcher translate`: pack schema + markup, export / import, string relocation, other-build alignment and the PAL lift. Re-exported as `legaia_patcher::translation`. |
| [`crates/party-swap`](crates/party-swap/README.md) | - | The Party / Delilas battle-model swap kernels (rig permutation, pivot-anchored rest-pose bake, field form, win poses, movesets, enemy-anim remap) as pure transforms; disc writes stay in the patcher. Re-exported as `legaia_patcher::party_swap`. |
| [`crates/code-hooks`](crates/code-hooks/README.md) | - | The patcher's MIPS code-injection layer: instruction encoders + R3000 test simulator, the injection arenas, and the hook mods that need nothing above them (shiny Seru, Seru-trade overlay, enemy HP bar, bonus drop, flee EXP, softlock fixes). Re-exported by `legaia-patcher` at the old paths. |
| [`crates/arts-patch`](crates/arts-patch/README.md) | - | The patcher's Tactical Arts layer: combo randomizer, chain-art and Super Art power, the per-art AP override and oscillating-AP hooks, and the Super Art list on the battle move list and the Status page. Re-exported by `legaia-patcher` at the old paths. |
| [`crates/delilas-party`](crates/delilas-party/README.md) | - | The play-as-Delilas mod above the `party-swap` kernels: the orchestrator that writes the swapped battle and field models onto a disc, plus the arts reskin, movesets, signature attacks, effects, voices and the duel field scene. Re-exported by `legaia-patcher` at the old paths. |
| [`crates/texture-replace`](crates/texture-replace/README.md) | - | Image replacement modules (scene TIMs + palette contexts, party battle art, monster skins, save-slot portraits): decode, re-encode against the retail palettes and budget, write through `DiscPatcher`. Re-exported by `legaia-patcher` at the old paths. |

**Track 2 - engine reimplementation (from-scratch Rust)**

| Crate | Binary | Scope |
|---|---|---|
| [`crates/engine-core`](crates/engine-core/README.md) | - | The renderer-free simulation half: `World`, scene host and loading, battle loop, menu runtime, LGSF saves, the `World` entry points of the moved sessions. Re-exports `engine-battle` / `engine-minigames` / `engine-minigame-scenes` / `engine-effects` / `engine-system` / `engine-menus` / `engine-field` at their old paths. |
| [`crates/engine-battle`](crates/engine-battle/README.md) | - | The battle kernels that never touch `World`: monster AI, spoils, AP gauge, stats, catalogs, encounters, level-up, Seru learning, arts tracking, per-frame animation passes. |
| [`crates/engine-minigames`](crates/engine-minigames/README.md) | - | The minigame rules engines (slot machine, Baka Fighter, dance, prize exchange, Muscle Dome ladder), with no world, scene or renderer. Re-exports `engine-fishing` at the old paths. |
| [`crates/engine-fishing`](crates/engine-fishing/README.md) | - | The fishing rules engine below `engine-minigames`: `PondSession`, the rod / lure / line actors, venue chrome, hub screen and venue floor grid. |
| [`crates/engine-minigame-scenes`](crates/engine-minigame-scenes/README.md) | - | The minigames' 3D scene surfaces that load through a `read_prot` closure rather than a `Scene`: the Baka Fighter duel and the Muscle Dome arena. |
| [`crates/engine-effects`](crates/engine-effects/README.md) | - | The `World`-free effect kernels: action effect scripts, ribbon and render-mode-4 emitters, summon scene-graph driver, PROT-0900 screen effects, object effects. |
| [`crates/engine-system`](crates/engine-system/README.md) | - | The `World`-free runtime system below engine-core: pad input + retail pad pump, stream chunk installer, MDEC DMA sync, sound state, fades, mode-entry initialisers, RAM-cell map, capture observations. |
| [`crates/engine-menus`](crates/engine-menus/README.md) | - | The `World`-free menu front end: item / equipment catalogs, inventory-use and spell-menu sessions, list-row model, window-widget host, title + boot screens, memory-card flow, inn, item bag + equip session, save / status / pause / shop screens, option values, Muscle Dome loadout. Re-exports `engine-dialog` at the old paths. |
| [`crates/engine-dialog`](crates/engine-dialog/README.md) | - | The `World`-free dialog layer below `engine-menus`: dialog pager (panel, row window, typewriter pacing, picker slide), text balloon, and the inline-dialogue / cutscene-timeline context state. |
| [`crates/engine-field`](crates/engine-field/README.md) | - | The `World`-free field kernels: actor programs and per-actor kernels, follow-camera parameters and eases, player clip playback, transition / save-screen actors, op-`0x49` submode, field event queue, CLUT effects, battle anim / SFX cue routers, world-map controller, MAN field-script decoders, mode seat (`ModeWorld`), field camera (`CameraWorld`). |
| [`crates/engine-ui`](crates/engine-ui/README.md) | - | Renderer-agnostic UI draw-list builders, re-exporting the shared wgpu-free render kernels of `render-kernels` - the leaf both the native renderer and the browser play page use. |
| [`crates/render-kernels`](crates/render-kernels/README.md) | - | The wgpu-free render kernels both hosts share: GTE math, `screen_prim` + its CPU rasteriser, `vram_capture`, the effect emitters, `battle_numerals`, enhanced `scene_lighting`. Re-exported by `engine-ui`. |
| [`crates/engine-render`](crates/engine-render/README.md) | - | winit 0.30 + wgpu 26; software PSX VRAM (1024×512 R16Uint, per-prim CBA/TSB + CLUT decode in fragment shader); text overlay via the `legaia-font` atlas. |
| [`crates/engine-audio`](crates/engine-audio/README.md) | `note-trace` | cpal-backed audio mixer + from-scratch SPU + SsAPI-shape SEQ sequencer; BGM cross-fade + volume ramp; `audio-webaudio` feature adds `WebAudioOut` (`ScriptProcessorNode`-based) for WASM targets. |
| [`crates/engine-vm`](crates/engine-vm/README.md) | - | Actor / field / effect / move VMs + the **world-map entity SM** (`FUN_801DA51C`). Re-exports the battle VM kernels from `engine-battle-vm`, the motion VMs from `engine-motion-vm` and the field-VM disassembler from `legaia-asset`. |
| [`crates/engine-battle-vm`](crates/engine-battle-vm/README.md) | - | The battle-side VM kernels below `engine-vm`: battle action SM (`FUN_801E295C`), `battle_formulas`, battle camera script + `psx_camera`, cast-module ticks, battle-overlay leaves. |
| [`crates/engine-motion-vm`](crates/engine-motion-vm/README.md) | - | The two per-actor motion VMs below `engine-vm`: `motion_vm` (`FUN_8003774C`, pursue / patrol / face-target) and `ambient_motion` (`FUN_80038158`, scripted motion over MAN tail-section 1). Re-exported by `engine-vm` at the old paths. |
| [`crates/engine-session`](crates/engine-session/README.md) | - | The game session both play hosts run: `BootSession` (scene host + per-frame order) and the `AudioBgmDirector`, generic over the audio output (`AudioSink`). No wgpu / winit (cpal only through `engine-audio` on native), so it builds for `wasm32`; both play hosts tick it. |
| [`crates/engine-screens`](crates/engine-screens/README.md) | - | The shop / prize-exchange / inn / banner screens both play hosts draw: projects engine-core state into engine-ui builders once (`shop_overlay_frame`); hosts keep only input assembly, stage scale and upload. |
| [`crates/parity`](crates/parity/README.md) | - | The engine measured against retail: the VRAM / mode / audio / PCM oracles, the recomp differential's `sim_trace`, and the retail comparison corpus (`retail_compare*`). Drives the native `BootSession`; tool code no play host links. |
| [`crates/engine-shell`](crates/engine-shell/README.md) | `legaia-engine` | Top-level native driver: `play` / `play-window`, `save` / `load`, `play-str`, `record` / `replay`, `config`, and the parity-oracle subcommands; `--help` groups every one. |
| [`crates/asset-viewer`](crates/asset-viewer/README.md) | `asset-viewer` | Combined viewer: TIM, TMD, VAB, SEQ, stage geometry, PROT browser, scene-bundle presets, `save-icons` sheet, dialog box, field-VM scene runner with dialog rendering, battle-scene SM driver, and `world` (the `engine-core` composite ticking over a CDNAME scene). |
| [`crates/web-viewer`](crates/web-viewer/README.md) | - | WASM target: disc browser, TIM / TMD / MES / SEQ / VAB inspectors, full-map field scenes, the client-side ROM patcher (nothing is uploaded), equipment model assembly, `.glb` export. |

### Ghidra-side scripts - [`ghidra/scripts/`](ghidra/scripts/)

Jython analysis scripts that run inside the `ghidra` compose service. The script catalogue lives in [`docs/tooling/ghidra.md`](docs/tooling/ghidra.md#script-catalogue). Per-function decompiled-C dumps land in `ghidra/scripts/funcs/<addr>.txt` (gitignored - they're Sony-derived).

### Host-side scripts - [`scripts/`](scripts/README.md)

Helper scripts that run on the host (not in the Ghidra container), mapped in [`scripts/README.md`](scripts/README.md): `ci/` (the pre-commit + CI gates and build/install helpers), `ghidra-analysis/` (overlay extraction + MIPS/GTE disassembly), `asset-investigation/` (TIM/TMD/slot-4/scene RE one-offs), `recomp/` (static-recomp differential oracle), `vrc-diorama/` (VRChat battle diorama), `lib/` (sourced bash helpers), `git-hooks/` (the shipped hook), `engine/` + `replays/` (determinism-replay fixtures), plus `pcsx-redux/` + `mednafen/` capture automation. `scripts/scenarios.toml` (the capture-scenario manifest) and `manage-states.py` stay at the top level as operational entry points.

## Common commands

```bash
cargo build --release                                    # all binaries → target/release/
cargo fmt --all -- --check                               # CI gate
cargo clippy --all-targets --workspace -- -D warnings    # CI gate (warnings = failure)
cargo test --workspace --profile release-test           # CI's test profile: release opt-level, no LTO
cargo test -p legaia-asset                               # single-crate
cargo test --workspace test_name                         # single test by name
cargo test -p legaia-engine-core --test integration foo::  # one tests/foo.rs (a crate with several tests/*.rs builds them as one `integration` binary)
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

- **`crates/patcher/tests/*_real.rs`** - disc-round-trip oracles: patch a feature (drops / encounters / chests / steals / arts / doors / shops / starting items / starting level / equipment / item prices / unused content / weapon specialty / monster stats / move power / element affinity / spell costs) onto a scratch copy, re-decode off the patched image, assert the multiset/invariants are preserved + every touched sector stays EDC/ECC-valid + a fixed seed is byte-deterministic.
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
- **A save state replays the RAM of the disc that booted it.** Loading a state onto a *different* (e.g. freshly patched) disc keeps the old resident SCUS, overlays, battle models and effect state - the disc bytes under test are masked until the game re-loads them, so a fix can look absent and a fixed defect can look present. Verify disc edits from a cold boot or a memory-card load, never a mid-battle state; bridge SCUS mismatches with `legaia-patcher scus-pokes`. Details: [`docs/tooling/pcsx-redux-automation.md`](docs/tooling/pcsx-redux-automation.md#memory-card-capture-tier).
- **PCSX-Redux silently applies a `.ppf` sitting beside the image you hand it** (`cdrom/ppf.cc`), so a probe run against a disc directory you did not stage can measure a *patched* game while every log line names the retail path. It is the sibling of the save-state trap above - there, the RAM is not the disc's; here, the disc is not the disc's. Point `--iso` at a scratch copy in a directory you control. Details: [`docs/tooling/pcsx-redux-automation.md`](docs/tooling/pcsx-redux-automation.md).
- **MIPS LUI+ADDIU pairs are not auto-resolved by Ghidra's reference manager.** Direct xref queries return zero hits even when the address is heavily used. Use `ghidra/scripts/find_lui_writers.py` (edit `LO`/`HI` to your target range). Details: [`docs/tooling/ghidra.md`](docs/tooling/ghidra.md).
- **CDNAME `#define` numbers are raw in-RAM TOC indices, so every extraction filename label is shifted +2.** The named content for `#define name N` lives at extraction entry `N − 2` (`legaia_prot::cdname::block_for_extraction_index`), which is what dissolves historical "CDNAME labels mislead" cases such as `vab_01` carrying no VAB header. A CDNAME label is a hint, not a verdict: when attributing an entry, confirm with the loader-call constant or the magic bytes, and say which index space you mean. Details: [`docs/formats/cdname.md`](docs/formats/cdname.md#numbering-space).
- **An `(PROT entry N, offset K)` coordinate measured before the entry-size correction may name the wrong entry.** Entry size is the sector gap to the next entry (`toc[p+3] - toc[p+2]`); the superseded expression over-read into neighbours, so a constant whose `K` ran past entry `N`'s real end still hits the right *bytes* while naming the wrong *owner*. When a constant of this shape fails, suspect the pairing before the reader. Details: [`prot.md`](docs/formats/prot.md).
- **LZS "decompresses without error" is not a validity signal.** The 4 KB ring buffer initialises to zeros, so most random inputs decode to plausible-looking output. Always magic-check the *decoded* bytes. Details: [`docs/formats/lzs.md`](docs/formats/lzs.md).
- **Legaia SEQ has a u32 BE version field** (not PsyQ's u16) and its meta events carry **NO MIDI variable-length `length` field** - `0xFF 0x51` + 3 tempo bytes (no `0x03`), `0xFF 0x2F` ends track (no `0x00`). Reading a phantom length byte drops the first-body tempo override, pinning playback ~3x fast against the 240 BPM placeholder header. A meta **is** a running status: retail latches `0xFF`, so a data byte after a meta is the next meta's kind (PROT 1045's closing tempo). `ppqn = 480`; engine `Sequencer` clocks in exact integer SPU samples. Details: [`docs/formats/seq.md`](docs/formats/seq.md).
- **SEQ data in `scene_vab_stream` entries lives at non-zero offsets, and so do the VAG bodies.** The stream is `[chunk0 hdr][VAB header][chunk1 hdr][VAG bodies][chunk hdr][SEQ]` and some entries put the SEQ chunk first, so never probe fixed offsets: use `SceneAssets::seq_in_stream_entries` / `bgm_seq_offset` for the SEQ and `legaia_vab::vag_body_origin` for the bodies. Details: [`vab.md`](docs/formats/vab.md).
- **There is one pack format and two readers for it, plus the magic-prefixed effect bundle.** `asset::pack` reads the pack at offset 0; `prot::timpack` reads the same pack four bytes in, past a `(TIM_LIST << 24) | size` DATA_FIELD chunk header - which is what its `word_index*4 + 4` and its `marker == 0x01` test both encode. The former "field-pack" (`0x01059B84`) is that same chunk header on a scene's texture pack, not a third format ([`field-pack.md`](docs/formats/field-pack.md)). The effect bundle (`0x02018B0C`) is its own thing. See the pack pages linked under "Pack formats" above.
- **A field-VM global's name is the primary reader's, not the most interesting reader's.** `_DAT_8007B854` read as an input lock for as long as nobody enumerated its references; it is the ambient-particle master gate, raised and cleared by two arms of op `0x4C` nibble 3, and none of its six references disc-wide is pad state. The same failure named the monster record's `+0x20` an instant-death immunity byte when its primary reader is a VRAM-rect widener. Enumerate the references before naming the word.
- **Legaia TMDs are a custom variant.** Magic `0x80000002`, custom 8-byte group header, per-mode descriptor table at `DAT_8007326c`. Details: [`docs/formats/tmd.md`](docs/formats/tmd.md).
- **Ghidra promotes intra-function labels to fake `FUN_xxxxxxxx` calls.** When you see `iVar = FUN_801xxxxx(); return iVar;` in a giant dispatcher's C decomp, cross-check `grep -n "0x<addr>" overlay_<dump>.txt` - if the address appears as a `j` target inside that same function's disassembly, it's a label, not a call. Each such "label-call" is really `addiu s8, s8, N; j epilogue` (the standard PC-delta exit idiom). Catalogued for FUN_801de840 in [`docs/subsystems/script-vm.md`](docs/subsystems/script-vm.md#intra-function-label-catalogue) - applies to the dispatcher pattern in any large MIPS function, not just the field VM.
- **Port and document from the disassembly, not the decompiled C.** The label-call idiom above is one form of a broader rule: the C is a rendering, and its artifacts (dropped register arguments, reordered stores, mis-based dumps, `0 instructions` dumps, annotations read as evidence, …) have each already put a false claim into these docs. How to spot each, and what counts as evidence: [`docs/tooling/ghidra.md`](docs/tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims).

## Ghidra container quick reference

`docker-compose.yml` defines a single `ghidra` service, built from `docker/ghidra.Dockerfile` - a UID/GID-matched wrapper over `blacktop/ghidra:latest` so dumps come back owned by the host user:

- `./extracted:/data:ro` - disc-extracted files (read-only into Ghidra).
- `./ghidra/projects:/projects` - Ghidra project DB (gitignored; local only).
- `./ghidra/scripts:/scripts` - analysis scripts (read-write so dumps land back on host).

Workflow: `docker compose up -d ghidra` once, then `docker compose exec ghidra /ghidra/support/analyzeHeadless ...` per query. Don't restart the service per command. Full setup + per-query invocations: [`docs/tooling/ghidra.md`](docs/tooling/ghidra.md).

To add a new function dump, edit the `TARGETS` list in `ghidra/scripts/dump_funcs.py` and run the post-script - output lands in `ghidra/scripts/funcs/<addr>.txt`. Then update [`docs/reference/functions.md`](docs/reference/functions.md) if the entry point is notable.

For overlay-specific dumps use per-overlay scripts (e.g. `dump_shop_overlay.py`, `dump_levelup_overlay.py`, `dump_cutscene_overlay.py`, `dump_str_fmv_overlay.py`): a header, a `TARGETS` list and one `dump_targets(currentProgram, TARGETS)` call into the shared `lib_dump.py`, which skips addresses not in the current program and names output `overlay_<label>_<addr>.txt`. Run with `-process overlay_<label>.bin -noanalysis -postScript /scripts/dump_<label>.py`. One-off investigation scripts live in `ghidra/scripts/archive/`.

Jython 2.7 (Ghidra-bundled) chokes on Unicode in source unless an encoding declaration is added - keep `ghidra/scripts/*.py` ASCII-only.

## Writing rules for committed docs

- Present tense. State what the format / function / subsystem **is**, not when it was figured out.
- No session numbers, dates, "ported in session N" markers, before-vs-after counts.
- No rot-prone counts of project state (tests, crates, function-coverage percentages).
- Stable invariants of the disc itself (PROT entry counts, opcode counts) are fine.
- Provenance citations: `see ghidra/scripts/funcs/<addr>.txt` and `FUN_801XXXXXX in PROT entry NNNN_<name>`.
- Operational state (progress, dates, session logs, status tables) lives in git log + agent memory, not in committed docs. Don't cite agent-memory filenames from a committed doc either - they aren't public files.
- Keep prose out of table cells. A cell is a one-line "what this covers"; if it needs a paragraph, the paragraph goes in a section on the linked page. `scripts/ci/check-doc-density.py` enforces this (>800-char lines, >150-word cells) across `docs/`, crate READMEs, **and this file** - it is in scope precisely because it decayed furthest while exempt. Passing is a floor, not a target: prose that lands at 799 chars was written for the linter, not a reader.
