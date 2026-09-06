# Inventory - one array, an active window, and the "split bag"

Legaia keeps one shared 256-slot inventory in a single flat array in main
RAM. Every accessor is bounded by an **active window** that collapses to one
128-slot half whenever a character travels alone. That window is the whole
story behind the "divided" inventory players describe, and behind the
folklore "72-slot" bound.

## Summary

- **Storage.** `0x80085958`, 256 slots x 2 bytes `[item id : u8][count : u8]`.
  Id `0` marks a free slot; stacks cap at 99. The array is SC block
  `+0x1818` inside the `0x1A18`-byte live game-state block at `0x80084140`,
  so it rides verbatim into every memory-card save.
- **Access.** Nothing indexes the array raw. Every producer and consumer
  (pause menu, field VM `GIVE_ITEM`, battle rewards, shops, equip swap-back)
  goes through the `SCUS_942.54` helpers that scan and bound-check only
  inside the active window - five of them carry the add / consume / capacity /
  reserve / normalize contract, but thirteen SCUS functions touch the array in
  all (see [the reference census](#every-reference-to-the-array)), and two
  capture-class cast modules reach it without the window at all.
- **Window.** Three `gp`-relative halfwords - `gp[+0x2D2]` start,
  `gp[+0x2D4]` end, `gp[+0x2D6]` span - written by exactly one function,
  `FUN_8004313C`, from the party roster: `[0, 256)` with two or more members,
  `[0, 128)` for Vahn alone, `[128, 256)` for any other lone character.
- **Consequence.** A solo character has an isolated pocket of the bag. They
  can neither see nor spend the party's items, and what they pick up alone
  never reaches the party inventory. Storage is never physically
  partitioned - only access is.
- **Quirk.** The add helper stores the item id before it bound-checks, so a
  completely full window leaks the id one slot past `end`. Only the count
  store is guarded.

## Contents

- [Storage](#storage)
- [Accessors](#accessors)
- [The active window](#the-active-window)
- [Design intent](#design-intent)
- [Adjacent divisions people conflate with the halves](#adjacent-divisions-people-conflate-with-the-halves)
- [The add helper's off-by-one](#the-add-helpers-off-by-one)
- [Function map](#function-map)
- [Provenance](#provenance)

## Storage

| Item | Value |
|---|---|
| Array | `0x80085958` |
| Slots | 256, 2 bytes each, `[id][count]` |
| In the save block | SC `+0x1818` (= `0x80084140 + 0x1818`) |
| Free slot | `id == 0`; occupancy keys on the id alone, so a live id with a zero count still survives compaction |
| Stack cap | 99 |
| New-game seed | `FUN_80034A6C` writes exactly one slot: `(0x77 Healing Leaf, x5)`; both callers pre-zero the whole range first |

Cheat databases call this region *Have 99 Items* / *Item Modifier*. The
*Have 99 Items* code covers `0x80085958..0x800859E8` - 72 slots - which is
the size of the general-items **display page** that code happened to target,
not an engine bound (see [the folklore check](#the-active-window)).

## Accessors

A sweep of all 129 functions in the pause-menu overlay finds **zero** direct
array writes: its 17 inventory operations all call into a small SCUS helper
family, passing item ids or helper-returned slot numbers. The field VM's
`GIVE_ITEM` op (`0x39`, chests and scripts), battle rewards
(`FUN_8004E568`), shops, and the equip swap-back (refunding displaced gear)
take the same road. There is no raw-index sort or swap primitive anywhere in
retail.

| Helper | Role |
|---|---|
| `FUN_800421D4` | add - find a matching id, else the first free slot |
| `FUN_80042310` | consume by id - zeroes the emptied id in place and returns; never compacts on its own |
| `FUN_80042EE0` | capacity check - paired with reserve before adds in the equip swap-back path |
| `FUN_80043048` | reserve - second half of the capacity / reserve pair |
| `FUN_800423E0` | normalize - calls window setup first, merges duplicate stacks (cap 99), pulls occupied slots down into holes |
| `FUN_8004313C` | window setup - the sole SCUS writer of `gp[+0x2D2 / +0x2D4 / +0x2D6]` |

Every helper scans and bound-checks only inside the window the last call to
`FUN_8004313C` installed.

## The active window

The helpers never see "256 slots". They see `[start, end)` in three
`gp`-relative halfwords, and `FUN_8004313C` is the only function in
`SCUS_942.54` that writes them (11 call sites; an overlay writer has not been
excluded). It picks the window from the party roster: member count at
`0x80084594`, roster id bytes at `0x80084598`.

| Members (`0x80084594`) | Story flag 20 | First roster byte | Window installed |
|---|---|---|---|
| `0` | - | - | none - the previous window stays |
| `1` | set (`FUN_8003CE64(0x14)`) | - | `[0, 256)` |
| `1` | clear | `0` (Vahn) | `[0, 128)` |
| `1` | clear | `!= 0` | `[128, 256)` |
| `>= 2` | not tested | not tested | `[0, 256)` |

The span also lands in `gp[+0x2D6]`, so `gp[+0x2D4]` is only ever `128` or
`256`. Slots outside the window exist in RAM but are out of bounds to add,
consume, capacity and normalize alike.

**Live cross-check** on a mid-game battle state: party count 3, window
`(0, 256, 256)`, 160 contiguous occupied slots - a bag any 72-slot model
truncates.

**Folklore check - the "72-slot inventory".** 72 is not an engine bound. It
is the size of the general-items display page the *Have 99 Items* GameShark
code happened to target. The accessors bound on `gp[+0x2D4]`, which is only
ever 128 or 256. Any arithmetic built on a 72-slot bag - including the old
ACE out-of-bounds ceilings - is void.

## Design intent

The game has story segments where the party splits and the player controls
one character alone. The window gives that character an isolated pocket of
the bag. Vahn owns the lower half because the game opens with Vahn alone:
the early-game bag simply *is* `[0, 128)` until the party forms. Once two or
more members are present the full 256 opens and the halves stop mattering.

## Adjacent divisions people conflate with the halves

When someone says Legaia's inventory is "divided", they may be looking at
one of three unrelated boundaries. Only the first is the window mechanic.

| Division | Where | What it is |
|---|---|---|
| The window halves | `FUN_8004313C`, `gp[+0x2D2..+0x2D6]` | Runtime bounds over one shared array, keyed on party composition. Storage is never partitioned - only access is. |
| Per-character gear | `0x80084708 + n*0x414`, equip bytes `+0x196..+0x19D` | Equipped weapon, armor and Goods live inside each character's `0x414`-byte [record](../formats/save-record.md), not in the bag. Equipping refunds the displaced item through the add helper; once worn, it belongs to the record. |
| Menu pages | consumables `0x77..0x8E`, equipment below, books and key items above | The pause menu's tabs filter the one array by item-id band. A page is a view, not a second store - and the 72-slot figure is a page size. |

## The add helper's off-by-one

`FUN_800421D4` scans for a matching id, then for the first free slot. Its id
store precedes the bound check: on a completely full window the scan exits at
`i == end` and the id byte lands one slot past the window, at
`0x80085958 + gp[+0x2D4]*2` - `0x80085A58` (`end = 128`) or `0x80085B58`
(`end = 256`). Only the count store is guarded.

This off-by-one is the core of the still-open arbitrary-code-execution
reachability thread. Two cautions the earlier work established:

- An exec probe at `pc = 0x800422BC` fires on **every** successful add, before
  the guard - a hit there is not out-of-bounds evidence by itself.
- The earlier reading "`0x800859E8` = SC `+0x18A8`, the first key-item slot"
  rested on the 72-slot page being the window; it is not.

Closing the question needs a bag genuinely filled to `end` (256 with a
multi-member party) with the hit shown to land past the guard. The thread's
state is tracked in [`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).

### Every reference to the array

The array is reachable in exactly one addressing idiom, and the enumeration of
its users is closed by three measured structural facts rather than by a survey.

- **The `gp`-relative form is arithmetically impossible.** `0x80085958 - gp`
  is `0xA640` and the array ends at `gp + 0xA840`; a signed 16-bit
  displacement reaches only `gp + 0x7FFF`. No `imm(gp)` instruction can name
  any byte of it - which removes, a priori, the one reference form that has
  produced false negatives elsewhere in this repo.
- **The literal-word form has zero occurrences.** Neither `0x80085958` nor the
  block base `0x80084140` appears as a 32-bit word anywhere in `SCUS_942.54`
  or the 1233 `PROT` entries, so no pointer table offers an indirect route in.
- **Every access materialises the block base inline**, as
  `lui rX,0x8008` / `addiu rX,rX,0x4140` then `sll slot,1` / `addu` /
  `lbu|lb|sb rW,0x1818(rZ)` (id) or `0x1819` (count). Nothing reads a slot as
  a halfword or word.

Decoding every instruction of the based images for that displacement pair
gives **125 sites across six images**: `SCUS_942.54` 51 (13 functions,
`0x8003004C..0x800430A0`), PROT 0899 menu 55 (19 functions - four of them
behind a `lui` in a branch delay slot, which a linear scan misses), PROT 0897
field 5, PROT 0898 battle 2, and the two cast modules below. Every other based
overlay and every other PROT entry: zero. Exactly two of the 125 are absolute
rather than indexed, and both are the new-game seed's slot-0 write
(`0x80034B10` / `0x80034B18`); **no instruction on the disc names an address
inside the key-item band** - it is only ever reached through a runtime index.

The **two cast modules** are the consumers no window bounds.
[PROT 0941](../formats/spell-table.md) (Steal / Stone Circle) picks a random
slot and clamps it against a live count (`lh v1,0xB5EA(a1)` at `0x801F7828`);
PROT 0954 (Fatal Decision) does not - `0x801F81A4` leaves `rand & 0xFF`, so it
samples the **whole** 256-slot array, accepting the first slot whose id, count
and item-record price (`+2`) are all non-zero, retrying up to `0x400` times
(`0x801F81FC`). It is the only reader that reaches the key-item band without
going through the menu window.

Two habits of the older prose the census corrects. The `& 0x3ff` slot mask
belongs to 4 sites, not to readers in general - the SCUS packed-handle
decoders `FUN_8002FF8C` / `FUN_800302E4` / `FUN_80032A44`, where the handle's
low bits carry the slot and its high nibble carries a tag
(`0x800302F4 andi v1,a1,0xf000`). And that mask is **not** a 256-slot bound:
`0x3FF` admits slot 1023, i.e. `0x5FE` past the array's end, so what confines
those reads is whoever builds the handle, not the mask.

The two windows fail differently, and only one of them is settled by
arithmetic. `FUN_800421D4`'s merge pass keys on the id byte, so each non-zero
id occupies at most one slot and `0` is the empty sentinel - at most **255**
distinct ids can ever be live. The **full** window is 256 slots, so a hole
always remains and its scan cannot reach `end`. The **half** windows are 128
slots, and the id space does not save them: the static [item-name
table](../formats/item-table.md) `PTR_DAT_8007436C` carries **250** non-empty
names over its 256 ids (only `0x00`, `0x12`, `0x1A`, `0x52`, `0xB9` and `0xFD`
are blank), so 128 distinct live ids is arithmetically reachable. What actually
bounds the half-window case is how much of the item population is obtainable
while a character travels alone - a progress bound, not a capacity one, and
one nobody has measured.

## Function map

| Function | Role | Notes |
|---|---|---|
| `FUN_8004313C` | window setup | sole SCUS writer of `gp[+0x2D2 / +0x2D4 / +0x2D6]`; 11 callers; branches on party count, story flag 20, first roster byte |
| `FUN_800421D4` | add (find-or-insert) | id store precedes the bound check (the off-by-one); count store is guarded |
| `FUN_80042310` | consume by id | zeroes the emptied id in place; never compacts on its own |
| `FUN_80042EE0` | capacity check | paired with reserve before adds in the equip swap-back path |
| `FUN_80043048` | reserve | second half of the capacity / reserve pair |
| `FUN_800423E0` | normalize (merge + squeeze) | calls window setup first; merges duplicate stacks (cap 99); pulls occupied slots down into holes; occupancy = `id != 0` alone |
| `FUN_80034A6C` | new-game seed | writes exactly slot 0 = `(0x77 Healing Leaf, x5)`; both callers pre-zero the whole range first |

## Provenance

Ghidra-traced disassembly of `SCUS_942.54` plus live emulator cross-checks: a
three-member mid-game battle state for the window read, and a menu-overlay
sweep of all 129 functions (`dump_menu_inventory_refs.py`) for the
no-raw-writes claim. Cheat-device names (*Have 99 Items*) are cited as
third-party anchors, not as engine facts. The per-cell detail also lives in
[`memory-map.md`](../reference/memory-map.md#0x80085958---item-inventory);
this page is the mechanism-first view.

## See also

- [`memory-map.md`](../reference/memory-map.md) - the RAM map this array sits in.
- [`save-record.md`](../formats/save-record.md) - the per-character record equipped gear lives in.
- [`field-menu.md`](field-menu.md) - the pause-menu renderer that walks the bag over the window.
- [`script-vm.md`](script-vm.md) - the field VM's `GIVE_ITEM` op.
