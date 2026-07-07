# Scene-prefixed asset bundles

Four related shapes account for the dominant per-scene asset layouts on the disc. All of them lead with a 4-byte chunk0 header in the form `(type << 24) | size`, with `type = 0x00` - the same encoding as a [DATA_FIELD streaming](data-field.md) chunk header. The standard streaming walker would interpret `type=0x00` as the TIM dispatcher slot; specialised loaders in the runtime know to dispatch chunk0 differently based on the *content* magic at offset +4.

## Contents

- [scene_tmd_stream - bare-TMD prefix](#scene_tmd_stream---bare-tmd-prefix)
- [scene_vab_stream - VAB-prefix](#scene_vab_stream---vab-prefix)
- [scene_v12_table - scene header + event-script bundle](#scene_v12_table---scene-header--event-script-bundle)
- [scene_asset_table - count-prefixed asset bundle](#scene_asset_table---count-prefixed-asset-bundle)
- [scene_scripted_asset_table - scripted prefix + canonical bundle](#scene_scripted_asset_table---scripted-prefix--canonical-bundle)
- [tmd_size_prefix - truncated TMD-prefix](#tmd_size_prefix---truncated-tmd-prefix)
- [scene_event_scripts - prescript-only](#scene_event_scripts---prescript-only)
- [See also](#see-also)

## scene_tmd_stream - bare-TMD prefix

The dominant scene-asset layout. Implementation: `crates/asset/src/scene_tmd_stream.rs`. ~12% of all PROT entries match. Walked by `FUN_8001FE70` (the battle-init custom walker) - **not** by `FUN_8002541C` / `FUN_8001F05C` despite the chunk-header packing matching the standard format.

```text
+0x00          u32 chunk0_header   ; (type=0x00 << 24) | size
+0x04          Legaia TMD          ; magic 0x80000002, fills `size` bytes
+0x04 + size   streaming chunks    ; FUN_8001FE70-style tail until
                                   ; terminator OR end-of-file
```

Strict structural detection:
1. `buf.len() >= 32`.
2. `buf[4..8] == 0x80000002` (Legaia TMD magic).
3. `buf[8..12] == 0` (TMD on-disc flags; runtime sets to 1 after pointer fixup).
4. `buf[12..16]` = `nobj`, `1 <= nobj <= 64`.
5. Chunk0 header at `buf[0..4]` has type byte 0.
6. TMD body size (low 24 bits of chunk0 header) is 4-aligned and at least `12 + nobj * 28`; `4 + size <= buf.len()`.
7. Streaming tail at offset `4 + tmd_size` walks at least one valid chunk header OR a terminator.

### Streaming tail - `FUN_8001FE70` walker

Past the leading TMD, each chunk is `[u32 header][payload]` with header packed as `(type << 24) | (size & 0x00FFFFFF)`. The retail walker (`FUN_8001FE70`, called from `FUN_800520F0` battle scene loader) dispatches:

| Type byte | Action |
|---|---|
| `0x01` | Upload payload as a single PSX TIM via `FUN_800198E0` (LoadImage). |
| `0x02` | Stop the walk (terminator). |
| any other | Skip silently (advance to next chunk). |
| size = 0 | Stop the walk (zero-size header is the canonical terminator). |

The type-byte semantics differ from the standard `FUN_8001F05C` dispatcher: there `type = 0x01` means `TIM_LIST` (a `[count + offsets + TIMs]` pack), but here it means "single bare TIM". So although the chunk-header packing is identical, calling `FUN_8002541C` on a `scene_tmd_stream` entry would mis-dispatch and crash. The runtime knows to use `FUN_8001FE70` for these entries.

### Concatenated sub-streams (the "two-list" shape)

Some entries (e.g. `0006_town01.BIN`) carry **two (or more) complete sub-streams** concatenated - each a full `[chunk0 TMD][type-0x01 TIM chunks][terminator]` block on a `0x800` (sector) boundary, zero-padding filling the gap. The second sub-stream has its **own** leading TMD; the "continuation" TIMs belong to it:

```text
+0x00000  chunk0 = TMD body 0x383c        ] sub-stream 0
+0x03840  type=0x01 TIM chunk             ]  (FUN_8001FE70's reach)
+0x0ba64  type=0x01 TIM chunk             ]
+0x13c88  terminator (zero-size header)   ]
+0x13c8c..0x13fff: zero padding to the next sector
+0x14000  chunk0 = TMD body 0x2c20        ] sub-stream 1
+0x16c24  type=0x01 TIM chunk             ]  (own TMD; the "continuation")
+0x1ee48  type=0x01 TIM chunk             ]
+0x2706c  terminator                      ]
```

`FUN_8001FE70` walks one sub-stream and **returns `param_1 + 1`** (past the terminator) - the next sub-stream's region - so a sector/slot-indexed caller can walk the rest. The single static caller `FUN_800513F0` (battle init) calls it **once**, so battle uploads only sub-stream 0; the multi-sub-stream caller is the per-scene field/town dispatch (`FUN_8001F7C0` → `FUN_80020224` → `FUN_8001F05C`, overlay-resident, capture-blocked). Enumerate the blocks with [`scene_tmd_stream::sub_streams`](../../crates/asset/src/scene_tmd_stream.rs) (each a full sub-stream with its own TMD); [`scene_tmd_stream::battle_tim_chunks`](../../crates/asset/src/scene_tmd_stream.rs) reports sub-stream 0's TIMs as `WalkSource::Tail` and the later ones as `WalkSource::Continuation`.
The engine's field-mode loader uses both to **skip** these battle-only TIMs (the row-479 NPC palettes aren't field-resident - matching retail).

Reading:

```rust
use legaia_asset::scene_tmd_stream;
if let Some(s) = scene_tmd_stream::detect(&buf) {
    let tmd = legaia_tmd::parse(&buf[s.tmd_range()])?;  // bare TMD, no wrapper
    for chunk in &s.tail_chunks {
        // each chunk is (type, size, payload) per data-field.md
    }
}

// Surface every type-0x01 TIM upload chunk - both in-tail and continuation.
for c in scene_tmd_stream::battle_tim_chunks(&buf) {
    // c.payload_offset is the inner PSX TIM magic offset.
    // c.source distinguishes Tail (FUN_8001FE70-reachable) from
    // Continuation (past the first terminator).
}
```

## scene_vab_stream - VAB-prefix

Same outer wrapper as `scene_tmd_stream` but the leading chunk carries a Sony VAB sound bank instead of a TMD. The single largest distributed-VAB carrier in the corpus. Implementation: `crates/asset/src/scene_vab_stream.rs`. ~17% of all PROT entries match.

```text
+0x00          u32 chunk0_header  ; LE: type=0x00 in high byte, size=N in low 24 bits
+0x04          u32 magic          ; 0x56414270 ('VABp' read as LE u32 = 'p' 'B' 'A' 'V')
+0x08          u32 version        ; 7 in retail (must be ≤ 10)
+0x0C..        VAB header tail + programs[] + tones[] + VAG offsets
+0x04 + N      streaming chunks   ; standard DATA_FIELD chunks until terminator
                                  ; OR end-of-file
```

Strict gate validates the VAB header: `version <= 10`, `program_count <= 128`, `tone_count <= 128`. The `chunk0_size` low byte is consistently `0x20` (sector-aligned to 32-byte boundary).

Cluster anatomy:
- 119 of 123 entries in the CDNAME `vab_01` cluster (1072..1194) match - the standard distributed-bank layout.
- 53 entries in `sound_data2` (878..890), 19 in `music_01`, 14 in `monster_data` / `battle_data`, plus scattered hits in `teien`, `other5`, `player_data`.

Reading:

```rust
use legaia_asset::scene_vab_stream;
use legaia_vab::parse_header;

if let Some(s) = scene_vab_stream::detect(buf) {
    let header = parse_header(buf, s.vab_range().start)?;
    println!("VAB v{} ps={} ts={}", header.version, header.ps, header.ts);
}
```

## scene_v12_table - scene header + event-script bundle

A scene-named container that bundles a small runtime-fixup header with a full event-script prescript at sector offset `0x800`. Implementation: `crates/asset/src/scene_v12_table.rs`. 97 PROT entries match - one per scene. Detailed reference: [`scene-v12-table.md`](scene-v12-table.md).

```text
+0x000   u16  N + 4              ; runtime fixup-slot offset; header field
+0x002   u16  0x0012             ; constant magic
+0x004   u16  0x0000             ; constant
+0x006   u16  0x0014             ; constant magic (= byte offset of records)
+0x008   u16  param              ; count of inline records (0..=192 in retail)
+0x00A   u16  N                  ; runtime fixup-slot offset; header field
+0x00C   u16  0x0000             ; constant
+0x00E   u16  N + 2              ; runtime fixup-slot offset; header field
+0x010   u32  0                  ; padding to 0x14
+0x014   param × 4 bytes         ; inline record table
+end_records (= 0x14 + 4*param)  ; runtime writes three pointers here
                                 ; (slots at +N, +N+2, +N+4 - zero on disc).
+end_records .. 0x800            ; zero padding
+0x800   u16  script_count       ; scene event-scripts prescript
+0x802   script_count × u16      ;   offset table relative to +0x800
+0x800 + offsets[i]              ;   per-record field-VM bytecode
```

The header's `u16[0]`, `u16[5]`, `u16[7]` are algebraically tied to a single per-scene constant `N`: `u16[0] = N + 4`, `u16[5] = N`, `u16[7] = N + 2`, and `N = 4 * param + 22` (= byte distance from the file head to the first runtime fixup slot). Strict structural checks combine the three constant words, the algebraic ties, and the `N/param` algebra. Across the entire 1234-entry PROT corpus this matches **97** entries with zero false positives - and **every** match parses cleanly as a scene-event-scripts prescript at `+0x800`.

The post-header dense data is the [scene_event_scripts](#scene_event_scripts---prescript-only) prescript - a word-aligned per-scene actor/event command structure, **not** field-VM (`FUN_801DE840`) bytecode (see that section for the falsification). The pre-header table at `+0x14` is per-scene runtime metadata: `param` records of 4 bytes each, grouped by the third byte (`b2`) into 1..N scene regions; the last byte is always `0x01` (probably a "live" flag). See [`scene-v12-table.md`](scene-v12-table.md) for the per-byte semantics.

Each scene block on the disc carries **both** a v12 entry (this format, prescript at `+0x800`) and a sister `scene_event_scripts` entry (prescript at offset 0, no v12 header). Both carry the same word-aligned record structure. The genuine per-scene field-VM scripts live elsewhere - in the scene MAN sub-asset (`FUN_8003A1E4` → `FUN_801DE840`; see [`subsystems/script-vm.md`](../subsystems/script-vm.md)).

## scene_asset_table - count-prefixed asset bundle

The on-disc form of the scene asset table that the field loader reads when entering a town/dungeon. Implementation: `crates/asset/src/scene_asset_table.rs`.

```text
+0x00   u32  count                  ; descriptor count (6 or 7)
+0x04   u32  meta1                  ; varies - purpose unknown
+0x08   count × (u32 type_size, u32 data_offset)
                                    ; each pair packs `(type<<24)|size`
+H      asset payload region        ; LZS-compressed in some entries,
                                    ; raw in the rest
        (H = 8 + count*8: 0x40 for count 7, 0x38 for count 6)
```

The table is **`count`-prefixed**, not fixed-7: the runtime walker `FUN_80020224` reads `count` from `+0x00` and loops that many descriptors, calling the [asset-type dispatcher](asset-type.md) `FUN_8001F05C` with `source = table_base + descriptor.data_offset`. Two `count` values appear in the retail corpus:

- **`count = 7`** - kingdom-bundle scenes (most towns/dungeons; first descriptor `TimList`). First descriptor's `data_offset` is `0x40`.
- **`count = 6`** - the early standalone-town scenes (`town01` = Rim Elm, `town0c`, …) whose CDNAME block has no separate scripted-table entry.
  - First descriptor is `Tmd` (`town01`) or `Flag(0x0A)` (`town0c`); first `data_offset` is `0x38`.
  - These were the scenes that previously appeared to "have no MAN in the static bundle" - their table sits in the block's 2nd PROT entry (e.g. `town01` = entry 4, `town0c` = entry 22) and is `count=6`, so a strict `count==7 && first_offset==0x40` detector skipped it.
  - Pinned via a runtime write-watchpoint on the MAN buffer `_DAT_8007b898` (`scripts/pcsx-redux/autorun_man_source.lua`) and byte-verified against the live RAM MAN.

Each descriptor is `(type_size, data_offset)`:
- `type_size` packs `(type_byte << 24) | (size & 0x00FF_FFFF)` - the same packing the [asset-type dispatcher](asset-type.md) accepts directly.
- `data_offset` is a file-relative byte position of that descriptor's own independent LZS stream, addressed against the bundle entry's **extended on-disc footprint** (`Archive::read_entry`), *not* the TOC-indexed sub-region (`Archive::read_entry_indexed`).
  - Descriptor 0's offset is always the header end `8 + count*8`.
  - Later descriptors frequently fall past the indexed end and into the trailing-overlay sectors that the per-PROT TOC crops off - e.g. `0588_juui1.BIN`'s indexed view is 67584 B but `desc[4].data_offset` is 177413, valid against the 186368 B extended footprint.
  - `size` is the **decompressed** byte count passed to [`legaia_lzs::decompress`].

The **`Tmd` descriptor (type 2)** carries the scene's **environment geometry** - an `asset::pack` of Legaia TMDs (terrain, buildings, props) inside that descriptor's LZS stream (`town01` = 121 meshes, ≈8041 verts).

- Because the meshes are LZS-packed, a raw-only TMD scan misses them; the engine's `SceneResources` walks each entry's LZS-decompressed sections (`tmd_scan::scan_entry`) to load them, then renders the field with every TIM uploaded (`upload_all_tims`, matching the retail field loader).
- `Scene::load` fetches `SceneAssetTable` entries at their **extended footprint** (`ProtIndex::entry_bytes_extended`) so the sweep reaches the streams past the TOC-indexed end - the `opdeene` prologue's entire vignette geometry pack (72 TMDs + 51 TIMs, entry 0749) sits there, invisible to the indexed view. That 51-TIM set is not only mesh textures: one is the baked **112×32 caption strip *"It was the Seru."*** (LZS offset `0x01EC30`, two CLUT palettes for the fade), the pre-rendered image the scene renderer draws between the two narration crawls - the opening's dramatic reveal is a scene texture, not a font string (see [`cutscene.md`](../subsystems/cutscene.md#narration-playback---the-crawl-roller-fun_80037174)).
- The per-mesh world placement + mesh selection for this static geometry come from the field map file's object table (`FUN_8003a55c`; parser `legaia_asset::field_objects`, which resolves each object's `pack_index` into this pack) - see [`field-locomotion.md`](../subsystems/field-locomotion.md#object-record-format-0x0000-0x20-byte-stride); `legaia-engine play-window` renders the town from it.
- **The environment pack is not always in the bundle entry `find_bundle` returns.** A single-entry town keeps its MAN and its geometry in one `SceneAssetTable` entry (`town01` = entry 4). But a scripted **cutscene** scene splits them: `opdeene` keeps its MAN + event scripts in a `SceneScriptedAssetTable` (entry 748, which `find_bundle` returns) and its 72-TMD vignette geometry in a *separate* `SceneAssetTable` sibling (entry 749). A world-map kingdom bundle does the reverse (geometry in the `SceneScriptedAssetTable` that `find_bundle` returns, an unrelated sub-area in the sibling `SceneAssetTable`).
- So neither "the bundle entry" nor "the first `SceneAssetTable`" is universally the geometry pack. The placement `pack_index` indexes the scene-owned PROT entry that actually produced the most environment TMDs (`opdeene` 749, `town01` 4, `map01` 85); the renderer selects the env pool by that criterion. Keying it on the bundle entry left `opdeene` with zero environment meshes - the whole prologue rendered blank.
- **Some dungeon scenes carry the whole asset table *inside* their `scene_v12_table` entry**, at file offset `0x1000` - not as a first-class `SceneAssetTable` / `SceneScriptedAssetTable` sibling.
  The v12 runtime-fixup header wins the classifier at offset 0, so the standalone detector never probes `0x1000`. `find_bundle` adds a v12 fallback that scans a `SceneV12Table` entry's 0x800-aligned offsets for the first `scene_asset_table` carrying a type-3 (MAN) descriptor and returns it as `BundleSource::V12Embedded { table_offset: 0x1000 }`; extraction resolves `0x1000 + data_offset` against the extended footprint exactly as for the scripted variant.
  the v12-family dungeons (`rikuroa`, `dolk2`) have **no** MAN-bearing bundle at all: their base+3 table is the MAN-less `count=4` form, and the scene MAN is the type-3 chunk of the block's standalone `data_field_streaming` entry (`rikuroa` extraction 157, partitions `[13, 29, 64]`; `dolk2` extraction 70, `[29, 73, 17]`) - resolved by the engine's `field_man_payload` streaming fallback. (The historical "v12 entry 164 / 76 embeds the MAN at `0x1000`" reading decoded the NEXT block's bundle through the CDNAME-shifted scene window plus the extended-footprint over-read; see [`scene-v12-table.md`](scene-v12-table.md#the-embedded-man-at-0x1000-is-an-extended-footprint-over-read).) Disc-gated coverage: `crates/engine-core/tests/v12_bundle_man_disc.rs`.

Type-sequence variants (count=7 unless noted):

| Tuple | Notes |
|---|---|
| `(1, 2, 3, 4, 5, 6, 7)` | Standard count-7 bundle: `(TimList, Tmd, Man, Mes, Move, Anm, Vdf)`. |
| `(1, 3, 4, 5, 6, 7, 0x14)` | Skips Tmd; trailing `0x14 = Flag(0x14)` sentinel. |
| `(2, 3, 4, 5, 6, 7, 0x14)` | Skips TimList. |
| `(10, 2, 3, 4, 5, 6, 7)` | Leading `Flag(0xA)` sentinel. |
| `(1, 2, 3, 4, 6, 7, 0x14)` | Skips Move. |
| `(2, 3, 5, 6, 7, 0x14)` | **count-6** early-town variant (`town01`): `(Tmd, Man, Move, Anm, Vdf, Flag)`. MAN at index 1. |
| `(10, 2, 3, 5, 6, 7)` | **count-6** early-town variant (`town0c`): leading `Flag(0xA)`, MAN at index 2. |

Sizes ~60 KB to ~452 KB.

### Slot→asset mapping (the runtime walk)

The mapping is **positional + offset-based** - there is no separate indirection table; the descriptor's `data_offset` field *is* the indirection. The runtime walker `FUN_80020224` reads `count = *base`, then for each `slot` dispatches `asset_type_dispatch(base + descriptor[slot].data_offset, type_size, …)` with descriptors at `base + 8 + slot*8` (stride 8 bytes). So slot `i` is the `i`-th 8-byte descriptor; its payload starts at `base + data_offset` and its handler is keyed by `type_size >> 24`. The full three-function chain (buffer allocation at `FUN_8001E1B4` → file load at `FUN_8001F7C0` → walk at `FUN_80020224` → dispatch at `FUN_8001F05C`) is pinned under the [asset-loader subsystem](../subsystems/asset-loader.md#asset-descriptor-walker-fun_80020224--the-slotasset-mapping).

`scene_asset_table::resolve` returns the table plus the base it is relative to, covering **both** the bare variant (base 0) and the prescript-prefixed `scene_scripted_asset_table` variant (base at the post-prescript 0x800-aligned offset); `SceneAssetTable::slots` reproduces the positional walk and `payload_range(slot, base)` resolves a slot's payload span:

```rust
use legaia_asset::scene_asset_table;
if let Some(r) = scene_asset_table::resolve(buf) {
    for s in r.table.slots() {
        let span = r.table.payload_range(s.slot, r.table_base).unwrap();
        println!("slot {}: {} size={} payload@{:#x}..{:#x}",
                 s.slot, s.asset_type.name(), s.size, span.start, span.end);
    }
}
```

A disc-gated corpus test (`scene_asset_table_walk_real`) verifies this walk against every classified entry (88 bare + 79 scripted): the first slot anchors at `base + header_end` and every slot's type is a legal dispatcher type. The relocation of the loaded file into the asset buffer (`_DAT_8007b85c`) and the exact base the walker receives for the scripted variant are runtime values (capture-blocked); the static `resolve` reconstructs the base structurally.

## scene_scripted_asset_table - scripted prefix + canonical bundle

A composite shape that pairs a `[u16 count][u16 offsets[count]]` script prescript at offset 0 with a canonical 7-asset scene table at the next 0x800 sector boundary. Implementation: `crates/asset/src/scene_scripted_asset_table.rs`. ~6% of all PROT entries match.

```text
+0x00              u16  count             ; 1..=4096 - number of script records
+0x02              u16  offsets[count]    ; offsets[0] = 2 + count*2,
                                          ; monotonically non-decreasing
+offsets[i]        record               ; word-aligned (16-bit) command
                                          ; record (opener: 0xFFFF 0x0000
                                          ; header sentinel; NOT field-VM -
                                          ; see scene_event_scripts below)
+0x800-aligned     u32  count = 7         ; canonical scene-asset-table lead
...                                       ; same layout as scene_asset_table
```

Strict gate validates **both** the prescript and the inner asset table:
1. `u16[0]` is the record count (`1..=4096`).
2. `u16[1]` algebraically ties to the count: `offsets[0] = 2 + count*2`.
3. All offsets monotonic, in-bounds.
4. Past the last record offset, the next `0x800`-aligned position carries `u32 count = 7` plus a valid `scene_asset_table` header (first descriptor at `+0x40`, all type bytes `<= 0x14`).

The two-level gate is what makes this detector zero-false-positive: the prescript shape alone occasionally matches arbitrary `[count][offsets]`-shaped data, but the asset-table check at the next sector boundary is a strong second signal.

The prescript is a **per-scene move-VM stager table** (summon-stager record format), **not** field-VM (`FUN_801DE840`) bytecode - see the [scene_event_scripts](#scene_event_scripts---prescript-only) section for the full chain. Each record is `[i16 model_sel][u16 flags][move-VM bytecode]` (the `0xFFFF 0x0000` lead = `model_sel = -1` transform node); installed by the field VM via `FUN_800252EC` and run by the move VM `FUN_80023070`. The genuine per-scene field-VM scripts live in the scene MAN sub-asset.

## tmd_size_prefix - truncated TMD-prefix

Sister to `scene_tmd_stream` for the *truncated* case: same outer shape (`[u32 prefix][TMD magic at +4][zero flags][nobj]`), but the on-disc payload is **shorter than the prefix claims**. Implementation: `crates/asset/src/tmd_size_prefix.rs`. ~3% of all PROT entries match.

```text
+0x00   u32  total_size       ; claimed total in-memory size, > on-disc len
+0x04   u32  0x80000002       ; Legaia TMD magic
+0x08   u32  0x00000000       ; TMD flags (on-disc; runtime sets to 1)
+0x0C   u32  nobj             ; small (typically 2 or 4)
+0x10   object_table[nobj]    ; 28 bytes per object (PsyQ TMD layout)
+0x10 + nobj*0x1C             ; primitive data (truncated at sector boundary)
```

All object pointers (`vert_top`, `norm_top`, `prim_top`) point **within the prefix-claimed total size** - so the on-disc file is genuinely a prefix of a larger logical resource, not a malformed header.

Strict structural checks:
1. TMD magic at `+4`, flags == 0 at `+8`, `1 <= nobj <= 8`.
2. `claimed_total > buf.len()` - distinguishes from `scene_tmd_stream` which catches the complete case.
3. Object table fits on disc.
4. Each object's vert / normal / primitive ranges fit within the claimed total.

The 34 hits are all 12 KB files (6 sectors). The runtime consumer hasn't been located; likely the loader allocates `claimed_total` bytes of RAM and either (a) zero-fills the missing tail, or (b) streams the remainder from another PROT entry.

## scene_event_scripts - prescript-only

Sister of `scene_scripted_asset_table` for the case where the same `[u16 count][u16 offsets]` prescript exists at offset 0, but the post-prescript payload is **not** a canonical 7-asset table. Implementation: `crates/asset/src/scene_event_scripts.rs`. ~20 PROT entries match.

```text
+0x00              u16  count             ; 3..=4096
+0x02              u16  offsets[count]    ; offsets[0] = 2 + count*2,
                                          ; monotonically non-decreasing,
                                          ; all <= file size
+offsets[i]        record               ; word-aligned (16-bit) command
                                          ; record; the bulk open with the
                                          ; `0xFFFF 0x0000` header sentinel
                                          ; and terminate with a `0x0008` word
...                                       ; bulk asset payload after the
                                          ; prescript (per-scene secondary
                                          ; header; format unconfirmed -
                                          ; appears to be a small `(count,
                                          ; descriptor[count])` table at
                                          ; the next 0x800 boundary, with
                                          ; alternating `(type, size)` and
                                          ; runtime-buffer offset pairs)
```

Strict structural detection:
1. Prescript shape valid (count `3..=4096`, `offsets[0] == 2 + count*2`, monotonic, in-bounds).
2. **Frame-opener rate ≥ 50 %** of records start with the `0xFFFF 0x0000` record header sentinel.

The frame-opener rate is what makes this detector zero-false-positive on its own. Random `[count][offsets]`-shaped data carries no `0xFFFF` opener at the record positions; real scene-event-script bundles carry it on the majority of records (50–92 %).

**These records are NOT field-VM (`FUN_801DE840`) bytecode** (the long-standing assumption). Running the field-VM disassembler over them yields a 65–88 % decode-error rate; the bytes are 16-bit **word-aligned** (low byte = opcode, high byte 0 on ~83 % of body words), framed records terminate with a `0x0008` word, and the opcodes sit mostly below the field VM's `0x22` opcode floor - a record like `FF FF 00 00 25 00 29 00 25 00 2A 00 08 00` reads cleanly word-aligned (`cmd(0x25,0x29) cmd(0x25,0x2A) term(0x08)`) but is garbage byte-by-byte. So `0xFFFF 0x0000` is a per-record header sentinel, not a field-VM frame divider.
Record 0 on towns is a fixed 768-byte run of 8-byte spawn rows - the scene's
master ambient stager (see the consumer census below).
The records still encode per-scene structure (actor/NPC placement, event triggers,
interaction hooks). The records are **move-VM (`FUN_80023070`) records in the
summon-stager format** - `[i16 model_sel][u16 flags][move-VM bytecode]`,
byte-identical in shape to the per-summon stagers (`legaia_asset::summon_overlay`):
the `0xFFFF 0x0000` lead is `model_sel = -1` (a transform/pivot node, the dominant
kind) + `flags = 0`, and the `0x0008` terminator is move-VM opcode `0x08` (Halt).

Runtime chain: the per-scene field VM (`FUN_801DE840`) installs a record by id via
the installer **`FUN_800252EC`** (`record = bundle_base + offsets[id]`, bundle base
= `_DAT_8007b8d0` = the field scratch `_DAT_1f8003ec + 0x12800`) → the part-stager
**`FUN_80021B04`** (`actor[+0x48] = record`, `actor[+0x70] = 2` PC, tick fn
`FUN_80021DF4`) → the move VM **`FUN_80023070`** runs `record+4` each frame. So the
prescript is the *per-scene* sibling of the summon stagers - same record format,
same consumer. (Not field-VM, not a bespoke command VM, not vestigial; live
kingdom-overworld RAM shows the records resident at `_DAT_8007b8d0` with actors
executing them through the move VM.)

**Which scripts install which records (the consumer census).** Walking every
scene MAN's field-VM scripts for op-`0x34` sub-`3` (the literal-id install op)
resolves the once-open "dual consumer" question: the bundle has **one**
consumer - every record is a move-VM stager, installed by id from two script
homes. Partition-1 carries dedicated **effect-actor** records (Shift-JIS-named
"effect"; whole script = `install id N` + infinite loop) that stage the scene's
ambient effects on entry - most scenes install **record 0** this way, the
scene's *master ambient record* (on towns an 8-byte-periodic run of spawn rows,
the "768-byte dispatch table" shape - it is a stager record like the rest, not
a separate table). Partition-2 cutscene timelines install the per-shot effect
ids (one timeline installs many ids, re-installing a multi-part effect's id per
part). The id space is the record index directly - retail relocates the bundle
to RAM as a compact `[u16 count][u16 offsets[count]]` table at `_DAT_8007b8d0`
(what `FUN_800252EC` indexes: `base + 2 + id*2`), and a town01 field state
shows count = the file bundle's record count with record 0's bytes at the
first offset. There is **no field-VM consumer** of prescript bytes (the
engine's historical record-0-as-field-VM fallback has no retail counterpart).
Census + RAM pin: `engine-core/tests/scene_prescript_consumer_census_disc.rs`;
the script-side scanner is
`legaia_engine_core::man_field_scripts::scene_stager_installs`.

Pinned by the disc-gated `scene_event_records_word_aligned_real` +
`prescript_move_stager_records_real` tests (the latter: 78 entries / 1855 records,
100% valid stager-kind leads); `legaia_asset::scene_event_scripts::move_stager_records`
parses the records (as `summon_overlay::SummonPart`) and `record_words` surfaces the
raw word stream. The genuine per-scene field-VM *scripts* live in the scene MAN
sub-asset (see [`subsystems/script-vm.md`](../subsystems/script-vm.md)); this
prescript is the move-VM *stager* table those scripts spawn from.

Detection runs after `scene_scripted_asset_table` and `scene_asset_table`, so any composite layouts those detectors recognize claim their entries first.

## See also

- [Scene v12 table](scene-v12-table.md) - the per-scene runtime-fixup header + record table.
- [Field-pack](field-pack.md) - one of the bundled scene asset layouts.
- [asset::pack](pack.md) - the in-chunk pack the bundles embed.
- [`subsystems/asset-loader.md`](../subsystems/asset-loader.md) - the loader chain that resolves the bundles.
