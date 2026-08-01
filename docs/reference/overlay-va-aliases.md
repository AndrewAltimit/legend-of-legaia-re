# Overlay VA aliases in the dump corpus

Some addresses in `ghidra/scripts/funcs/` are **phantom virtual addresses**: the
bytes are real and the disassembly is real, but the VA printed beside each
instruction belongs to no runtime image. Grepping such an address finds a
`FUN_` header and a plausible body, so it reads exactly like a genuine, merely
undocumented function - which is why they accumulate.

This page records the two measured causes, the arithmetic that undoes each, and
the re-keying for the addresses where it has been checked. It is the
address-level companion to
[`tooling/dump-corpus-integrity.md`](../tooling/dump-corpus-integrity.md) (why a
printed address is a property of the load base) and
[`tooling/call-target-integrity.md`](../tooling/call-target-integrity.md) (the
sibling problem for `jal` targets).

## The two errors

Both apply to imports of the **PROT 0897** extraction. They are independent, so
one dump can carry either, both, or neither.

| Error | Cause | Effect on the printed VA |
|---|---|---|
| **Base offset `0xE818`** | Image imported at `0x801C0000`; PROT 0897's recovered base is `0x801CE818`. | true VA `- 0xE818` |
| **Footprint over-read `0x25000`** | PROT 0897's own content is `0x25000` bytes; the extraction footprint runs past it into PROT 0898's image, which is byte-identical to 0897's file from `+0x25000`. | PROT 0898 VA `+ 0x25000` |

The bases and the `0x25000` own-content boundary are recorded in the committed
overlay map `crates/asset/data/static-overlays.toml`. What this page adds is the
per-address consequence and the measurement that confirms it.

### Combining them

- Base-tagged dump (header carries `base=0x801CE818`), address in the over-read
  tail: printed `=` PROT 0898 VA `+ 0x25000`.
- Untagged `0x801C0000`-based dump, address in the over-read tail: printed `=`
  PROT 0898 VA `+ 0x25000 - 0xE818` `=` **`+ 0x167E8`**.
- Untagged dump, address inside 0897's own content: printed `=` true VA
  `- 0xE818`.

In an `0x801C0000`-based import, 0897's own content occupies
`0x801C0000..0x801E5000` (`0x801CE818..0x801F3818` re-based down by `0xE818`).
A printed VA above that window in such an import is a **PROT 0898** address,
recovered as `printed - 0x167E8`.

## Measured re-keys

Each row below was checked by taking the phantom dump's opening mnemonic stream
and matching it against the instruction stream **starting at the re-keyed VA**
inside a PROT 0898 dump - a base-tagged static extraction (`overlay_0898_*`,
`overlay_0898_static_*`) or a runtime capture. Matches run to the end of the
shorter body or to the first indirect jump, after which Ghidra's linear listing
follows different case bodies in each dump.

| Phantom VA | Delta | Re-keys to | Notes |
|---|---|---|---|
| `0x801E6388` | `0x167E8` | inside `801CFA48` | 9 instructions matched. |
| `0x801E63E0` | `0x167E8` | inside `801CFA48` | 24 instructions matched. |
| `0x801E6F30` | `0x167E8` | `801D0748` | The battle main dispatcher. |
| `0x801EE4B8` | `0x167E8` | inside `801D71B8` | 9 instructions matched. |
| `0x801F1FC8` | `0x167E8` | inside `801DB7B0` | See [`functions.md`](functions/script-vms.md#801db7b0). |
| `0x801F4318` | `0x167E8` | `801DDB30` | See [`functions.md`](functions/battle.md#801ddb30). |
| `0x801F8D0C` | `0x167E8` | `801E2524` | Battle-action leaf, 75 instructions. |
| `0x801FDDE8` | `0x25000` | `801D8DE8` | HUD / element renderer. |
| `0x80202BCC` | `0x167E8` | `801EC3E4` | The arts-power kernel. |

`0x801FDDE8` is the diagnostic row. Its dump header **is** base-tagged
`base=0x801CE818`, so the base is right and only the over-read applies - which
is why its delta differs from every neighbour. A correct base tag does not make
a printed VA real.

`0x801F8D0C` is the independent check on the whole law. Read on its own terms,
that body is a per-frame pass over battle-context bytes `+0x28B` (selector) and
`+0x28C` (clock), emitting up to four `FUN_801E2650` layers gated at
`0xF0`/`0xE0`/`0xD0` and walking the clock by `DAT_1F800393 << 3` to a `0xF0`
ceiling. That is, line for line, the already-documented battle Arts announcement
banner [`801E2524`](functions/audio.md#audio) - which is exactly where
`- 0x167E8` puts it.
The arithmetic and the semantics agree without either having been used to derive
the other.

### Interiors of the re-keyed bodies

These carry no independent body of their own (the dump either resolves its
`entry=` to one of the addresses above, or has no instructions at all), so they
inherit their parent's re-keying rather than being measured separately.

| Phantom VA | `- 0x167E8` | Lands inside |
|---|---|---|
| `0x801E7504` | `0x801D0D1C` | `801D0748` |
| `0x801EF91C` | `0x801D9134` | `801D9110` |
| `0x801F1F4C` | `0x801DB764` | `801DB510` |
| `0x801F1FD4` | `0x801DB7EC` | `801DB7B0` |
| `0x801F7B88` | `0x801E13A0` | `801E09F8` |
| `0x801F89B8` | `0x801E21D0` | `801E1D98` |
| `0x801F8C08` | `0x801E2420` | `801E23EC` |
| `0x80202B30` | `0x801EC348` | `801EC0DC` |
| `0x80203A50` | `0x801ED268` | `801EC3E4` |
| `0x802046B8` | `0x801EDED0` | `801EC3E4` |
| `0x802059F8` | `0x801EF210` | `801EF014` |

### A re-key describes the dump, not the address

Both tables above say what a *dump* printed at a VA really contains. That is not
the same claim as "no function lives at this VA", and for one class of row the
two come apart: a printed VA **below `0x801F3818`** is inside PROT 0897's own
`0x25000` of content, so the `0x25000` over-read half of the `0x167E8` delta
explains the dump's bytes but says nothing about the address. The field overlay
can and does hold real code there.

`0x801F1F4C` is the measured instance. Its dump re-keys to battle-action
`0x801DB764`, and that re-key stands - but the VA is field file `+0x23734`, and
disassembling `0897` at its own base shows `jr ra` at `0x801F1F48` followed by a
leaf that gates on `_DAT_8007B450` and exits `jr ra` at `0x801F1FCC` /
`0x801F1FD4`. So the address is VA-aliased, not phantom, and the two listed
"interiors" `0x801F1FC8` / `0x801F1FD4` are interior to *that* leaf as well.

The check is cheap and worth running before any row below `0x801F3818` is read
as "names nothing": disassemble the mapped image at the VA and look for the
`jr ra` / prologue pair. Above `0x801F3818` the over-read argument is the whole
story and no such second reading exists.

### Below `0x801CE818` nothing is real

Every mapped overlay bases at `0x801CE818` (slot A) or `0x801F69D8` (slot B), so
**no extracted image contains a VA below `0x801CE818`**. A printed address in
`0x801C0000..0x801CE818` therefore names no overlay function whatever its dump
looks like, and for the 0897-family programs the correction is the plain
`+ 0xE818`. This needs no dump at all - it follows from the base map. (The
`overlay_0896_*` programs take different corrections in this band - see
[the sweep](#prot-0896-two-programs-one-law-each).)

The failure signature to watch for is a write-up that calls two bodies a "twin",
a "relocation copy" or a "sibling" on the strength of identical instructions with
branch targets offset by a constant. That constant *is* the base error. PSX
overlays are not relocated, so two genuinely distinct functions do not come out
instruction-for-instruction identical.

| Phantom VA | `+ 0xE818` | Match | Was written up as |
|---|---|---|---|
| `0x801C1634` | `801CFE4C` | 202 / 202 instructions by VA | "byte-for-byte structural twin" of the collision probe |
| `0x801C2B2C` | `801D1344` | 296 / 296 | "code-identical relocation copy" |
| `0x801C36AC` | `801D1EC4` | 245 / 245, operands included | a distinct warp-reposition handler |
| `0x801C9688` | `801D7EA0` | 208 / 208, operands included | "field-mode equivalent" of the horizon emitter |

Each was checked against a **base-correct** dump of the target
(`overlay_cutscene_dialogue_*` / `overlay_world_map_*`), not against another
0897 import.

### The inverse direction

The law runs backwards too. `overlay_0897_xxx_dat_801cf408.txt` prints a body at
`0x801CF408` whose stream is identical to the 133-instruction body that seven
independent RAM captures place at `0x801DDC20` - exactly `+ 0xE818`. Inside
0897's own content the untagged import is simply `0xE818` low; the over-read
tail is where the second term appears.

## Evidence grade

`disassembly`. The deltas are measured from instruction streams, not inferred
from filenames. Two independent corroborations:

- `0x25000` and `0xE818` are already recorded in the committed overlay map for a
  different pair of addresses; the rows above are fresh instances of the same two
  constants arising from the same two mechanisms.
- Every re-keyed target is attested by a base-tagged static extraction and/or by
  several independent runtime captures that agree with each other.

## The byte-level sweep

The stream match above needs a long instruction run, so it left no verdict
exactly where the corpus is worst: short dumps, and data regions Ghidra
rendered as bogus instructions.
[`resolve-phantom-va.py`](../../scripts/ghidra-analysis/resolve-phantom-va.py)
closes that gap. Given a dump and an explicit list of candidate readings -
(image, VA the image's first byte occupies under that reading) - it compares
the dump against every candidate at the printed VA itself: canonical tokens
for code, and, for rows where Ghidra decoded a raw data word as an
instruction (`nop`; the `<load> rt,imm(zero)` shape a `0x801Cxxxx..0x801Fxxxx`
pointer word decodes to), the rendering re-encoded into the exact 32-bit word
and compared byte-for-byte. A pointer word carries 32 discriminating bits, so
a data region decides a candidate as firmly as code does. Every verdict below
is a candidate whose every compared row agrees while each rival disagrees or
does not map the VA at all.

### The boundary band near `0x801E5000`

In the untagged `0x801C0000` import, 0897's own content ends at printed
`0x801E5000`. Near that boundary the two-error arithmetic could not exclude
either reading; swept, every dump printed in `0x801E4000..0x801E6000` from
the 0897-family programs resolves to exactly one, and the strata change
precisely at `0x801E5000`:

| Printed | Bytes are | True VA |
|---|---|---|
| `0x801E4404` | 0897 own data - a record carrying the function pointer `0x801E6400` | `0x801F2C1C` |
| `0x801E4420` / `0x801E45AC` / `0x801E45BC` | 0897 own content (`0x801E45AC` is four `nop`s of alignment padding) | `+ 0xE818` |
| `0x801E4AF0` | 0897 own data - a pointer table into the field data band `0x801CF1xx` / `0x801CF7xx`; 14/14 words | `0x801F3308` |
| `0x801E4C38` (+ its `801e4c58` duplicate) | 0897 own data - a table of field function pointers (`0x801F1138`, `0x801EE328`, `0x801EE5D4`, ...); 13/13 words | `0x801F3450` |
| `0x801E5134` (`overlay_0897_xxx_dat` program) | 0898 own code, file `+0x134` | `0x801CE94C` |
| `0x801E5520` / `0x801E5E84` / `0x801E5FB0` | 0898 leading string segment - ASCII rendered as code | `0x801CED38` / `0x801CF69C` / `0x801CF7C8` |
| `0x801E5668` / `0x801E573C` (+ `801e57f0`) | 0898 own code | `- 0x167E8` |
| `0x801E4000` / `0x801E4470` / `0x801E4A8C` / `0x801E5134` (`overlay_0897_` program) / `0x801E5338` / `0x801E5B4C` | 0897 own content **at the printed VA** - these prints are correct | printed |

So the two addresses this section once left open are 0897 own-content after
all - and they are *data*, which is why no stream match could decide them:
the words themselves did, at full strength. `0x801E5134` is the aliasing
exemplar: two dumps print the same VA from different programs, one correct
(a real field routine) and one a phantom of 0898's `0x801CE94C`.

### `0x8020D05C`

The dump carries zero instructions (`size=1 bytes, 0 instructions`,
`halt_baddata()`, and a `SYNC(0)` in the C rendering), so only the candidate
map can be compared - and it decides. The untagged `0x801C0000` import maps
the VA to 0898 own content at file `+0x2805C`, true VA `0x801F6874`: a
`(pointer, count)` record table (`0x24` stride) into 0898's own `0x801CF9xx`
data band, whose `0x0000000A` / `0x00000014` words have no R3000 decoding -
the bad-data halt - and whose `0x0000000F` word is the `SYNC(0)`. Every rival
reading maps the VA to real code that would have decoded (base-tagged 0897
import → a clean prologue at 0898 `0x801E805C`; the untagged 0896 import →
0898 code at `0x801ED874`), so all are excluded.

The "double alias" dissolves into one printed VA per program: `- 0x167E8` is
the ordinary untagged-tail re-key, landing on real 0898 data at `0x801F6874`,
not on a function. The phantom body printed at `0x801F5748` (11108 bytes, so
its printed extent covers `0x801F6874`) belongs to the *base-tagged* program,
where that printed VA holds 0898 VA `0x801D1874`, interior of `FUN_801D0748`.
Both chains terminate in PROT 0898; neither names a function entry at any
`0x8020xxxx` or `0x801F68xx` address.

### PROT 0896: two programs, one law each

The `overlay_0896_*` prefix covers **two** Ghidra imports of the same
over-read footprint (own content `0x9000`, then 0897's file, then 0898's),
and every addressed dump in the family resolves under exactly one of them -
no exceptions, no ambiguous rows:

| Program | Header tag | Printed `- 0x801C0000` | Bytes are | Re-key |
|---|---|---|---|---|
| untagged | none | `< 0x9000` | 0896 own content | file = printed `- 0x801C0000`; **no true VA** - 0896's link base is unrecovered |
| untagged | none | `0x9000 ..< 0x2E000` | field (0897) | true = printed `+ 0x5818` |
| untagged | none | `>= 0x2E000` | battle (0898) | true = printed `- 0x1F7E8` |
| tagged | `base=0x801C5818` | any | 0896 own content | file = printed `- 0x801C5818`; no true VA |

The byte-level partition and the header tags identify the same split
independently: exactly the twelve dumps that resolve into 0896's own content
at `- 0x801C5818` carry the `[overlay_0896 base=0x801C5818]` tag, and every
untagged dump obeys the three-band law. `0x801C5818` is the phantom 60-vote
jal-recovered base recorded in
[`static-overlay-pipeline.md`](../tooling/static-overlay-pipeline.md) - the
tagged program is an import performed at it.

Two cross-checks pin the pair: file `+0x5C90` is printed at `0x801C5C90` by
the untagged program and at `0x801CB4A8` by the tagged one; file `+0xD1C` is
printed at `0x801C0D1C` (`overlay_0896_bat_back_dat`, untagged) and at
`0x801C6534` (tagged). And the one `0x9000` step this section once carried as
an isolated measurement - `0x801EFF30` re-keying to `801D0748` - is now an
instance of the untagged battle band (`- 0x1F7E8`).

The trap the split creates: five tagged dumps print in
`0x801C9000..0x801CE818`, where the untagged law reads "`+ 0x5818` = field" -
their bytes are 0896's own content, so the three-band table is a
**per-program** law, not a per-prefix one. A `overlay_0896_*` printed VA
re-keys only after the dump's header tag (or its bytes) picks the program.

### `0x801FD4C0`

The dump's content begins at printed `0x801FD150` (the requested-address
filename defect), and its opening 64 tokens reproduce at 0898 file
`+0x18150` - so the dump is the **battle** image's `FUN_801E6968`, and
printed `0x801FD4C0` re-keys to 0898 VA `0x801E6CD8`, interior of that body.
The field image's `FUN_801E6B34`, the other occupant of the aliased VA, is
not what the dump holds.

## See also

- [`tooling/dump-corpus-integrity.md`](../tooling/dump-corpus-integrity.md) - the
  general rule and the sweep script.
- [`tooling/static-overlay-pipeline.md`](../tooling/static-overlay-pipeline.md) -
  how base-tagged static extractions are produced.
- [`functions.md`](functions.md) - the entry-point directory the re-keyed
  addresses resolve into.
