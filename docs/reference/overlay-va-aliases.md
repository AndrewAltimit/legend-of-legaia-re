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
| `0x801F1FC8` | `0x167E8` | inside `801DB7B0` | See [`functions.md`](functions.md#801db7b0). |
| `0x801F4318` | `0x167E8` | `801DDB30` | See [`functions.md`](functions.md#801ddb30). |
| `0x801F8D0C` | `0x167E8` | `801E2524` | Battle-action leaf, 75 instructions. |
| `0x801FDDE8` | `0x25000` | `801D8DE8` | HUD / element renderer. |
| `0x80202BCC` | `0x167E8` | `801EC3E4` | The arts-power kernel. |

`0x801FDDE8` is the diagnostic row. Its dump header **is** base-tagged
`base=0x801CE818`, so the base is right and only the over-read applies - which
is why its delta differs from every neighbour. A correct base tag does not make
a printed VA real.

`0x801F8D0C` is the independent check on the whole law. Read on its own terms,
that body is a per-frame pass over battle-context bytes `+0x28B` (stage) and
`+0x28C` (level), emitting up to four `FUN_801E2650` layers gated at
`0xF0`/`0xE0`/`0xD0` and walking the level by `DAT_1F800393 << 3` to a `0xF0`
ceiling. That is, line for line, the already-documented battle screen-flash ramp
[`801E2524`](functions.md#audio) - which is exactly where `- 0x167E8` puts it.
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

### Below `0x801CE818` nothing is real

Every mapped overlay bases at `0x801CE818` (slot A) or `0x801F69D8` (slot B), so
**no extracted image contains a VA below `0x801CE818`**. A printed address in
`0x801C0000..0x801CE818` therefore names no overlay function whatever its dump
looks like, and the correction is the plain `+ 0xE818`. This needs no dump at
all - it follows from the base map.

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

## What this does not settle

- **The window boundary is not swept.** `0x801E4AF0` and `0x801E4C38` sit just
  under `0x801E5000`, so the law says they are 0897 own-content
  (`+ 0xE818` → `0x801F3308` / `0x801F3450`, just inside 0897's tail) - but a
  `- 0x167E8` re-key also lands them inside a dumped 0898 body. Neither reading
  is excluded by a stream match, because neither dump carries enough
  instructions to match on. Addresses within a few hundred bytes of the boundary
  need a byte-level check against both images.
- **`0x8020D05C` chains.** Its `- 0x167E8` re-key lands inside the body printed
  at `0x801F5748`, which the overlay map already identifies as a phantom print
  of `801D0748`. A doubly-aliased address is not covered by the arithmetic
  above; left open.
- **`0x801FD4C0`.** Re-keys to `0x801E6CD8`, which two different images cover
  with two different entries (`801E6968` and `801E6B34`). Which one owns it is
  not settled here.
- **PROT 0896 has its own deltas.** A `0x9000` step relates `0x801EFF30` to
  `0x801E6F30` (so `0x1F7E8` to `801D0748`). One measurement is not a law; no
  0896 rule is stated.

## See also

- [`tooling/dump-corpus-integrity.md`](../tooling/dump-corpus-integrity.md) - the
  general rule and the sweep script.
- [`tooling/static-overlay-pipeline.md`](../tooling/static-overlay-pipeline.md) -
  how base-tagged static extractions are produced.
- [`functions.md`](functions.md) - the entry-point directory the re-keyed
  addresses resolve into.
