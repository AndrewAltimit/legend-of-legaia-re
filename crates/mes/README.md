# legaia-mes

Partial parser for Legaia MES (asset type `0x04`) blobs.

MES is the SCUS asset-type byte `0x04` in dispatcher `FUN_8001f05c`. The
dispatcher just allocates a 4-byte-aligned buffer and decodes the
payload (LZS or raw); the bytecode interpreter and format-specific
parsing live in an overlay we haven't fully reversed.

## Two on-disc layouts

Both have been observed in real RAM captures (a town-init blob and an
in-dialog blob):

### `Format::Compact`

Magic `0x00000404` (LE bytes `04 04 00 00`) followed by 36 zero bytes,
a 16-byte header of runtime-patched pointers, a 32-byte i16 array, an
8-byte "count + size" word pair, then a u24 LE offset table, then
bytecode at the end. Used for short message sets (~4 KB).

### `Format::Records`

No fixed magic. A stream of variable-stride records marked by recurring
`0x44 0x78` markers (typically every 20–36 bytes). Used for large
NPC-dialog sets.

## Bytecode tokens (observed in `Format::Compact`)

| Token | Meaning |
|---|---|
| `0x00` | end-of-message terminator (very common). |
| `0x61 XX` | print glyph `XX`. Confirmed by an observed sequential run `61 9D 61 9E ... 61 AA` - clearly an alphabet sequence. |
| `0x65 XX` | similar single-byte-arg opcode (likely "small numeric" or "wait N frames"). |
| `0x4C XX` | 2-byte token (1-byte arg) - recurring control. |
| `0x26 XX YY` | 3-byte token (2-byte arg) - possibly "page break" when arg is `0xFEFF`. |
| `0x21 0x21 0x26 0xFE 0xFF` | recurring 5-byte sequence - likely a fixed page-break / message-boundary marker. |

All other opcodes are emitted as `Token::Unknown` with the raw byte.
Future reverse-engineering of the bytecode interpreter (when a
dialog-rendering overlay is captured) will fill in the meanings.

## What this crate does NOT do

- Decode the bytecode to readable text itself. The glyph→character mapping
  *is* known - the proportional dialog font (glyph atlas + width table) is
  extracted by [`crates/font`](../font/README.md) (`font-extract --disc`, or
  the `legaia-extract` `font/` step), and the byte space `0x20..=0x7E` maps to
  plain ASCII. The text-decoding consumers live elsewhere: the engine's
  dialog renderer draws MES glyph streams through `legaia-font`, and the
  translation codec (`legaia-patcher translate export`, see
  [`docs/tooling/translation.md`](../../docs/tooling/translation.md)) is the
  user-facing dialog-text path.
- Validate offset tables against the bytecode region. The offset-table
  base/encoding (u24 LE vs another stride) is empirical and not yet
  cross-checked against the interpreter.
- Handle `Format::Records` beyond locating record boundaries.

## Bytecode interpreter

`interp::Interpreter` walks the offset-table-driven bytecode of a
`Format::Compact` blob and emits a higher-level `MesEvent` stream
(`Glyph` / `EndOfMessage` / `PageBreak` / `Op65` / `Op4c` / `Op26` /
`Unknown`). `Interpreter::render_summary` formats events as a printable
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

## CLI

```bash
mes info       <path>             # detect format + summary
mes disasm     <path>             # walk bytecode tokens
mes json       <path>             # JSON dump
mes events     <path> [--index N] # walk one message via the interpreter
mes stats-all  <path>             # event-type histogram across every message
```

## See also

- [`docs/formats/mes.md`](../../docs/formats/mes.md)
- [`docs/subsystems/script-vm.md`](../../docs/subsystems/script-vm.md)
  - opcode `0x3F` of the field VM is the dialog opener.
