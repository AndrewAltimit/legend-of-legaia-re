# MAN relocation (variable-length destination editing)

How to **resize** a field-VM `0x3F` named-scene-change ("door / exit")
destination in a decompressed scene **MAN** and rebuild the buffer so every
internal offset stays valid. This is what lets the [randomizer](../tooling/randomizer.md)
re-point a door at a scene with a differently-sized name. Engine: `legaia_asset::man_edit`.

## The destination table is the partition-2 record-offset table

A `0x3F` op carries its destination inline:

```text
0x3F [i16 index][u8 name_len][name_len name bytes][entry_x][entry_z][dir]
```

The ops are **partition-2 MAN records** (the named cutscene-timeline-style
records - see [encounter.md](encounter.md) for the MAN layout, including the
`[u8 name_len][name*2 SJIS][cond-blocks]` partition-2 header). They are reached
at runtime through the **partition-2 record-offset table**: on a transition the
field controller sets the VM bytecode base to
`man_base + data_region + partition2[slot]` and runs that record's small script
by fall-through (`SysFlag.Set; CFlag.Set; EFFECT; yield; 0x3F; entry-trailer;
JmpRel-1 self-loop`), whose `0x3F` op warps by the inline name. Selection is by
stable **slot index**, so the op's `index` field is only the destination-scene id
passed to the warp packet (`FUN_8001FD44`), not a record selector.

This was pinned by a live PCSX-Redux dispatch trace (`autorun_door_dispatch_trace.lua`
on the `drake_castle_to_worldmap` capture): the executing op's bytecode base
minus the MAN base equalled `data_region + partition2[0]` exactly. Corpus census
(clean partition walk, disc-wide): 160 destination ops across 48 scenes, 153 in
partition 2; **zero absolute-reference ops** at/after any destination op.

The practical consequence: the destination "index" *is* a structural offset
table the MAN parser already exposes, so resizing a record is safe - fix the
table, and every door stays addressable.

Note the table is **flat** across the three partitions (`[P0..P1..P2]`), and it
is indexed that way by the runtime resolver `FUN_8003C8F0` - but each partition
prefixes its record with a *different* header, so a record's script start is not
a function of the index alone. The per-partition header table is in
[`script-vm.md`](../subsystems/script-vm.md#record-headers-are-per-partition-the-record-index-space-is-flat).

## Relocation surface

A mid-buffer insert/delete of `delta` bytes at a destination op needs these
fixups (all offsets per [`man_section`](../../crates/asset/src/man_section.rs)):

1. **Partition record-offset tables** (`MAN[0x2B..]`, u24LE entries relative to
   `data_region_offset`): bump every entry whose record starts after the edit by
   `delta`. This table *is* the door dispatch index.
2. **`u24_at_28`** (header `MAN[0x28..0x2B]`, u24LE section-0 offset relative to
   `data_region`): the section chain sits after the records, so it shifts by the
   total delta.
3. **Intra-record relative jumps** in the edited record whose source/target
   straddle the edit: `field_disasm` `0x26 JMP_REL`, `0x42 COND_JMP`,
   `0x4D BBOX_TEST`, `0x4E` conditional sub-ops, `0x70` SystemFlag-test. The
   stored u16 delta is recomputed; a jump wholly on one side is unaffected (its
   endpoints shift together). The delta field sits exactly at the jump's relative
   base (`target - delta`), so the rewrite is op-agnostic. (37 of 160 corpus ops
   have such a jump.)
4. **External descriptor** (`scene_asset_table`): the MAN's *decompressed* size
   is stored only in the scene-bundle descriptor word `(type<<24)|size`. Rewrite
   it with `scene_asset_table::encode_size_word` after recompressing.

`data_region_offset` is derived (`0x2B + 3*total_records`) and does not move.

## Safety

- **Absolute-reference ops** (`0x45 0xC0` camera-apply, `0x4E` abs-jump) are
  record-local and can't be relocated blindly; `apply_dest_edits` errors out if
  it finds one in an edited record (none exist in the retail corpus), and the
  caller leaves that scene unchanged.
- **Validate-or-skip**: `man_edit::validate` re-parses + re-walks the rebuilt MAN
  and confirms each edited op now decodes as a `0x3F` carrying the intended name.
- **Footprint**: the recompressed MAN must fit the original asset's on-disc
  footprint (the gap to the next descriptor). A scene that can't grow in place -
  the big overworld hubs, whose next asset is flush after the MAN - is skipped
  and reported rather than relocating the whole bundle.

## Generalized interior-text growth

The same relocation machinery generalizes from a door's destination *name* to an
arbitrary interior byte run - the dialog-segment text a localization grows or
shrinks. `man_edit::apply_text_edits` takes a set of `TextEdit { offset, old_len,
new_bytes }` (replace the `old_len` bytes at `offset` with `new_bytes` of any
length) and rebuilds the MAN through the identical `rebuild_man` path: partition
record-offset tables, `u24_at_28`, and straddling intra-record relative jumps are
all fixed, and the external descriptor decompressed-size word is the caller's to
rewrite after recompressing.

A dialog segment is a field-VM `0x49 0x00` message op whose text run is framed
`0x1F <text> 0x00`; the decoder recovers the op's width by walking the message
bytes to the `<= 0x1E` terminator, so a grown run keeps the fall-through decode
in sync and every relative jump after it is still found + relocated.

Two invariants keep this safe:

1. **Record-region gate.** Every edit must lie strictly before section 0 (the
   record region) inside a partition record with no absolute-reference op. Dialog
   is field-VM script = partition records, never a data section, so an edit that
   would touch the section chain (whose length prefixes this pass does not fix)
   or an abs-jump / camera-apply target is refused.
2. **Round-trip backstop.** `man_edit::text_edits_preserve_scripts(original,
   rebuilt)` re-walks every record in both buffers and requires an identical
   instruction stream - same opcodes in order, every control-flow target
   resolving to the same instruction ordinal. A mis-relocated jump diverges the
   ordinal and is caught; the caller drops the growth and falls back to same-size
   abbreviation for that scene.

**Budget = the MAN's on-disc footprint.** The recompressed grown MAN must still
fit the gap from its compressed stream to the next asset descriptor (same LBA, no
disc relayout). Across the retail USA disc the scene-bundle PROT entries are
**sector-aligned with zero padding**, and each compressed MAN already fills its
footprint, so in-place growth fits a wordier localization only when the rewritten
MAN recompresses no larger than the original - a small fraction of scenes. The
rest are residual sector-crossers whose deficit is under one 2048-byte sector;
supplying that sector is a disc-level relayout (what the PAL discs did at
mastering - see [pal-localizations.md](../tooling/pal-localizations.md)).

## See also

- [Randomizer](../tooling/randomizer.md) - the door feature this enables.
- [PAL localizations](../tooling/pal-localizations.md) - the dialog-growth consumer + fit rate.
- [encounter.md](encounter.md) - the MAN multi-section layout.
- [`field_disasm`](../subsystems/script-vm.md) - the field-VM opcode decoder the
  jump fixup uses.
