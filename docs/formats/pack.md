# Pack format (inside TIM_LIST and TMD chunks)

A simple `(count, offset_table, data)` layout used as the data payload of a TIM_LIST or TMD chunk inside a [DATA_FIELD streaming](data-field.md) container.

Implementation: `crates/asset/src/pack.rs`.

## Layout

```
u32 count
u32 word_offset[count]   // each is in 4-byte words from the start of THIS pack data
... sub-asset bytes, packed back-to-back ...
```

Sub-asset `i` lives at byte range `[word_offset[i] * 4 .. word_offset[i+1] * 4)`, with the last sub-asset ending at the chunk's end.

## Example

A TIM_LIST chunk header followed by a 2-TIM pack:

```
chunk header:    6c 02 01 01    type=0x01 (TIM_LIST), size=0x01026C
pack count:      02 00 00 00    count = 2
offset[0]:       03 00 00 00    word offset 3 → byte 12 (= start of TIM 0)
offset[1]:       8b 20 00 00    word offset 0x208B → byte 0x822C (= start of TIM 1)
[then 2 PSX TIMs back-to-back]
```

## Distinction from the standalone TIM-pack

The [standalone TIM-pack](tim-pack.md) reader is not a second format. Its 8-byte "header" (a `byte[3] == 0x01` / `byte[2] < 0x10` discriminator pair, then a `u32` count at `+4`) and its constant `+4` on each word offset are a DATA_FIELD `TIM_LIST` chunk header `(0x01 << 24) | payload_len` followed by this pack, read with the chunk header still attached. Member `i` lands on the same byte either way (`4 + word_offset[i] * 4`). Use this reader on a bare pack (a chunk payload) and `prot::timpack` - or skip the 4-byte header first - on a whole chunk.

## See also

- [prot::timpack](tim-pack.md) - the reader for this pack behind a `TIM_LIST` chunk header.
- [field-pack](field-pack.md) - **not** a third pack: the `0x01059B84` word is a DATA_FIELD chunk header wrapping an `asset::pack` of scene TIMs (this page's format).
- [DATA_FIELD streaming](data-field.md) - the streaming container these packs live inside.
