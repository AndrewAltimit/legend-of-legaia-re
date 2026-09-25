# Translation / language packs

`legaia-patcher translate` turns the user-facing text of a retail disc you
supply into an editable **YAML language pack**, and applies a filled pack back
onto a copy of that disc. It is built for community translations: export,
edit the `translation:` fields in any text editor or script, import.

Nothing is redistributed. The pack is generated from your own disc, and what
you share is a PPF patch or, better, a *filled* distributable pack plus these
instructions.

New to this? Start with the beginner guide,
[Translating the game](../../guides/translating.md). This folder is the
reference:

| Page | Covers |
|---|---|
| [`index.md`](index.md) (this page) | Pack shapes, the workflow, what may be committed, the site, coverage, and the full CLI reference. |
| [`pack-format.md`](pack-format.md) | The YAML schema, sections and key shapes, text markup and encoding, line granularity. |
| [`space-and-budgets.md`](space-and-budgets.md) | How much room each kind of text has, where that room comes from, and what happens when a line does not fit. |
| [`dialog-import.md`](dialog-import.md) | How scene dialog is rewritten and recompressed, rollback, relayout, streaming dungeon scenes, the dialog-carrier gate, and the ordering around the randomizer. |
| [`ui-strings.md`](ui-strings.md) | The overlay `ui_menu` pools, strict pools, `system_text`, and the battle command chips. |
| [`textures-and-fonts.md`](textures-and-fonts.md) | Text baked into textures, and the scope of a font patch. |

Implementation: [`crates/patcher/src/translation/`](../../../crates/patcher/src/translation/)
(the module docs cover the internals). Writes go through
[`legaia_patcher::disc::DiscPatcher`](../randomizer.md): every touched sector's
EDC/ECC is re-encoded. No LBA moves unless you opt into the whole-sector
relayout ([`dialog-import.md`](dialog-import.md#disc-relayout---allow-relayout)).

## Two pack shapes

A pack comes in two shapes that share one schema:

| Shape | `source:` | Holds | Lives |
|---|---|---|---|
| **working** | yes | the disc's own text (for the translator to read) | your machine only - never redistributed |
| **distributable** | no | only the *new* translated text, keyed by disc coordinates | shareable / committable |

`translate strip` turns a filled working pack into a distributable one. It
drops every `source:` and `context:` field and every unfilled entry, leaving a
pure `key -> translation` lookup table plus the byte-budget hint.

The `key` (`scus:str:0x<va>`, `scus:party:<n>`, `man:<prot>:0x<off>`,
`raw:<prot>:0x<off>`, ...; all shapes in
[`pack-format.md`](pack-format.md#sections-and-key-shapes)) is a disc
coordinate, not text, so a distributable pack carries none of the original
script. **That is the only shape it is safe to commit or publish.** The shipped
packs at [`site/lang/`](../../../site/lang/) are exactly this, and a disc-free
test (`translation_shipped_packs.rs`) fails the build if any tracked pack
still carries a `source:` field.

A distributable pack has no source to self-check against, so its `budget` is
only a *hint*. `import` (and `translate stats --input`) re-measures every
target on the disc being patched - the string's own span, the segment's own
`0x1F .. 0x00` framing - and rejects any entry that doesn't fit, or whose
on-disc length disagrees with the hint. That length check is the wrong-disc
guard a working pack gets from its `source`. Same-size in place is enforced
from the disc, never from the pack.

## Workflow

Every disc argument takes a raw Mode 2/2352 `.bin` or a `.cue` sheet
(resolved to the `.bin` it references). `stats` and `import` summarize
skipped / over-budget entries per reason; pass `--verbose` to print every
entry individually.

```bash
# 1. Dump the source text into a working pack (once).
legaia-patcher translate export --input "Legend of Legaia (USA).bin" -o legaia_en.yaml

# 2. Make a skeleton for your language (fr de es it pl pt-BR ja ru zh ko ...).
#    --resume seeds it from an already-published pack so you can keep editing a
#    shipped translation without anyone redistributing the source.
legaia-patcher translate init --lang fr --from legaia_en.yaml \
    --contributor "you" [--resume site/lang/fr.yaml] -o legaia_fr.yaml

# 3. Fill `translation:` fields (editor, script, AI pass - your choice).
#    --chunk N splits the skeleton into N-entry files for a parallel bulk fill.
#    Recombine them onto the base pack (--pack repeatable, applied in order;
#    every input is read before the output is written, so -o may be the base).
legaia-patcher translate merge --base legaia_fr.yaml \
    --pack legaia_fr.001.yaml --pack legaia_fr.002.yaml -o legaia_fr.yaml

# 4. Check coverage + encodability/budget. Add --input to dry-run the pack
#    against a real disc (the only way to validate a distributable pack).
legaia-patcher translate stats --pack legaia_fr.yaml [--input DISC.bin]

# 5. Publish: strip the source to make the distributable, committable pack.
legaia-patcher translate strip --pack legaia_fr.yaml -o site/lang/fr.yaml

# 6. Apply to a scratch copy (and/or emit a shareable PPF).
legaia-patcher translate import --input "Legend of Legaia (USA).bin" \
    --pack legaia_fr.yaml --output legaia_fr.bin --patch legaia_fr.ppf
```

Entries with an empty `translation:` are left byte-identical on the disc, so a
partially filled pack is always playable.

Import is **idempotent**: re-running the same pack over a patched image
applies nothing. It is also **incremental**: fill more entries, re-import onto
a fresh copy. What import does when a scene's lines have moved, or when a
scene's dialog no longer fits, is on
[`dialog-import.md`](dialog-import.md).

### What may / may not be committed

- **Distributable packs may be committed** - they are new authored text plus a
  coordinate table, and the shipped `site/lang/*.yaml` packs are tracked.
- **Working packs must not** - they carry the game's script. `/translations/`
  and `legaia_*.yaml` stay gitignored; keep every source-bearing pack there.
- No disc / exe / asset bytes, ever.

### On the site

The in-browser ROM patcher ([`site/js/rom-patcher-app.js`](../../../site/js/rom-patcher-app.js))
offers the shipped packs directly (a language dropdown, default **None**), plus
an *import my own pack* path and an *export a starter pack from my disc*
button. The page's controls map onto the CLI:

| Page control | Does | CLI / WASM equivalent |
|---|---|---|
| Export a starter pack from my disc | a fresh working pack of your disc's text | `translate export`; WASM `export_lang_pack` |
| Export a working copy of this pack | with a pack chosen: its filled lines copied onto a fresh export, English beside each | `init --resume`; `export_lang_pack`'s `resume` argument |
| Make a shareable pack | shown for an imported pack; strips the English out | `translate strip`; `strip_lang_pack` |
| Check pack against my disc | disc-measured dry run with the coverage report | `stats --input`; `validate_lang_pack` |
| Give translated dialog more room | the whole-sector relayout | `import --allow-relayout`; `patch_rom`'s `lang_relayout`, `validate_lang_pack`'s `relayout` |
| Download skipped lines (.csv) | every skipped line of the last check or patch | the `issues` array of the report |
| Translation from another disc I own | lifts a second disc's text in the tab | `translate lift-official`; `lift_official_pack` |

The relayout checkbox runs in the dialog phase, before every randomizer pass,
and the check dry-runs it too, so the check predicts what the patch lands. The
download is then the grown `.bin`; the page ships no PPF in any case, so
nothing changes for the output shape.

The skipped-lines CSV has three columns: `key`, `reason` and `message`. A
translator editing the YAML in their own tools can find each line by its key.
The `reason` column is the grouped reason the coverage report counts (see
[Why did my line stay English?](../../guides/translating.md#why-did-my-line-stay-english)).

The page applies the language pack in **two phases around** the randomizer
passes ([ordering](dialog-import.md#ordering-dialog-before-the-randomizer-names-after))
via `patch_rom`'s `lang_pack` argument, and validates a chosen pack against
the user's disc with `validate_lang_pack` before patching.

The *translation from another disc I own* path takes a second user-supplied
disc - an official PAL localization or a fan-patched disc of any Latin build
(see [`pal-localizations.md`](../pal-localizations.md#lifting-a-fan-translation)).
It runs `translate lift-official` in the tab via `lift_official_pack` -
neither disc is uploaded - then feeds the lifted pack through the same
`lang_pack` argument. It is honest about the fit: most of the official dialog
does **not** fit the USA disc's sector-aligned scenes, and the coverage report
says how much landed and why the rest did not. See
[`pal-localizations.md`](../pal-localizations.md#in-the-browser).

After a patch (and on validate) the page shows the **per-section coverage
report**: applied / skipped counts per section plus a skip-reason breakdown
(over budget, scene does not recompress, not on this disc, not encodable),
from the `lang` / `report` object `patch_rom` / `validate_lang_pack` return.
Nothing is uploaded; the packs are static assets fetched from `site/lang/`.

## Coverage

Covered:

- the SCUS name tables: items, item types, spells, Tactical Arts, accessory
  passives, party names;
- the `0x1F`-segment dialog corpus (scene bundles + raw event-script carriers):
  NPC dialog, cutscene dialog and narration, picker labels, chest flavor text;
- the overlay-resident UI strings (`ui_menu`) and the SCUS system strings
  outside the name tables (`system_text`), described on
  [`ui-strings.md`](ui-strings.md);
- the world-map place-name cells (`place_names`);
- the enemy names (`monster_names`).

Not covered (out of scope for this pipeline):

- textures with baked-in text (title screen, save/load UI, boot logos, the
  opening's `It was the Seru.` caption); these are enumerated with their
  footprint constraints on [`textures-and-fonts.md`](textures-and-fonts.md);
- the world-map label table trailing each kingdom MAN and each scene MAN's
  section-2 banner name ([`place-names.md`](../../formats/place-names.md)) -
  two of the three carriers a place name has; only the SCUS quick-travel
  cells are in the pack;
- raw lines the segment scanner declines. Outside a walked MAN the scanner is
  conservative by design: a raw line that fails its quality gate is simply
  not exported and stays English
  ([which segments count as dialog](pack-format.md#which-segments-count-as-dialog)).
  Junk entries the scanner does export (dev-debug strings, menu-chrome labels
  such as `=Next=`) are harmless: leave them untranslated.

## CLI reference

Every subcommand lives under `legaia-patcher translate`. `--help` on any of
them prints the same flags with their full descriptions.

### `export`

Exports every cataloged user-facing string from a disc into a working pack
with empty `translation:` fields. The output carries the game's text: keep it
local, never commit it.

| Flag | Meaning |
|---|---|
| `--input <DISC>` | the retail disc image (`.bin`, or a `.cue` resolved to its `.bin`) |
| `-o, --output <PACK>` | where to write the pack (YAML) |

### `init`

Produces an empty per-language skeleton from an exported pack: same keys,
sources and budgets, cleared translations, stamped header. Give it either
`--from` or `--input`.

| Flag | Meaning |
|---|---|
| `--lang <CODE>` | target language code (`fr`, `de`, `es`, `it`, `pt-BR`, ...); non-Latin scripts also need a [font patch](textures-and-fonts.md#font-patch-scope) |
| `--from <PACK>` | an existing exported pack to derive from |
| `--input <DISC>` | ...or export straight from a disc image |
| `--contributor <NAME>` | contributor names for the header (repeatable) |
| `--resume <PACK>` | pre-fill from an existing working or distributable pack, matched by key (e.g. a shipped `site/lang/*.yaml`) |
| `--chunk <N>` | also split the skeleton into chunk files of at most N entries (`<output stem>.001.yaml`, ...); recombine with `merge` |
| `-o, --output <PACK>` | where to write the skeleton |

### `merge`

Merges the filled entries of several packs into the first one, matched by key
(chunks of a bulk fill, or a shipped pack plus your edits). Every input is
read before the output is written, so `-o` may name the base.

| Flag | Meaning |
|---|---|
| `--base <PACK>` | base pack; defines the entry set (keys, sources, budgets) |
| `--pack <PACK>` | a pack whose translations merge onto the base (repeatable, applied in order) |
| `-o, --output <PACK>` | where to write the merged pack |

### `stats`

Coverage and validation report: per-section translated / total counts, plus
encodability and budget checks on every filled entry. Without `--input` it is
an offline check against the pack's own budgets. With `--input` it is a full
dry run: every entry is planned exactly as `import` would, in memory, and
nothing is written. That is the only way to validate a distributable pack.

| Flag | Meaning |
|---|---|
| `--pack <PACK>` | the language pack |
| `--input <DISC>` | dry-run the pack against this disc image |
| `--verbose` | print every skipped / over-budget entry instead of the per-reason summary |

### `strip`

Strips a filled pack down to the distributable shape: filled entries only,
keys + translations + the budget hint, every `source:` / `context:` removed.

| Flag | Meaning |
|---|---|
| `--pack <PACK>` | the filled working pack |
| `-o, --output <PACK>` | where to write the distributable pack |
| `--notes <TEXT>` | overwrite the pack's `notes:` header line |

### `import`

Applies a filled pack to a copy of a disc. Untranslated entries stay
byte-identical, and each touched sector's EDC/ECC is re-encoded. The tool
never writes to `--input`.

| Flag | Meaning |
|---|---|
| `--input <DISC>` | the retail disc image |
| `--pack <PACK>` | the filled language pack |
| `--output <BIN>` | write the patched image here (contains Sony bytes - local play only) |
| `--patch <PPF>` | write a portable PPF 3.0 patch here (safe to share) |
| `--allow-relayout` | let overflowing scene dialog grow its scene by whole sectors ([relayout](dialog-import.md#disc-relayout---allow-relayout)); grows the image, so it needs `--output` and refuses `--patch` |
| `--verbose` | print every skipped entry instead of the per-reason summary |

### `lift-official`

Lifts the text of another Latin-script disc into a USA-keyed working pack:
name tables id-for-id, dialog by positional segment pairing. The source is an
official PAL localization or a fan-patched disc of any Latin build; JP discs
are refused. The output carries the game's text - keep it local. Full
reference: [`pal-localizations.md`](../pal-localizations.md#lifting-an-official-translation).

| Flag | Meaning |
|---|---|
| `--from <DISC>` | the disc to lift from (a PAL SCES build, or a fan-patched disc - USA included) |
| `--target <DISC>` | the USA disc whose coordinate space the pack is keyed to |
| `-o, --output <PACK>` | where to write the filled working pack |
| `--fold-accents` | ASCII-fold the accented glyphs the NTSC font lacks ([accent folding](../pal-localizations.md#accent-folding)) |
| `--language <CODE>` | language code to stamp; defaults to the source build's own, so a fan patch names it here |
| `--baseline <DISC>` | the retail disc a fan patch was built on; every line that disc also carries is blanked, leaving only the translator's own text |

### `diff-disc`

Cross-region alignment report between the disc the importer patches and an
official localization: how well the dialog corpus aligns id- and
order-for-order, and how much of the official text fits the same-size budget.
Counts and byte values only, no game text, so it is safe to run and log.

| Flag | Meaning |
|---|---|
| `--input <DISC>` | the target disc the importer patches |
| `--other <DISC>` | the official-localization disc to align against |

### `fit-report`

Measures how much of an official localization fits the USA target under the
per-string versus per-MAN (generalized rewriter) budget, and how many scene
MANs remain sector-crossers. Counts only, safe to log.

| Flag | Meaning |
|---|---|
| `--from <DISC>` | the official-localization disc |
| `--target <DISC>` | the USA target disc |

## AI example packs

Machine-translated packs built with this pipeline are **examples / starting
points only** - the tooling exists so communities can produce and iterate on
real human translations. Prefer community packs; treat any AI-filled pack as a
draft to correct, and credit editors in `contributors`.
