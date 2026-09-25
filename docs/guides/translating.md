# Translating the game

This guide is for translators. You need no programming: a disc image, a text
editor, and either a web browser or the `legaia-patcher` command-line tool.
You export the game's text into a file, write your translation next to each
line, and apply that file back onto a copy of your disc.

The technical reference lives in
[`tooling/translation/`](../tooling/translation/index.md); this page links into
it wherever you want the details.

## What you need

- **Your own copy of Legend of Legaia (USA)** (`SCUS-94254`) as a `.bin`
  image (Mode 2/2352), or its `.cue` sheet. The packs are keyed to the USA
  disc, so they do not apply to the European or Japanese discs.
- **A text editor** that saves UTF-8, such as Notepad++, VS Code, Kate or
  TextEdit in plain-text mode. The pack is a YAML file: plain text with
  indentation.
- **A way to apply the pack**, either:
  - the ROM patcher page of the project site, which runs in your browser and
    uploads nothing; or
  - the `legaia-patcher` program from a release archive (or built from
    source, see [getting-started.md](getting-started.md)).
- **An emulator** to play the result.

## How a translation works

The game's text is spread across the disc: item and spell names in the main
program, dialog inside each scene's data, menu labels inside the menu code.
The tools collect all of it into one file, a **language pack**. Each line of
text becomes one entry:

```yaml
  - key: 'scus:str:0x80012260'    # where the text lives on the disc
    context: 'item 0x79'          # a hint for you
    source: 'Healing Berry'       # the English text
    translation: ''               # you write here
    budget: 13                    # how many bytes fit here
```

You only ever edit `translation:`. Leave `key`, `source` and `budget` alone.
An entry you leave empty stays English, so a half-finished pack is always
playable.

The disc was never meant to be edited, so every piece of text has a fixed
amount of room. That room is the **budget**. How strict it is depends on the
kind of text; [How much room does a line have?](#how-much-room-does-a-line-have)
explains it.

## Working pack and shareable pack

A pack comes in two forms:

- The **working pack** is the one you edit. It contains the English text from
  your disc (`source:`), so you can see what you are translating. Because that
  is the game's own script, **never share a working pack**.
- The **shareable pack** contains only your translations, each tied to its
  place on the disc by its `key`. It carries no English text, so you can post
  it, send it to others, or contribute it to the project. Anyone who owns the
  disc can apply it.

You turn a working pack into a shareable one with *Make a shareable pack* on
the site, or `translate strip` on the command line.

## Path A: in your browser

The ROM patcher page does everything in the browser tab. Your disc is read on
your machine and never uploaded.

1. **Open the page and choose your disc.** Open the ROM patcher page and pick
   your `.bin` under *Disc image*.
2. **Open the Language section** at the top of the form.
3. **Export a starter pack.** With *Language* set to *None*, press
   *Export a starter pack from my disc*. You get `legaia_en.working.yaml`, a
   working pack of the whole game with every `translation:` empty. To continue
   an existing translation instead, choose that language (or *Import my own
   pack*) first; the button becomes *Export a working copy of this pack*, and
   the file has that pack's lines filled in, the English beside each one.
4. **Edit the pack.** Open the file in your text editor. Change the
   `language:` line near the top to your language code (`fr`, `pt-BR`, ...),
   add yourself under `contributors:`, and fill in `translation:` fields. Save
   often. The [text markup](#text-markup-quick-reference) section below lists
   what to keep.
5. **Load your pack.** Set *Language* to *Import my own pack (.yaml)* and pick
   your file.
6. **Check pack against my disc.** This runs the whole import in memory and
   reports, per section, how many lines would land and why the others would
   not. Nothing is written.
7. **Download skipped lines (.csv)** appears when some lines did not land. The
   spreadsheet has one row per line: its `key` (search for it in your pack),
   a short `reason`, and the full `message`. The
   [troubleshooting table](#why-did-my-line-stay-english) explains each one.
   Fix what you can and check again.
8. **Patch.** Optionally tick *Give translated dialog more room* (see
   [below](#giving-dialog-more-room)), then press *Patch & download* at the
   bottom of the page. You get a translated copy of your disc to play. The
   language patch combines with any randomizer options you also chose.
9. **Make a shareable pack.** When you are ready to share, press
   *Make a shareable pack (.yaml, no English)*. The downloaded file holds only
   your translations; that is the file to pass around.

The patched disc image is for your own play only; do not share it.

## Path B: on the command line

The same six steps with `legaia-patcher`. Replace the disc path with yours;
a `.cue` works wherever a `.bin` does.

```bash
# 1. Export the disc's text into a working pack (once).
legaia-patcher translate export --input "Legend of Legaia (USA).bin" -o legaia_en.yaml

# 2. Make an empty pack for your language.
#    Add --resume site/lang/fr.yaml to start from a published pack.
legaia-patcher translate init --lang fr --from legaia_en.yaml \
    --contributor "Your Name" -o legaia_fr.yaml

# 3. Fill in translation: fields in your editor. Working in a team?
#    `init --chunk 500` splits the pack into pieces; merge puts them back.
legaia-patcher translate merge --base legaia_fr.yaml \
    --pack legaia_fr.001.yaml --pack legaia_fr.002.yaml -o legaia_fr.yaml

# 4. Check it against your disc (nothing is written).
legaia-patcher translate stats --pack legaia_fr.yaml --input "Legend of Legaia (USA).bin"

# 5. Make the shareable pack.
legaia-patcher translate strip --pack legaia_fr.yaml -o fr.yaml

# 6. Apply it to a copy of your disc, and write a PPF patch as well.
legaia-patcher translate import --input "Legend of Legaia (USA).bin" \
    --pack legaia_fr.yaml --output legaia_fr.bin --patch legaia_fr.ppf
```

`stats` and `import` print skipped lines grouped by reason; add `--verbose`
to see every line with its key. Every flag is listed in the
[CLI reference](../tooling/translation/index.md#cli-reference).

The tool never writes to your original disc image. `--output` is a new
patched image (for your own play), and `--patch` is a small PPF file that
turns a clean disc into the translated one; a PPF is safe to share.

## How much room does a line have?

The `budget:` field is the number of **bytes** a translation may take in
place. For plain letters, one letter is one byte; a markup token such as
`{c1:00}` is two bytes. What happens when you go over depends on the kind of
text.

### Names

Item, item-type, spell, art and accessory names (and their descriptions) are
stored one after another, each in a slot rounded up to a multiple of 4 bytes.
So the spare room after a name depends on how long the English happens to be:
`Potion` has one spare byte, `Antidote` three, `Medicine` none. That is why
two names of the same length can have different budgets.

A name longer than its slot **moves automatically**: the tools pack all the
names tighter, collect the bytes that shorter translations give up, and place
the longer names there. You do not have to do anything. It only fails when the
whole table has run out of room (`no free run`); then shorten that name or
some others in the same table.

Monster names grow their record instead, up to 15 bytes, the longest name the
game already uses. Party names (9 bytes) and world-map place names (31 bytes)
are fixed fields that cannot grow.

A name that fits in bytes can still be too wide on screen: an item list has a
fixed column for the quantity, so a very long item name runs into it.

Details: [Space and budgets](../tooling/translation/space-and-budgets.md).

### Menu and battle labels

Menu commands, battle messages and system prompts sit in the game code and
cannot move. Their budget is a hard limit, and it is tight: a translation can
be shorter than the English but rarely much longer. Abbreviate. Details:
[UI strings](../tooling/translation/ui-strings.md).

### Dialog

For dialog, the budget shown on each line is only a hint. The real limit is
the **scene**. Each scene's dialog is stored compressed, and on the USA disc
each compressed scene fills its space exactly, with no room left over.

So a single line longer than the English usually still fits: the tools move
the rest of the scene along to make room. What cannot grow is the scene as a
whole. Translated text also compresses a little worse than the English, so a
scene can overflow even when no line is longer than before.

When a scene overflows, the tools put some of its lines back in English,
starting with the lines that cost the most room, until it fits. Each of those
lines is reported. Shorten them (or other lines in the same scene) and check
again. Details: [Dialog import](../tooling/translation/dialog-import.md).

### Giving dialog more room

*Give translated dialog more room* on the site, `--allow-relayout` on the
command line, removes the scene limit. A scene that overflows gets extra
2048-byte sectors, and the rest of the disc shifts along, so every line fits
at full length. The official European discs were mastered with extra room in
the same way. It costs three things:

- **The disc image grows**, usually by a few KB.
- **There is no PPF.** A PPF can only describe same-size changes, so the
  command line refuses `--patch` with `--allow-relayout`; you get only the
  patched image.
- **Save states break.** An emulator save state made on another disc layout
  no longer matches. Start from a fresh boot or a memory-card save.

It affects only dialog. Names never need it.

## Why did my line stay English?

A line stays English when its `translation:` is empty, or when the importer
skipped it. Every skip comes with a message. The table groups the messages the
way the site's coverage report and CSV `reason` column do; `N` and `M` stand
for numbers.

| Message | What it means | What to do |
|---|---|---|
| **Group: over budget** | | |
| `translation needs N bytes but the in-place budget is M (shorten the text)` | a label, fixed field or line has no way to grow | shorten it to M bytes or fewer |
| `... and the scene MAN could not be grown to fit it (shorten this line)` | the tools could not make room in this scene | shorten the line, or give dialog more room |
| `... and the script walk does not reach this line, so it cannot be relocated (shorten this line)` | this line can only be replaced in place | keep it at or under its English length |
| **Group: scene dialog does not recompress into its footprint** | | |
| `scene N: rolled back - the scene's dialog no longer recompresses into its N byte footprint (shorten this line)` | the scene was full, so this line was put back in English | shorten it or other lines of the same scene, or give dialog more room |
| **Group: name longer than the name tables have room for** | | |
| `... the name tables have no free run that long to move it into (shorten this name, or others in the same tables to free room)` | every free byte in the name table is used | shorten this name or others in the same section |
| **Group: monster name longer than 15 bytes** | | |
| `monster N: name needs N bytes but a monster name holds at most 15 ...` | the game has a 15-byte limit for enemy names | shorten to 15 bytes |
| **Group: not encodable in the retail glyph set** | | |
| `translation not encodable: 'é' (U+00E9) is not in the retail glyph set ...` | the game font has no such letter | write it without the accent (`e`) |
| `translation not encodable: malformed escape ...` | a `{` does not start a valid token | fix the token, or write a literal brace as `{7b}` |
| `translation not encodable: stray '}' ...` | a `}` without its `{` | write a literal brace as `{7d}` |
| `translation not encodable: 0xNN is not a 2-byte opcode ...` | a `{xx:yy}` token with the wrong first byte | copy the token exactly from `source:` |
| `pack source doesn't encode (corrupted pack?)` | the `source:` field was changed | restore it, or re-export |
| **Group: not on this disc (wrong image or conflicting patch)** | | |
| `disc bytes don't match the pack source (different disc revision or a conflicting patch) - skipped` | the disc does not hold the text the pack expects | use a clean, unpatched USA disc |
| `this disc's text is N bytes but the pack expects M - the pack was not built for this image ...` | the same, for a shareable pack | use a clean, unpatched USA disc |
| **Group: other** | | |
| `PROT entry N: M other line(s) of this scene no longer sit where the pack expects them ... the whole scene is skipped` | the disc was already patched, so the scene moved | import onto a fresh copy of the original disc |
| `segment framing not found at the keyed offset - skipped` | the line is not where the key says | import onto a fresh copy; do not edit keys |
| `the text framing at this offset is a coincidence inside a decoded instruction's operands ...` | the entry was never real dialog | leave it empty |
| `PROT entry N is not a dialog carrier on this disc ... - skipped` | the entry was never real dialog (older exports could list these) | leave it empty, or re-export |
| `party name must fit 9 bytes` | the party-name field is fixed | shorten to 9 bytes |
| `a monster name takes printable glyphs only ...` | an enemy name takes plain characters only, apart from its badge | use plain letters; keep the `{5e:xx}` badge and ` $N` suffix exactly as in `source:` |
| `growing the record for a N-byte name would make it larger than any retail record ...` | this enemy's record cannot grow that far | shorten the name |
| `unrecognized key shape - skipped` | the `key:` was edited | restore the key |
| `no NUL-terminated string at this VA`, `no named monster record at this id`, `scene MAN not found in this PROT entry` and similar | the key does not match this disc | check you are using the USA disc and an unedited key |

## Text markup quick reference

Most text is written as-is. A few special forms keep the game's formatting
working:

| Write | Means | Rule |
|---|---|---|
| `\|` | a new line inside the text box | move it to where your sentence should break |
| `{c1:00}` | a character's name | keep it; you may move it within the sentence |
| `{c2:..}` `{c3:..}` `{c5:..}` | an item / magic / art name | keep it |
| `{cf:..}` | a colour change | keep it, around the same words |
| `{ce:..}` and other `{xx:yy}` | spacing and icons | keep it |
| `{xx}` | a single special byte (`{01}` = item icon) | keep it |
| `{7b}` / `{7d}` | a literal `{` / `}` | use these instead of typing braces |

The substituted names come from the tables you are also translating, so write
sentences that stay grammatical whichever name appears.

**No accents.** The game's font only has the plain English letters, digits and
common punctuation (printable ASCII). There is no `é`, `ñ`, `ß` or `ç`, and no
Cyrillic, Greek, Chinese, Japanese or Korean. Write French as `Epee`, not
`Épée`. Adding letters to the font is a separate, much larger project
([font-patch scope](../tooling/translation/textures-and-fonts.md#font-patch-scope)).

**Automatic folding.** A few typographic characters that word processors and
AI tools like to insert are replaced for you: curly quotes become `'` and `"`,
en and em dashes and the minus sign become `-`, the ellipsis `…` becomes
`...`, and a non-breaking space becomes a plain space. Anything else outside
the font is reported, character by character, as not encodable.

Full reference: [Text markup and encoding](../tooling/translation/pack-format.md#text-markup-and-encoding).

## Testing your translation

- **Start from a fresh boot or a memory-card save.** An emulator save state
  holds the game's memory from the disc it was made on, including text already
  loaded. Loaded onto your patched disc, it shows the old text (or breaks, if
  the disc was relaid out).
- **Test on a copy.** Keep your original disc image untouched; the tools never
  write to it.
- **Check line breaks on screen.** A text box shows up to three lines, and
  consecutive dialog entries in the pack are consecutive rows in the same box.
  Read a scene's lines together.
- **Watch lists and labels.** Byte room is not screen width; a long item name
  can run into the quantity column.

## Glossary

| Term | Meaning |
|---|---|
| Language pack | the YAML file holding the text and your translations |
| Working pack | a pack with the English `source:` text; for your own use only |
| Shareable pack | a pack with only your translations and keys; safe to share (also called a distributable pack) |
| Key | where a piece of text lives on the disc, such as `man:4:0x210` |
| Source | the English text on your disc, shown for reference |
| Budget | how many bytes a translation may take in place |
| Byte | one letter, digit or punctuation mark; a `{xx:yy}` token is two |
| Scene | one area of the game; its dialog is stored compressed as one block |
| Footprint | the fixed space a scene's compressed dialog occupies on the disc |
| Relayout | growing a scene by whole sectors, shifting the rest of the disc |
| PPF | a small patch file that turns a clean disc into the patched one |
| Token | a `{...}` code standing for a name, colour, icon or special byte |

## FAQ

**Is there a flag to let names grow, like `--relocate-names`?** No. A name
longer than its slot moves automatically on every import. There is nothing to
switch on.

**Can I test with an emulator save state?** No. A save state carries the
memory of the disc it was made on, so it shows old text and breaks on a
relaid-out disc. Boot the patched disc fresh, or load a memory-card save.

**What may I share publicly?** The shareable pack (made with
*Make a shareable pack* or `translate strip`) and a PPF patch. Never share a
working pack, a lifted pack, or a patched disc image: they contain the game's
own text and data.

**Can I translate into Russian, Greek, Japanese, Chinese or Korean?** Not with
the retail font, which has only plain English letters. It would need a font
patch, which these tools do not do.

**Can I start from someone else's translation?** Yes. On the site, choose that
language (or import their pack) and press *Export a working copy of this pack*.
On the command line, pass `--resume their-pack.yaml` to `translate init`.

**Can I combine a translation with the randomizer?** Yes. On the site, pick a
language and any randomizer options, then patch once. The dialog is applied
before the randomizer and the names after it, so neither disturbs the other.

**Why is my dialog line fine but still reported?** Dialog is limited per
scene, not per line. Another line in the same scene may have used the room;
shortening any line of that scene helps.

**Can I apply a pack to the European disc?** No. Packs are keyed to the USA
disc. If you own a European or fan-translated disc, the site's
*Translation from another disc I own* option (or `translate lift-official`)
turns its text into a pack for the USA disc; see
[pal-localizations.md](../tooling/pal-localizations.md).

**Can I translate the title screen logo?** Some text is drawn as a picture,
not stored as text. Those need replacement artwork; see
[textures and fonts](../tooling/translation/textures-and-fonts.md).

## Where to go next

- [Translation reference](../tooling/translation/index.md) - pack shapes, the
  workflow, what may be committed, and every CLI flag.
- [Pack format](../tooling/translation/pack-format.md) - the YAML schema,
  every section and key shape, markup.
- [Space and budgets](../tooling/translation/space-and-budgets.md) - how much
  room every kind of text has, and why.
- [Dialog import](../tooling/translation/dialog-import.md) - how scenes are
  rewritten, rolled back and relaid out.
- [UI strings](../tooling/translation/ui-strings.md) - menu and battle labels.
- [Textures and fonts](../tooling/translation/textures-and-fonts.md) - text in
  pictures, and the font.
- [pal-localizations.md](../tooling/pal-localizations.md) - lifting text from
  an official European or fan-translated disc.
- [modding-and-translation.md](modding-and-translation.md) - the rest of the
  patcher: randomizer, monster edits, textures.
