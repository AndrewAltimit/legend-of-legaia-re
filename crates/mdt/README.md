# legaia-mdt

`move.mdt` - the runtime "move" buffer that holds character animation
and attack-move data (Tactical Arts).

## Where the data lives

In retail the file is loaded by `FUN_8002541c` case `0x0F` ("move.mdt")
via `FUN_800255b8`, which reads the PROT entry indexed by
`_DAT_80084540 + 4` into `_DAT_8007b85c` and raw-copies it into a
freshly-allocated buffer at `_DAT_8007b888`. The asset dispatcher
(`FUN_8001f05c` case `0x05`, `s_move_malloc_err`) writes the same global
when it fires, but no streaming chunk of type 5 exists in retail - that
branch is never taken in practice. The MOVE2 sibling
(`_DAT_8007b840`, dispatcher case `0x0B`) holds moves with id > `0x3FF`.

## What the consumer expects

`FUN_800204f8` is the sole runtime reader. Per its disassembly:

```text
masked_id = move_id & 0x3FF                  // 10-bit id space
buf       = (flags & 0x01000000) ? alt_table : (move_id < 0x400 ? MOVE : MOVE2)
record    = buf + *(u32*)(buf + masked_id*4)

record[0..1]   - reserved/unknown
record[1] & 1  - "use frame divisor" flag
record[2..3] u16 - max-position * 16 (clamps animation playhead)
record[4..5] u16 - reserved
record[6]      u8  - frame divisor (only when flag bit set)
record[7..]    - per-frame data (size determined by record[2..3])
```

So the on-disc layout the consumer expects is:

```text
u32 offset_table[1024]   // indexed by (move_id & 0x3FF)
u8  records[]            // each at the offset given by the table
```

## What's actually in the named PROT entries

`0972_move_program_no.BIN` (24576 B) and `0973_move_program_no.BIN`
(47104 B) are CDNAME-named "move_program_no" but their byte layout does
**not** match the consumer-derived offset-table format. They look like
a flat array of fixed 128-byte records. The CDNAME label is misleading
(same gotcha as `vab_01`); the real `move.mdt` is loaded by string-path
elsewhere.

## What the move VM does

Per-frame, `FUN_80021df4` (the actor tick) calls `FUN_80023070`, the
71-opcode move-table VM (jump table at `0x80010778`). Opcode `0x2F`
escapes to `FUN_801D362C` in the field overlay, which runs a 61-sub-op
extension VM (jump table at `0x801CE868`). See
[`docs/subsystems/move-vm.md`](../../docs/subsystems/move-vm.md) and
[`ghidra/scripts/funcs/80023070.txt`](../../ghidra/scripts/funcs/80023070.txt).

## CLI

```bash
mdt classify <file>    # offset-table form vs fixed-record form
mdt records  <file>    # walk the fixed-record layout
mdt slots    <file>    # walk the offset-table layout
```

## See also

- [`docs/formats/mdt.md`](../../docs/formats/mdt.md)
- [`docs/subsystems/move-vm.md`](../../docs/subsystems/move-vm.md)
