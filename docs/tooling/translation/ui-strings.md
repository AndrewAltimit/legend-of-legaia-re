# UI strings

Menu labels, battle messages and system prompts are not in any name table or
dialog segment. They are NUL-terminated C strings in data segments, reached
directly from code, so the pack addresses them by pinned VA windows. Two
sections carry them:

- `ui_menu` - strings in the **overlay** images (`ui:<prot>:0x<va>` keys);
- `system_text` - strings in `SCUS_942.54` outside the name tables
  (`scus:str:0x<va>` keys).

Both are overwritten in place and never move; their room is described on
[`space-and-budgets.md`](space-and-budgets.md#ui-and-system-pools).

## `ui_menu`

The section covers the pause-menu / options / shop / equip / status command
labels, the in-battle system messages and help lines, the Seru-magic effect
lines, the field overlay's shop / inn / record-screen / name-entry prompts,
the Delilas-bout preamble, the memory-card messages, the sparring-tutorial
prompts, and the steal and fatal-decision result lines.

The pools are pinned by disc-coordinate VA windows in
`legaia_patcher::translation::ui`. Every load base is the slot the overlay
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
and patched like the name tables (span + alignment padding), except that they
never move.

### Battle command chips

The battle command chips are text, not icon art: each is the payload string
of a [screen-element placement record](../../reference/memory-map.md#0x80076c10---one-table-three-names)
(`Attack`, `Item`, `Run`, `Begin`, `Auto`, `Command` in the executable's
small-data pool). The magic arm draws the character's Ra-Seru name from the
battle pool instead (see
[`battle.md`](../../subsystems/battle.md#where-the-words-come-from)).
