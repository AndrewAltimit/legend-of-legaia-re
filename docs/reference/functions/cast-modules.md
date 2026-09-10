# Key functions: slot-B cast / summon modules - the addresses that are not entries

Part of the [key function directory](../functions.md) - the conventions for
reading these tables (bare hex = function entry, `0x`-prefixed = data /
instruction, overlay-VA caveats) are on the
[index page](../functions.md#how-to-use-this-page).

The band's **real** entry rows are on
[`battle.md`](battle.md#slot-b-summon--cast-modules-prot-09030966): the two
PROT 0898 dispatchers, the two entry tables, and the per-module routine table
with its DATA / PORT verdicts. The mechanism behind them is
[`cast-module.md`](../../subsystems/cast-module.md).

This page is the **negative** half of the same directory, and it exists because
the grep lands here either way. Most of the addresses a byte-denominated
instrument reports in this band are not function entries of the image they are
reported under - and the answer to "what is at slot-B VA X in image Y" is
usually "nothing of Y's". A reader who finds no row on `battle.md` needs a row
somewhere saying so, rather than concluding the address is undocumented.

**The split is by class, not by band, and the entry rows stay where they are.**
`battle.md` holds every slot-B address that *is* an entry - the two 0898
dispatchers, the two entry tables, the 21 trampolines, the twelve
trampoline-reached tick bodies and the per-module `Tick` / `Stager` table -
because they are battle behaviour and a reader looking for a routine looks
under the subsystem. This page holds only addresses that are not entries of the
image they are reported under. Moving the entry rows here would put the band's
behaviour on a page whose whole subject is that an address means nothing, and
would break the one property that makes the pair usable: exactly one of the two
pages answers any given address.

## The three-way test

Every slot-B address falls into exactly one of three classes, and the test is
mechanical - no judgement, no dump filename ([why a filename is not evidence:
`dump-corpus-integrity.md`](../../tooling/dump-corpus-integrity.md)):

1. **A real entry of this image.** Its `addiu sp, sp, -F` prologue is inside
   the image's code region, and one of PROT 0898's two tables, or the module's
   own trampoline, names it. These are `battle.md`'s rows.
2. **The image's own data tail.** Force-disassembled it decodes 9-34%
   implausible opcodes (`tge`, `jalx`, `movf`, `j` outside RAM); a real body in
   the same band decodes 0%. Read as 4-byte units it is two little-endian
   halfwords - GTE-shaped constants (`0x1000` = 1.0 in 12-bit fixed point,
   `0x00FF` / `0x00C0` / `0x0080` colour bytes), signed deltas, and a recurring
   `0x00000000` / `0x10001000` / `0xFF89000C` separator triple. This is the
   **spawn-record band** - `[i16 model_sel][u16 flags][move-VM bytecode]`
   records the module hands to `FUN_80050ED4` / `FUN_80021B04` in `$a2`,
   addressed by the consumer's own `lui`/`addiu` pair. 62 of the 64 images
   carry one. Layout, recovery and the one span it cannot bound:
   [`slot-b-module-layout.md`](../../formats/slot-b-module-layout.md). The
   region is **not** a tail in the layout sense - records interleave with
   bodies in at least two images, so "everything past the last function" is
   the wrong rule for finding it.
3. **Another image's bytes.** Every image in the band ends in a byte-identical,
   **same-file-offset** run of another extracted image, ending exactly at the
   shorter image's own length - build residue, not shared library code. Nine
   images end in PROT 0899's (the menu overlay, slot A, a different base),
   which is what settles the direction. Full argument:
   [`cast-module.md`](../../subsystems/cast-module.md#a-module-image-ends-in-another-images-bytes).

## Worklist runs that own no dump

Each row is a run `disc-coverage.py` reports as un-dumped **code** in the image
named. None is. `tail from` is the first byte the run's inherited half starts
at - everything below it in the run is that image's own data - and the last
column says how much of that half a dump of the **source** image already
covers, the balance in every partial case being the source image's own data
tail rather than a missing dump.

| Run (slot-B VA) | image | tail from | inherited from | covered at the source |
|---|---|---|---|---|
| `0x801F74BC`..`0x801F7CBC` | 939 `cast_spore_gas` | `0x801F7B08` | 938 `cast_chaos_breath` | 192 of 436 B, `801F7AB8` |
| `0x801F7630`..`0x801F7B30` | 949 `cast_water_crystals` | - | - (all its own data) | - |
| `0x801F7770`..`0x801F7D34` | 943 `cast_curse` | `0x801F7A0F` | 942 `cast_power_up` | all 805 B, `801F69F4` |
| `0x801F7AB4`..`0x801F8638` | 961 `cast_dead_end_crisis` | `0x801F82F0` | 960 `cast_plasma_strike` | all 840 B, `801F74E4` |
| `0x801F7BA8`..`0x801F816C` | 952 `cast_bloody_horns` | `0x801F7BC0` | 951 `cast_chaos_flare` | all 1452 B, `801F77E8` |
| `0x801F7EE4`..`0x801F8078` | 906 `summon_gizam` | `0x801F7FD8` | 905 `summon_stager_x83` | all 160 B, `801F69D8` |
| `0x801F80C8`..`0x801F816C` | 953 `cast_terio_punch` | `0x801F80C8` | 951 `cast_chaos_flare` (via 952) | all 164 B, `801F77E8` |
| `0x801F8130`..`0x801F8504` | 949 `cast_water_crystals` | `0x801F8250` | 948 `cast_cross_beam` | all 692 B, `801F726C` |
| `0x801F8318`..`0x801F8618` | 909 `summon_viguro` | `0x801F8364` | 908 `summon_zenoir` | 448 of 692 B, `801F8310` |
| `0x801F842C`..`0x801F88EC` | 965 `cast_doomsday` | `0x801F842C` | 964 `cast_element_change` | all 1216 B, `801F69D8` |
| `0x801F864C`..`0x801F89D8` | 911 `summon_orb` | `0x801F864C` | 910 `summon_swordie` | all 908 B, `801F81DC` + `801F89D4` |
| `0x801F8A24`..`0x801F8EAC` | 908 `summon_zenoir` | `0x801F8BB0` | 904 `summon_theeder` | all 764 B, `801F8B84` |
| `0x801F8A80`..`0x801F8EAC` | 910 `summon_swordie` | `0x801F8DB4` | 904 `summon_theeder` | all 248 B, `801F8B84` |
| `0x801F8D2C`..`0x801F99D8` | 912 `summon_freed` | `0x801F92E0` | 899 `menu` (slot A) | all 1784 B, `0x801D0F1C` + `0x801D1290` |
| `0x801F91E4`..`0x801F99D8` | 904 `summon_theeder` | `0x801F9208` | 899 `menu` (slot A) | all 2000 B, `0x801D0F1C` + `0x801D1290` |
| `0x801F92B8`..`0x801F99D8` | 917 `summon_barra` | `0x801F92F9` | 918 `summon_kemaro`, then 899 `menu` | its last 1650 B, `0x801D1290` |
| `0x801F9388`..`0x801F99D8` | 918 `summon_kemaro` | `0x801F9388` | 899 `menu` (slot A) | all 1616 B, `0x801D0F1C` + `0x801D1290` |
| `0x801F9AB8`..`0x801F9BA8` | 964 `cast_element_change` | `0x801F9B80` | 957 `summon_effect_table` | all 40 B, `801F99F4` |

The menu rows are named by their **menu** VA, because that is where the bytes
are code: file offset `+0x2A80` is slot-B `0x801F9458` but menu `0x801D1298`,
and only at the menu base do its branches stay inside the image. Its operands
are menu tables - the live game-state window `0x80084140`, the equipment
bonus table `0x80074F68`, the item-effect table `0x800752C0`, the passive
name/description table `0x8007625C`, the menu overlay's own `0x801E46B0`.

## Prologues that belong to the neighbour

A prologue scan over these images finds five `addiu sp, sp, -F` words outside
the frame-matched partition. All five sit **inside** an inherited tail, so each
is a function head of the image the tail came from and names nothing in the
image it was found in. Read them at the owner.

| Prologue (slot-B VA) | found in | is the entry of |
|---|---|---|
| `0x801F8078` | 906 `summon_gizam` | 905 `summon_stager_x83` spawn stager |
| `0x801F816C` | 952, 953 | 951 `cast_chaos_flare` (a trampoline; see [`battle.md`](battle.md#slot-b-summon--cast-modules-prot-09030966)) |
| `0x801F88EC` | 965 `cast_doomsday` | 964 `cast_element_change` |
| `0x801F89D4` | 911 `summon_orb` | 910 `summon_swordie` spawn stager |
| `0x801F9458` | 904, 912, 917, 918 | 899 `menu`, at its own VA `0x801D1298` |

## Why the runs keep coming back

`disc-coverage.py`'s classifier is a heuristic over bytes and cannot see any of
the above, so it re-reports these runs on every regeneration. The record that
keeps a reader from re-walking them is the `NOT_CODE` block in
[`ghidra/scripts/dump_static_overlay.py`](../../../ghidra/scripts/dump_static_overlay.py),
which now carries the band; the worklist row itself is not a defect. See
[`disc-coverage.md`](../../tooling/disc-coverage.md) for what the worklist is
and is not.
