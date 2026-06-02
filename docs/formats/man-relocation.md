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
records — see [encounter.md](encounter.md) for the MAN layout, including the
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
table the MAN parser already exposes, so resizing a record is safe — fix the
table, and every door stays addressable.

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
  footprint (the gap to the next descriptor). A scene that can't grow in place —
  the big overworld hubs, whose next asset is flush after the MAN — is skipped
  and reported rather than relocating the whole bundle.

## See also

- [Randomizer](../tooling/randomizer.md) — the door feature this enables.
- [encounter.md](encounter.md) — the MAN multi-section layout.
- [`field_disasm`](../subsystems/script-vm.md) — the field-VM opcode decoder the
  jump fixup uses.
