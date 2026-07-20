# Static overlay-extraction pipeline

Most of Legaia's gameplay code lives in RAM **overlays** paged into the
`0x801C0000+` overlay window per game mode (title / field / battle / menu /
world-map / cutscene / minigames). The established way to reverse them is to
capture an emulator save state and import the live RAM image into Ghidra at its
runtime base - see [`overlay-capture.md`](overlay-capture.md).

This page documents the **static** complement: extracting each overlay directly
from `PROT.DAT` and disassembling it at its load base, with identity attached
from the first byte. It **complements** the dynamic captures; it does not
replace them (see [Scope + limits](#scope--limits)).

Implementation: [`legaia_asset::static_overlay`](../../crates/asset/src/static_overlay.rs);
CLI `asset overlay …`; committed map
[`crates/asset/data/static-overlays.toml`](../../crates/asset/data/static-overlays.toml).

## Why static extraction works

PSX overlays are normally **clean copies** of a fixed-VA-linked blob: the loader
DMAs the bytes into the overlay window, runs `FlushCache`, and jumps in - there
is no per-load relocation. Legaia's overlay code ships as MIPS-code entries
inside `PROT.DAT` (the [`mips_overlay`](../formats/mips-overlay.md) /
[`overlay_ptr_table`](../formats/overlay-ptr-table.md) detectors flag the small
ones; the big scene overlays are raw too, just data-section-first). So the
on-disc entry **is** the loaded code, modulo the runtime-written `.bss`.

This is proved two ways:

- **Static reproducibility.** The as-loaded bytes extracted from any copy of the
  disc hash to a committed sha256 (`asset overlay verify`). No Sony bytes are
  committed - only the hash.
- **Runtime byte-match** (disc + save-state gated). The on-disc bytes are
  byte-identical to the resident RAM image over the entire `.text`+`.rodata`
  region. For the battle overlay (PROT 0898 at base `0x801CE818`) the on-disc
  bytes match RAM for the first `0x28800` of `0x29800` bytes - 100 % of
  code+rodata - with only the trailing `0x1000`-byte `.bss` diverging (the
  runtime zeroes / writes it after the copy). Test:
  [`crates/mednafen/tests/static_overlay_clean_copy.rs`](../../crates/mednafen/tests/static_overlay_clean_copy.rs).

## What it buys (and the limit)

- **Solves the VA-aliasing identity problem structurally.** Many overlays link
  to the same VA range - `0x801DD864` is a battle-action function in one overlay
  and a muscle-dome function in another - which is why the repo disambiguates
  with `overlay_<label>_<addr>` naming + behavioural fingerprints. Statically,
  an overlay is **"PROT entry N at base X"**: identity from the source entry, not
  a guessed label.
- **Reproducible from the user's disc**, with no curated save state - including
  overlays nobody ever captured.
- **It does not unblock runtime-value captures** (`gp[0x754]==3`, watchpoint
  results, `ctx[+0x274]` bytes). Those still need live probes
  ([`pcsx-redux-automation.md`](pcsx-redux-automation.md)). This is a
  workflow + coverage + identity win; the dynamic captures stay authoritative
  for runtime values.

## Base recovery

The load base is recovered **statically** from the overlay's own internal `jal`
call graph ([`static_overlay::recover_base`](../../crates/asset/src/static_overlay.rs)).
For the true base `B`, every internal call target `T` maps to file offset
`T - B`, which begins a function prologue (`addiu sp, sp, -X`). Tallying
`B = T - prologue_offset` over every (distinct-call-target, prologue-offset)
pair, the true base wins by a landslide (the field overlay recovers `0x801CE818`
with 60 corroborating call targets; battle with 44).

This is decisive enough to **catch and correct mislabelled overlays**. The
historical "PROT 0896 = options/pause-menu overlay" label is wrong: PROT 0896
(CDNAME `bat_back_dat`) is not an options/menu overlay at all (the options-menu
equipment aggregator `FUN_801CF650` lands in its over-read *string* section,
not on code; see the cautionary tale below for what its recovered base really
was). The **real
options/menu overlay is PROT 0899** at base `0x801CE818` - found by byte-searching
the corpus for `FUN_801CF650`'s instruction signature (`0x801CF650` ↔ PROT 0899
file `0xe38`), corroborated by 101/139 captured menu-dump functions aligning as
prologues and by jal-recovery (30 votes). PROT 0899 and the field overlay
(PROT 0897) are **VA-alias siblings in slot A** - both load at `0x801CE818` at
different times, so `0x801CF650` is a `"Give"` string in 0897 but the equip
aggregator in 0899. That is the exact aliasing this pipeline exists to
disambiguate. (PROT 0896 is the pipeline's **cautionary tale**: its
whole-file recovery returns a convincing 60-vote base `0x801C5818`, but the
votes come from the FIELD overlay's bytes carried in 0896's over-read tail
from file `+0x9000` - that code's self-consistency at `0x801CE818` fixes the
result to `0x801CE818 − 0x9000` *by construction*. Restricted to the head's
own code the recovery yields no landslide, so 0896's true link base is
unrecovered, and a live mode-24 entry capture refuted the old "mode-24 OTHER
overlay" reading (the SCUS-resident OTHER INIT streams each minigame's own
overlay directly into slot A; 0896's bytes appear nowhere in RAM across the
window or in any parked library state - probe
[`autorun_minigame_overlay_capture.lua`](../../scripts/pcsx-redux/autorun_minigame_overlay_capture.lua)
+ [`overlay_residency.py`](../../scripts/pcsx-redux/overlay_residency.py)).
Moral: when an entry's footprint over-reads a KNOWN overlay, subtract the
aliased region before trusting a recovered base. See
[`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).)

## The committed map

[`crates/asset/data/static-overlays.toml`](../../crates/asset/data/static-overlays.toml)
is the entry→base map - one record per overlay:

| Field | Meaning |
|---|---|
| `prot_index` | `PROT.DAT` entry the overlay is extracted from (the identity). |
| `base_va` | Load base inside the overlay window; statically recovered, RAM-confirmed where a capture exists. |
| `form` | `raw` (entry bytes are the as-loaded bytes) or `lzs` (decompress; needs `decompressed_size`). |
| `clean_copy_bytes` | Length of the RAM-verified `.text`+`.rodata` prefix (for `verified` rows). |
| `eligibility` | `verified` (RAM byte-matched) / `static` (base-recovered + function-anchored, not RAM-prefix-verified) / `ineligible` (runtime-relocated - keep on the dynamic path). |
| `base_source` | How `base_va` was determined: `jal` (internal call-graph recovery - default; the reproducibility test asserts the recovery agrees), `capture` (byte-matched a resident RAM anchor/region), `cross_ref` (taken from another pinned RE result in-tree). |
| `anchor_va` | Optional known function VA that must land on a function prologue (`addiu sp, sp, -X`) at `base_va` - a capture-free, disc-reproducible base cross-check. Decisive for `cross_ref`/`capture` rows where the jal-recovery assertion is skipped (e.g. a slot-A minigame sibling anchored by a documented minigame function). |
| `fingerprint_sha256` | sha256 of the as-loaded bytes - the disc-derived reproducibility anchor. |
| `notes` | Which subsystems / entry points live here. |

### Slot A vs slot B

The overlay loaders manage two independently swappable slots (`*DAT_8001038C`
and `*DAT_80010390`; see [`prot.md`](../formats/prot.md#overlay-loaders-parallel-slots)).

- **Slot A** (`~0x801CE818`) holds the big scene overlays - field (0897), battle
  (0898), menu (0899), the STR/MDEC **cutscene** overlay (0970), the mode-0
  **DEBUG MODE** overlay (0971), and the
  **minigame** overlays (fishing 0972, slot machine 0975, baka fighter 0976,
  dance 0980 - the mode-24 door-warp sub-id slots, see
  [`script-vm.md § 0x3E WARP`](../subsystems/script-vm.md#0x3e-warp-mode-24-minigame-door-warp)).
  These are VA-alias siblings (same base, resident at different
  times). The field/battle/menu/cutscene rows have dense internal call graphs
  (`base_source = jal`); the minigame rows are cross-checked instead by a
  documented minigame function landing on a prologue at the base (`anchor_va`),
  since their footprints over-read each other (one minigame's code is duplicated
  across consecutive entries at `base + N×0x800`, so jal-recovery can latch a
  phantom base - the canonical entry is the one recovering `0x801CE818`, which is
  also the entry the warp actually streams; the historical "slot machine = 0973
  with a `0x4000` over-read prefix at `0x801CA818`" row was that phantom - the
  same image matched inside 0973's over-read tail). Note: the "world-map", "save", and "shop" UIs are **not**
  separate entries - the overworld controller `FUN_801E76D4` lives in the field
  overlay (0897) and the save + shop sessions live in the menu overlay (0899);
  `asset overlay find-sig` confirms each function's signature byte-matches only
  that entry.
- **Slot B** (link base `0x801F69D8`,
  `summon_overlay::SUMMON_OVERLAY_LINK_BASE`) holds the player-summon / effect /
  minigame-data blobs from the `0900..0969` PROT cluster. These **timeshare one
  buffer**, so a save state catches an inseparable *mix* of two overlays (e.g. in
  a mid-cast Gimard save the 0900 render overlay has overwritten the
  stager) - there is no clean whole-overlay RAM prefix, and most have too sparse
  an internal call graph to jal-recover. Their base comes from a capture anchor
  (`base_source = capture`; the 0900 render region `0x801F79D8..0x801F8DD8`
  byte-matches RAM, pinning the base) or a cross-referenced RE result
  (`base_source = cross_ref`). The base is cross-checked the **slot-B way**: a
  high fraction of the overlay's internal absolute self-pointers (`lui
  0x801f/0x8020 ; addiu`) must resolve in-file at the committed base
  (`static_overlay::pointer_resolution`; 80–100 % for the mapped rows - the
  reproducibility test asserts ≥ 70 %). This is precisely where static
  extraction earns its keep: the *disc* entry disassembles cleanly at the link
  base even though the *runtime* buffer is unusable.

  **The slot-B cluster is heterogeneous.** The summon-stager arithmetic range
  `0903..=0913` (spell ids `0x81..=0x8B` under the corrected loader index math
  `param + 0x37F` in extraction space - the historical "Gimard = 0905" label
  was the `+ 0x381` off-by-2) is **fully capture-pinned, with zero
  exceptions**: every spell id in the block was observed mid-cast loading its
  arithmetic slot (loader-B current id at `0x8007BC4C`). 0907 inside the range
  is **Nighto's stager** - its ASCII head title "Hell's Music" is the attack's
  display name (the SCUS spell table carries the same string; `summon.dat`
  lists it among the attack-name records, parallel to Gimard's "Burning
  Attack"); the earlier "Disco King dance-song" identity is **refuted** (the
  dance overlay, 0980, contains no slot-B loader callsite - its music is
  sequenced BGM via the sound streaming loader). The same correction reframes
  0924 "Ultimate Rave" / 0927 "Dark Eclipse": attack-titled, stager-shaped
  (`FUN_80021B04` part-spawn census), loader callsites computed - which action
  ids drive them is the open piece. The cluster also holds the **GAME OVER**
  overlay (0902) and summon-effect data (0957 - its head is a summon string
  table, `Puera` + `Damage`/`Recover`/`Both` effect labels, NOT a dance song;
  correcting an earlier `overlay-ptr-table` reading). See
  [`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).

### A small overlay does not clear the slot

Slot A is a buffer, not a container: a load DMAs `size` bytes to `0x801CE818`
and nothing zeroes the remainder. An overlay smaller than its predecessor
therefore leaves that predecessor's tail resident and executable-looking, and a
save state taken while it is up captures a **stack of strata** rather than one
overlay.

PROT 0971 (`debug_menu`) is the clear case, because its own content is only
`0x1800` bytes. Resolving every function dump from a DEBUG MODE capture against
the extracted images, address-ordered, reads:

| Capture VA range | Bytes belong to |
|---|---|
| `0x801CE818` .. `+0x1800` | PROT 0971, the overlay actually loaded |
| `+0x1800` .. `+0xB000` | PROT 0972 (fishing), a previous load |
| `+0xB000` .. | PROT 0897 (field), an older load still |

Each boundary is exactly the previous occupant's length, which is what makes
the reading structural rather than a guess. Two consequences worth carrying:

- **A capture's slot-A bytes are not one overlay's**, so "this dump came from
  the DEBUG MODE capture" bounds nothing. Resolve the bytes per VA.
- **The strata are also why whole-file jal-recovery mis-fires here.** PROT
  0971's footprint over-reads PROT 0972 from `+0x1800`, and 0972's code is
  self-consistent at `0x801CE818`, so the recovery lands on
  `0x801CE818 - 0x1800 = 0x801CD018` with a comfortable 29 votes. Same
  mechanism as the PROT 0896 cautionary tale above; the map records
  `base_source = "capture"` for 0971 precisely so the reproducibility test does
  not assert the phantom.

## CLI

```bash
# Inspect the map.
asset overlay list

# Reconnaissance sweep: recover each entry's base + print its leading dev
# string (the identity tell). Triages the overlay corpus; --base filters to one
# slot (e.g. the slot-A base). Not committed anywhere - reproducible from disc.
asset overlay scan extracted/PROT.DAT --from 895 --to 985
asset overlay scan extracted/PROT.DAT --from 895 --to 985 --base 0x801CE818

# Locate a function-head signature across the corpus and, given the function's
# VA, infer the host overlay's load base. The capture-free byte-search that
# pins an overlay's PROT entry (how the menu overlay was found from
# FUN_801CF650's signature). The signature is the function's first few
# instructions as little-endian hex.
asset overlay find-sig extracted/PROT.DAT "1e80043c a046838c e0ffbd27" --anchor-va 0x801DC6B4

# Re-extract from your PROT.DAT and assert every committed fingerprint
# reproduces (bit-for-bit, from any copy of the disc).
asset overlay verify extracted/PROT.DAT

# Extract each eligible overlay's as-loaded bytes to a gitignored dir (these
# are Sony code).
asset overlay extract extracted/PROT.DAT --out extracted/overlays

# Emit Ghidra import helpers: a per-overlay Jython rename script + a shell
# driver that imports each overlay at its base, program named overlay_<label>.
asset overlay ghidra --out extracted/overlays

# Regenerate map rows (recover bases + hash bytes); review before committing.
asset overlay generate extracted/PROT.DAT --index 897 --index 898
```

## Importing into Ghidra

`asset overlay extract` writes `overlay_<label>_<prot>.bin` (the as-loaded form)
and `asset overlay ghidra` writes the matching import driver. Copy the blobs
into the compose service and run the driver (mirrors
[`overlay-capture.md`](overlay-capture.md), but sourced from the disc):

```bash
asset overlay extract extracted/PROT.DAT --out extracted/overlays
asset overlay ghidra  --out extracted/overlays
docker compose cp extracted/overlays/. ghidra:/data/
bash extracted/overlays/import_static_overlays.sh
```

Each overlay imports at its recovered base with the program named
`overlay_<label>`, so functions land at their real addresses with identity
attached from the source PROT entry. The Jython scripts carry the
`# @runtime Jython` / `# @category Legaia` headers and are ASCII-only.

### Verification that closes the loop

A statically-extracted-and-disassembled overlay reproduces the same functions at
the same addresses as the existing captured `overlay_<label>_<addr>.txt` dumps:
the field overlay puts `FUN_801D6704` (MAIN_INIT) at `base+0x07eec` (a clean
prologue) and the field/event VM `FUN_801DE840` at `base+0x10028`; the battle
overlay puts the per-actor state machine `FUN_801E295C` at `base+0x14144`. These
anchors are asserted against the disc bytes in
[`crates/asset/tests/static_overlay_extract.rs`](../../crates/asset/tests/static_overlay_extract.rs)
and against live RAM in the clean-copy test.

## Scope + limits

- **An extracted image is a footprint, not an overlay.** `read_entry` returns
  `[entry start, entry start + footprint)`, which runs into the following
  entries' sectors; the runtime slice is only `[entry start, next entry
  start)`. The tail is harmless while you are reading a function at its own
  address, and actively misleading the moment you ask *which overlay owns this
  VA* - the tail answers, with a neighbour's code, at an address its own
  overlay never occupies.

  The own-content length is measurable without the TOC: it is where another
  entry's head appears inside the image, and it comes out sector-aligned.
  PROT 0897 owns `0x25000` (to VA `0x801F3818`), 0898 `0x28800`, 0899
  `0x25000`, 0971 `0x1800`, 0972 `0xB000`. Note 0898's `0x28800` is exactly
  its recorded `clean_copy_bytes`, so its "trailing `0x1000` diverges in RAM"
  is simply PROT 0899's head, which was never loaded.

  The cost of skipping this: `FUN_801F5748` was read as the field overlay's
  inventory hub for a long time. `0x801F5748 - 0x801CE818 = 0x26F30` is
  `0x1F30` past where 0897 ends, so those bytes are PROT 0898's battle
  dispatcher `FUN_801D0748` - a real routine at a phantom address. The dump is
  correctly *based*; the image simply answered for a VA it does not own.

- Static extraction is for overlays that are **clean copies**. The byte-match
  catches the exceptions: an overlay whose on-disc bytes do not match the
  resident image is runtime-relocated/constructed - mark it `ineligible` and
  keep it on the dynamic path. Don't force it static.
- The fingerprint covers the `read_entry` footprint. For a few entries that
  footprint over-reads adjacent shared sectors past the real overlay (the
  on-disc entries overlap); the over-read tail is harmless noise in the Ghidra
  disassembly - the real functions still land at their real addresses.
- This pipeline does **not** address runtime values. The dynamic-capture
  workflow ([`overlay-capture.md`](overlay-capture.md),
  [`pcsx-redux-automation.md`](pcsx-redux-automation.md)) remains essential and
  authoritative for `gp`-relative globals, watchpoint results, and any value
  the overlay constructs at run time.

## See also

- [`overlay-capture.md`](overlay-capture.md) - the dynamic save-state capture
  workflow this complements.
- [`mips-overlay.md`](../formats/mips-overlay.md) /
  [`overlay-ptr-table.md`](../formats/overlay-ptr-table.md) - the detectors that
  flag overlay-code PROT entries.
- [`prot.md`](../formats/prot.md) - TOC math (indexed vs footprint) the
  extraction reads.
- [`boot.md`](../subsystems/boot.md) - the overlay loaders + the overlay window.
