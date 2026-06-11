# MDT - move tables (Tactical Arts)

The runtime buffer format consumed by `FUN_800204F8` (Tactical Arts move-table consumer; the same function the [script VM](../subsystems/script-vm.md) opcode `0x22` `EXEC_MOVE` invokes). Implementation: `crates/mdt`.

The per-frame data inside each MDT record is **bytecode for the move VM** - see [`subsystems/move-vm.md`](../subsystems/move-vm.md) for the 71-opcode dispatcher (`FUN_80023070`) that walks it.

## Layout the consumer reads

```
buf                           ← _DAT_8007B888 (MOVE) or _DAT_8007B840 (MOVE2)
+0x000  u32 offset_table[1024]   ; indexed by (move_id & 0x3FF)
        ; entry == 0 means "no record for this id"
        ; otherwise entry is a byte offset into `buf`

at offset_table[id]:
record:
  +0x00  u8  reserved
  +0x01  u8  flags                 ; bit 0 = "use frame divisor"
  +0x02  u16 max_position_x16      ; clamps the playhead at (this * 16) - 1
  +0x04  u16 reserved
  +0x06  u8  divisor               ; only consulted when flags & 1
  +0x07+ per-frame data            ; size = max_position_x16 * 16 (approx)
```

Routing: if actor flag bit `0x01000000` is set, use the alternate base `_DAT_8007B75C`. Otherwise use `MOVE` for `move_id < 0x400` and `MOVE2` for `move_id >= 0x400`.

The per-frame interpretation in `FUN_800204F8` clamps `actor[0x68]` (current playhead) to `[0, max_position_x16 * 16)`, advances by `actor[0x6A]` (frame delta) optionally divided by `record[6]`, and reads the per-frame data into the per-actor animation state.

## The `move_program_no` name (dissolved mismatch)

The extraction files *named* `0972` / `0973` `move_program_no.BIN` are not move tables at all — under the [+2 filename-numbering shift](cdname.md#numbering-space) they sit in the `other_game` block: 0972 is the **fishing minigame overlay** (dev `other1`) and 0973 is the 1-sector `OTHER2` dev module. The `move_program_no` define actually covers **extraction 0970..0971** — a `\DATA\MOV*.STR` FMV program/path table + debug strings (the block names **MOV**ie program numbers, see [str-fmv-table.md](str-fmv-table.md)), not Tactical-Arts moves. The historical "flat 128-byte record array" reading of 0972/0973 was a loose parse of overlay code/data.

`crates/mdt` parses both layouts and surfaces a verdict (`OffsetTableLayout` / `FlatRecordTable` / `Unknown`); `mdt classify` correctly reports that neither file matches the runtime buffer layout above — expected, since they aren't move data.

### Caveat: `MoveBuffer::parse` over-reads past the real table boundary

Real per-scene Move buffers have offset tables shorter than the consumer-facing 1024-entry mask (most use 8-30 ids) and pack record data densely past the real table end.

- `MoveBuffer::parse` keeps reading u32s past the real boundary, where record bytes masquerade as offsets.
- Most of those over-read entries point past the buffer end and get counted as `bogus_offsets`, so the strict `MoveBuffer::fitness()` score (`used - 2*bogus`) is strongly negative for valid retail data (e.g. `0086_map01.BIN`: used=1020 bogus=973 → fitness=-926).
- Use `MoveBuffer::looks_like_move_buffer()` instead: it requires `records.len() > 0 && used > bogus`, which 75/79 retail per-scene Move buffers pass while random / non-Move data still fails.

`classify()`'s `OffsetTableLayout` verdict also routes through `looks_like_move_buffer`, so the CLI reports the same shape the engine accepts.

## On-disc source - per-scene `scene_asset_table` slot 4

The MOVE base pointer (`_DAT_8007B888`) is **populated per scene** during area transitions, not from a single boot-time PROT entry. Every per-scene CDNAME block's second PROT entry (the slot-1 entry classified as `scene_asset_table`) carries an `Asset(0x05) = Move` descriptor - that descriptor's payload is the runtime MOVE table for that scene.

```text
PROT entry at scene_block + 1            ← class = scene_asset_table
  u32 count = 7
  u32 meta1
  7 × (u32 type_size, u32 data_offset)   ← descriptor[4].type_byte == 0x05 (Move)
  ...payload...
```

Examples (verified by mednafen save-state diff against `_DAT_8007B888`):

| Scene block | Slot-1 PROT entry | Move size | Notes |
|---|---|---|---|
| `dolk` (60) | `0061_dolk.BIN` | `0xE370` (58224) | Loaded as `MOVE` at `0x800E412C` (Drake Castle save). |
| `suimon` (77) | `0078_suimon.BIN` | `0x09A0` (2464) | Loaded as `MOVE` at `0x801355D0` (Suimon-block saves). |
| `map01` (85) | `0086_map01.BIN` | `0x7E30` (32304) | Loaded as `MOVE` at `0x8011A624` (every `map01`-resident save, including the menu and battle states layered on top of `map01`). |

The `meta1` u32 in the scene_asset_table header is the per-scene meta value the loader carries forward.

- Each descriptor (including `desc[4]` = Move) is its own independently LZS-compressed stream at `data_offset` bytes into the bundle entry's **extended on-disc footprint** (`Archive::read_entry`), decompressing to exactly `size` bytes.
- Several scenes have Move descriptor offsets that fall past the TOC-indexed end and into trailing-overlay sectors - readers must use the extended footprint (or `ProtIndex::entry_bytes_extended`) rather than `Archive::read_entry_indexed`.
- See [`engine-core::scene_bundle::extract_move_payload`](../subsystems/engine.md) for the canonical pattern.

`scene_asset_table::move_descriptor` exposes the slot lookup as a typed accessor:

```rust
let s = legaia_asset::scene_asset_table::detect(&prot_bytes)?;
let move_descriptor = s.move_descriptor()?; // type_byte = 0x05
```

The `MOVE2` (`_DAT_8007B840`) base is zero across every observed save state, suggesting it's only populated by a small number of scenes that need an alternate move table; the analogous "Move2" descriptor type in [`scene_scripted_asset_table`](../formats/scene-bundles.md) hasn't been observed in the corpus yet.

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
