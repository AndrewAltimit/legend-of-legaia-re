# `bse.dat` - the battle sound-effect descriptor bank

`bse.dat` is the **runtime half of the sound-effect descriptor table**: the
rows that back cue ids `>= 0x200` while a battle is on screen, in exactly the
row format the static `SCUS_942.54` table uses for ids `< 0x200`
([`sfx-table.md`](sfx-table.md)). Detection class: `bse_bank`. Parser
[`crates/asset/src/bse_bank.rs`](../../crates/asset/src/bse_bank.rs).
Confidence: **Confirmed** (disassembly) for the identification, the header
word, the stride, the record columns and the row-to-cue-id mapping.

Two pointers matter and they are not the same thing. `_DAT_8007B8D0`
(`gp+0x5B8`) is the sound subsystem's shared **current-bundle** slot, which
whatever loaded last owns; `gp+0x678` is the session handle to *this* bank's
record table, and it is the one the battle cue router writes through - see
[below](#the-two-pointers).

## Which entry it is, and when it loads

`FUN_8001FA88` allocates a `0x1800`-byte buffer into `_DAT_8007B8D0`
(`jal 0x80017888`, `a1 = 0x1800`) and then fills it down one of two branches
on the dev/retail flag `_DAT_8007B8C2`:

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

### It is a battle load, not a boot load

`FUN_8001FA88` has exactly **one** caller anywhere on the disc: `jal 0x8001fa88`
at `0x80051A3C`, inside `FUN_800513F0` - **battle init**, the routine that also
installs the battle-form party meshes
([`battle-data-pack.md`](battle-data-pack.md)). `FUN_800513F0` in turn is called
once, from the battle-scene per-frame tick `FUN_80046A20` at `0x80046F74`, under
the setup-phase gate `ctx[+0x11] == 0` (`ctx = gp[0xA0C]`), which the same block
then increments. So the bank is loaded at **battle-scene setup** and reloaded on
every battle.

That corrects the earlier reading on this page - "the sound subsystem loads one
bank at init and keeps a pointer for the rest of the session" - and the same
"at init" phrasing in [`sfx-table.md`](sfx-table.md) and
[`field-ambient-fx.md`](../subsystems/field-ambient-fx.md). The claim rested on
`FUN_8001FA88` looking like a sound-init routine (it also loads the per-set
`.dpk`, `byindex_sync_loader(param_1 + 5, …)`); nobody had resolved its caller.
Sweeps: `find-address-word-refs.py 8001fa88 --prot` over SCUS, the 31 based
overlay images and all 1233 PROT entries returns that one `jal` and no word,
`j`, branch or materialisation pair.

`see ghidra/scripts/funcs/8001fa88.txt`, `800513f0.txt`, `80046a20.txt`.

### The two pointers

`_DAT_8007B8D0` is where the bank *lands*, and it holds this bank only until
the next field scene loads. The slot is the sound subsystem's
**current-bundle** pointer, and the field asset loader repoints it at the
scene's prescript bundle on every field load: `FUN_8001F7C0`
`0x8001F840..0x8001F864` computes `*(0x1F800314 + 0xD8) + 0x12800` and stores it
to `0x8007B8D0` (`sw v0,-0x4730(at)`). The slot-machine and arena overlays do
the same.

That is why `FUN_8001FA88` does not simply leave the address there. Its tail
(`0x8001FB8C..0x8001FBC0`) re-reads the buffer, resolves `base + offsets[0]`
through the `+0x02` header word, and stores **that** to `gp+0x678` -
`sw a0,0x678(gp)`, the one and only writer. `see ghidra/scripts/funcs/8001fa88.txt`,
`8001f7c0.txt`.

The consequence is a clean split of duties, both of them pinned:

| Pointer | Absolute | Who touches it | What for |
|---|---|---|---|
| `gp+0x5B8` | `0x8007B8D0` | drainer `FUN_80016B6C` `0x80016C30`, per-actor trigger `FUN_800250D4` `0x80025104` | **reads** a descriptor for cue `>= 0x200` out of whatever bundle is current |
| `gp+0x678` | `0x8007B990` | router `FUN_8004FE5C` `0x8004FFAC` / `0x8004FFE0` / `0x80050078`, debug sound test `0x801CEE48` / `0x801CEFC0` / `0x801CF038` / `0x801CF0A0` | **writes** one descriptor column before enqueueing the cue |

Because `FUN_8004FE5C`'s five call sites are all battle-side - the arts-voice
cue `FUN_8004C140` (`0x8004C614`), the per-frame actor maintenance pass
`FUN_8004CE2C` (`0x8004D660`), the per-frame anim sound-cue player
`FUN_800508DC` (`0x80050B3C`), the melee kernel in overlay 0898
(`0x801EEBE8`) and the summon effect table in overlay 0957 (`0x801F814C`) -
the writer only ever runs while `gp[0x5B8]` still holds this bank, so the write
and the read land on the same row. The debug sound test in overlay 0971 writes
the same column through the same pointer without that guarantee.

### How the consumer was found

`gp+0x678` had no traced consumer for a long time, and the reason is a
measurement gap rather than a scarcity of code. Two reference forms are
invisible to
[`find-address-word-refs.py`](../tooling/address-reference-scan.md): a
`disp(gp)` access carries no address at all, and a `lui rX, hi` + `lw rY,
lo(rX)` pair puts the low half on the *load*, where that tool's pair scan
accepts only `addiu` and `ori`. A five-form sweep of `0x8007B990` therefore
reported "no word, no jump, no branch, no materialisation pair - in any image".
Both forms are covered by
[`find-gp-relative-refs.py`](../tooling/address-reference-scan.md#the-gp-relative-and-luiload-forms),
which finds the writer and all seven readers.

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
It is the identical expression `FUN_80016B6C` applies to `gp[0x5B8]`
(`0x80016C38..0x80016C58`) and that `FUN_800252EC` / `FUN_800250D4` apply to the
same slot - which is why the two readings of entry 1195
[below](#888-vs-1195) are decoded by one piece of code.

### Record columns

Each record is 8 bytes, and the columns are the [static SFX
descriptor](sfx-table.md#table-base--record-layout) columns - not by analogy but
because **one block of code decodes both tables**. `FUN_80016B6C` picks the arm
at `0x80016C24` (`slti v0,s0,0x200`), resolves either `0x8006F198 + id*8`
(`0x80016CA4`) or this bank's `record[id - 0x200]` (`0x80016C5C..0x80016C70`),
and then falls into the *same* field reads at `0x80016CB0` onward:

| Offset | Name | Field | Where it goes |
|---|---|---|---|
| `+0` | `p` | program / VAG index | `lbu a2,0x0(s2)` at `0x80016D48` → arg 3 of `FUN_80065034` |
| `+1` | `t` | tone / ADSR-region base | `lbu a3,0x1(s2)` at `0x80016D4C`, `+i` per voice at `0x80016D6C` |
| `+2` | `l` | note-level voice attribute | `lbu t0,0x2(s2)` at `0x80016D50` → arg 5 |
| `+3` | `n` | low 5 bits = voice count; bit `0x20` = sustained | `lbu s4,0x3(s2)` at `0x80016CBC`, split at `0x80016CFC` / `0x80016D00` |
| `+4` | `id` | category - picks the 12-byte mixer record `0x80091508 + id*12`, whose `+0xB` is the enable gate (`0x80016CE4`) and whose `+8` is the VAB slot the cue keys (`lb a1,0x8(s0)` at `0x80016D44`) | `lbu s3,0x4(s2)` at `0x80016CB0` |
| `+5..7` | - | no runtime reader; zero in every row of both carriers | - |

The field names are the designer's own, from the runtime debug format string
`FUN_80016B6C` prints off `+0..+4` at `0x80016C74..0x80016C98` - the same string
[`sfx-table.md`](sfx-table.md) records for the static table.

The disc bears the layout out. Entry 888's 297 rows: `p` spans `0..76`, `t`
spans `0..12`, `l` is `60..69` and equals `60 + t` in all but a handful of rows,
`n` takes only `0x01`, `0x02`, `0x21`, `0x22` (voice count 1 or 2, with or
without the sustained bit), `id` is `0` in 289 rows and `2` in 8, and `+5..7`
are zero in all 297.

**This supersedes the `[a][b][key][flags][u32 v]` shape reading** this page
carried while the consumer was untraced. The names were descriptions of column
*behaviour*, and one of them was wrong about the type: `+4` is a `u8` category
followed by three unused bytes, not a `u32`. "Only 0 and 2 occur" was a real
observation about the authored defaults, not about the field's range.

### Row index = cue id − `0x200`, and `+4` is rewritten per cue

The battle cue router `FUN_8004FE5C` writes the category column of the row it
is about to enqueue, from a per-actor byte
(`*(0x801C9370[category] + 0x22C) + 0x80`, or the literal `2`), then pushes the
cue id into the 4-slot ring `DAT_8007B6D8`:

| Leg | Site | Address written | Ring id |
|---|---|---|---|
| high (`id >= 0x64`) | `sb v1,-0x31c(v0)` at `0x8004FFEC` | `gp[0x678] + id*8 - 0x31C` | `id + 0x19C` |
| low (`0x1B <= id < 0x48`, non-party) | `sb v1,0x40c(v0)` at `0x80050084` | `gp[0x678] + id*8 + 0x40C` | `id + 0x281` |

Both reduce to the same expression once written against the ring id: `gp[0x678]
+ ring_id*8 - 0x1000 + 4`, i.e. **`record[ring_id - 0x200] + 4`** - byte `+4` of
exactly the row the drainer will resolve for that cue. In row terms the high
leg is `row = id - 100` and the low leg `row = id + 129`, so the two tile
`id 0x64..0xFF` onto rows `0..155` and `id 0x1B..0x47` onto rows `156..200`.
The high leg also takes `id >= 0x100` when the attacker is **not** a party seat
(`category >= 3`) - the arts-voice leg claims those only for `category < 3` -
and its `row = id - 100` keeps climbing there, which is what the 297-row table
leaves headroom for.

The debug sound test in overlay 0971 confirms the mapping independently and
without arithmetic. At `0x801CEE44..0x801CEE5C` it stores `7` at `0xDC(gp[0x678])`
- byte `+4` of row 27 - and enqueues cue `0x21B` = `0x200 + 27`. Its other three
arms (`0x801CEFD0`, `0x801CF048`, `0x801CF0B0`) write categories `7`, `8` and
`2` through the `-0x31C` expression for a menu-selected id, then enqueue
`id + 0x19C`.

So the authored `id` column is a **default**, and a live cue's VAB slot is
chosen by the actor that fired it. `see ghidra/scripts/funcs/8004fe5c.txt`,
`80016b6c.txt`; overlay 0971 read with
`disasm-overlay-fn.py --base 0x801CE818 --addr 0x801CEDF0`.

## 888 vs 1195

| Extraction | Extent | Records | Role |
|---|---:|---:|---|
| 888 | 4096 | 297 | `bse.dat` - the battle occupant of the `>= 0x200` bank, loaded by `FUN_8001FA88` (`0x37A`) |
| 1195 | 2048 | 7 | one scene block's prescript record 0 - the same bank format, per scene |

The two are **not** rival readings of one asset. The `>= 0x200` bank is a
*role*, and the format on this page is what fills it; which file fills it
depends on the mode. In battle that is `bse.dat`; in the field it is the
scene's own prescript record 0
([`field-ambient-fx.md`](../subsystems/field-ambient-fx.md#the-master-ambient-record-0---the-per-scene-sfx-descriptor-bank)),
authored per scene - jou reserves 96 rows and fills 40, `rugi` carries 21.

Entry 1195 is `other1 + 2`, a
[`scene_event_scripts`](scene-bundles.md#scene_event_scripts---prescript-only)
prescript slot, and its whole payload is one such bank: header `[u16 count = 1]
[u16 offsets[0] = 4]` - the same four bytes a `[tag][body_offset]` header would
be - then 7 rows, then zero fill to the sector end (the last non-zero byte is at
`0x38`). Every row carries `id = 2` - one constant category, which is what a
per-scene bank keying one variable VAB slot looks like (the town prescripts
[`field-ambient-fx.md`](../subsystems/field-ambient-fx.md#the-master-ambient-record-0---the-per-scene-sfx-descriptor-bank)
works carry `3`; the constant is per scene, not global). That is a scene's bank,
not a second `bse.dat`, and the earlier
"nothing in the bytes decides it" hedge is resolved by the format being one
format with two occupants rather than by position alone.

The detector keeps matching 1195, and should: it is genuinely this record
format. What the class name `bse_bank` means is *the format*, not *the file* -
`bse.dat` is the carrier that named it.

## Detection

`u16@+0x02 == 4`, a small `u16@+0x00`, and at least six 8-byte records whose
`+5..+7` are zero, terminated by an all-zero record. Across the PROT corpus that
matches these two entries and nothing else - the per-scene banks do not match
because in a scene bundle record 0 sits inside the prescript container rather
than at offset 0 of the PROT entry.

The zero-trailer test is the whole discriminator, and it used to be spelled as
"the `u32` at `+4` is under `0x100`". That is the same predicate read through
the superseded `u32 v` column: a category byte can never exceed `0xFF`, so the
bound could only ever fail on a non-zero trailer.

## See also

- [`sfx-table.md`](sfx-table.md) - the static `id < 0x200` half of the same
  table, the column names, and the category → VAB-slot mapping.
- [`../subsystems/field-ambient-fx.md`](../subsystems/field-ambient-fx.md#the-master-ambient-record-0---the-per-scene-sfx-descriptor-bank) -
  the field-mode occupant of the `>= 0x200` bank.
- [`sound-driver.md`](sound-driver.md) - the path cluster the dev branch's
  `"bse.dat"` string lives in, and the per-set `.dpk` the same function loads
  second.
- [`../tooling/address-reference-scan.md`](../tooling/address-reference-scan.md#the-gp-relative-and-luiload-forms) -
  the two reference forms that hid this bank's consumers.
- [`prot.md`](prot.md) - entry extents, and why the footprint is the real one.
