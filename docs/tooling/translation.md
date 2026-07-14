# Translation / language packs

`legaia-rando translate` turns a user-supplied retail disc's user-facing text
into an editable **YAML language pack**, and applies a filled pack back onto a
disc copy as a same-size in-place patch. It is built for community
translations: export, edit the `translation:` fields with any text editor or
script, import. Nothing is redistributed - the pack is generated from your own
disc, and the shareable artifact is a PPF patch or (better) a *filled* pack
plus these instructions.

Implementation: [`crates/rando/src/translation/`](../../crates/rando/src/translation/)
(module docs cover the internals). Writes go through
[`legaia_rando::disc::DiscPatcher`](randomizer.md) - every touched sector's
EDC/ECC is re-encoded, no LBA ever moves.

## Workflow

```bash
# 1. Dump the source text (once).
legaia-rando translate export --input "Legend of Legaia (USA).bin" -o legaia_en.yaml

# 2. Make a skeleton for your language (fr de es it pt-BR ja ru zh ko ...).
legaia-rando translate init --lang fr --from legaia_en.yaml \
    --contributor "you" -o legaia_fr.yaml

# 3. Fill `translation:` fields (editor, script, AI pass - your choice).

# 4. Check coverage + encodability/budget before burning a disc.
legaia-rando translate stats --pack legaia_fr.yaml

# 5. Apply to a scratch copy (and/or emit a shareable PPF).
legaia-rando translate import --input "Legend of Legaia (USA).bin" \
    --pack legaia_fr.yaml --output legaia_fr.bin --patch legaia_fr.ppf
```

Entries with an empty `translation:` are left byte-identical on the disc, so a
partially filled pack is always playable. Import is idempotent (re-running the
same pack over a patched image applies nothing) and incremental (fill more
entries, re-import onto a fresh copy).

**Do not commit exported packs to this repository** - they contain the game's
copyrighted text. `/translations/` and `legaia_*.yaml` are gitignored.

## YAML schema

```yaml
format: 'legaia-text-pack-v1'
language: 'fr'
game: 'Legend of Legaia (USA) SCUS-94254'
contributors: ['...']
notes: '...'
sections:
  items:               # one list per section, fixed order
  - key: 'scus:str:0x80012260'   # stable provenance key
    context: 'item 0x79'          # human context, not machine-read
    source: 'Healing Berry'       # US text, markup form
    translation: ''               # fill me
    budget: 13                    # max encoded bytes for the translation
```

Sections and their patch mechanisms:

| Section | Contents | Key shape | Mechanism |
|---|---|---|---|
| `items` | item names (MES `{c2:xx}`/`{c4:xx}` substitutions) | `scus:str:0x<va>` | overwrite the NUL-terminated string in `SCUS_942.54` in place, re-terminate |
| `item_types` | shared item "type" strings (second record pointer) | `scus:str:0x<va>` | same |
| `spells` | spell/magic names (`{c3:xx}`) | `scus:str:0x<va>` | same |
| `arts` | Tactical Arts names (`{c5:xx}`) | `scus:str:0x<va>` | same |
| `accessory_passives` | Goods-menu passive names + descriptions | `scus:str:0x<va>` | same |
| `party_names` | new-game roster names (Vahn/Noa/Gala/Terra) | `scus:party:<n>` | fixed 10-byte NUL-padded field (9-byte budget) |
| `scene_dialog` | NPC/event dialog in the scene-bundle MANs | `man:<prot>:0x<off>` | edit the `0x1F`-segment inside the LZS-decompressed MAN (space-padded to its exact length), recompress, must fit the original compressed footprint |
| `inline_text` | dialog/narration in raw carriers (v12 event-script prescripts, streaming-MAN dungeon scenes) | `raw:<prot>:0x<off>` | space-padded same-size overwrite directly in the PROT entry |

Strings pointer-shared by several table slots export once (the `context`
lists the referencing ids); interior pointers clamp the `budget`. Duplicate
PROT TOC entries over the same disc bytes are deduplicated by LBA.

The dialog sections are *line-granular*: the pager packs up to three
consecutive segments into one box (`docs/formats/mes.md`), so consecutive
entries in the pack are consecutive rows on screen. Translate them as a
group and keep each row inside its own budget.

## Text markup + encoding

The glyph atlas is indexed by byte with `0x20..=0x7E` as plain ASCII
(`docs/formats/dialog-font.md`), so markup is mostly literal text:

- printable ASCII maps to itself; `|` is the in-game newline glyph (`0x7C`);
- `{xx:yy}` is a 2-byte token: `{c1:00}` = character-name substitution,
  `{c2:79}` = item-name substitution, `{c3:..}` magic, `{c5:..}` art,
  `{cf:0n}` color change, `{ce:..}` spacing/icon escape - keep these in the
  translation wherever the source has them;
- `{xx}` is a bare byte (`{01}` item-icon prefix, high glyph tiles);
- literal braces are written `{7b}` / `{7d}`.

`encode` (string → game bytes) is the exact inverse of the exporter's decode
and reports **per-character** errors: anything outside printable ASCII is not
in the retail glyph set. Common typographic lookalikes (smart quotes, en/em
dashes, ellipsis, NBSP) are folded automatically. **Accented Latin, Cyrillic
and CJK are not encodable** - the retail font simply has no such glyphs. A
full non-Latin translation needs a font patch (new glyph tiles + width table),
which this pipeline does not attempt; French/Italian/etc. must be written
unaccented (`Epee` not `Épée`).

## Budgets (the same-size constraint)

Every patch is same-size in place, so a translation's *encoded* length is
capped by `budget`:

- SCUS strings: the original string's byte span (shorter is fine - the string
  is re-terminated; bytes past the NUL are never read).
- Dialog segments: the original segment's exact byte length. Shorter
  translations are padded with spaces so the `0x1F ... 0x00` framing (and
  every script offset around it) never moves. There is **no record resize**
  for dialog: segment pools interleave with script bytecode whose relative
  jumps assume fixed offsets, so in-place is the safe contract.
- A whole scene's edits must additionally recompress into the MAN's original
  LZS footprint. Text compresses well; if a scene ever overflows, the import
  reports it per scene and skips only that scene - shorten its longest lines
  and re-run.

`translate stats` checks all of this offline. On import each target is also
verified against the pack's `source`; a mismatch (wrong disc revision, or a
conflicting randomizer patch that moved the text) skips the entry with a
per-key warning rather than writing blind.

## Coverage + limitations

Covered: the SCUS name tables (items, item types, spells, Tactical Arts,
accessory passives, party names) and the `0x1F`-segment dialog corpus (scene
bundles + raw event-script carriers) - NPC dialog, cutscene dialog and
narration, picker labels, chest flavor text.

Not covered (out of scope for this pipeline):

- overlay-resident UI strings (battle/menu labels referenced by pointer from
  code in the PROT overlays);
- textures with baked-in text (title screen, the prologue caption TIM);
- the segment scanner is conservative by design - a dialog line that fails
  its quality gate is simply not exported and stays English. Junk entries the
  scanner does export (dev-debug strings, the odd data run that reads as
  text) are harmless: leave them untranslated.

The dialog exports the raw source lines including substitution escapes;
translations must keep grammatical agreement working around them (the
substituted names come from the tables you also translate).

## AI example packs

Machine-translated packs built with this pipeline are **examples /
starting points only** - the tooling exists so communities can produce and
iterate on real human translations. Prefer community packs; treat any
AI-filled pack as a draft to correct, and credit editors in `contributors`.
