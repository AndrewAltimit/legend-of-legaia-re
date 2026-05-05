# MES dialog format

Container format for Legaia's dialog text. Two on-disc variants share an offset table + bytecode tail. The renderer that turns the bytecode into glyphs lives in the dialog overlay (uncaptured); `crates/mes` parses the container and walks the bytecode tokens but cannot map glyph indices to text yet.

## Variants

| Variant | Discriminator | Notes |
|---|---|---|
| Compact | First u16 = `0x0404` | Smaller; embedded inline in `data\battle\efect.dat` and similar. |
| Records | First two bytes = `0x44 0x78` | Used by larger dialog blobs; also the form RAM-extracted from town overlays. |

Both share a header → offset table → bytecode body shape. The token alphabet observed across known captures:

| Token | Meaning |
|---|---|
| `Glyph` | Print one glyph (the renderer maps the glyph index to a tile from the dialog font) |
| `Op65` | Dialog-control op (parameters TBD) |
| `Op4C` | Dialog-control op |
| `Op26` | Branch / jump within the bytecode |
| `End` | Terminator |
| `Unknown` | Bytes that the parser surfaces unresolved |

## Live blob example

A town-overlay save state captured a live MES blob in RAM at `0x80109270` (3893 bytes). The header + bytecode structure matches both Compact and Records expectations after small variant-specific tweaks. The blob has been used to validate the Rust parser end-to-end.

## CLI

```
mes info     <PATH>     # detect variant + report header
mes disasm   <PATH>     # walk the bytecode, print tokens
mes json     <PATH>     # emit machine-readable JSON
```

## What's missing

- The MES *renderer* (the per-MES bytecode interpreter that turns the offset-table records into a stream of glyph bytes) is in the dialog-only overlay — we don't yet have a save state for that overlay specifically. Without it, MES glyph-index streams can't be lowered to the per-byte string the [dialog font](dialog-font.md) renderer expects.

The proportional dialog font itself is now decoded — see [dialog-font.md](dialog-font.md) for the VRAM source rect, width table, and escape semantics.

The dialog opener in SCUS is `FUN_8001FD44`. It sets `_DAT_1F800394 |= 0x40` (a "dialog active" lock at the global story-flag bank) and is the function the [field/event script VM](../subsystems/script-vm.md) opcode `0x3F` calls.
