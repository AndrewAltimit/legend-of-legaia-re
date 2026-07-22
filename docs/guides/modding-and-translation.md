# Modding and translation

`legaia-rando` patches a disc image you supply: randomizer features, code-hook
extras, and a full community-translation toolchain. Everything is built on
same-size in-place sector edits with the EDC/ECC re-encoded, so patched images
stay valid discs. Commands use the bare `./tool` form (source builds live at
`target/release/`).

Two safety properties to rely on:

- **The tool never writes to `--input`.** The original image is opened
  read-only; output goes to the PPF patch and (optionally) a separate patched
  copy. Keep your original dump pristine anyway - it is your only source.
- **Nothing shareable contains Sony bytes.** The PPF patch and the manifest
  are safe to share; a patched `.bin` (`--output`) is for local play only,
  never redistribute it.

## 1. Randomize (onto a scratch copy)

```bash
./legaia-rando randomize \
    --input "/path/to/Legend of Legaia (USA).bin" \
    --seed 12345 \
    --patch legaia-12345.ppf \
    --output legaia-12345.bin \
    --manifest legaia-12345.toml
```

A few seconds on a modern machine. The run prints a `disc: <label>` line
identifying the input build, the resolved numeric seed (so any run can be
reproduced), and a spoiler-safe change summary. The same seed + options over
the same disc is byte-deterministic: you and a friend get identical patches.
`--output` also writes a `.cue` beside the `.bin` so emulators can open it
directly. `--dry-run` plans without writing.

The default `--drops shuffle` re-rolls monster item drops; dozens of flags
cover encounters, chests, steals, arts combos, doors, shops, monster stats,
spell costs, equipment, and code-hook extras (`--equipment-drops`,
`--flee-exp`, `--enemy-ally`, `--shiny-seru`, ...). Read-only preview
subcommands - `drops`, `chests`, `steals`, `arts`, `doors`, `shops`,
`monster-stats`, `spell-costs`, and more - list what a feature would touch
before you commit. Full feature reference:
[randomizer.md](../tooling/randomizer.md).

## 2. The region guard

The randomizer's offsets and injected code target the USA build
(`SCUS-94254`). Running `randomize` or `verify` against any other disc - the
PAL `SCES` builds included - stops with a region error, because a USA-built
patch "applies" to a PAL disc yet produces a corrupt hybrid.
`--allow-region-mismatch` overrides the guard; only pass it when you know the
patch was built for that exact disc. The PAL discs themselves are documented
in [pal-localizations.md](../tooling/pal-localizations.md).

## 3. Share and verify a patch

Share the `.ppf` (and the manifest). The recipient applies and checks it
against their own dump in one step:

```bash
./legaia-rando verify \
    --input "/path/to/their Legend of Legaia (USA).bin" \
    --patch legaia-12345.ppf \
    --output legaia-12345.bin
```

`verify` applies the patch to a copy, confirms every record applied cleanly,
and re-parses the result. The same region guard applies here - that is what
catches "this patch was built against a different disc" before anyone plays a
corrupt hybrid.

## 4. Hand-edit one monster (stats / element / name)

The randomizer shuffles monster stats wholesale; for a curated rebalance you
edit records directly. Every monster lives in the `battle_data` archive (PROT
entry 867) as an LZS-compressed block - `monster-block` does the slot math and
the compression for you, so the edit loop is dump → hex-edit → write:

```bash
# 1. Dump the decoded block (monster-stats lists the 1-based ids).
./legaia-rando monster-block --input "/path/to/disc.bin" --id 10 --dump m10.bin

# 2. Edit m10.bin in any hex editor. The stat record starts at +0x00:
#    HP +0x0C, AGL +0x0E, MP +0x10, ATK +0x12, DEF +0x14/+0x16, INT +0x18,
#    SPD +0x1A (u16 little-endian each), element +0x1D (u8), gold +0x44,
#    exp +0x46, drop item/chance +0x48/+0x49. The name string is in the same
#    block at the offset the u32 at +0x00 points to.

# 3. Re-pack onto a copy (and emit a shareable PPF).
./legaia-rando monster-block --input "/path/to/disc.bin" --id 10 \
    --write m10.bin --output edited.bin --patch m10.ppf
```

The write is the same machinery as the randomizer - the block is recompressed
(the encoder need not match Sony's bytes; the retail decoder accepts any valid
stream), padded back into its fixed `0x14000`-byte slot, and the touched
sectors get their EDC/ECC re-encoded. With `--output` the tool re-parses the
patched image and confirms the record still decodes before declaring success.
Full record layout: [battle.md](../subsystems/battle.md); the equivalent flags
on the extracted tree are `asset monster-archive --dump-block` /
`--write-block` ([extracting-assets.md](extracting-assets.md)).

## 5. Translate the game

The `translate` family exports the disc's text as an editable YAML **language
pack** and imports a filled pack back as a same-size in-place patch
([translation.md](../tooling/translation.md) is the full reference):

```bash
# 1. Export the text (working pack - contains game text, keep it private).
./legaia-rando translate export --input "/path/to/disc.bin" -o legaia_en.yaml

# 2. Make a skeleton for your language and fill the `translation:` fields.
./legaia-rando translate init --lang fr --from legaia_en.yaml \
    --contributor "you" -o legaia_fr.yaml

# 3. Check coverage, encodability, and byte budgets as you go.
./legaia-rando translate stats --pack legaia_fr.yaml --input "/path/to/disc.bin"

# 4. Apply to a copy (and emit a shareable PPF).
./legaia-rando translate import --input "/path/to/disc.bin" \
    --pack legaia_fr.yaml --output legaia_fr.bin --patch legaia_fr.ppf
```

The constraint that shapes every entry: **same-size in place**. Each string's
`budget:` is its maximum encoded byte length; `stats` and `import` re-measure
every target on the disc and skip anything that does not fit, summarizing
skips per reason (`--verbose` for the full per-key list). Partially filled
packs are always playable - untranslated entries stay byte-identical. The
retail font is printable-ASCII-only, so accented text must be written
unaccented (`Epee`, not `Épée`).

Before sharing a filled pack, `translate strip` removes every `source:` field,
leaving only your translations keyed by disc coordinates - the only shape that
is safe to publish. `merge` recombines chunked packs.

Owners of an official PAL disc can lift its French / German / Italian text
onto USA coordinates instead of translating from scratch:

```bash
./legaia-rando translate lift-official --from "/path/to/SCES-disc.bin" \
    --target "/path/to/USA.bin" -o legaia_de.yaml
```

`diff-disc` and `fit-report` are the text-free measurements behind this - how
well the two discs align and how much official text fits the USA budgets. See
[pal-localizations.md](../tooling/pal-localizations.md).

## 6. Inspect saves

`save-tool` reads PSX memory-card images (`.mcr` and friends, e.g. from an
emulator's card directory):

```bash
./save-tool dir   card.mcr                 # directory entries (magic-checked)
./save-tool saves card.mcr                 # active save blocks + product codes
./save-tool character card.mcr --block 1 --offset 0x5C8
./save-tool party     card.mcr --block 1 --offset 0x5C8 --count 3
```

In a retail Legaia save block the character-record array starts at block
offset `0x5C8` with a `0x414`-byte stride: `0x5C8` is the first character,
`0x9DC` the second, and so on. Each record's display name sits at
record-relative `+0x2A7`. `--offset` / `--block` accept `0x`-hex or decimal.
`sc-diff` diffs the save block of two cards to pin what a gameplay change
wrote. Record layout: [save-record.md](../formats/save-record.md).

## 7. Look things up without a disc

`gamedata-tool` bakes in the curated game-data tables (arts, magic, items,
weapons, armor, accessories, enemies, shops, casino, fishing) - handy as
ground truth beside the randomizer's read-only listings:

```bash
./gamedata-tool find "Healing Leaf"
./gamedata-tool list arts --character Vahn
./gamedata-tool arts-by-command Vahn "Arms,Ra-Seru,High"
./gamedata-tool shop "Rim Elm"
./gamedata-tool dump-json enemies
```

Sources and caveats: [gamedata.md](../reference/gamedata.md).

## 8. Cheat databases

`cheat-tool` parses GameShark / Pro-Action-Replay cheat collections and
classifies each code by the RAM region it targets. The community Legaia
NTSC-U databases are embedded, so no input file is needed:

```bash
./cheat-tool list                    # built-in databases
./cheat-tool classify                # per-category roll-up
./cheat-tool extract-offsets         # codes that land in the character record
./cheat-tool parse my-cheats.cht     # or point it at your own file
```

A file that parses to zero entries warns on stderr rather than silently
printing `[]`. Background + pinned offsets: [cheats.md](../reference/cheats.md).

## Related docs

- [randomizer.md](../tooling/randomizer.md) - every randomizer feature + how the code hooks work.
- [translation.md](../tooling/translation.md) - pack schema, markup, budgets, what may be committed.
- [pal-localizations.md](../tooling/pal-localizations.md) - the official PAL text and how it lifts.
- [save-record.md](../formats/save-record.md) - the character-record byte layout.
- [getting-started.md](getting-started.md) - back to the tool index.
