# legaia-mes

Parser, bytecode interpreter and dialog pacer for Legaia MES (asset type
`0x04`) dialog blobs. The `Compact` layout is decoded end to end; the `Records`
layout is located to its record boundaries.

MES is the SCUS asset-type byte `0x04` in dispatcher `FUN_8001f05c`. The
dispatcher allocates a 4-byte-aligned buffer and decodes the payload (LZS or
raw); the byte encoding is read off the four SCUS text routines
(`FUN_8003CA38` stride walker, `FUN_80036044` glyph count, `FUN_80036888`
renderer, `FUN_80036514` substitution expander).

## Two on-disc layouts

### `Format::Compact`

Magic `0x00000404` (LE bytes `04 04 00 00`) followed by 36 zero bytes
(offsets in the `compact` constants module),
a 16-byte header of runtime-patched pointers, a 32-byte i16 array, an
8-byte "count + size" word pair, then a u24 LE offset table, then
bytecode at the end. Used for short message sets (~4 KB).

### `Format::Records`

No fixed magic. A stream of variable-stride records marked by recurring
`0x44 0x78` markers (typically every 20–36 bytes). Used for large
NPC-dialog sets.

## Bytecode tokens

`iter_tokens` streams a message as `Token`s using the shared byte
classification of the four SCUS routines:

| Bytes | `Token` |
|---|---|
| `0x00..=0x1E` | `EndOfMessage` - the walker stops. |
| `0x1F..=0x7F` (not `0x5E`), `0xA0..=0xBF`, `0xD0..=0xFE` | `Glyph` - font tile index. |
| `0xC0` / `0xC6` / `0xC8..=0xCD` `XX` | `WideGlyph` (2-byte stride). |
| `0xC1..=0xC5` / `0xC7` `XX` | `Substitute` - character / item / magic / art / terrain name. |
| `0xCE XX` (alias `0x5E`) | `Spacing` - advance without a glyph. |
| `0xCF XX` (alias `0xFF`) | `SkipTwo`. |
| `0x80..=0x9F` | `Control` - glyphs to the walker, but the dialog pager `FUN_801D84D0` halts on them. |

The input aliases are normalised in the iterator. Per-byte detail and the
name tables each substitution reads: [`docs/formats/mes.md`](../../docs/formats/mes.md#bytecode-encoding).

## What this crate does NOT do

- Decode the bytecode to readable text itself. The glyph→character mapping
  *is* known - the proportional dialog font (glyph atlas + width table) is
  extracted by [`crates/font`](../font/README.md) (`font-extract --disc`, or
  the `legaia-extract` `font/` step), and the byte space `0x20..=0x7E` maps to
  plain ASCII. The text-decoding consumers live elsewhere: the engine's
  dialog renderer draws MES glyph streams through `legaia-font`, and the
  translation codec (`legaia-patcher translate export`, see
  [`docs/tooling/translation/`](../../docs/tooling/translation/index.md)) is the
  user-facing dialog-text path.
- Handle `Format::Records` beyond locating record boundaries.

## Bytecode interpreter

`interp::Interpreter` (`FUN_80036514`) walks the offset-table-driven bytecode
of a `Format::Compact` blob and emits a higher-level `MesEvent` stream
(`Glyph` / `WideGlyph` / `Substitute` with a `SubstituteKind` / `Spacing` /
`SkipTwo` / `Control` / `EndOfMessage`). `DialogPlayer` paces it a few glyphs
per frame and pauses on `Control` bytes; `validate_compact` walks every
message and flags ones that read as non-bytecode. `Interpreter::render_summary` formats events as a printable
diff-friendly form; `EventStats` is a histogram. See
[`docs/formats/mes.md`](../../docs/formats/mes.md) for the event
catalogue.

## Option-picker decoder

`picker` decodes the multiple-choice menus embedded in field-VM inline
interaction scripts (open bytes `0x27`/`0x28`/`0x29` = 2/3/4 options). A
picker is `[open][N×2-byte i16 LE jump table][continuation][N × 0x1F label
segments]`; each 2-byte entry is a signed relative jump the inline-script
control handler `FUN_80038050` applies on confirm
(`new_pc = (open + 1 + index*2) + rel_jump`). `scan_pickers` finds every
genuine picker in an inline buffer (structural validation rejects coincidental
open bytes); `parse_picker_at` decodes one at a known offset;
`Picker::jump_target` resolves an option's branch. See
[`docs/formats/mes.md` § Picker control-region layout](../../docs/formats/mes.md).

## Dialog-box grouping

`dialog_box` groups a conversation branch's `0x1F` segments the way retail's
per-actor dialog SM (`FUN_80039B7C`) pages them: up to `LINES_PER_BOX` rows
per window, then the control byte after the last row decides what the pager
does next (`Dispatch`). `pack_box` / `pack_boxes` walk that grouping from a
starting lead - the view a caller editing a MAN's dialog needs, where what
matters is which lines share a window.

## CLI

```bash
mes info       <path>             # detect format + summary
mes disasm     <path>             # walk bytecode tokens
mes json       <path>             # JSON dump
mes events     <path> [--index N] # walk one message via the interpreter
mes stats-all  <path>             # event-type histogram across every message
mes boxes      <path> [--start H] [--limit N] [--all] [--unfiltered]
```

## See also

- [`docs/formats/mes.md`](../../docs/formats/mes.md)
- [`docs/subsystems/script-vm.md`](../../docs/subsystems/script-vm.md)
  - opcode `0x3F` of the field VM is the dialog opener.
