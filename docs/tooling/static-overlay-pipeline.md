# Static overlay-extraction pipeline

Most of Legaia's gameplay code lives in RAM **overlays** paged into the
`0x801C0000+` overlay window per game mode (title / field / battle / menu /
world-map / cutscene / minigames). The established way to reverse them is to
capture an emulator save state and import the live RAM image into Ghidra at its
runtime base — see [`overlay-capture.md`](overlay-capture.md).

This page documents the **static** complement: extracting each overlay directly
from `PROT.DAT` and disassembling it at its load base, with identity attached
from the first byte. It **complements** the dynamic captures; it does not
replace them (see [Scope + limits](#scope--limits)).

Implementation: [`legaia_asset::static_overlay`](../../crates/asset/src/static_overlay.rs);
CLI `asset overlay …`; committed map
[`crates/asset/data/static-overlays.toml`](../../crates/asset/data/static-overlays.toml).

## Why static extraction works

PSX overlays are normally **clean copies** of a fixed-VA-linked blob: the loader
DMAs the bytes into the overlay window, runs `FlushCache`, and jumps in — there
is no per-load relocation. Legaia's overlay code ships as MIPS-code entries
inside `PROT.DAT` (the [`mips_overlay`](../formats/mips-overlay.md) /
[`overlay_ptr_table`](../formats/overlay-ptr-table.md) detectors flag the small
ones; the big scene overlays are raw too, just data-section-first). So the
on-disc entry **is** the loaded code, modulo the runtime-written `.bss`.

This is proved two ways:

- **Static reproducibility.** The as-loaded bytes extracted from any copy of the
  disc hash to a committed sha256 (`asset overlay verify`). No Sony bytes are
  committed — only the hash.
- **Runtime byte-match** (disc + save-state gated). The on-disc bytes are
  byte-identical to the resident RAM image over the entire `.text`+`.rodata`
  region. For the battle overlay (PROT 0898 at base `0x801CE818`) the on-disc
  bytes match RAM for the first `0x28800` of `0x29800` bytes — 100 % of
  code+rodata — with only the trailing `0x1000`-byte `.bss` diverging (the
  runtime zeroes / writes it after the copy). Test:
  [`crates/mednafen/tests/static_overlay_clean_copy.rs`](../../crates/mednafen/tests/static_overlay_clean_copy.rs).

## What it buys (and the limit)

- **Solves the VA-aliasing identity problem structurally.** Many overlays link
  to the same VA range — `0x801DD864` is a battle-action function in one overlay
  and a muscle-dome function in another — which is why the repo disambiguates
  with `overlay_<label>_<addr>` naming + behavioural fingerprints. Statically,
  an overlay is **"PROT entry N at base X"**: identity from the source entry, not
  a guessed label.
- **Reproducible from the user's disc**, with no curated save state — including
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
historical "PROT 0896 = options/pause-menu overlay" label is wrong on two
counts: PROT 0896 (CDNAME `bat_back_dat`) recovers a self-consistent base of
`0x801C5818` and is the mode-24 OTHER overlay (the options-menu equipment
aggregator `FUN_801CF650` lands in its *string* section there). The **real
options/menu overlay is PROT 0899** at base `0x801CE818` — found by byte-searching
the corpus for `FUN_801CF650`'s instruction signature (`0x801CF650` ↔ PROT 0899
file `0xe38`), corroborated by 101/139 captured menu-dump functions aligning as
prologues and by jal-recovery (30 votes). PROT 0899 and the field overlay
(PROT 0897) are **VA-alias siblings in slot A** — both load at `0x801CE818` at
different times, so `0x801CF650` is a `"Give"` string in 0897 but the equip
aggregator in 0899. That is the exact aliasing this pipeline exists to
disambiguate. (PROT 0896's own identity-resolving capture is still open; see
[`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).)

## The committed map

[`crates/asset/data/static-overlays.toml`](../../crates/asset/data/static-overlays.toml)
is the entry→base map — one record per overlay:

| Field | Meaning |
|---|---|
| `prot_index` | `PROT.DAT` entry the overlay is extracted from (the identity). |
| `base_va` | Load base inside the overlay window; statically recovered, RAM-confirmed where a capture exists. |
| `form` | `raw` (entry bytes are the as-loaded bytes) or `lzs` (decompress; needs `decompressed_size`). |
| `clean_copy_bytes` | Length of the RAM-verified `.text`+`.rodata` prefix (for `verified` rows). |
| `eligibility` | `verified` (RAM byte-matched) / `static` (base-recovered + function-anchored, not RAM-prefix-verified) / `ineligible` (runtime-relocated — keep on the dynamic path). |
| `base_source` | How `base_va` was determined: `jal` (internal call-graph recovery — default; the reproducibility test asserts the recovery agrees), `capture` (byte-matched a resident RAM anchor/region), `cross_ref` (taken from another pinned RE result in-tree). |
| `fingerprint_sha256` | sha256 of the as-loaded bytes — the disc-derived reproducibility anchor. |
| `notes` | Which subsystems / entry points live here. |

### Slot A vs slot B

The overlay loaders manage two independently swappable slots (`*DAT_8001038C`
and `*DAT_80010390`; see [`prot.md`](../formats/prot.md#overlay-loaders-parallel-slots)).

- **Slot A** (`~0x801CE818`) holds the big scene overlays — field (0897), battle
  (0898), menu (0899). These are VA-alias siblings (same base, resident at
  different times) and have dense internal call graphs, so `base_source = jal`.
- **Slot B** (link base `0x801F69D8`,
  `summon_overlay::SUMMON_OVERLAY_LINK_BASE`) holds the player-summon / effect /
  minigame-data blobs from the `0900..0969` PROT cluster. These **timeshare one
  buffer**, so a save state catches an inseparable *mix* of two overlays (e.g. in
  a mid-cast Gimard save the 0900 render overlay has overwritten the 0905
  stager) — there is no clean whole-overlay RAM prefix, and most have too sparse
  an internal call graph to jal-recover. Their base comes from a capture anchor
  (`base_source = capture`; the 0900 render region `0x801F79D8..0x801F8DD8`
  byte-matches RAM, pinning the base) or a cross-referenced RE result
  (`base_source = cross_ref`). This is precisely where static extraction earns
  its keep: the *disc* entry disassembles cleanly at the link base even though
  the *runtime* buffer is unusable.

  **The slot-B cluster is heterogeneous.** It interleaves summon stagers (0905
  Gimard, …) with Disco King **dance-song** overlays (0907 "Hell's Music", 0924
  "Ultimate Rave", 0927 "Dark Eclipse", 0957) and other minigame data — so the
  historical contiguous "summon `0905..=0915`" range is over-broad (0907 is a
  dance song, not a summon). Only entries with a pinned identity are listed in
  the map; per-entry identity for the rest is open
  ([`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md)).

## CLI

```bash
# Inspect the map.
asset overlay list

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

- Static extraction is for overlays that are **clean copies**. The byte-match
  catches the exceptions: an overlay whose on-disc bytes do not match the
  resident image is runtime-relocated/constructed — mark it `ineligible` and
  keep it on the dynamic path. Don't force it static.
- The fingerprint covers the `read_entry` footprint. For a few entries that
  footprint over-reads adjacent shared sectors past the real overlay (the
  on-disc entries overlap); the over-read tail is harmless noise in the Ghidra
  disassembly — the real functions still land at their real addresses.
- This pipeline does **not** address runtime values. The dynamic-capture
  workflow ([`overlay-capture.md`](overlay-capture.md),
  [`pcsx-redux-automation.md`](pcsx-redux-automation.md)) remains essential and
  authoritative for `gp`-relative globals, watchpoint results, and any value
  the overlay constructs at run time.

## See also

- [`overlay-capture.md`](overlay-capture.md) — the dynamic save-state capture
  workflow this complements.
- [`mips-overlay.md`](../formats/mips-overlay.md) /
  [`overlay-ptr-table.md`](../formats/overlay-ptr-table.md) — the detectors that
  flag overlay-code PROT entries.
- [`prot.md`](../formats/prot.md) — TOC math (indexed vs footprint) the
  extraction reads.
- [`boot.md`](../subsystems/boot.md) — the overlay loaders + the overlay window.
