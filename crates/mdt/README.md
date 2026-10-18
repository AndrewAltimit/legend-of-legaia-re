# legaia-mdt

The runtime buffer retail labels "MOVE" (`_DAT_8007b888`): despite the
label, it is the scene's ANM **animation clip bank**, not move-VM bytecode
and not a Tactical Arts table
([`docs/formats/mdt.md`](../../docs/formats/mdt.md),
[`docs/formats/anm.md`](../../docs/formats/anm.md)).

## Where the data lives

The asset dispatcher (`FUN_8001f05c` case `0x05`, `s_move_malloc_err`)
allocates it from the scene bundle's type-`0x05` descriptor, or from a
DATA_FIELD stream's type-`0x05` chunk (`rikuroa`). The streamed
`move.mdt` path (`FUN_8002541c` mode `0x0F`) is reached by no retail
bundle ([`field-pack.md`](../../docs/formats/field-pack.md)). The MOVE2
sibling (`_DAT_8007b840`, dispatcher case `0x0B`) serves ids >= `0x400`.

## What the consumer expects

`FUN_800204f8` is the sole runtime reader. Per its disassembly:

```text
masked_id = move_id & 0x3FF                  // 10-bit id space
buf       = (flags & 0x01000000) ? alt_table : (move_id < 0x400 ? MOVE : MOVE2)
record    = buf + *(u32*)(buf + masked_id*4)

record[0]      - bone count
record[1] & 1  - "use frame divisor" flag
record[2..3] u16 - max-position * 16 (clamps animation playhead)
record[4..5] u16 - reserved
record[6]      u8  - frame divisor (only when flag bit set)
record[8..]    - per-frame bone entries, 8 bytes per object per frame
```

So the on-disc layout the consumer expects is:

```text
u32 offset_table[1024]   // indexed by (move_id & 0x3FF)
u8  records[]            // each at the offset given by the table
```

## What's actually in the named PROT entries

`0972_move_program_no.BIN` (45056 B) and `0973_move_program_no.BIN`
(47104 B) are CDNAME-named "move_program_no" but their byte layout does
**not** cleanly match the consumer-derived offset-table format. The
filename label is the [+2 numbering shift](../../docs/formats/cdname.md#numbering-space):
0972 is the fishing minigame overlay and 0973 the 1-sector `OTHER2` dev
module, so a "flat 128-byte record" reading of them is a loose parse of
overlay code ([`mdt.md`](../../docs/formats/mdt.md)). `mdt classify` reflects this: on these entries only the relaxed
move-buffer predicate matches, the strict offset-table fitness score is
negative, and the verdict is printed with a `(LOW CONFIDENCE)` tag plus a
note telling you to cross-check with `mdt records`.

## Not the move VM

The move VM (`FUN_80023070`, 71 opcodes) shares the "move" label and
nothing else: it does not read this buffer. See
[`docs/subsystems/move-vm.md`](../../docs/subsystems/move-vm.md).

## CLI

```bash
mdt classify <file>    # offset-table form vs fixed-record form
mdt records  <file>    # walk the fixed-record layout
mdt slots    <file>    # walk the offset-table layout
```

## See also

- [`docs/formats/mdt.md`](../../docs/formats/mdt.md)
- [`docs/subsystems/move-vm.md`](../../docs/subsystems/move-vm.md)
