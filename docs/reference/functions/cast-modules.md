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
   **spawn-record band** - `[i16 model_sel][u16 reserved][move-VM bytecode]`
   records the module hands to `FUN_80050ED4` / `FUN_80021B04` in `$a2`,
   addressed by the consumer's own `lui`/`addiu` pair. 62 of the 64 images
   carry one, and the `+0x02` halfword is zero in every record with no
   reader. Layout, recovery and the one span it cannot bound:
   [`slot-b-module-layout.md`](../../formats/slot-b-module-layout.md). The
   band does sit past an image's last function; the "records interleave with
   bodies" reading of PROT 0943 and 0961 was
   [the donor's residue](../re-do-not-re-walk.md#measurement-readings).
3. **Another image's bytes.** Every image in the band ends in a byte-identical,
   **same-file-offset** run of another extracted image, ending exactly at the
   shorter image's own length - build residue, not shared library code. Nine
   images end in PROT 0899's (the menu overlay, slot A, a different base),
   which is what settles the direction. Full argument:
   [`cast-module.md`](../../subsystems/cast-module.md#a-module-image-ends-in-another-images-bytes).

## Worklist runs that own no dump

Each row is a run `disc-coverage.py` reports as un-dumped **code** in the image
named. All but one are not - the exception is PROT 0949's `0x801F7630`, seven
frameless jump-table arms that are code and are now dumped
([below](#prot-0949s-seven-frameless-ramp-arms---entries-after-all)). `tail from` is the first byte the run's inherited half starts
at - everything below it in the run is that image's own data - and the last
column says how much of that half a dump of the **source** image already
covers, the balance in every partial case being the source image's own data
tail rather than a missing dump.

| Run (slot-B VA) | image | tail from | inherited from | covered at the source |
|---|---|---|---|---|
| `0x801F74BC`..`0x801F7CBC` | 939 `cast_spore_gas` | `0x801F7B08` | 938 `cast_chaos_breath` | 192 of 436 B, `801F7AB8` |
| `0x801F7630`..`0x801F7B30` | 949 `cast_water_crystals` | - | - (its own **code** below `0x801F76BC`, then its own record band) | - |
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

<!-- W1-A -->
## PROT 0949's seven frameless ramp arms - entries after all

These are the counter-example to this page: seven addresses a byte-denominated
instrument reported in the band that **are** entries of the image they were
reported under. They are frameless leaves, so the frame-matched partition names
none of them, and the 140-byte run they occupy was filed under "the module's own
data region". Their only reference is the jump table PROT 0949's stager
`FUN_801F75BC` dispatches through - and a routine reached only through a table
is still a routine.

The table is not the one at the image head. The stager forms its base with
`lui v0, 0x801F; addiu v0, v0, 0x69F0` and bounds it with `sltiu v1, a1, 0x8`,
so it owns eight arms at `0x801F69F0..0x801F6A10` - six words into the image's
leading VA run. Arm 0 is the fall-through body at `0x801F761C`, inside the
stager's own extent; arms 1..7 are the seven leaves. Together they are one
eight-step ramp on the victim actor. `see
ghidra/scripts/funcs/overlay_cast_water_crystals_0949_801f7630.txt` and
siblings.

| Entry | Arm | `+0x0C` (tint blend) | `+0x21D` (anim rate) |
|---|---:|---:|---:|
| `0x801F761C` (interior of `801F75BC`) | 0 | `0x200` | 7 |
| `801F7630` | 1 | `0x400` | 6 |
| `801F7644` | 2 | `0x600` | 5 |
| `801F7658` | 3 | `0x800` | 4 |
| `801F766C` | 4 | `0xA00` | 3 |
| `801F7680` | 5 | `0xC00` | 2 |
| `801F7694` | 6 | `0xE00` | 1 |
| `801F76A8` | 7 | `0x1000` | 0 |

`0x801F8504` in the same image is the opposite case and belongs in the table
above: PROT 0949's own content ends at file `+0x1828` (VA `0x801F8200`), and the
body at `0x801F8504` is PROT 0948's move-VM stager in 0949's inherited tail. It
frame-matches there, which is why a dump row once named it under 0949.

<!-- W1-C -->

## The player-Seru band's tick bodies, PROT 0909..0913

These six addresses are **class 1** by the test above - real entries of their
own images, named by PROT 0898's `0x801CF4EC` table (or, for `0x801F81DC`, by
three `jal` sites inside its own module). Their canonical home is therefore
[`battle.md`](battle.md#slot-b-summon--cast-modules-prot-09030966), beside the
band's other entry rows; the block is here because the grep for a slot-B VA
lands on this page either way, and because five of the six VAs *also* have a
class-2 or class-3 reading in a different image - `0x801F69D8` alone is a tick
body in six modules, a world-map dispatcher in PROT 0901 and data in a
save-state capture. The row that answers is the one whose `found in` column
matches the image you are reading.

| Entry (slot-B VA) | image | what it is | port |
|---|---|---|---|
| `801F69F4` | 0909 `summon_viguro` | Tick body, action id `0x87`. `beq`/`slti` chain over phases `0,1,2,4,5,6,7,9,0x0A..0x0E,0xFF`; enemy-row damage sweep at phase `0x0D` through `FUN_801DD0AC(0x12, 7, seat)`. The same VA in PROT 0968 is the boss stage-module tick on [`battle.md`](battle.md#801f69f4-hosts). | `legaia_engine_vm::cast_seru_ticks_b::viguro_tick` |
| `801F69EC` | 0910 `summon_swordie` | Tick body, action id `0x88`. Eleven consecutive arms `0..=0x0A` plus `0xFF`; stages and poses only - it writes no HP and calls no damage wrapper. | `cast_seru_ticks_b::swordie_tick` |
| `801F81DC` | 0910 `summon_swordie` | The module's per-slash **applier**, reached by `jal` at `0x801F78E8` / `0x801F7928` / `0x801F7A08`. Where PROT 0910's damage actually is. | `cast_seru_ticks_b::swordie_slash` |
| `801F69D8` | 0911 `summon_orb` | Tick body, action id `0x89`. Phases `0..=5`, `9`, `0x0A`, `0xFF`; arm `5` writes phase `9` directly, so `6..8` are unreachable. Arm `9` is a whole-row **heal** plus a status cleanse. The same VA in PROT 0901 is the world-map dispatcher on [`renderer.md`](renderer.md#801f69d8). | `cast_seru_ticks_b::orb_tick` |
| `801F69D8` | 0912 `summon_freed` | Tick body, action id `0x8A`. Twenty consecutive arms `0..=0x13` plus `0xFF`; enemy-row damage sweep at `0x11` through `FUN_801DD0AC(0x10, 7, seat)`. The same VA in PROT 0901 is the world-map dispatcher on [`renderer.md`](renderer.md#801f69d8). | `cast_seru_ticks_b::freed_tick` |
| `801F69F0` | 0913 `summon_nova` | Tick body, action id `0x8B`, the band's largest at 7260 B. Twenty-one arms `0..=0x14` plus `0xFF`; single-victim damage at `0x13` through `FUN_801DD0AC(0x12, 7, caster[+0x1DD])`. | `cast_seru_ticks_b::nova_tick` |

### Two of these VAs already have non-entry rows elsewhere

`0x801F69D8` and `0x801F69EC` carry scope rows in
[`scripts/ci/port-catalog-ignore.toml`](../../../scripts/ci/port-catalog-ignore.toml)
naming PROT 0901's world-map terrain dispatcher and PROT 0900's minigame tile
rasteriser, and `0x801F69F0` / `0x801F69F4` carry `worklist_data` rows taken
from a save-state capture of the slot-B buffer. None of those rows is wrong
and none of them is about these modules: they are the same address in a
different resident. The ignore file's rows and this table coexist because the
band's addresses are only meaningful as `(image, VA)` pairs, which is the same
reason the trampoline map is keyed on `(entry, body)`.

### The rendezvous phases

PROT 0909's chain does not name phases `3` or `8`: both fall through to the
epilogue with the busy register still `1`, so the tick parks. What releases
them is the module's own move-VM stager `FUN_801F7AF4`, whose arms `0` and `1`
each carry `lbu v0,0x279(v1); addiu v0,v0,1; sb v0,0x279(v1)` (`0x801F7BB8`
and `0x801F7C48`). The choreography is a rendezvous between the per-frame tick
and the effect script, and it is the only place in the band where a stager
arm other than `0` advances the phase.

<!-- W1-B -->

## The player-Seru tick entries, and the VA four of them share

The six addresses below **are** real entries of the images they are listed
under - class 1 of the [three-way test](#the-three-way-test) - so their
behaviour rows belong beside the band's other entries on
[`battle.md`](battle.md#slot-b-summon--cast-modules-prot-09030966). They are
disambiguated here because they are the sharpest case of the question this
page exists to answer: **four of the six wear the load base `0x801F69D8`**, in
four different images, and further images in the same band put capture-class
bodies at that address as well. An address alone names none of them.

Each is reached from PROT 0898's `0x801CF4EC` table through a 16-byte
trampoline stub that does nothing but `jal` the module entry, keep the return
in `s0` and jump to the shared tail `0x801F2128`. The stub addresses are read
out of PROT 0898's own bytes at base `0x801CE818` (the table is file `+0xCD4`,
the first stub file `+0x23724`), so the pairing does not depend on any dump.

| Tick entry | PROT entry | spell id | `0x801CF4EC` stub | extent | arms | wrapper sites | port |
|---|---|---|---|---|---|---|---|
| `801F69D8` | 903 `summon_gimard` | `0x81` Gimard | `0x801F1F3C` | 3396 B | `0..=12`, `0xFF` | `0x801F74AC` | `cast_seru_ticks_a::gimard_tick` |
| `801F69D8` | 904 `summon_theeder` | `0x82` Theeder | `0x801F1F4C` | 6020 B | `0..=14`, `0xFF` | `0x801F7D38` | `cast_seru_ticks_a::theeder_tick` |
| `801F69D8` | 905 `summon_stager_x83` | `0x83` Vera | `0x801F1F5C` | 5792 B | `0..=10`, `0xFF` | none | `cast_seru_ticks_a::vera_tick` |
| `801F69F4` | 906 `summon_gizam` | `0x84` Gizam | `0x801F1F6C` | 3404 B | `0..=4`, `6..=14`, `0xFF` | `0x801F7370` | `cast_seru_ticks_a::gizam_tick` |
| `801F69E8` | 907 `summon_nighto` | `0x85` Nighto | `0x801F1F7C` | 5568 B | `0..=15`, `0xFF` | none | `cast_seru_ticks_a::nighto_tick` |
| `801F69D8` | 908 `summon_zenoir` | `0x86` Zenoir | `0x801F1F8C` | 6456 B | `0..=12`, `0xFF` | three, below | `cast_seru_ticks_a::zenoir_tick` |

Extents are the frame-matched size in the **owning** image
(`disasm-overlay-fn.py <image> --base 0x801F69D8 --addr <va>`). "Arms" is the
contiguous phase run the `beq`/`slti` chain names plus its `0xFF` terminal;
PROT 0906's run has a hole at phase `5`, which no arm names.

Two of the six hold no damage wrapper at all and are not therefore quiet: PROT
0905 restores HP and cures status, and PROT 0907 sets HP to zero outright or
sets the confuse bits. Their sites are in the port's module doc.

### PROT 0908's three damage sites

`0x801F69D8` in PROT 0908 is the only routine in the band whose sites scale the
wrapper's return before clamping it, and the only one that mixes two clamp
shapes inside one routine.

| Site | arm | baked `a0` | scale | clamp |
|---|---|---|---|---|
| `0x801F76CC` | 8 | `0x12` | `dmg / 4` | `sltu` against `HP - 1` - cannot kill |
| `0x801F7C14` | 11 | `0x10` | `dmg * 3 / 4` | `sltu` against live HP |
| `0x801F7DF8` | 11 | `0x12` | `dmg * 2 / 3` | `sltu` against live HP |

All three call `FUN_801DD0AC` with `a1 = 7` (the shared kernel's summon
branch). The third sweeps `actor_table[3 ..= 6]` skipping the primary victim.

### Delay-slot constants in this set

Two of the baked powers are set in a **branch delay slot**, so a backward scan
from the `jal` that stops at the preceding branch loses them: PROT 0904's
`0x11` at `0x801F7D2C` and PROT 0906's `0x12` at `0x801F7364`. PROT 0907's
`0xFF` arm has the same shape for a non-damage constant - the animation rate it
writes is the `li v0, 0x2` in the dispatch chain's delay slot at `0x801F6B44`,
outside the arm entirely.
