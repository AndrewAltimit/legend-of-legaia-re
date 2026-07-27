# Phantom-print index: the `0x801C…` / `0x801D…` band

[`dump-corpus-integrity.md`](dump-corpus-integrity.md) establishes that a dump's
printed addresses are a property of the load base it was taken at, and names the
mis-based batch runs. This page is the worked consequence for one address band:
every `0x801Cxxxx` / `0x801Dxxxx` printed VA that carries a Ghidra dump but no
function-level write-up of its own.

**Read the headline before the tables.** Almost none of those addresses is a
function entry. The band is dominated by mis-based prints, and re-keying them
lands either inside the field/event VM `FUN_801DE840` or inside a routine that is
already documented under its real VA. A dump file named for an address in this
band is not evidence that a routine begins there, and porting "the function at
`0x801D_____`" from one of these dumps ports a slice of something else.

Every row below was resolved by matching the dump's opening instruction stream
against the statically extracted images, then reading the resolved VA out of the
image itself - not out of the dump. The method is the one
[`check-dump-base-integrity.py`](dump-corpus-integrity.md#re-running-the-sweep)
uses; the entry/interior verdict comes from the image, where a function boundary
is the `jr ra` + delay-slot pair immediately before the VA.

## Why this list is committable when the sweep's output is not

`dump-corpus-integrity.md` declines to commit the sweep's per-dump listing,
because that listing is keyed to dump *filenames* over a gitignored corpus and
rots the moment anyone adds a dump. This page is keyed to **printed VAs that
carry a catalog row**, and states what each one resolves to in an extracted
image. Adding a dump does not change any row here; only a change to
[`static-overlays.toml`](../../crates/asset/data/static-overlays.toml) can, and
that is exactly when the sweep is supposed to be re-run anyway.

Every address in the tables below is already carried by
[`port-catalog-ignore.toml`](port-catalog.md) under a `worklist_*` category, so
none of them is open port work. What the ignore list does not carry is *where the
bytes are*, and a merged reason that names the wrong true VA sends the next
reader to the wrong routine. This page is the resolution those reasons point at.
The exception is the closing section, [printed VAs that are entries after
all](#printed-vas-that-are-entries-after-all-in-a-different-overlay), whose rows
are open port work precisely because the phantom print is not the whole story.

## Re-key deltas measured in this band

Each delta below is measured, not assumed: it is the offset at which the dump's
opening instructions reproduce byte-for-byte in the named image. The two
right-hand columns are what a reader needs to convert a printed VA into
something real.

| Dump program | Delta | Bytes live in |
|---|---|---|
| `overlay_0897_*`, `overlay_0897_xxx_dat_*`, untagged `801d….txt` | `+0xE818` | field, PROT 0897 |
| `overlay_0896_*` untagged, printed `- 0x801C0000` in `0x9000..0x2E000` | `+0x5818` | field, PROT 0897 |
| `overlay_0896_*` untagged, printed `- 0x801C0000 >= 0x2E000` | `-0x1F7E8` | battle_action, PROT 0898 |
| `overlay_0896_*` untagged, printed `- 0x801C0000 < 0x9000` | - | PROT 0896's own content; link base unrecovered |
| `overlay_0896_*` tagged `base=0x801C5818` | - | PROT 0896's own content at file `printed - 0x801C5818`; link base unrecovered |
| `overlay_0899_xxx_dat_*` | `+0xE818` | menu, PROT 0899 |
| `overlay_0971_*` | `+0xE818` / `+0xD018` | debug_menu 0971 / fishing 0972 |
| `overlay_0977_*`, printed `- 0x801C0000 < 0x3800` | `+0xE818` | arena_init, PROT 0977 |
| `overlay_0977_*`, printed `- 0x801C0000 >= 0x4800` | `+0xA018` | field_battle_intro, PROT 0979 |
| `overlay_0978_*`, printed `- 0x801C0000` in `0x1000..0x5000` | `+0xD818` | field_battle_intro, PROT 0979 |
| `overlay_0978_*`, printed `- 0x801C0000 >= 0x5000` | `+0x9818` | dance, PROT 0980 |

The `overlay_0977_*` / `overlay_0978_*` batches are each **one** import of the
pre-correction over-read footprint at base `0x801C0000`, so the delta varies by
which entry's stratum the printed offset falls in (0977's footprint: own code to
`+0x3800`, then 0978, then 0979; 0978's: own code to `+0x1000`, then 0979, then
dance from `+0x5000`). The filename's `0977`/`0978` records which PROT entry was
fed to Ghidra, and this cluster's entries over-read each other, so it says
nothing about which overlay owns the code - the split rows above are the same
per-stratum re-key as the `0896` split below. The earlier reading of the
`+0x9818` batch as "imported at `0x801C5000`" described the same bytes through
an equivalent lens; the strata decode supersedes it because it also accounts
for the `+0xD818` and `+0xA018` siblings from the same two programs. Full
per-dump table:
[`re-settled-threads.md`](../reference/re-settled-threads.md#prot-0977--0978-extraction--the-dump-re-key).

The `0896` split is the re-key table from
[`dump-corpus-integrity.md`](dump-corpus-integrity.md#the-shift-clusters)
applied per row, and it is the trap in this band: two of these batches take
different deltas, and a `0896` VA re-keyed with the `0897` batch's `+0xE818`
lands `0x9000` past where the bytes are, in plausible-looking code. The
untagged/tagged split is the byte-level sweep's finding: `overlay_0896_*` is
**two** imports, and the tagged program's prints reach `0x801CE818` without
ever holding field bytes - so the `+0x5818` row applies only to untagged
dumps. Full partition + cross-checks:
[`overlay-va-aliases.md`](../reference/overlay-va-aliases.md#prot-0896-two-programs-one-law-each).

Two independent batches agree where they overlap, which is the check that makes
the deltas more than a curve fit. Printed `0x801C4520` (`+0xE818`) and printed
`0x801CD520` (`+0x5818`) both land on field `0x801D2D38`; printed `0x801C46A4`
and `0x801CD6A4` both land on field `0x801D2EBC`.

## The band is a sample of a corpus-wide shape

The tables below are the worked cases. The general result is measurable without
enumerating anything, and it is stronger than the tables suggest: when every
defective cited dump is re-resolved to the VA its bytes occupy and that VA is
put back to Ghidra in a program imported at the image's own base, **more of
those addresses turn out to be interiors of other routines than turn out to be
function entries.**

Two independent instruments say so, and they are built on different evidence:

- The shape sweep's name-mismatch check compares a dump's filename address
  against the address its content starts at. Every delta it reports is
  **negative** - the resolved entry always sits below the requested address.
  Nothing but `getFunctionContaining()` paired with a requested-address filename
  produces that one-sided distribution, so the signature identifies the defect
  rather than merely being consistent with it.
- Asking Ghidra directly, per address, in the correctly-based program, returns
  `INTERIOR of <function> at <entry>` for a large plurality of them - and the
  reported enclosing entry is again always below.

The practical consequence for anyone reading this page: **treat "there is a dump
file at this address" as carrying almost no information about whether a routine
begins there.** That was already the headline for this band. It holds across the
whole corpus, and it holds for base-correct dumps too - a mis-based print and a
genuine interior are different causes with the same symptom, and the second is
the more common of the two.

The right repair differs by cause, which is why the distinction is worth
keeping. A mis-based print is re-keyed: the routine exists, at another VA. An
interior is retired: no routine begins there, and re-dumping the address only
manufactures a second file asserting the same non-existent entry.

## Group 1 - re-keys into the field/event VM `FUN_801DE840`

These printed VAs are Ghidra's promotion of the field VM's intra-function jump
labels ([`script-vm.md`](../subsystems/script-vm.md#intra-function-label-catalogue))
seen through a mis-based print. Neither the printed VA nor the resolved VA is an
entry; the enclosing routine is the 43-opcode field/event VM, whose real entry is
`0x801DE840` in PROT 0897.

`+0xE818` prints (`overlay_0897_*`):
`0x801D0170` `0x801D02A8` `0x801D0ABC` `0x801D0E78` `0x801D0EEC` `0x801D1694`
`0x801D1744` `0x801D1854` `0x801D1CF0` `0x801D1CFC` `0x801D1D9C` `0x801D1EEC`
`0x801D20D4` `0x801D21AC` `0x801D227C` `0x801D261C` `0x801D2958` `0x801D2E90`
`0x801D3730` `0x801D3B0C` `0x801D3D84` `0x801D3DF0` `0x801D4BA0` `0x801D4C30`.

`+0x5818` prints (`overlay_0896_*`):
`0x801D9860` `0x801DB0F8` `0x801DB49C` `0x801DB844` `0x801DBBCC` `0x801DBBDC`
`0x801DD0BC`.

Two of those carry an extra caveat. `0x801D3DF0`'s dump is **gapped**: its first
three rows are the field VM's `addiu fp,fp,2` / `j <epilogue>` label-call idiom
printed at a second phantom VA (`0x801D0880`), and only the rows from
`0x801D3DF0` onward are contiguous - re-keyed, that body is field `0x801E2608`.
`0x801DBBCC`'s stream diverges from the image after four instructions at either
candidate delta, so its exact resolved VA is unpinned; both candidates
(`0x801E13E4`, `0x801E1390`) are inside `FUN_801DE840`, which is the only claim
this page makes about it.

## Group 2 - re-keys onto a documented function entry

For these the printed VA is a phantom of a routine that already has a real VA and
a write-up. The port work is at the right-hand column and is usually already
tracked there.

| Printed | Real | Image | Routine |
|---|---|---|---|
| `0x801C2520` | `0x801D0D38` | field | party-roster panel renderer, [`script-vm.md`](../subsystems/script-vm.md) |
| `0x801C4520` | `0x801D2D38` | field | 3-actor talk, [`script-vm.md`](../subsystems/script-vm.md) |
| `0x801C46A4` | `0x801D2EBC` | field | timed-flag scheduler consumer, [`functions.md`](../reference/functions.md) |
| `0x801C6248` | `0x801D4A60` | field | scripted actor-approach SM, [`field-locomotion.md`](../subsystems/field-locomotion.md) |
| `0x801C7840` | `0x801D6058` | field | ambient particle emitter, [`cutscene.md`](../subsystems/cutscene.md) |
| `0x801C8D00` | `0x801D7518` | field | scene actor re-hydration pass, [`actor-vm.md`](../subsystems/actor-vm.md) |
| `0x801C8FDC` | `0x801D77F4` | field | `0x4C 0xD8` actor-spawn helper, [`script-vm-menuctrl.md`](../subsystems/script-vm-menuctrl.md) |
| `0x801CCFC8` | `0x801D27E0` | field | scripted camera-focus SM, [`cutscene.md`](../subsystems/cutscene.md) |
| `0x801CD8A4` | `0x801DC0BC` | field | camera-mover per-frame tick, [`cutscene.md`](../subsystems/cutscene.md) |
| `0x801CEAF8` | `0x801DD310` | field | camera-mover install, [`cutscene.md`](../subsystems/cutscene.md) |
| `0x801D516C` | `0x801E3984` | field | world-map span draw, [`world-map.md`](../subsystems/world-map.md) |
| `0x801D5C58` | `0x801E4470` | field | attached-sprite projection tick, [`actor-vm.md`](../subsystems/actor-vm.md) |
| `0x801D886C` | `0x801DE084` | field | camera-param commit, [`cutscene.md`](../subsystems/cutscene.md) |
| `0x801D8B24` | `0x801E733C` | field | two-field value panel, [`functions.md`](../reference/functions.md) |
| `0x801DFB10` | `0x801EE328` | field | `ON RULA` travel-art actor, [`world-map.md`](../subsystems/world-map.md) |
| `0x801C3594` | `0x801D1DAC` | menu | pause-menu panel renderer, [`field-menu.md`](../subsystems/field-menu.md) |
| `0x801C6CF8` | `0x801D5510` | menu | shop quantity selector, [`shop.md`](../subsystems/shop.md) |
| `0x801C0F18` | `0x801CF730` | debug_menu | mode-6 TMD-TEST init, [`boot.md`](../subsystems/boot.md) |
| `0x801C3F44` | `0x801D0F5C` | fishing | rod / lure select screen, [`minigame-fishing.md`](../subsystems/minigame-fishing.md) |
| `0x801C7930` | `0x801D4948` | fishing | reeling-line actor, [`minigame-fishing.md`](../subsystems/minigame-fishing.md) |
| `0x801C97A4` | `0x801D67BC` | fishing | caught-fish mesh render, [`minigame-fishing.md`](../subsystems/minigame-fishing.md) |
| `0x801C5C58` | `0x801CF470` | dance | beat-clock SM, [`minigame-dance.md`](../subsystems/minigame-dance.md) |
| `0x801C7B40` | `0x801D1358` | dance | per-dancer actor handler, [`minigame-dance.md`](../subsystems/minigame-dance.md) |
| `0x801C82DC` | `0x801D1AF4` | dance | score / award routine, [`minigame-dance.md`](../subsystems/minigame-dance.md) |
| `0x801C8B04` | `0x801D231C` | dance | HUD render driver, [`minigame-dance.md`](../subsystems/minigame-dance.md) |
| `0x801C8D0C` | `0x801D2524` | dance | beat-track HUD, [`minigame-dance.md`](../subsystems/minigame-dance.md) |

## Group 3 - re-keys into the interior of a named routine

Same failure, one step worse: the resolved VA is not an entry either, so there is
nothing to port at either address. The right-hand column is the routine that owns
the bytes.

**"Not an entry" is a per-image fact, and the first row shows why the image has
to be named.** `0x801D4A80` is interior to `FUN_801D4A60` in **field(897)** - and
in the **menu** overlay the same VA is a real, already-ported function, the
window-34 content renderer. Both statements are true; they are different images
at one aliased address. A row here that omits the image reads as a contradiction
against any page that ports the other occupant.

| Printed | Real | Interior of |
|---|---|---|
| `0x801C6268` | `0x801D4A80` (in field(897)) | `FUN_801D4A60` |
| `0x801CAC44` | `0x801D045C` | `FUN_801D01B0` |
| `0x801CAE64` | `0x801D067C` | `FUN_801D01B0` |
| `0x801CC810` | `0x801D2028` | `FUN_801D1EC4` |
| `0x801CCE38` | `0x801D2650` | `FUN_801D25EC` |
| `0x801CD510` | `0x801D2D28` | `FUN_801D27E0` |
| `0x801CD628` | `0x801D2E40` | `FUN_801D2D38` |
| `0x801CD728` | `0x801D2F40` | `FUN_801D2EBC` |
| `0x801CD844` | `0x801D305C` | `FUN_801D2EBC` |
| `0x801CD9C0` | `0x801D31D8` | `FUN_801D31B0` |
| `0x801CDAFC` | `0x801D3314` | `FUN_801D31B0` |
| `0x801CDB1C` | `0x801D3334` | `FUN_801D31B0` |
| `0x801CDB48` | `0x801D3360` | `FUN_801D31B0` |
| `0x801CFCE4` | `0x801DE4FC` | mid-block; nearest prologue `FUN_801DE478` |
| `0x801D30B4` | `0x801D88CC` | `FUN_801D84D0` |
| `0x801D5594` | `0x801E3DAC` | `FUN_801E3984` |
| `0x801D55C8` | `0x801E3DE0` | `FUN_801E3984` |
| `0x801D59C8` | `0x801E41E0` | `FUN_801E3E00` |
| `0x801D5A24` | `0x801E423C` | `FUN_801E3E00` |
| `0x801D5DA0` | `0x801E45B8` | `FUN_801E4470` |
| `0x801D6B4C` | `0x801E5364` | `FUN_801E5338` |
| `0x801D7D4C` | `0x801E6564` | `FUN_801E6400` |
| `0x801D808C` | `0x801E68A4` | `FUN_801E6778` |
| `0x801D828C` | `0x801E6AA4` | `FUN_801E6984` |
| `0x801D873C` | `0x801E6F54` | `FUN_801E6B34` |
| `0x801D8894` | `0x801DE0AC` | `FUN_801DE084` |
| `0x801D896C` | `0x801E7184` | `FUN_801E6F70` |
| `0x801D8A2C` | `0x801DE244` | `FUN_801DE234` |
| `0x801D8BE0` | `0x801E73F8` | `FUN_801E733C` |
| `0x801D9C0C` | `0x801E8424` | `FUN_801E76D4` |
| `0x801DA8BC` | `0x801E90D4` | `FUN_801E76D4` |
| `0x801DA8F0` | `0x801E9108` | `FUN_801E76D4` |
| `0x801DB2C0` | `0x801E9AD8` | `FUN_801E76D4` |
| `0x801DBF7C` | `0x801EA794` | `FUN_801E9F64` |
| `0x801DC098` | `0x801EA8B0` | `FUN_801E9F64` |
| `0x801DC188` | `0x801EA9A0` | `FUN_801E9F64` |
| `0x801DC320` | `0x801EAB38` | `FUN_801EA9B0` |
| `0x801DC4C0` | `0x801EACD8` | `FUN_801EA9B0` |
| `0x801DC6A0` | `0x801EAEB8` | `FUN_801EAD98` |
| `0x801DC6E0` | `0x801EAEF8` | `FUN_801EAD98` |
| `0x801DC8C0` | `0x801EB0D8` | `FUN_801EAD98` |
| `0x801DCC40` | `0x801EB458` | `FUN_801EAD98` |
| `0x801DD000` | `0x801EB818` | `FUN_801EAD98` |
| `0x801DD8F0` | `0x801EC108` | `FUN_801EAD98` |
| `0x801DE37C` | `0x801ECB94` | `FUN_801ECA08` |
| `0x801DE468` | `0x801ECC80` | `FUN_801ECA08` |
| `0x801DE604` | `0x801ECE1C` | `FUN_801ECD0C` |
| `0x801DEAB4` | `0x801ED2CC` | `FUN_801ECD0C` |

All of the above are field-overlay (PROT 0897) VAs, and every owning routine is
already written up. `FUN_801E76D4` is the
[world-map controller](../subsystems/world-map.md) and `FUN_801EAD98` its
dev-menu renderer; `FUN_801E9F64` / `FUN_801EA9B0` / `FUN_801ECA08` /
`FUN_801ECD0C` are the same subsystem's walk, cancel-unwind, panel-sizer and
destination-picker routines. `FUN_801D01B0` is the
[player free-movement controller](../subsystems/field-locomotion.md),
`FUN_801D84D0` the [MES line pager](../subsystems/script-vm.md),
`FUN_801D25EC` the position-tween spawner, `FUN_801D1EC4` the walk-on tile
trigger, `FUN_801D31B0` a per-scanline `POLY_FT4` strip emitter, `FUN_801E3E00`
the scripted-move actor tick, `FUN_801E5338` the sparkle emitter, `FUN_801E6400`
the numeric-field draw, `FUN_801E6778` the equip-target character list,
`FUN_801E6984` the `MAP_CHANGE` list/cursor, `FUN_801E6B34` the name-entry grid
render and `FUN_801E6F70` the casino coin-exchange counter.

`0x801D7D4C` is dumped twice under two prefixes and therefore re-keys twice: the
`overlay_0897_xxx_dat` copy to `0x801E6564` as above, and the
`overlay_0896_bat_back_dat` copy to `0x801DD564`, inside `FUN_801DD4C4`.

## Group 4 - base-correct prints that are still interior

These dumps are at the right base. The printed VA is real, and it is still not an
entry - the ordinary Ghidra label-promotion case with no base error involved.

| Printed | Image | Interior of |
|---|---|---|
| `0x801CFBE4` | menu 0899 | `FUN_801CF88C` |
| `0x801CFCCC` | slot_machine 0975 | `FUN_801CF0D8` (reel SM) |
| `0x801D0E78` | field 0897 / baka 0976 | `FUN_801D0D38` / `FUN_801CF388` |
| `0x801D0EEC` | field 0897 / baka 0976 | `FUN_801D0D38` / `FUN_801CF388` |
| `0x801D1694` | field 0897 / baka 0976 | `FUN_801D1344` / `FUN_801CF388` |
| `0x801D16FC` | field 0897 | `FUN_801D1344` |
| `0x801D1744` | field 0897 / baka 0976 | `FUN_801D1344` / `FUN_801CF388` |
| `0x801D1854` | field 0897 / baka 0976 | `FUN_801D1344` / `FUN_801CF388` |
| `0x801D1880` | field 0897 | `FUN_801D1878` |
| `0x801D1AD4` | field 0897 | `FUN_801D1878` |
| `0x801D2600` | field 0897 | `FUN_801D25EC` |
| `0x801D4D44` | battle 0898 | `FUN_801D388C` |
| `0x801D6574` | menu 0899 / battle 0898 | `FUN_801D64A8` / `FUN_801D5854` |
| `0x801D6830` | field 0897 | `FUN_801D6704` (field main init) |
| `0x801D7110` | field 0897 | `FUN_801D6704` |
| `0x801DD6B8` | menu 0899 | `FUN_801DD35C` (title-overlay tick) |
| `0x801DE7F0` | battle 0898 | `FUN_801DDB30` |
| `0x801DFF48` | battle 0898 | `FUN_801DFDF0` |

`0x801D1880` and `0x801D2600` are a distinct sub-case worth naming: their dumps
are `raw <lo>..<hi>` window listings, not function dumps, and the address in the
filename is the window's **exclusive end bound**. Nothing ever claimed a routine
started there.

The two-image rows are genuine VA aliases - slot A holds a different overlay per
game mode, so one printed VA names two unrelated interiors. That is the class
[`worklist-classification.md`](worklist-classification.md#an-image-tag-is-a-program-name-not-an-overlay-identity)
warns must not be collapsed to one verdict.

## Group 5 - unresolved

| Printed | Why |
|---|---|
| `0x801C5CF8` | `overlay_0896` citation stub for `FUN_801C5C90`; 0896 own content, file `+0x5C90` |
| `0x801C5E28` | `overlay_0896` citation stub for `FUN_801C5C90`; 0896 own content, file `+0x5C90` |
| `0x801C6534` | `overlay_0896` **tagged** program; 0896 own content at file `+0xD1C` - the same function `overlay_0896_bat_back_dat` prints at `0x801C0D1C` |
| `0x801C8178` | `overlay_0896` citation stub for `FUN_801C802C`; 0896 own content, file `+0x802C` |
| `0x801C81A8` | `overlay_0896` untagged program; 0896 own content at file `+0x81A8` |
| `0x801C82E0` | `overlay_0896` citation stub for `FUN_801C81A8`; 0896 own content, file `+0x81A8` |
| `0x801C8F00` | data-region dump, no disassembly at all |
| `0x801CE9C4` | Ghidra `caseD_` switch fragment, tagged `base=0x801C0000`, resolves nowhere |

Two rows left this group when the 0977/0978 static extractions entered the
index: `0x801C6268` and `0x801C6CF8` (`overlay_0977_other_game` second dumps)
now re-key at `+0xA018` into the field-battle-intro overlay 0979
(`0x801D0280` / `0x801D0D10`) - the strata decode in the re-key table above.

The six `overlay_0896` rows are the only ones in the band that cannot be
re-keyed to a runtime VA even in principle. The byte-level sweep attributes
each to a PROT 0896 **file offset** (the sweep also separates the family's two
import programs - untagged `0x801C0000` and tagged `base=0x801C5818`, which is
how `0x801C6534` and `0x801C0D1C` print the same function; see
[`overlay-va-aliases.md`](../reference/overlay-va-aliases.md#prot-0896-two-programs-one-law-each)) -
but PROT 0896's link base is unrecovered, so no printed VA in the family's
own-content strata names a runtime address - see
[call-target integrity](call-target-integrity.md#scope-the-overlay_0896-window-below-0x801ce818).
The routines themselves are only reachable by recovering that base.

## Two real entries this resolution turned up

Re-keying pointed at two field-overlay routines that nothing else in `docs/`
cites. Both are read out of `overlay_field_0897.bin` at base `0x801CE818`.

**`FUN_801D5A24`** - an actor-spawn helper. It calls the SCUS pool allocator
`FUN_80020DE0(&DAT_801F26D8, _DAT_8007C34C)` with a spawn descriptor held in the
field overlay's own data segment, and on a non-null result writes `0` to the new
actor's `+0x54` and its own `a0` argument to `+0x50`. It is the parameterised
sibling of the [actor-VM](../subsystems/actor-vm.md) spawn helper that clears
both of those fields; the `+0x50` word is what distinguishes the two.

**`FUN_801DD4C4`** - a frameless per-actor tween step. It advances the actor's
`u16` counter at `+0x50` by the scratchpad frame delta at `0x1F800393`, clamps it
to the duration at `+0x9E` and raises bit `3` of the flag word at `+0x10` on the
clamp, then interpolates between the endpoints at `+0x14` and `+0x24` scaled by
the clamped counter before reading the actor's `+0x90` back-pointer. Its `jr ra`
predecessor pair makes it a genuine entry despite having no stack frame - the
[leaf case](worklist-classification.md#jr-ra-does-not-prove-a-function) that a
prologue-only entry test misses.

`0x801DFB10` is the same routine as `0x801E8B10` two batches over: `+0xE818` from
the `overlay_0897_xxx_dat` program and `+0x5818` from the `overlay_0896` one both
land on `0x801EE328`, which is the check that pins the pair. Its `FUN_801dfb10`
form is nonetheless cited as a scripted player-turn state machine keyed on `+0x54`
- the description belongs to `FUN_801EE328`, which reads `+0x54` in its first
instructions, and the address does not exist.

## Printed VAs that are entries after all, in a different overlay

The headline above holds for the tables: almost no address in this band begins a
routine where its dump prints it. A handful do, and they are the sharpest form of
the trap, because both readings are correct at once. The dump is mis-based and its
bytes belong somewhere else entirely - *and* the printed VA is a real function
entry, in an overlay that never dumped it. Re-keying the dump and stopping there
deletes that routine.

| Printed | The dump's bytes | The printed VA itself |
|---|---|---|
| `0x801D0290` | field 0897 `0x801DEAA8`, a `FUN_801DE840` label-call slice (`addiu s8,s8,2`) | battle 0898's 12-instruction overlay-local PRNG over `0x801F6950`, a leaf preceded by `jr ra` |

The distinguishing question is the one the
[entry-boundary test](worklist-classification.md#the-entry-boundary-test) asks:
does any image *begin* a routine at the printed VA. Where the answer is no, the
row is a phantom and belongs in the tables above; where it is yes, the row is port
work at the printed address and the re-key is a fact about one dump only.
`docs/reference/functions/battle.md` records the `0x801D0290` reading, with the
warning that the `overlay_0897` dump holds a different body at the VA.

## See also

- [`dump-corpus-integrity.md`](dump-corpus-integrity.md) - why a printed address is a property of the load base, and the standing sweep.
- [`call-target-integrity.md`](call-target-integrity.md) - the sibling question about decoded `jal` targets, and the PROT 0896 window.
- [`worklist-classification.md`](worklist-classification.md) - the per-row classes and what each one licenses.
