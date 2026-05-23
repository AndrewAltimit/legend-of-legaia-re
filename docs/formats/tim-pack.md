# Standalone TIM-pack format

A multi-TIM container used by certain standalone PROT entries. Distinct from the [pack format](pack.md) used inside streaming chunks.

Implementation: `crates/prot/src/timpack.rs`.

## Header (8 bytes)

```
u8  magic_lo            // arbitrary
u8  magic_hi            // arbitrary
u8  disc               // < 0x10  (discriminator byte, NOT the count)
u8  marker              // == 0x01
u32 tim_num             // entry count at +4; offset table follows at +8
```

The `byte[3] == 0x01` / `byte[2] < 0x10` pair is the magic discriminator; the entry count is the `u32 tim_num` at `+4` (so the offset table begins at byte `+8`). The detection function `is_tim_pack` checks the signature pair, that `tim_num` is positive, and that the offset table fits within the blob.

## Offset table

Each table entry is a `u32` word index, decoded as:

```
byte_offset = word_index * 4 + 4
```

The `+4` is the difference from the [pack format](pack.md): this format adds a constant offset, suggesting the offsets are relative to the END of the count word rather than the start of the pack.

## Item type detection

`detected_ext(item)` returns `"TIM"` if the first byte is `0x10` (PSX TIM magic), else `"BIN"`.

## See also

- [asset::pack](pack.md) - the structurally similar in-DATA_FIELD pack.
- [PROT.DAT TOC](prot.md) - the index whose standalone entries use this pack.
- [PSX TIM](tim.md) - the texture sub-asset bundled here.
