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

## Two pack shapes

A pack comes in two shapes that share one schema:

| Shape | `source:` | Holds | Lives |
|---|---|---|---|
| **working** | yes | the disc's own text (for the translator to read) | your machine only - never redistributed |
| **distributable** | no | only the *new* translated text, keyed by disc coordinates | shareable / committable |

`translate strip` turns a filled working pack into a distributable one: it drops
every `source:` and `context:` field and every unfilled entry, leaving a pure
`key -> translation` lookup table plus the byte-budget hint. The `key`
(`scus:str:0x<va>`, `scus:party:<n>`, `man:<prot>:0x<off>`, `raw:<prot>:0x<off>`)
is a disc coordinate, not text, so a distributable pack carries none of the
original script. **That is the only shape it is safe to commit or publish** -
the shipped packs at [`site/lang/`](../../site/lang/) are exactly this, and a
disc-free test (`translation_shipped_packs.rs`) fails the build if any tracked
pack still carries a `source:` field.

A distributable pack has no source to self-check against, so its `budget` is
only a *hint*: `import` (and `translate stats --input`) re-measures every target
on the disc being patched - the string's own span, the segment's own
`0x1F .. 0x00` framing - and rejects any entry that doesn't fit, or whose
on-disc length disagrees with the hint (the wrong-disc guard `source` gives a
working pack). Same-size in place is enforced from the disc, never from the pack.

## Workflow

```bash
# 1. Dump the source text into a working pack (once).
legaia-rando translate export --input "Legend of Legaia (USA).bin" -o legaia_en.yaml

# 2. Make a skeleton for your language (fr de es it pl pt-BR ja ru zh ko ...).
#    --resume seeds it from an already-published pack so you can keep editing a
#    shipped translation without anyone redistributing the source.
legaia-rando translate init --lang fr --from legaia_en.yaml \
    --contributor "you" [--resume site/lang/fr.yaml] -o legaia_fr.yaml

# 3. Fill `translation:` fields (editor, script, AI pass - your choice).
#    --chunk N splits the skeleton into N-entry files for a parallel bulk fill;
#    recombine them with `translate merge`.

# 4. Check coverage + encodability/budget. Add --input to dry-run the pack
#    against a real disc (the only way to validate a distributable pack).
legaia-rando translate stats --pack legaia_fr.yaml [--input DISC.bin]

# 5. Publish: strip the source to make the distributable, committable pack.
legaia-rando translate strip --pack legaia_fr.yaml -o site/lang/fr.yaml

# 6. Apply to a scratch copy (and/or emit a shareable PPF).
legaia-rando translate import --input "Legend of Legaia (USA).bin" \
    --pack legaia_fr.yaml --output legaia_fr.bin --patch legaia_fr.ppf
```

Entries with an empty `translation:` are left byte-identical on the disc, so a
partially filled pack is always playable. Import is idempotent (re-running the
same pack over a patched image applies nothing) and incremental (fill more
entries, re-import onto a fresh copy). When a scene's dialog no longer
recompresses into its MAN's on-disc footprint (translated text is less
repetitive than the source), import rolls back that scene's longest lines one at
a time rather than dropping the whole scene.

### What may / may not be committed

- **Distributable packs may be committed** - they are new authored text plus a
  coordinate table, and the shipped `site/lang/*.yaml` packs are tracked.
- **Working packs must not** - they carry the game's script. `/translations/`
  and `legaia_*.yaml` stay gitignored; keep every source-bearing pack there.
- No disc / exe / asset bytes, ever.

### On the site

The in-browser ROM patcher ([`site/js/rom-patcher-app.js`](../../site/js/rom-patcher-app.js))
offers the shipped packs directly (a language dropdown, default **None**), plus
an *import my own pack* path and an *export a starter pack from my disc* button.
It applies the language pack in **two phases around** the randomizer passes
(see the ordering note below) via `patch_rom`'s `lang_pack` argument, and
validates a chosen pack against the user's disc with `validate_lang_pack`
before patching. After a patch (and on validate) the page shows the
**per-section coverage report** - applied / skipped counts per section plus a
skip-reason breakdown (over budget, scene does not recompress, not on this
disc, not encodable) - from the `lang` / `report` object `patch_rom` /
`validate_lang_pack` return. Nothing is uploaded; the packs are static assets
fetched from `site/lang/`.

## Ordering: dialog before the randomizer, names after

Combined with the randomizer, a pack is applied in two phases
(`translation::import_pack_phase`, `ImportPhase::DialogOnly` /
`ImportPhase::NamesOnly`; a phase pair reports identically to one
`import_pack` run):

- **Dialog sections (`man:` / `raw:` keys) go first.** A dialog edit is
  same-size *inside the decompressed MAN*, keyed by a byte offset into it,
  whereas the door and starting-bag passes **relocate** records
  (variable-length insertion) - moving every byte after the splice. Applied
  first, the translated text simply rides along with any later relocation.
  The reverse order is not corrupting (the framing/source check skips a
  moved key) but it silently loses the relocated scenes' lines.
- **SCUS name sections (`scus:` keys) go last.** The equipment-bonus-drop
  pass classifies gear by matching the disc's item names against curated
  English names (`legaia_rando::equipment::equipment_pool`); with the item
  table already translated its pool comes back empty and the pass aborts.
  Nothing in the randomizer relocates a SCUS string, so translating the name
  tables after every pass is always safe - and every other name-keyed pass
  tests only whether a name is non-empty, which a translation preserves. The
  overlay `ui_menu` strings ride with this phase for the same reason: no
  randomizer pass relocates or classifies an overlay string.

The randomizer otherwise reads structure - records, tables, item **ids** -
never text, so translated strings never perturb it. A standalone
`translate import` (no randomizer) applies everything in one pass.

## YAML schema

Working pack (source-bearing - the shape a translator edits):

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

Distributable pack (source-less - the shape that is committed / shipped): the
same document with `source:` and `context:` removed and only filled entries
kept.

```yaml
  items:
  - key: 'scus:str:0x80012260'
    translation: 'Baie Soin'
    budget: 13
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
| `ui_menu` | overlay-resident UI strings: pause-menu / options / shop / equip / status command labels + in-battle system messages | `ui:<prot>:0x<va>` | overwrite the NUL-terminated string in the PROT **overlay** entry in place at `file offset = va - base_va`, re-terminate (short writes zero-fill the old span) |

Strings pointer-shared by several table slots export once (the `context`
lists the referencing ids); interior pointers clamp the `budget`. Duplicate
PROT TOC entries over the same disc bytes are deduplicated by LBA. The SCUS name
pools are 4-byte aligned, so each string's `budget` also claims the 0..3 bytes
of zero padding after its terminator (verified zero per string, and never past
another pointed-to string) - about 1.5 extra bytes on average, which is what
lets the tightest name tables translate at all.

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
  LZS footprint. Text compresses well, and the repack falls back to an
  optimal-parse LZS encoder (`legaia_lzs::compress_optimal` - exact
  shortest-encoding DP, incl. back-references into the decoder's initial zero
  window) when the fast greedy parse just misses the budget, so even the
  couple of retail MANs with zero compressed slack stay editable. If a scene
  still overflows, the import rolls back its longest lines one at a time
  (each with a per-key diagnostic) rather than dropping the whole scene -
  shorten the reported lines and re-run.

`translate stats` checks all of this offline. On import each target is also
verified against the pack's `source`; a mismatch (wrong disc revision, or a
conflicting randomizer patch that moved the text) skips the entry with a
per-key warning rather than writing blind.

### The dialog-carrier gate (`raw:` writes)

The `0x1F <text> 0x00` dialog framing is short enough to occur **by
coincidence** all over the disc's binary asset banks: sequenced music
(`music_01`), VAB sample banks (`vab_01`), the battle-character mesh/animation
packs (`battle_data`, PROT 1204 `other5`), monster archives, and every scene's
first ANM slot all contain runs that read as a two- or three-letter "segment".
The per-segment quality gate (`segments::qualifies`) accepts those runs - they
are printable and letter-shaped - so a scanner that trusted them would hand a
translator a write **into binary data**. Overwriting such a coincidental hit
corrupts the asset with a same-size write that passes every framing/budget
check yet freezes the game: a garbled SEQ hangs the sound driver as New-Game
BGM starts, and a garbled PROT-1204 pack freezes the in-battle menu that
renders a character's battle form (e.g. Meta's Seru-magic list).

Both **export** and **import** therefore gate every `raw:` write on a per-entry
**dialog-carrier** check (`segments::is_dialog_carrier`): a PROT entry is a real
raw text carrier only if it carries at least `MIN_CARRIER_PROSE` prose segments
(a segment with an interior space and enough letters to be a multi-word line).
Across the retail disc the two populations separate with a wide margin - binary
banks top out at two coincidental prose hits per entry, while the smallest
genuine event-script / dungeon-MAN carrier has eight - so the gate keeps every
real carrier and refuses every binary bank. Import re-runs the check on the
disc it is patching and skips a non-carrier entry with a per-key diagnostic;
export never emits a `raw:` key for one, so freshly generated packs are clean.
SCUS name-table (`scus:`) and scene-MAN (`man:`) writes are unaffected - those
targets are structurally addressed, not scanned.

## Coverage + limitations

Covered: the SCUS name tables (items, item types, spells, Tactical Arts,
accessory passives, party names), the `0x1F`-segment dialog corpus (scene
bundles + raw event-script carriers) - NPC dialog, cutscene dialog and
narration, picker labels, chest flavor text - and the overlay-resident UI menu
strings (`ui_menu`): the pause-menu / options / shop / equip / status command
labels and the in-battle system messages, which are NUL-terminated C strings in
the menu (PROT 0899) and battle (PROT 0898) overlay data segments rather than in
any table or dialog segment. These are pinned by disc-coordinate VA windows in
`legaia_rando::translation::ui` (menu pool `0x801CE81C..`, battle pool
`0x801F4B98..`, both load base `0x801CE818`; see
[`field-menu.md`](../subsystems/field-menu.md)). They are tight: the pool is
4-byte aligned with little slack, so a same-size translation of a short label
(`@Items` is six bytes) can be shorter than English but rarely much longer - the
in-battle `Attack` / `Arts` / `Magic` / `Item` command ring is drawn as
UI-icon sprites (no text string to translate).

Not covered (out of scope for this pipeline):

- textures with baked-in text (title screen, save/load UI, boot logos, the
  in-battle command ring); these are enumerated with their footprint
  constraints under [Textures with baked-in text](#textures-with-baked-in-text)
  below;
- the segment scanner is conservative by design - a dialog line that fails
  its quality gate is simply not exported and stays English. Junk entries the
  scanner does export (dev-debug strings, the odd data run that reads as
  text) are harmless: leave them untranslated.

The dialog exports the raw source lines including substitution escapes;
translations must keep grammatical agreement working around them (the
substituted names come from the tables you also translate).

## Textures with baked-in text

Some UI text is not a string at all - it is pixels in a TIM. Those are outside
the string/dialog/`ui_menu` scope: replacing one means authoring new glyph art
at the **exact same TIM footprint** (identical width / height / bpp / CLUT
layout) so the same-size in-place `DiscPatcher` write applies. The `tim` crate
can encode a PNG back to a TIM, so a byte-identical-footprint swap is
mechanically possible; the blocker is art authoring (and, for logos, rights),
not the pipeline. None are patched here - each is a scoped follow-up. Legally,
the boot / publisher logos must be left untouched regardless.

The text-bearing textures on the retail disc:

| Texture | Where | Baked text | Footprint / notes |
|---|---|---|---|
| Title wordmark | PROT 0888 (dup 0889 / 0890), `legaia_asset::title_pak` | "Legend of Legaia" logo, `PRESS START BUTTON`, TM / (C) copyright bands; an unused `<DEMO>` band retail never samples | Bands are sub-rects of one 256x256 TIM (`TITLE_BAND_*`). The logo is title art (a proper noun); `PRESS START BUTTON` is a candidate for a same-footprint band swap. Copyright bands must stay. |
| Title menu `NEW GAME` / `CONTINUE` | title overlay (unindexed `PROT.DAT` gap between TOC 899 and 900) | rendered at runtime from the **dialog-font glyph atlas**, not a baked band (retail ignores the embedded footer band) | So this is *text*, but it lives in the title overlay code region the pipeline does not address by coordinate. Follow-up: pin the two label strings' VA window like a `ui_menu` pool. |
| Save/Load UI | PROT 0899 `+0x16908` (`SLOT n` pill) + the pre-`init_data` `PROT.DAT` gap (`Load` panel TIM) + the title-overlay memcard atlas `0x801E5120` | baked `SLOT 1..` pill label, the `Load` panel wordmark, and Japanese memcard strings in the atlas | Small 4bpp TIMs at fixed offsets; a same-footprint pill/panel swap is feasible. See [`save-screen.md`](../subsystems/save-screen.md). |
| Config-screen TIMs | PROT 0899 `+0x169DC` / `+0x1F91C` | small option-screen chrome TIMs that sit after the config **string** pool (the strings are the `ui_menu` menu labels, already translatable) | Chrome art; only replace if a label is baked rather than drawn from the string pool. |
| In-battle command ring | battle overlay UI-icon sprites | `Attack` / `Arts` / `Magic` / `Item` are drawn as **icon sprites**, not text | No string exists; a worded localization would need new icon art. This is why the `ui_menu` battle pool covers `Spirit` / `Defense` / `Escape` / `Begin` (real strings) but not the four ring commands. |
| Boot / publisher logos | PROT 0895 `init.pak` (`legaia_asset::init_pak`) - PROKION, SCEA / Sony | brand logos ("licensed by", studio marks) | **Do not alter** - trademark art, not localizable text. Listed only so a sweep does not mistake them for translatable UI. |
| Opening prologue caption | opening-sequence baked caption TIM (the narration **crawl** itself is `0x1F`-framed text and *is* covered via the dialog corpus) | a baked caption still shows English under the crawl | The crawl narration translates through `scene_dialog` / `inline_text`; only the baked caption TIM would need an art swap. |

A font patch (new glyph tiles + width table in the menu glyph atlas at
`PROT.DAT` offset `0x11218`, see [`boot.md`](../subsystems/boot.md) and
[`dialog-font.md`](../formats/dialog-font.md)) is the separate, larger effort
that would lift the printable-ASCII-only limitation for accented Latin / other
scripts across *all* text, not just these textures; it is out of scope here.

## AI example packs

Machine-translated packs built with this pipeline are **examples /
starting points only** - the tooling exists so communities can produce and
iterate on real human translations. Prefer community packs; treat any
AI-filled pack as a draft to correct, and credit editors in `contributors`.
