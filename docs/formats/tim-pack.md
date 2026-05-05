# Standalone TIM-pack format

A multi-TIM container used by certain standalone PROT entries. Distinct from the [pack format](pack.md) used inside streaming chunks.

Implementation: `crates/prot/src/timpack.rs`.

## Header (8 bytes)

```
u8  magic_lo            // arbitrary
u8  magic_hi            // arbitrary
u8  count               // < 0x10
u8  marker              // == 0x01
u32 (offset table starts here, count entries)
```

The `byte[3] == 0x01` byte is the magic discriminator; `byte[2]` is the count (limited to `<16`, hence the cap). The detection function `is_tim_pack` checks both.

## Offset table

Each table entry is a `u32` word index, decoded as:

```
byte_offset = word_index * 4 + 4
```

The `+4` is the difference from the [pack format](pack.md): this format adds a constant offset, suggesting the offsets are relative to the END of the count word rather than the start of the pack.

## Item type detection

`detected_ext(item)` returns `"TIM"` if the first byte is `0x10` (PSX TIM magic), else `"BIN"`.
