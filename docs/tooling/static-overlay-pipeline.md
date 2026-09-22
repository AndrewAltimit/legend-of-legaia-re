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
  region. The battle overlay (PROT 0898 at base `0x801CE818`) is the clean
  case: its entry is `0x28800` bytes and every one of them matches RAM, which
  is also its committed `clean_copy_bytes`. Where an entry carries a
  runtime-written `.bss` tail (PROT 0899), `clean_copy_bytes` records the
  verified prefix and the tail is expected to diverge. Test:
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
with 59 corroborating call targets; battle with 44). `asset overlay scan`
prints the winning base and its vote count per entry, so the recovery is
reproducible from the disc rather than taken on trust.

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
disambiguate. (PROT 0896 is the pipeline's **cautionary tale**: recovered over
the old over-reading window it returned a convincing 60-vote base
`0x801C5818`, but the votes came from the FIELD overlay's bytes carried in
0896's over-read tail from file `+0x9000` - that code's self-consistency at
`0x801CE818` fixes the result to `0x801CE818 − 0x9000` *by construction*.
Scanned over its own `0x9000`-byte entry it yields no landslide, so 0896's
true link base is
unrecovered, and a live mode-24 entry capture refuted the old "mode-24 OTHER
overlay" reading (the SCUS-resident OTHER INIT streams each minigame's own
overlay directly into slot A; 0896's bytes appear nowhere in RAM across the
window or in any parked library state - probe
[`autorun_minigame_overlay_capture.lua`](../../scripts/pcsx-redux/autorun_minigame_overlay_capture.lua)
+ [`overlay_residency.py`](../../scripts/pcsx-redux/overlay_residency.py)).
Moral: when an entry's footprint over-reads a KNOWN overlay, subtract the
aliased region before trusting a recovered base. See
[`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).)

### One prologue is not a landslide

The tally degenerates when an image has only a few function prologues. Every
internal call target `T` votes for `B = T - prologue_offset`, so with a single
prologue every target votes for a *different* base and nothing can win by more
than one vote - and the winner is then decided by whichever target happens to be
**external**, because an external target is the one that lands on that prologue
at an implausible base.

PROT `0981` (the dev monster-test harness) is the case on this disc. The image
carries one prologue, at file `+0x38`, and one call out of the slot-A band; the
recovery reports a base that puts that external target on the prologue, with a
single vote. Three things from the image's own operands contradict it, and all
three point at the shared slot-A base:

- `lui`+`addiu` pointer resolution is 21 of 23 there and 0 of 23 at the
  recovered base.
- Eight of the nine in-image `j` / `jal` targets land inside the file there; the
  ninth is the external call.
- The 324-byte routine at file `+0x1AC` is byte-identical to the one printed at
  `0x801CE9C4` in a world-map RAM capture, and those bytes occur in no other
  PROT entry on the disc - so the image really is resident at `0x801CE818`, and
  `0x801CE9C4 - 0x1AC` gives the base directly.

The row is therefore `base_source = "cross_ref"`, which is what stops the
reproducibility test from asserting a recovery that is known to be wrong. A
one-vote recovery is not evidence of a base; below the map's `--min-votes`
floor, read the operands instead.

### A resolution ratio is not a base test

There is a second failure mode, and the reading it produced was wrong. Take
every address the image's own `lui`+`addiu` pairs and `j` / `jal` targets form
inside the overlay band, and slide a window the width of the file across them;
the best window position is then read as the best any base could do. A pinned
control - the menu overlay at the slot-A base - holds all but two of more than
twelve hundred such addresses, PROT `0896` topped out a little above four
fifths at every position, and the conclusion drawn was that no base makes it
self-consistent.

That conclusion does not follow, and on this image it is false.

A ratio over `lui` pairs is not a base test. It is one-sided in the direction
that matters here: a base whose two high halves catch **few** pairs scores
perfectly on all of them. `pointer_resolution` on `0896` reports 65 of 65 at
the refuted slot-A base `0x801CE818` and 110 of 177 at the base the call graph
recovers - so on this image the metric ranks the wrong answer first. It also
cannot see the call graph at all, and the call graph is where an image's own
structure is.

`recover_base` does see it, and it lands. Over the corrected `0x9000`-byte
entry it reports `0x801D4DF0` on ten corroborating targets of eleven distinct
internal `jal` targets, above the map's default `--min-votes` floor - so
`asset overlay scan --from 896 --to 896` has been printing the answer with no
flag at all. Three further signals, each independent of that vote:

- All 218 internal `j` instructions (122 distinct targets) land inside the file
  at that base, and none does at the two bases previously cited for it.
- Ten of the eleven `jal` targets land on an `addiu sp, sp, -X` prologue; the
  eleventh lands on the image's first frameless leaf at file `+0x424`.
- Three runs of consecutive in-image VA words - file `0x2b0..0x424`,
  `0x84a8..0x8504`, `0x8960..0x89b4` - resolve every word into the image, and
  one of them holds the base itself as a word.
- The corpus already holds dumps of this image taken under two different
  phantom import bases, and the base reconciles them: re-disassembling the file
  at `printed_va - 0x801C5818` reproduces the tagged program's opening
  instructions exactly, and the function the two programs print at `0x801C6534`
  and `0x801C0D1C` is one function at file `+0xD1C`, i.e. `0x801D5B0C`
  ([`phantom-print-index.md`](phantom-print-index.md)).

The image is in the map now, and what makes it unusual is not its base. Of its
322 calls into the SCUS address range, **zero** land on a function entry of
this disc's `SCUS_942.54`, where the two slot-A controls score 1203 of 1307
(`0897`) and 793 of 884 (`0899`) under the same entry test; no constant shift
of the executable within +-`0x20000` brings more than 7 of its 42 distinct SCUS
targets onto an entry. It is a foreign-build image: internally consistent,
externally linked against an executable this disc does not carry. That is why
no USA capture finds it resident, and it is a conclusion about the *loader*
that the image's own operands reach without an emulator.

## The committed map

[`crates/asset/data/static-overlays.toml`](../../crates/asset/data/static-overlays.toml)
is the entry→base map - one record per overlay:

| Field | Meaning |
|---|---|
| `prot_index` | `PROT.DAT` entry the overlay is extracted from (the identity). |
| `base_va` | Load base inside the overlay window; statically recovered, RAM-confirmed where a capture exists. |
| `form` | `raw` (entry bytes are the as-loaded bytes) or `lzs` (decompress; needs `decompressed_size`). |
| `content_bytes` | The overlay's own content length: its PROT entry's sector extent, `(toc[p+3] - toc[p+2]) * 2048`. Present on every row; the denominator every byte-denominated instrument over the image uses. |
| `content_source` | How `content_bytes` was derived. `prot_entry_extent` on every row today. |
| `clean_copy_bytes` | Length of the RAM-verified `.text`+`.rodata` prefix (for `verified` rows). A strength-of-evidence figure, **not** a length - see below. |
| `eligibility` | `verified` (RAM byte-matched) / `static` (base-recovered + function-anchored, not RAM-prefix-verified) / `ineligible` (runtime-relocated - keep on the dynamic path). |
| `base_source` | How `base_va` was determined: `jal` (internal call-graph recovery - default; the reproducibility test asserts the recovery agrees), `capture` (byte-matched a resident RAM anchor/region), `cross_ref` (taken from another pinned RE result in-tree). |
| `anchor_va` | Optional known function VA that must land on a function prologue (`addiu sp, sp, -X`) at `base_va` - a capture-free, disc-reproducible base cross-check. Decisive for `cross_ref`/`capture` rows where the jal-recovery assertion is skipped (e.g. a slot-A minigame sibling anchored by a documented minigame function). |
| `fingerprint_sha256` | sha256 of the as-loaded bytes - the disc-derived reproducibility anchor. |
| `notes` | Which subsystems / entry points live here. |

### `content_bytes` is not `clean_copy_bytes`

The two fields look interchangeable and are not, and the failure mode is silent
in the flattering direction.

- `content_bytes` says **how long the image is**. It comes from the entry's own
  sector extent ([`prot.md`](../formats/prot.md)), which is exactly the slice the
  runtime loader streams into the overlay window, and it needs neither a dump
  corpus nor a capture to derive.
- `clean_copy_bytes` says **how much of the image a resident RAM capture has
  byte-verified**. On PROT 0898 the two coincide, because every byte of the entry
  matches RAM. On PROT 0899 they differ by `0x_f174` bytes.

Using `clean_copy_bytes` as a length makes an overlay's measured coverage look
*better* (a smaller denominator) while measuring less of it, so nothing about
the result signals the mistake. Both instruments that need an image's length -
[`disc-coverage.py`](disc-coverage.md) and the byte-attribution sweep
`scripts/ghidra-analysis/attribute-dump-extents.py` - therefore take
`content_bytes`.

Several `notes` figures for an overlay's own content predate the PROT entry-size
correction and were measured on the over-read footprint. Where such a figure
disagrees with the entry extent, the entry extent is right and the note records
what the old figure had folded in: the arena-init overlay's "own content
~`0x4800`" had absorbed PROT 0978's two sectors, and the battle overlay's
"`0x28800` of `0x29800`, trailing `0x1000` .bss" had absorbed PROT 0899's first
two sectors.

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
  0x801f/0x8020 ; addiu`) must resolve in-file at the committed base. This is
  precisely where static extraction earns its keep: the *disc* entry
  disassembles cleanly at the link base even though the *runtime* buffer is
  unusable.

  **A reference that leaves the image is not automatically evidence against
  the base.** Counting every one of them against it - which the check did -
  rejects three modules whose entry VAs the PROT 0898 tables place exactly like
  their neighbours', and rejects them for doing two things a cast module is
  supposed to do. `0x801F6978` / `0x801F6980` (PROT 0915, seven pairs) are
  *below* the slot-B base, inside PROT 0898's own image: a module reading its
  host overlay's globals. `0x801FA320..0x801FA3B8` (PROT 0935, eight pairs) and
  `0x801F7D3C` / `0x801F7F2C` (PROT 0926, two) are *above* the image's end but
  inside the shared slot-B buffer: the post-image working storage a PSX overlay
  reaches past its loaded bytes, `.bss`-shaped and by definition absent from the
  file. The reproducibility test excludes both regions from the measurement -
  neither credited nor charged - keeping the ratio a statement about
  self-references. Those three read 6/6, 1/1 and 11/11 with the exclusion, are
  mapped, and the acceptance floor rose from 0.60 to 0.90 because the noise the
  old floor accommodated is gone. The bounds come from the map itself: a
  committed slot-A row's span, and the longest committed slot-B image.

  **`pointer_resolution` is one-sided and needs the string-anchor
  counterpart.** The metric scans only pointers whose `lui` half matches the
  candidate base's own two hi-halves and counts any in-window hit - so an
  overlay that densely references **fixed structures of a co-resident overlay
  in the rival slot's VA band** can score high at a base it never loads to.
  The GAME OVER overlay (PROT 0902) is the case that proved it: 44/48 of its
  `lui 0x801f/0x8020` pairs are external references to fixed battle-band
  structures at `0x801F7724..0x801F90FC` that happen to fall inside
  `[0x801F69D8, +0x6000)`, scoring 91 % at the slot-B base - but its true base
  is slot A `0x801CE818`, pinned by the mode-18 loader chain (`FUN_80025B30` =
  `overlay_loader_a(7,0)` → PROT 902, then `jal 0x801CE844` = file `+0x2C`,
  the first prologue) and by in-file string anchors (`0x801CE820` → the
  `gameover.pak` path string at `+0x8`, `0x801CEC48` → the `GAME OVER` caption
  at `+0x430`). The hardened check is `static_overlay::string_anchor_votes`: a
  pointer that decodes to the **start of one of the file's own NUL-terminated
  string literals** only does so at the true base, so the reproducibility test
  compares votes at the committed base against the rival slot base and fails
  any anchor-less row the rival wins (0902 scores 2 at slot A vs 0 at slot
  B). One caveat bounds the metric: references to a **co-resident** overlay's
  head string table alias onto this file's own head strings when both keep
  string tables at matching small offsets (PROT 0977's arena code passes the
  slot-B `field_back_read` module's dev strings at `0x801F69D8+{0, 0x20,
  0x84}`, which alias onto its own roster strings at the same offsets), so a
  pinned prologue anchor outranks the raw vote count and exempts the row from
  the comparison.

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
  ids drive them is the open piece. The cluster also holds summon-effect data
  (0957 - its head is a summon string table, `Puera` +
  `Damage`/`Recover`/`Both` effect labels, NOT a dance song; correcting an
  earlier `overlay-ptr-table` reading). The **GAME OVER** overlay (0902) is
  **not** slot B - its old slot-B row was the `pointer_resolution` false
  positive dissected above; it is a slot-A row. The Delilas cast modules
  `0958..0960` are the decoded members of the band: RAM-anchored at the
  slot-B link base `0x801F69D8` (scenario `nivora_duel_mid_blazing_slash`
  holds 958 byte-resident there), module anatomy on
  [`cast-module.md`](../subsystems/cast-module.md); they are not yet rows
  in `static-overlays.toml`. See
  [`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).

### A slot-B image is based by its own operands, not by `scan`

The cast-band images carry no internal `jal`, so `asset overlay scan` recovers
nothing for any of them and reports a blank base (PROT 0967, at the same base, is
the exception: it calls its own leaf `FUN_801F7628` from two sites) - which reads as "no base" and is only
"no votes in the form this sweep counts". Two other forms in the image's own
operands do decide it, and both are properties of the bytes rather than of the
load base, which is what makes them evidence.

- **A head jump table.** The leading words of a module are in-image VAs
  ([`slot-b-module-layout.md`](../formats/slot-b-module-layout.md)). Their being
  in-window is a per-word test that a wrong base fails for all of them at once,
  and the word after the run is the first body's prologue. PROT `0968` has seven
  such words followed by `addiu sp,sp,-0x50`.
- **`j` targets and spawn-record pointers.** A `j` encodes its destination, and
  a spawn record's address is materialised by a `lui`/`addiu` pair and handed to
  `FUN_80050ED4` in `$a2`. PROT `0969` has no head table at all, and every one
  of its three internal `j` targets and both of its spawn records lands inside
  its own `0x800`-byte window at the slot-B base.

Both entries are battle modules that read the battle context's phase byte at
`+0x289` and cue `FUN_8004FCC8` with a [`bse.dat`](../formats/bse-dat.md)
runtime-bank id (`0x20A` / `0x20B`). Neither is in the `0903..0966` band the
three PROT `0898` entry tables reach
([`cast-module.md`](../subsystems/cast-module.md)), and neither's game-facing
identity is pinned.

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
- **The strata are also what once made jal-recovery mis-fire here.** Over the
  old over-reading window, PROT 0971's image ran into PROT 0972 from `+0x1800`,
  and 0972's code is self-consistent at `0x801CE818`, so the recovery landed on
  `0x801CE818 - 0x1800 = 0x801CD018` with a comfortable 29 votes - the same
  mechanism as the PROT 0896 cautionary tale above. Scanned over its own
  `0x1800`-byte entry, 0971 has too sparse an internal call graph to recover a
  base at all, which is why the map records `base_source = "capture"` for it.

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
and `asset overlay ghidra` writes the matching import driver
(`import_static_overlays.sh`). The driver imports each blob from `/data/<bin>`,
and `/data` is `./extracted` bind-mounted **read-only** - so the blobs reach the
container by being placed in `extracted/` on the host, not by `docker compose
cp`. Run the driver from the repo root (mirrors
[`overlay-capture.md`](overlay-capture.md), but sourced from the disc):

```bash
asset overlay extract extracted/PROT.DAT --out extracted/overlays
asset overlay ghidra  --out extracted/overlays
cp extracted/overlays/*.bin extracted/     # -> /data/<bin> inside the container
bash extracted/overlays/import_static_overlays.sh
```

A long-lived container can have a **stale** `/data` - the bind is resolved when
the service starts, so a container started before the host directory was
re-created sees an empty `/data` no matter what is in `extracted/` now, and the
`cp` above then reaches nothing. Check with `docker compose exec ghidra ls
/data` before trusting the driver. The route that does not depend on the bind is
`docker compose cp <blob> ghidra:/tmp/<blob>` followed by an `-import
/tmp/<blob>`; `/data` is mounted read-only, so a `docker compose cp` **into it**
is refused rather than silently dropped.

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

### A map row is not an import

The map is the committed artifact and the Ghidra project is not, so the two go
out of step silently and in one direction: a row added to
`static-overlays.toml` after the band was imported has an extracted image, a
verified base and no Ghidra program. Nothing reports that. `disc-coverage.py`
reports the *consequence* - the image's floor sits at 0.0% because no dump is
attributable to it - which reads as "this overlay resists analysis" rather than
as "this overlay was never opened".

Three slot-B modules sat that way: PROT 0915 (Mushura), 0926 (the id with no
tick arm) and 0935 (Earthquake), the last three to get map rows. Importing them
at `0x801F69D8` and running the ordinary frame partition covers 0915 and 0935
completely and 0926 in eight bytes, which is all the code that module has.

When a row is added to the map, import its image in the same pass. The tell that
one was missed is a `code_floor` of `0.0%` on a row whose image is on disk.

### PROT 0896: the image with a map row and no loader

The map's one foreign-build row is also the last one to be imported, and its
function map is worth recording because nothing else on this disc will ever
reach it. The image (`overlay_jp_options_status_0896.bin`, base `0x801D4DF0`)
lays out as a length-prefixed Shift-JIS label pool and jump tables at file
`0x0..0x424`, code at `0x424..0x8470`, a data segment to `0x8a18` and a
byte-curve tail to the end of the `0x9000` entry.

Analysis at that base creates fifty-one functions spanning `0x801D5214` to
`0x801DD260` and leaves two holes. The larger, file `+0x5EA0..+0x6FF8`, holds
four more: `0x801DAC90`, `0x801DAFC8`, `0x801DB35C` and `0x801DB70C`, each named
by the image's own twenty-one-word handler table at file `+0x8960` and each
opening four instructions **above** its `addiu sp, sp, -X`, which is why a
prologue scan does not see them and why the walk's `jr ra` + delay-slot split
lands on all four (`WALK_RANGES` row in `ghidra/scripts/dump_static_overlay.py`).
The smaller hole, file `+0x7E08`, is a lone `jr ra; nop` - a null routine whose
address the image holds as a word at file `+0x8708`.

`FUN_801DC410` is the page dispatcher: `lw v0, -0x28B0(at)` with
`at = 0x801E0000 + page*4` reads the handler table at `0x801DD750` and `jalr`s
the arm, after caching the page index at `0x801DD304`/`0x801DD308` and loading
the argument word from `0x80083124`. Its head prints the image's `FWIN ERR %d`
string (file `+0x3D4`) when the word at `0x801DD2F4` is set.

The code splits in two, and the split is visible in the call graph rather than
in the strings:

- **Character-status pages**, file `+0x424..+0x3B7C`. `FUN_801D5214` is their
  record loader - it scales its argument by `0x414` and reads a record array at
  `0x801CF86C`, i.e. **below** this image's own base, in the companion image
  occupying `0x801CE818..0x801D4DF0` - copying the halfwords at record `+0x00`,
  `+0x04`, `+0x0C`, `+0x0E`, `+0x10`, `+0x12`, `+0x14` and `+0x16` into the
  eight-word scratch at `0x801DD7B0`. `0x414` is the USA build's per-character
  record stride too ([`save-record.md`](../formats/save-record.md)), but the USA
  build keeps that array at `0x80084708`, not in the overlay window.
- **Options / config pages**, file `+0x3B7C..+0x8470`. Every one of them runs
  through the image's two shared leaves `FUN_801D896C` and `FUN_801D8BE0` and a
  single SCUS-range draw call at `0x8003D38C`.

Nothing here is a port target, and the reason is stronger than "no host calls
it": **no address this image names in the SCUS range denotes anything on this
disc.** Of the twenty-nine SCUS-range `jal` targets its dumps cite, not one is a
function entry in this disc's `SCUS_942.54` - none opens a frame and none
follows a `jr ra` + delay slot; every one lands mid-body, which is what a
foreign build's link edits look like read through the wrong executable. The
same caution applies to its SCUS-range *data* references: `0x8007AA14` and
`0x8007B124` are heavily used here and are not evidence about the USA globals at
those addresses. The rows are filed in
`scripts/ci/port-catalog-ignore.toml` under `[jp_options_status_overlay]` and
`[worklist_foreign_build_scus]`.

### The link-time tables are corroboration, not a discriminator

PROT 0898 carries three tables that name an entry point in every slot-B module
([`cast-module.md`](../subsystems/cast-module.md#the-entry-tables-and-where-the-addresses-live)),
so a VA in the slot-B window can be checked against a *link-time* claim about
which module owns it. `attribute-dump-extents.py` decodes all three and emits a
`resolved_by_table` class, and the shape of that rule is set by what the tables
turn out to be worth:

- They are applied **only** below the window floor - an extent whose dump
  carries one or two instructions, where the byte test alone declines to name an
  image. Above the floor the window is its own evidence.
- The table's verdict is never taken on its own. A table VA routinely names
  several modules, because many modules put their entry at the same offset; and
  a dump printed at a table VA can hold some *other* module's bytes, which is the
  case at `0x801F69EC` - the entry of PROT 0910, where five RAM-capture dumps
  hold neither 0910's bytes nor any other extracted image's. The rule therefore
  fires only when the table names exactly one module **and** that module's own
  content reproduces the window at that VA.
- It does **not** override `identical`. Byte-identical content in several
  sibling modules is genuinely present in each of them, and a coverage figure
  has to credit each; the table names only the module whose *entry* the VA is.

Measured over the whole corpus the rule signs six extents, 48 bytes, and every
one of them is an extent the short at-VA byte test already resolves to a single
image on its own. The tables add provenance, not separation - which is worth
recording, because the intuition that a link-time table must out-resolve a
two-instruction window is exactly backwards here.

## Scope + limits

- **An overlay image is exactly its entry, and older dumps are not.**
  [`Archive::read_entry`](../../crates/prot/src/archive.rs) returns
  `[entry start, next entry start)` - `size_sectors` and nothing belonging to a
  neighbour - and that is what the `asset overlay` commands read, so an
  extracted image ends where the overlay ends. The historical over-reading
  window (`toc[p+5] - toc[p+3] + 4`) survives only as the diagnostic
  `read_entry_declared_span`; see [`prot.md`](../formats/prot.md).

  The entry lengths are therefore the own-content lengths: PROT 0897 owns
  `0x25000` (to VA `0x801F3818`), 0898 `0x28800`, 0899 `0x25000`, 0971
  `0x1800`, 0972 `0xB000`.

  What that correction cost while it was outstanding: `FUN_801F5748` was read
  as the field overlay's inventory hub for a long time. `0x801F5748 -
  0x801CE818 = 0x26F30` is `0x1F30` past where 0897 ends, so those bytes are
  PROT 0898's battle dispatcher `FUN_801D0748` - a real routine at a phantom
  address. The dump was correctly *based*; the over-read image simply answered
  for a VA it does not own. Any function dump or `(entry, offset)` coordinate
  taken from an over-read image still carries that flaw - re-derive it from the
  entry rather than trusting the printed VA.

- Static extraction is for overlays that are **clean copies**. The byte-match
  catches the exceptions: an overlay whose on-disc bytes do not match the
  resident image is runtime-relocated/constructed - mark it `ineligible` and
  keep it on the dynamic path. Don't force it static.
- The fingerprint covers exactly the `read_entry` span, so it changes if and
  only if the entry's own bytes change. An overlay whose own content stops
  short of its entry (a small overlay padded out to the next sector boundary)
  still hashes the whole entry; the padding is harmless noise in the Ghidra
  disassembly - the real functions land at their real addresses.
- **An image with no internal `jal` still partitions - by frame, not by call
  graph.** Most slot-B modules call nothing inside themselves, so base recovery
  has no votes to count and Ghidra's analysis has no entry points to follow;
  the image reads as one undifferentiated blob and every dump of it is
  unattributable. The bytes still carry the partition: walk from each prologue
  (`addiu sp, sp, -X`) to the first `jr ra` whose delay slot restores *that*
  frame, and the extents fall out with no dump corpus and no capture. Two
  cross-checks make it evidence rather than a guess - each recovered head is
  named by a row of the host overlay's own entry tables, and it is named in
  that image and in no other.

  The refinement the band forced: **the table is the authority, not the
  prologue.** A module's entry can sit a few instructions *above* its prologue,
  where the routine materialises a global before setting up the frame (PROT
  0946 and 0953 both enter at `0x801F69FC` with the prologue at `0x801F6A0C`).
  A prologue scan alone reports the later address and quietly disagrees with
  the caller.
- **A reference that leaves the image is not evidence against the base.** The
  pointer-resolution vote counts in-file pointers that land on plausible
  content; a module legitimately points at its host overlay's data and at the
  `.bss` past its own end, and those misses are not base errors. Three rows
  (PROT 0915 / 0926 / 0935) stay unmapped on that gate alone.
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
