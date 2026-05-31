# Randomizer / disc patcher

Track-1-adjacent tooling that edits gameplay data on a **user-supplied** retail
disc image: it shuffles monster item drops (and, as the work lands, random
encounters and treasure contents) and writes the result back into the `.bin`.
It does not touch the clean-room engine.

Crate: [`crates/rando`](../../crates/rando/README.md) (`legaia-rando`). It ships
only code — no game bytes — and every test that needs real data is disc-gated,
so CI runs without a disc.

## Why this needs three new capabilities

Most editable values live *inside* a Legaia LZS stream that the asset
dispatcher decompresses at load. Changing one is therefore
decompress → mutate → recompress → write-back, which needed three pieces the
preservation track never had (it only ever *read* the disc):

1. **An LZS encoder** — `legaia_lzs::compress`. The retail game ships only a
   decoder (`FUN_8001A55C`); there was no way to produce a stream it accepts.
   See [LZS compression](../formats/lzs.md).
2. **Mode 2/2352 sector write-back** — `legaia_iso::write`. Overwriting the
   2048-byte user payload of a CD sector also requires recomputing its 4-byte
   EDC and 276-byte P/Q ECC, or the sector reads as corrupt. See
   [PSX disc geometry](../formats/disc.md).
3. **A disc bridge** — `legaia_rando::disc::DiscPatcher`, which ties the editing
   primitives to the sector write-back through the PROT.DAT TOC.

## Editing model: same-size, in place

Every edit overwrites bytes **in place** and never changes a byte count, so no
LBA, PROT TOC, or ISO 9660 directory record ever moves. That keeps the patch a
pure byte-overwrite (plus EDC/ECC recompute) with no risk of cascading offset
shifts. It works because the edit targets fit a fixed slot with slack:

- The monster `battle_data` archive (PROT entry 867) gives each monster a fixed
  `0x14000`-byte slot laid out `[u32 decompressed_size][LZS stream]`. The
  decoded record length is unchanged by a drop edit, and the original stream was
  produced by Sony's packer; our greedy packer is weaker but still fits the slot
  comfortably (`repack_slot` rejects the rare case where it would not). The slot
  is re-emitted zero-padded back to `0x14000`.

## CLI: `legaia-rando`

The top-level binary turns a disc + seed into a portable patch:

```bash
legaia-rando drops     --input DISC.bin                       # read-only listing
legaia-rando randomize --input DISC.bin --seed myrun --drops shuffle
legaia-rando randomize --input DISC.bin --seed 0xC0FFEE --drops random \
    --encounters shuffle --patch run.ppf --output patched.bin --manifest run.toml
legaia-rando verify    --input DISC.bin --patch run.ppf       # apply + sanity-check
```

`randomize` plans the run, applies it to an in-memory copy of the disc, diffs
the result against the original, and writes the changes as a **PPF 3.0** patch
(default `<input>.ppf`). `--output` also writes a full patched `.bin` for local
play. The seed is resolved from a number or a hashed string and always printed,
so a run reproduces exactly; the same seed yields a byte-identical patched image
and PPF. `--drops` and `--encounters` each take `shuffle` / `random` / `none`.
`--dry-run` reports the plan without writing; `--manifest` writes a small TOML
record of the seed + options + change counts (no game bytes, safe to share). The
`verify` subcommand applies a PPF to a copy of the user's disc and confirms the
result still parses end to end — a recipient's check that a shared patch + seed
match their own disc.

Because an edit changes bytes *inside* an LZS stream, the whole touched stream
is re-packed, so the changed-byte count (and the PPF) is dominated by
re-compression churn, not by the gameplay delta — this is inherent to editing
compressed data, and every edit stays same-size. `--drops random` reads the SCUS
item table off the disc for the valid item pool; the other modes need no
external table.

### Random encounters

Formations live in the per-scene MAN asset (type `0x03`, descriptor index 2 of a
scene bundle), inside an LZS stream; each formation record is
`[3 reserved][u8 count 0..4][u8 ids...]` (see
[encounter records](../formats/encounter.md)). `apply::randomize_encounters`
walks every PROT entry, and for each scene bundle it locates the MAN
(`SceneEncounters::locate`, straight from the entry bytes — no engine
dependency), decompresses it, rewrites the formation monster ids
(`Shuffle` redistributes the existing ids, `Random` draws from the pool),
recompresses, and writes the stream back over the original (the LZS decoder
stops at the descriptor's decompressed size, so a same-or-shorter re-pack is
safe). The id pool is **per scene** — only ids the scene already uses — so every
swapped-in monster is one the scene loads; no missing model, no crash.

### Treasure chests (RE done; patcher pending a field-VM walk)

A chest's item is given by the field-VM **`GIVE_ITEM` opcode `0x39`**, encoded
`[0x39, item_id]` — the item id is a **single inline operand byte** in the
per-scene field-VM event-script bytecode, not a per-scene table. (Pinned in the
dispatcher `FUN_801DE840` case `0x39` at `0x801E0448`: inventory-window setup
`FUN_8004313C` then add-by-id `FUN_800421D4(item_id, 1)`, PC += 2. The
standalone `FUN_801D71F0` add-item copy is dead/uncalled. See
[script-vm.md](../subsystems/script-vm.md).) This resolves the open RE question
("inline operand vs table") that gated the chest randomizer.

Randomizing chests is therefore a same-size byte edit (rewrite the id after each
`0x39`), but it requires an **opcode-aware field-VM walk** to find the `0x39`
sites — a naive `0x39` byte scan is unsafe (a literal `0x39` inside text or
another opcode's operand would be a false hit, the same desync that bites the
`0x3F` scan). That walker is the remaining prerequisite; it is not yet available
to the Track-1 randomizer crate (the field-VM disassembler currently lives in the
engine crates), so the chest patcher is deferred behind that infra decision.

### Re-pack slack

A scene MAN is packed with **no compressed slack** (the next asset starts right
after it), so the re-packed stream must be no larger than the original. The
[LZS re-packer's lazy matching](../formats/lzs.md#encoding-re-packing) makes that
hold for every scene MAN but one (and for every monster-archive slot). The rare
stream that still overflows is **skipped** (its scene / slot left unchanged) and
recorded in the apply report rather than aborting the run; the CLI prints the
skipped entries.

## The patch chain

A PROT-entry-relative edit maps to a disc byte range like this:

```text
disc image (2352-byte sectors)
  -> ISO 9660: PROT.DAT lives at disc sector prot_lba
    -> PROT TOC: entry N starts at start_lba[N] * 2048 bytes into PROT.DAT
      -> asset: an edit at offset_in_entry bytes into the entry
```

so a PROT-entry-relative offset becomes the PROT.DAT-logical offset
`start_lba[N] * 2048 + offset_in_entry`, which
`legaia_iso::write::patch_file_logical` turns into physical-sector writes plus
EDC/ECC re-encode. `DiscPatcher::patch_prot_entry` is the generic entry point;
`patch_monster_slot` / `monster_slot` are the `battle_data` helpers.

## EDC/ECC: not game-specific

The error-correction math is the generic CD-ROM scheme from ECMA-130 / the
Yellow Book — the same EDC (CRC, reversed polynomial `0xD8018001`) and
Reed-Solomon P/Q ECC (over GF(2⁸), generator `0x11D`) every PSX disc and every
mastering tool uses. The header (`0x00C..0x010`) is treated as zero per the
Form 1 convention, so parity is independent of the sector's MSF address. It
embeds no game bytes. The decisive correctness check is the disc-gated test that
re-encodes real PROT.DAT sectors and reproduces their stored EDC/ECC
bit-for-bit.

## Tests

| Test | Gate | What it proves |
|---|---|---|
| `crates/lzs` unit tests | CI | `decompress(compress(x)) == x` across literals, RLE, repeats, pseudorandom, >4 KB-window input; structured data actually compresses |
| `crates/asset` `lzs_compress_roundtrip_real` | disc-gated | the encoder round-trips real monster records + LZS-container sections, and compresses them |
| `crates/iso` `write` unit tests | CI | encode is idempotent / self-consistent; corrupting user data invalidates until re-encoded; ECC is address-independent; a seam-straddling patch keeps both sectors valid |
| `crates/iso` `ecc_real` | disc-gated | the encoder reproduces real PROT.DAT sectors' EDC/ECC bit-for-bit; a one-byte patch + restore round-trips a real sector exactly |
| `crates/rando` unit tests | CI | seeded planner determinism; shuffle preserves the drop multiset; surgical `set_drop`; PPF diff/write/apply round-trip; a synthetic-disc patch round-trips through the disc → ISO → PROT chain |
| `crates/rando` `disc_patch_real` | disc-gated | patch a real monster's drop onto a scratch copy of the disc; it re-decodes off the patched image with neighbours untouched and sectors valid |
| `crates/rando` `rando_cli_real` | disc-gated | full-archive shuffle: plan from a seed → apply → each monster reads its planned drop (skipped slots unchanged) → diff into a PPF that reproduces the patched image; deterministic for a fixed seed |
| `crates/rando` `encounter_patch_real` | disc-gated | whole-disc encounter shuffle: re-decode every patched scene MAN off the disc and assert counts + id multiset preserved, ids in-pool, sectors EDC/ECC-valid, deterministic |

Disc-gated tests read `LEGAIA_DISC_BIN`; with it unset they skip and pass.

## No-Sony-bytes hygiene

The crate never embeds, commits, or redistributes game bytes. A patched `.bin`
contains Sony data and is never committed; the intended distribution form is a
patcher tool + seed, and/or the **PPF patch** the CLI emits. A PPF carries only
the deltas between the user's original disc and the patched one — it is
meaningless without the original image the user already owns, so it is safe to
share where a patched `.bin` is not.

## See also

- [`crates/rando`](../../crates/rando/README.md) — the crate.
- [LZS compression](../formats/lzs.md) — the encoder this builds on.
- [PSX disc geometry](../formats/disc.md) — the Mode 2/2352 sector layout.
- [PROT.DAT TOC](../formats/prot.md) — entry → LBA addressing.
- [Monster animation](../formats/monster-animation.md) and
  [encounter records](../formats/encounter.md) — the data the randomizer edits.
