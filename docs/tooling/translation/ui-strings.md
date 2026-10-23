# UI strings

Menu labels, battle messages and system prompts are not in any name table or
dialog segment. They are NUL-terminated C strings in data segments, reached
directly from code, so the pack addresses them by pinned VA windows. Two
sections carry them:

- `ui_menu` - strings in the **overlay** images (`ui:<prot>:0x<va>` keys);
- `system_text` - strings in `SCUS_942.54` outside the name tables
  (`scus:str:0x<va>` keys).

Both are overwritten in place, and a string longer than its span moves with
every instruction or pointer that reaches it rewritten; their room, and what
pins a string in place, is described on
[`space-and-budgets.md`](space-and-budgets.md#ui-and-system-pools).

## `ui_menu`

The section covers the pause-menu / options / shop / equip / status command
labels, the in-battle system messages and help lines, the Seru-magic effect
lines, the field overlay's shop / inn / record-screen / name-entry prompts,
the Delilas-bout preamble, the memory-card messages, the sparring-tutorial
prompts, and the steal and fatal-decision result lines.

The pools are pinned by disc-coordinate VA windows in
`legaia_translate::ui`. Every load base is the slot the overlay
occupies (see [`field-menu.md`](../../subsystems/field-menu.md) and
[`static-overlay-pipeline.md`](../static-overlay-pipeline.md)). A string is
written at `file offset = va - base_va` in its PROT overlay entry.

| Overlay | Pool | Holds |
|---|---|---|
| menu, PROT 0899 | `0x801CE81C..` | options-screen choices, `@`-marked command labels, stat labels, shop / equip / status strings |
| battle, PROT 0898 | `0x801F4B98..` | `Spirit` / `Defense` / `Escape` / `Begin`, the victory / defeat / escape / ambush messages |
| field, PROT 0897 | `0x801CF048..` | stat labels, `Cannot equip`, the shop (`Sell` / `Buy` / `Your Gold` / `Total Cost` / `OK?`), the Genesis Tree heal prompt, `Resume` / `End Game` |
| field, PROT 0897 | `0x801CF6AC..` | the new-game name-select prompts |
| battle tutorial, PROT 0967 | `0x801F7684..` | the sparring-tutorial prompts ([`battle.md`](../../subsystems/battle.md)) |
| cast modules 0941 / 0954 | `0x801F83A0..` / `0x801F8F30..` | steal results; `MP zero` / `Items lost` / `Gold lost` |
| battle, PROT 0898 (strict) | `0x801CE818..`, `0x801CED18..`, `0x801CF638..`, `0x801F6844..` | `Turns Left:` / `HP Left:`, the Hyper Arts list help, `Counterattack successful!` / `Points returned`, the Seru-magic effect lines, ` ran away.` |
| field, PROT 0897 (strict) | `0x801CF51C..`, `0x801CF650`, `0x801CF698`, `0x801CF748` | the record screen, `Give up?`, the name-entry `Is this name okay?`, `[Nameless]` |
| menu, PROT 0899 (strict) | `0x801CEC78..`, `0x801CEF18..` (three windows) | the Delilas-bout preamble, the memory-card / save messages |
| menu, PROT 0899 (strict) | `0x801E46E4..0x801E4720` | window 7's spell level-up sentence, window 8's art-learned line ([templates](#templates-whose-operands-the-renderer-patches)) |

## Templates whose operands the renderer patches

Two menu-overlay strings are templates: the renderer writes an operand into
one of their tokens before drawing them, so the token is part of the
contract, not of the text.

| String | Renderer | Rule |
|---|---|---|
| `0x801E46E4`, `{c3:01}'s magic level increased!` | `FUN_801DCCB4` (window 7) | `sb v1,0x1(a0)` at `0x801DCD14` writes the leveled spell's id into byte 1, so the line must **open** with the `{c3:..}` token |
| `0x801E4700`, `{c1:00} can now use\|the {cf:06}{c5:00}{cf:07} move!` | `FUN_801DCD58` (window 8) | `FUN_8003CBF8(template, 0xC1 / 0xC5, 1)` finds each token, stepping every `0xC0..=0xCF` token with its operand, so both tokens may sit anywhere but must stay |

The search answers index `0` when a token is missing, and the operand then
lands on byte 1 as well. Import refuses a translation that breaks either rule
(`translation::ui::PATCHED_OPERANDS`); `{c3:01} gagne un niveau !` imports,
`Niveau de {c3:01} !` does not.

## What the port reads

The port draws the game's own strings rather than English of its own, so a
translation of any of these reaches retail and the port alike:

| Image | Strings | Reader |
|---|---|---|
| menu overlay, PROT 0899 | the pause menu's rows and tab titles, the Items / Equip / Status / Magic labels, the Options rows and values, the shop strings, the Throw Out / Door of Light / Incense confirms, Yes / No, the two templates above | `legaia_game_tables::menu_pool`, `engine-menus::pause_screens`, `engine-menus::shop::ShopLabels` |
| battle overlay, PROT 0898 | `Spirit`, the commit chip's `Begin` / `Escape`, the formation banners, the victory report, the loss window's line | `legaia_ui_formats::battle_ui_strings`, `battle_party_panel::{VictoryText, DefeatText}` |
| `SCUS_942.54` | the command chips, `Reselect`, the steal captions, the sparring opener | `legaia_ui_formats::battle_ui_strings` |
| field overlay, PROT 0897 | the name-entry prompts and its Yes / No | `legaia_ui_formats::name_entry_prompts` |

Two properties of the readers make that hold for a translated disc:

- **They follow a move.** A label longer than its span moves with its
  references rewritten ([`space-and-budgets.md`](space-and-budgets.md#moving-a-label)),
  so a reader slicing the retail VA would read whatever the compaction left
  there (with every menu label lengthened, the Options row `Display Off` read
  `Immediate`). Each reader keeps one reference site per string - the
  drawing routine's `lui` / `addiu` pair, a screen-element record's payload
  word, the Options screen's or the name-entry screen's pointer table - and
  decodes it on the image at hand (`legaia_overlay_images::string_refs`).
  On retail every site forms its own VA; a site whose bytes no longer read
  as one (another build, a code mod) falls back to the retail VA.
- **They keep high glyph bytes.** An accent cell (`{82}` for `é` under
  `accents: font`, or a PAL glyph) comes back as the Latin-1 `char` of its
  byte, and `Font::layout_ascii` hands such a `char` back to the font as that
  one byte.

The victory report's second line is drawn by column: retail draws the whole
line and the two figures' number records over its blank runs, and the port
draws the line's word groups (split at runs of two or more spaces) at the
three measured columns. A translation that keeps three word groups lands on
those columns; any other shape is drawn whole from the pen, the way retail
draws it.

The disc oracles `engine-menus/tests/translated_menu_strings_disc.rs` and
`engine-battle-vm/tests/translated_battle_strings_disc.rs` fill these strings
with longer, accented translations, import them in memory, and read the
patched images back through the readers the hosts call: each returns what
the import wrote. Both check their reference tables against the import's own
reference scan of the retail images.

## English the port draws that a pack cannot reach

These lines are drawn from literals in the port, or read where a moved string
no longer is. Most visible first:

1. **Field-overlay strings read at a fixed VA**: the Incense wear-off
   notice, the code-lock header, the flag window legend, the coin counter's
   labels, the tile board's quit prompt (`engine-field::field_overlay`,
   `engine-minigames::tile_board`,
   `engine-core::incense_notice`). An in-place translation shows; one long
   enough to move is read at the retail VA.
2. **The Seru-absorb caption** (`legaia_game_tables::absorb_caption`,
   PROT 0898 `0x801F4DFC` / `0x801F4C28`): the same fixed-VA read.
3. **Port-only screens and captions**: the arts editor, key rebind, `PRESS
   START`, `ENCOUNTER!`, `K.O.`, `(no usable items)`, `(no targets)`, `No
   prizes remain.`, the `PAGE` tag, the dance minigame's `READY` / `GO!`.
   Retail has no string for them, so a pack has no key; they need a port-side
   string table of their own.
4. **The fallbacks for a load without the disc** (each reader's English
   default): drawn only when the image is absent, never on a disc load.

On a PAL disc the port reads the USA coordinates of differently laid-out
overlays ([`pal-localizations.md`](../pal-localizations.md#how-the-string-pools-pair));
the supported route to a PAL language is a lifted pack imported onto the USA
disc.

## Strict pools

A **strict** pool (`UiStringPool::strict`) covers a window that mixes
player-facing strings with debug `printf` formats and pointer tables. It walks
each string token-aware: the name token `{c1:00}` carries a `0x00` argument
that is not the terminator, so a NUL-to-NUL scan would cut
`{c1:00}'s level increased!` in two.

A chunk is kept only when it reads as UI text: glyphs and tokens only, two
letters, no `%d`-style conversion, no underscore. A label a pointer table runs
into without a NUL is keyed on its text.

The import reads the same way, so a strict string's budget and wrong-disc
guard see the whole string.

## `system_text`

The executable's system strings outside the name tables: the pause menu's
empty-list messages and equipment-slot names, the battle command chips, the
level-up lines, `All Allies` / `Reselect`, battle steal / spoils result lines,
the sparring-tutorial opener, and the equip-screen `Remove` / `Save` labels.
They are read from pinned VA windows (`translation::ui::SCUS_STRING_POOLS`)
and patched like the name tables (span + alignment padding); a longer one
moves by the code-string rules ([`space-and-budgets.md`](space-and-budgets.md#moving-a-label)),
into its own pools' compaction or the name pools' free runs.

### Battle command chips

The battle command chips are text, not icon art: each is the payload string
of a [screen-element placement record](../../reference/memory-map.md#0x80076c10---one-table-three-names)
(`Attack`, `Item`, `Run`, `Begin`, `Auto`, `Command` in the executable's
small-data pool). The magic arm draws the character's Ra-Seru name from the
battle pool instead (see
[`battle.md`](../../subsystems/battle.md#where-the-words-come-from)).
