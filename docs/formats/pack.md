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

The [standalone TIM-pack](tim-pack.md) form is structurally similar but uses an 8-byte byte-marker header (with `byte[3] == 0x01` and `byte[2] < 0x10` as discriminators) and stores count in `byte[2]`. The pack-inside-streaming-chunks form here uses a full `u32` count and lacks the marker prefix — use the right format for the source.
