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
| `crates/rando` unit tests | CI | seeded planner determinism; shuffle preserves the drop multiset; surgical `set_drop`; a synthetic-disc patch round-trips through the disc → ISO → PROT chain |
| `crates/rando` `disc_patch_real` | disc-gated | patch a real monster's drop onto a scratch copy of the disc; it re-decodes off the patched image with neighbours untouched and sectors valid |

Disc-gated tests read `LEGAIA_DISC_BIN`; with it unset they skip and pass.

## No-Sony-bytes hygiene

The crate never embeds, commits, or redistributes game bytes. A patched `.bin`
contains Sony data and is never committed; the intended distribution form is a
patcher tool + seed (and/or a portable patch file the user applies to their own
disc).

## See also

- [`crates/rando`](../../crates/rando/README.md) — the crate.
- [LZS compression](../formats/lzs.md) — the encoder this builds on.
- [PSX disc geometry](../formats/disc.md) — the Mode 2/2352 sector layout.
- [PROT.DAT TOC](../formats/prot.md) — entry → LBA addressing.
- [Monster animation](../formats/monster-animation.md) and
  [encounter records](../formats/encounter.md) — the data the randomizer edits.
