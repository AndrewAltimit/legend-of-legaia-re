# MDT — move tables (Tactical Arts)

The runtime buffer format consumed by `FUN_800204F8` (Tactical Arts move-table consumer; the same function the [script VM](../subsystems/script-vm.md) opcode `0x22` `EXEC_MOVE` invokes). Implementation: `crates/mdt`.

The per-frame data inside each MDT record is **bytecode for the move VM** — see [`subsystems/move-vm.md`](../subsystems/move-vm.md) for the 71-opcode dispatcher (`FUN_80023070`) that walks it.

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

## CDNAME mismatch

The CDNAME-named `0972` / `0973` `move_program_no.BIN` files are flat 128-byte stride record arrays — they **don't** match the runtime buffer layout above. `mdt classify` flags this. The actual on-disc source of `MOVE` / `MOVE2` is a different PROT entry whose runtime PROT-index is held in `_DAT_80084540`; pinning that down requires a memory-write watchpoint on `_DAT_8007B888`.

`crates/mdt` parses both layouts and surfaces a verdict (`OffsetTableLayout` / `FlatRecordTable` / `Unknown`).

## CLI

```
mdt classify <PATH>                        # which layout?
mdt records  <PATH> --limit 8              # decode as flat record table
mdt slots    <PATH> --limit 8              # decode as offset-table layout
```
