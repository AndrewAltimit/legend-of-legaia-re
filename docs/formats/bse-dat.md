# `bse.dat` - master sound bank

The sound subsystem loads one bank at init and keeps a pointer into it for the
rest of the session. Detection class: `bse_bank`. Parser
[`crates/asset/src/bse_bank.rs`](../../crates/asset/src/bse_bank.rs).
Confidence: **Confirmed** for the identification, the header word and the record
stride; **Unknown** for what the record columns mean.

## Which entry it is

`FUN_8001FA88` allocates a `0x1800`-byte buffer into `_DAT_8007B8D0`
(`jal 0x80017888`, `a1 = 0x1800`) and then fills it down one of two branches on
the dev/retail flag `_DAT_8007B8C2`:

| Branch | How it names the file |
|---|---|
| dev (`== 0`) | path opener on `0x8007B3AC` (`lui a0,0x8008` / `addiu a0,a0,-0x4c54`), the `"bse.dat"` string in the [sound-driver path cluster](sound-driver.md) |
| retail (`!= 0`) | `byindex_sync_loader(0x37A, …, 1)` - `li a0,0x37a` in the branch-delay slot at `0x8001FAD0` |

Both branches write the **same destination buffer**, so the dev file name and
the retail TOC index name the same asset: raw TOC `0x37A` = **extraction entry
888** under the [+2 numbering correction](cdname.md#numbering-space).
`see ghidra/scripts/funcs/8001fa88.txt`.

The size agrees. `byindex_sync_loader` resolves through `FUN_8003E8A8`, whose
sector count is the entry's size - 2 sectors for entry 888, which fits the
`0x1800`-byte destination. (The historical `toc[p+5] - toc[p+3] + 4` expression
claims 88 sectors there; see
[`prot.md`](prot.md#tocp5---tocp3--4-is-not-an-entrys-size).)

## Layout

```text
+0x00   u16  tag            ; 1 in both retail carriers
+0x02   u16  body_offset    ; 4 - byte offset of the record table
+body   record[]            ; 8 bytes each, terminated by an all-zero record
```

The `+0x02` word is consumed as a **byte offset**, not a count. The tail of
`FUN_8001FA88` computes `gp[0x678] = base + ((s16)u16@+2 / 2) * 2` - `lhu
v1,0x2(a0)`, sign-extend, round toward zero, `>> 1`, `<< 1` - i.e. a
round-to-even of the offset, leaving `gp[0x678]` pointing at the record table.

Each record is 8 bytes:

```text
+0  u8   a       ; walks 0,1,2,... across the table
+1  u8   b       ; small sub-index within `a`
+2  u8   key     ; clusters on 60 (0x3C) and tracks `b`
+3  u8   flags   ; low bits 1/2, plus a 0x20 bit
+4  u32  v       ; small integer - only 0 and 2 occur
```

**The column names are shape, not semantics.** They describe how each byte
behaves across the table. The obvious reading - a `(program, tone, unity key)`
triple, since `0x3C` = 60 is middle C and the sibling
[SFX descriptors](sfx-table.md) are also 8-byte program/tone records - is a
**hypothesis**: no consumer of `gp[0x678]` has been traced, so nothing here
asserts what the columns mean. What is pinned is the loader, the destination,
the header word's use as a byte offset, and the 8-byte stride.

## The two carriers

| Extraction | Extent | Records | Reached by |
|---|---:|---:|---|
| 888 | 4096 | 297 | `FUN_8001FA88`, retail branch (`0x37A`) |
| 1195 | 2048 | 7 | nothing in the dump corpus |

Entry 1195 is the same format with a 7-record table. Its raw TOC index `0x4AD`
appears as a load literal in **no** dumped function, while its block neighbours
`0x4B0` / `0x4B1` do (the slot-machine assets) - so the absence is not simply
that nothing in that block has been dumped. The dump corpus is not complete, so
this is evidence of an unused sibling, not proof of one; see
[`disc-coverage.md`](../tooling/disc-coverage.md) for what the corpus does and
does not cover.

### 1195 has a second reading, and the bytes do not separate them

Entry 1195 is `other1 + 2` - the slot every block on the disc seats a
[`scene_event_scripts`](scene-bundles.md#scene_event_scripts---prescript-only)
prescript in. Under that reading its header is `[u16 count = 1][u16 offsets[0]
= 4]` rather than `[u16 tag = 1][u16 body_offset = 4]`: the same four bytes.
The record table matches too - prescript record 0 is a run of 8-byte rows
shaped `[0][index][0x3C + index][flags]` plus a small trailing word, which is
exactly the `a` / `b` / `key` / `flags` walk this page describes, and it appears
that way at slot 2 of scene blocks across the disc.

Nothing in the bytes decides it. What separates the two carriers is position
and reachability: 888 is `sound_data2 + 13`, not a prescript slot, and it is
the one `FUN_8001FA88` loads. So this page's identification stands for 888, and
1195 is better read as its block's prescript that happens to consist of that
one table. The detector keeps 1195 because it runs before the prescript tier;
that ordering is a convention, not evidence.

## Detection

`u16@+0x02 == 4`, a small `u16@+0x00`, and at least six 8-byte records whose
trailing `u32` stays under `0x100`, terminated by an all-zero record. Across the
PROT corpus that matches these two entries and nothing else.

## See also

- [`sound-driver.md`](sound-driver.md) - the path cluster the dev branch's
  `"bse.dat"` string lives in, and the per-scene `.dpk` the same function loads
  second.
- [`sfx-table.md`](sfx-table.md) - the static SCUS sound-effect descriptors,
  also 8-byte program/tone records, and the table whose ids `>= 0x200` are
  documented as coming from a runtime bank instead.
- [`prot.md`](prot.md) - entry extents, and why the footprint is the real one.
