# Scene-prefixed asset bundles

Four related shapes account for the dominant per-scene asset layouts on the disc. All of them lead with a 4-byte chunk0 header in the form `(type << 24) | size`, with `type = 0x00` - the same encoding as a [DATA_FIELD streaming](data-field.md) chunk header. The standard streaming walker would interpret `type=0x00` as the TIM dispatcher slot; specialised loaders in the runtime know to dispatch chunk0 differently based on the *content* magic at offset +4.

## scene_tmd_stream - bare-TMD prefix

The dominant scene-asset layout. Implementation: `crates/asset/src/scene_tmd_stream.rs`. ~12% of all PROT entries match.

```text
+0x00          u32 chunk0_header   ; (type=0x00 << 24) | size
+0x04          Legaia TMD          ; magic 0x80000002, fills `size` bytes
+0x04 + size   streaming chunks    ; standard DATA_FIELD chunks until terminator
                                   ; OR end-of-file
```

Strict structural detection:
1. `buf.len() >= 32`.
2. `buf[4..8] == 0x80000002` (Legaia TMD magic).
3. `buf[8..12] == 0` (TMD on-disc flags; runtime sets to 1 after pointer fixup).
4. `buf[12..16]` = `nobj`, `1 <= nobj <= 64`.
5. Chunk0 header at `buf[0..4]` has type byte 0.
6. TMD body size (low 24 bits of chunk0 header) is 4-aligned and at least `12 + nobj * 28`; `4 + size <= buf.len()`.
7. Streaming tail at offset `4 + tmd_size` walks at least one valid chunk header OR a terminator.

Reading:

```rust
use legaia_asset::scene_tmd_stream;
if let Some(s) = scene_tmd_stream::detect(&buf) {
    let tmd = legaia_tmd::parse(&buf[s.tmd_range()])?;  // bare TMD, no wrapper
    for chunk in &s.tail_chunks {
        // each chunk is (type, size, payload) per data-field.md
    }
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

## scene_v12_table - twin-offset header

A scene-named table whose 8-word header carries three constant magic words and two algebraic ties to a record count. Implementation: `crates/asset/src/scene_v12_table.rs`. 97 PROT entries match - one per scene.

```text
+0x00   u16  N + 4          ; first offset table base = N + 4
+0x02   u16  0x0012         ; constant magic
+0x04   u16  0x0000         ; constant
+0x06   u16  0x0014         ; constant - second-table base offset *header*
+0x08   u16  ?              ; per-scene parameter (varies; semantics unknown)
+0x0A   u16  N              ; record count for the first table
+0x0C   u16  0x0000         ; constant
+0x0E   u16  N + 2          ; second offset table base = N + 2
...                          ; trailing dense data (>96% nonzero past 0x2000)
```

Strict structural checks: the three constant words at fixed offsets, plus `u16[0] == N + 4` and `u16[7] == N + 2` (algebraic ties to `u16[5] = N`), plus `8 <= N <= 4096`. The constants alone are nearly enough; the algebraic ties produce zero false positives across the entire PROT corpus.

Sizes range from ~30 KB to ~387 KB; median ~270 KB. Density >96% nonzero past offset `0x2000` - the leading 8 KB is the structured header / offset tables, the rest is dense payload data.

The runtime consumer hasn't been located. Likely candidates: per-scene navmesh / collision data, scene-event trigger tables. The class name reflects the structural signature, not a guessed semantic; it should change once the consumer is reversed.

[`ghidra/scripts/find_scene_v12_consumers.py`](../../ghidra/scripts/find_scene_v12_consumers.py) is the consumer-search complement: it walks every captured program for `lh` / `lhu` instructions at `+2` and `+6` immediate offsets - the offsets where the header's two constant magic words live. Functions that touch both offsets are high-confidence candidates for the v12 reader. Run with `-process` once per overlay; group hits cluster inside the consumer.

## scene_asset_table - canonical 7-asset bundle

The on-disc form of the scene asset table that the field loader reads when entering a town/dungeon. Implementation: `crates/asset/src/scene_asset_table.rs`. 80 PROT entries match.

```text
+0x00   u32  count = 7              ; literal `07 00 00 00`
+0x04   u32  meta1                  ; varies - purpose unknown
+0x08   7 × (u32 type_size, u32 data_offset)
                                    ; each pair packs `(type<<24)|size`
+0x40   asset payload region        ; LZS-compressed in some entries,
                                    ; raw in the rest
```

Each descriptor is `(type_size, data_offset)`:
- `type_size` packs `(type_byte << 24) | (size & 0x00FF_FFFF)` - the same packing the [asset-type dispatcher](asset-type.md) accepts directly.
- `data_offset` for descriptor 0 is a file-relative byte offset (always `0x40`). For descriptors 1..6, it's a **runtime-buffer offset** within the loader's working RAM, *not* a file-relative offset. Many real entries have `data_offset > file_size` for descriptors past the first; the loader presumably decompresses the payload region into a working buffer and resolves the descriptor offsets there.

Type-sequence variants found across the 80 entries:

| Tuple | Count | Notes |
|---|---|---|
| `(1, 2, 3, 4, 5, 6, 7)` | 67 | Standard scene bundle: `(TimList, Tmd, Man, Mes, Move, Anm, Vdf)`. |
| `(1, 3, 4, 5, 6, 7, 0x14)` | 7 | Skips Tmd; trailing `0x14 = Flag(0x14)` sentinel. |
| `(2, 3, 4, 5, 6, 7, 0x14)` | 4 | Skips TimList. |
| `(10, 2, 3, 4, 5, 6, 7)` | 1 | Leading `Flag(0xA)` sentinel. |
| `(1, 2, 3, 4, 6, 7, 0x14)` | 1 | Skips Move. |

All scene-named (`izumi`, `cave01`, `bylon`, `dolk`, `vell`, `urudre1`, `chitei2`, `nilboa`, `keikoku`, `concnow`, `jouina/b/c/e`, …). Sizes ~60 KB to ~452 KB.

Reading:

```rust
use legaia_asset::scene_asset_table;
if let Some(t) = scene_asset_table::detect(buf) {
    for (i, d) in t.descriptors.iter().enumerate() {
        println!("desc[{}]: type={:#04x} size={} off={:#x}",
                 i, d.type_byte, d.size, d.data_offset);
    }
}
```

The runtime consumer is the field-loader chain documented under the [asset-loader subsystem](../subsystems/asset-loader.md): `FUN_8001F7C0` + `FUN_800255B8` plus the dispatcher at `FUN_8001F05C` consumes each descriptor pair after LZS-decoding the payload region into a working buffer.

## scene_scripted_asset_table - scripted prefix + canonical bundle

A composite shape that pairs a `[u16 count][u16 offsets[count]]` script prescript at offset 0 with a canonical 7-asset scene table at the next 0x800 sector boundary. Implementation: `crates/asset/src/scene_scripted_asset_table.rs`. ~6% of all PROT entries match.

```text
+0x00              u16  count             ; 1..=4096 - number of script records
+0x02              u16  offsets[count]    ; offsets[0] = 2 + count*2,
                                          ; monotonically non-decreasing
+offsets[i]        record bytecode        ; per-record opcodes (typical
                                          ; opener: 0xFFFF 0x0000 sentinel +
                                          ; field-VM-shaped frame ops)
+0x800-aligned     u32  count = 7         ; canonical scene-asset-table lead
...                                       ; same layout as scene_asset_table
```

Strict gate validates **both** the prescript and the inner asset table:
1. `u16[0]` is the record count (`1..=4096`).
2. `u16[1]` algebraically ties to the count: `offsets[0] = 2 + count*2`.
3. All offsets monotonic, in-bounds.
4. Past the last record offset, the next `0x800`-aligned position carries `u32 count = 7` plus a valid `scene_asset_table` header (first descriptor at `+0x40`, all type bytes `<= 0x14`).

The two-level gate is what makes this detector zero-false-positive: the prescript shape alone occasionally matches arbitrary `[count][offsets]`-shaped data, but the asset-table check at the next sector boundary is a strong second signal.

The prescript is plausibly the **scene event-script bytecode** that the field VM (`FUN_801DE840`) executes when the scene loads. The 0xFFFF 0x0000 sentinels at record starts strongly resemble field-VM frame-divider opcodes. The runtime is presumed to walk the prescript first (loading scene-specific event scripts), then load the asset bundle from the sector-aligned position. The exact prescript opcode set is unconfirmed pending more reverse work.

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
+offsets[i]        record bytecode        ; per-record opcodes; the bulk
                                          ; of records open with the
                                          ; field-VM frame sentinel
                                          ; `0xFFFF 0x0000`
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
2. **Frame-opener rate ≥ 50 %** of records start with the field-VM `0xFFFF 0x0000` sentinel.

The frame-opener rate is what makes this detector zero-false-positive on its own. Random `[count][offsets]`-shaped data carries no `0xFFFF` opener at the record positions; real scene-event-script bundles carry it on the majority of records (50–92 %).

The prescript records are field-VM (`FUN_801DE840`) event scripts - the same per-frame bytecode shape used by `scene_scripted_asset_table` (`0xFFFF 0x0000` is the field VM's frame divider opcode). Records likely encode: scene-enter triggers, NPC dialogue scripts, cut-scene sequences, pickup / interaction scripts. The per-scene asset payload that follows is loaded by these scripts at runtime.

Detection runs after `scene_scripted_asset_table` and `scene_asset_table`, so any composite layouts those detectors recognize claim their entries first.
