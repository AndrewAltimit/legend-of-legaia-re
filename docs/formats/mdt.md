# MDT - move tables (Tactical Arts)

A move table is a per-scene buffer of **move records**: short bytecode programs that pose and animate an actor, addressed by a move id. The buffer opens with a table of byte offsets, one per id, and each record is a small header followed by per-frame data. The per-frame data is bytecode for the [move VM](../subsystems/move-vm.md) (`FUN_80023070`, 71 opcodes).

The consumer is `FUN_800204F8`, the same function the [script VM](../subsystems/script-vm.md) opcode `0x22` `EXEC_MOVE` invokes. Implementation: [`crates/mdt`](../../crates/mdt/README.md).

## Layout the consumer reads

```
buf  (MOVE: _DAT_8007B888, MOVE2: _DAT_8007B840)
+--------------------------------+--------------------------------------------+
| u32 offset_table[]             | records, densely packed                    |
| indexed by move_id & 0x3FF     |                                            |
+--------------------------------+--------------------------------------------+
        |  offset_table[id] = byte offset into buf  (0 = no record)
        v
      record:  [reserved][flags][max_position_x16][reserved][divisor][per-frame data ...]
```

| Offset | Size | Field | Meaning | Confidence |
|---|---|---|---|---|
| `buf + id*4` | u32 | `offset_table[id]` | Byte offset of the record for `move_id & 0x3FF`; `0` means no record | Confirmed |
| `rec + 0x00` | u8 | reserved | | Unknown |
| `rec + 0x01` | u8 | `flags` | Bit 0 = use the frame divisor | Confirmed |
| `rec + 0x02` | u16 | `max_position_x16` | Clamps the playhead at `(this * 16) - 1` | Confirmed |
| `rec + 0x04` | u16 | reserved | | Unknown |
| `rec + 0x06` | u8 | `divisor` | Consulted only when `flags & 1` | Confirmed |
| `rec + 0x07` | about `max_position_x16 * 16` | per-frame data | Move-VM bytecode | Inferred (size) |

The consumer masks the id to 10 bits, so the table is *addressable* up to 1024 entries. Real tables are much shorter - see the caveat below.

Routing: if actor flag bit `0x01000000` is set, the base is `_DAT_8007B75C`. Otherwise `MOVE` serves `move_id < 0x400` and `MOVE2` serves `move_id >= 0x400`.

Per frame, `FUN_800204F8` clamps `actor[0x68]` (the playhead) to `[0, max_position_x16 * 16)`, advances it by `actor[0x6A]` (frame delta), optionally divided by `record[6]`, and reads the per-frame data into the per-actor animation state.

## The `move_program_no` name is not this format

The CDNAME define `move_program_no` covers **extraction 0970..0971**: a `\DATA\MOV*.STR` FMV program/path table plus debug strings. It names **MOV**ie program numbers ([str-fmv-table.md](str-fmv-table.md)), not Tactical-Arts moves.

The extraction files *labelled* `0972` / `0973` `move_program_no.BIN` are not move tables either. Under the [+2 filename-numbering shift](cdname.md#numbering-space) they sit in the `other_game` block: 0972 is the **fishing minigame overlay** (dev `other1`) and 0973 is the 1-sector `OTHER2` dev module. A "flat 128-byte record array" reading of them is a loose parse of overlay code and data.

`crates/mdt` parses both layouts and reports a verdict (`OffsetTableLayout` / `FlatRecordTable` / `Unknown`); `mdt classify` reports that neither file matches the runtime buffer layout above.

### Caveat: `MoveBuffer::parse` over-reads past the real table boundary

Real per-scene Move buffers have offset tables shorter than the consumer-facing 1024-entry mask (most use 8-30 ids) and pack record data densely past the real table end.

- `MoveBuffer::parse` keeps reading u32s past the real boundary, where record bytes masquerade as offsets.
- Most of those over-read entries point past the buffer end and get counted as `bogus_offsets`, so the strict `MoveBuffer::fitness()` score (`used - 2*bogus`) is strongly negative for valid retail data (e.g. `0086_map01.BIN`: used=1020 bogus=973 → fitness=-926).
- Use `MoveBuffer::looks_like_move_buffer()` instead: it requires `records.len() > 0 && used > bogus`, which 75/79 retail per-scene Move buffers pass while random / non-Move data still fails.

`classify()`'s `OffsetTableLayout` verdict also routes through `looks_like_move_buffer`, so the CLI reports the same shape the engine accepts.

## On-disc source - per-scene `scene_asset_table` slot 4

The MOVE base pointer (`_DAT_8007B888`) is **populated per scene** during area transitions, not from a single boot-time PROT entry. Each scene's `scene_asset_table` entry carries an `Asset(0x05) = Move` descriptor, and that descriptor's payload is the runtime MOVE table for the scene. The table is extraction entry `define + 1` - the entry whose filename label is the block's second, and retail slot 3 of the block under the [+2 numbering shift](cdname.md#numbering-space).

```text
extraction entry (scene define + 1)      ← class = scene_asset_table
  u32 count = 7
  u32 meta1
  7 × (u32 type_size, u32 data_offset)   ← descriptor[4].type_byte == 0x05 (Move)
  ...payload...
```

Examples (verified by mednafen save-state diff against `_DAT_8007B888`):

| Scene block (define) | `scene_asset_table` entry | Move size | Notes |
|---|---|---|---|
| `dolk` (60) | `0061_dolk.BIN` | `0xE370` (58224) | Loaded as `MOVE` at `0x800E412C` (Drake Castle save). |
| `suimon` (77) | `0078_suimon.BIN` | `0x09A0` (2464) | Loaded as `MOVE` at `0x801355D0` (Suimon-block saves). |
| `map01` (85) | `0086_map01.BIN` | `0x7E30` (32304) | Loaded as `MOVE` at `0x8011A624` (every `map01`-resident save, including the menu and battle states layered on top of `map01`). |

The `+0x04` u32 in the scene_asset_table header is the sum of the descriptors' decompressed sizes; retail never reads it (`FUN_80020224` reads `+0x00` then steps from `+0x08`) - see [scene-bundles.md](scene-bundles.md#scene_asset_table---count-prefixed-asset-bundle).

- Each descriptor (including `desc[4]` = Move) is its own independently LZS-compressed stream at `data_offset` bytes into the bundle entry (`Archive::read_entry`), decompressing to exactly `size` bytes. The payload lies inside the entry's own sectors - an entry's size is the sector gap to the next TOC entry ([prot.md](prot.md)).
- See [`engine-core::scene_bundle::extract_move_payload`](../subsystems/engine.md) for the canonical pattern.

`scene_asset_table::move_descriptor` exposes the slot lookup as a typed accessor:

```rust
let s = legaia_asset::scene_asset_table::detect(&prot_bytes)?;
let move_descriptor = s.move_descriptor()?; // type_byte = 0x05
```

The `MOVE2` (`_DAT_8007B840`) base is zero across every observed save state, and no Move2 payload appears in a retail [scene bundle](scene-bundles.md) (Inferred: only a few scenes, if any, populate it). The `scene_scripted_asset_table` class is not a carrier; it is an entry-size over-read ([prot.md](prot.md#the-prescript-prefixed-asset-table-was-an-over-read)).

## CLI

```
mdt classify <PATH>                        # which layout?
mdt records  <PATH> --limit 8              # decode as flat record table
mdt slots    <PATH> --limit 8              # decode as offset-table layout
```

## See also

- [`subsystems/move-vm.md`](../subsystems/move-vm.md) - the move-table opcode VM that runs these bytecode streams.
- [Art records](art-data.md) - the per-character art-record layer above the move tables.
- [`subsystems/battle-action.md`](../subsystems/battle-action.md) - the battle action state machine that drives the moves.
