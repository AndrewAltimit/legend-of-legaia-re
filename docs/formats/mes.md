# MES dialog format

Container format for Legaia's dialog text. Two on-disc variants share an offset table + bytecode tail. The bytecode encoding is a stream of glyph bytes interleaved with substitution opcodes; the interpreter is statically linked into `SCUS_942.54` (it is not overlay-resident).

## Variants

| Variant | Discriminator | Notes |
|---|---|---|
| Compact | First u16 = `0x0404` | Smaller; embedded inline in `data\battle\efect.dat` and similar. |
| Records | First two bytes = `0x44 0x78` | Used by larger dialog blobs; also the form RAM-extracted from town overlays. |

Both share a header → offset table → bytecode body shape.

### Compact variant — fixed header layout

```
+0x00   u32 LE = 0x00000404       ; magic
+0x04   ...                        ; unused padding (0x24 bytes)
+0x28   u32  back_ptr             ; runtime pointer (patched on load)
+0x2C   u32  forward_ptr          ; runtime pointer
+0x30   u32  expanded_size        ; byte count of the expanded blob
+0x34   u32  count                ; number of messages in the offset table
+0x38   i16[8]                    ; per-line metrics array (16 bytes)
+0x48   ...                        ; additional header fields (16 bytes)
+0x58   u16  ?                    ; pre-table header word
+0x5A   u32  ?                    ; pre-table header dword
+0x5E   u16  ?                    ; pre-table header word
+0x60   u16  ?                    ; pre-table header word
+0x62   u24 LE [N]                ; offset table — 3 bytes per entry, up to 0x56 entries
...
+0xC8   bytecode region starts here
```

`count` gives the number of messages; the offset table spans `0x62..0xC8`
(maximal extent = 0x56 u24 entries = 86 messages at this point in the structure).
Each offset is a byte offset from `0xC8` to the start of that message's bytecode.

### Records variant

The Records format has no fixed header. The parser identifies record boundaries
by scanning for recurring `0x44 0x78` marker pairs (at least 4 hits required).
Each marker starts a variable-stride record; the inter-record contents are
the bytecode and any embedded header fields. Full per-record structure is not
yet reversed — capture a town or field overlay to observe how the runtime
parses this variant.

## Bytecode encoding

Reverse-engineered from the four SCUS interpreter functions ([`FUN_8003CA38`](#fun_8003ca38--glyph-stride-walker), [`FUN_80036044`](#fun_80036044--text-width-measurement), [`FUN_80036888`](#fun_80036888--text-renderer), [`FUN_80036514`](#fun_80036514--substitution-expander)). The same byte-classification table is used by all four; only the action per byte differs.

| Byte range | Stride | Meaning |
|---|---|---|
| `0x00..0x1E` | 1 | End-of-message / line terminator. The walker stops here. |
| `0x1F..0x5D` | 1 | Single-byte glyph (font tile index). |
| `0x5E XX` | 2 | **Alias** — the substitution expander rewrites this in-place to `0xCE (XX-0x2D)`. |
| `0x5F..0xBF` | 1 | Single-byte glyph. |
| `0xC0 XX` | 2 | 2-byte wide glyph (no substitution). |
| `0xC1 XX` | 2 | Substitute character name. Reads name from save record at `0x80084708 + XX*0x414`; XX = 99 means "current party leader" (`DAT_80084597`). |
| `0xC2 XX` | 2 | Substitute item name from `PTR_DAT_8007436C[XX*3]`. |
| `0xC3 XX` | 2 | Substitute magic name from `PTR_s_Magic_800754D0[XX*3]`. |
| `0xC4 XX` | 2 | Substitute item name (different consumer site than `0xC2`; same `PTR_DAT_8007436C` table). |
| `0xC5 XX` | 2 | Substitute spell name from 2D table at `DAT_80075EC4`, keyed by `(XX>>6, XX&0x3F)`. |
| `0xC6 XX` | 2 | 2-byte wide glyph (no substitution; not in any switch case). |
| `0xC7 XX` | 2 | Substitute terrain / quest name from `DAT_80073F24 + XX*8`. |
| `0xC8..0xCD XX` | 2 | 2-byte wide glyph (stride only). |
| `0xCE XX` | 2 | Spacing op. The width-measure increments the glyph counter without emitting; the renderer uses `XX` as a horizontal offset. |
| `0xCF XX` | 2 | Skip 2 bytes (passthrough — `XX` is rendered alone, not paired with the `0xCF` prefix). |
| `0xD0..0xFE` | 1 | Single-byte glyph. |
| `0xFF` | 1 | **Alias** — the substitution expander rewrites this to `0xCF`. |

The "is this a substitution opcode?" gate in `FUN_80036044` is the integer test `(byte + 0x40) < 8`, which catches `0xC0..0xC7`. Within that range the cases `0xC1..0xC5` and `0xC7` are explicit; `0xC0` and `0xC6` fall through to "no substitution" (still 2-byte stride).

## Interpreter functions

### `FUN_8003CA38` — glyph stride walker

16-instruction primitive that returns the count of bytes (= glyphs) until the next terminator. The classification logic is just:

```c
int FUN_8003CA38(byte *p) {
  int n = 0;
  for (; *p > 0x1E; p++) {
    if ((*p & 0xF0) == 0xC0) { p++; n++; }
    n++;
  }
  return n;
}
```

Used by the dialog window pager to compute line lengths cheaply.

### `FUN_80036044` — text width measurement

Walks the bytecode and returns total width. Adds the substitution dispatch on top of the stride walker — for each `0xC1..0xC5` or `0xC7` byte, it follows the substitution pointer into the corresponding name table and recursively walks that string's width too. Calls itself implicitly by re-running the same `(byte > 0x1F)` loop on the substituted string.

### `FUN_80036888` — text renderer

The actual draw loop. Same byte classification, but emits glyphs into the text-actor buffer and forwards spacing ops to the cursor advancer. Calls [`FUN_80036514`](#fun_80036514--substitution-expander) at the start to expand substitutions into a working buffer.

### `FUN_80036514` — substitution expander

Reads source bytecode from `param_2` and writes expanded bytecode to `param_1`. Two input-time aliases are normalised:

| Source byte | Rewritten as |
|---|---|
| `0x5E XX` | `0xCE (XX - 0x2D)` |
| `0xFF` | `0xCF` |

Then it walks the input and inlines `0xC1..0xC5` / `0xC7` substitutions: each substitution opcode is replaced by the bytes of the substituted name, copied character-by-character.

## Dialog window pager — `FUN_801D84D0`

Lives in the dialog overlay (mc4 capture). Distinct from the byte-level interpreter — this is the per-frame state machine that pages text on input. 26 outer states (`_DAT_801F2734`, range `0..0x19`) covering load / scroll / drain / wait-for-input / done. Stores per-line bytecode pointers in `_DAT_801F3540[line]` (16-line buffer at `0x801F3580`). Test `(byte & 0x7F) < 0x20` is used to detect line terminators (catches both `0x00..0x1F` and `0x80..0x9F`).

The crate-level Rust port of this pager lives in [`crates/engine-vm`](../../crates/engine-vm/README.md) as the dialog-window state machine.

## Live blob example

A town-overlay save state captured a live MES blob in RAM at `0x80109270` (3893 bytes). The header + bytecode structure matches both Compact and Records expectations after small variant-specific tweaks. The blob is used to validate the Rust parser end-to-end.

## CLI

```
mes info       <PATH>             # detect variant + report header
mes disasm     <PATH>             # walk the bytecode, print decoded ops
mes json       <PATH>             # emit machine-readable JSON
mes events     <PATH> [--index N] # walk the interpreter for one message
mes stats-all  <PATH>             # event-type histogram across every message
```

## Related

- [`dialog-font.md`](dialog-font.md) — proportional dialog font in VRAM.
- [`reference/functions.md`](../reference/functions.md) — all four interpreter functions plus `FUN_8001FD44` (dialog opener).
- [`subsystems/script-vm.md`](../subsystems/script-vm.md) — script VM op `0x3F` calls the dialog opener.
