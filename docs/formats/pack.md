# Pack format (inside TIM_LIST and TMD chunks)

A pack is the simplest container on the disc: a count, a table of offsets, then the members back to back. It is how several textures or several meshes travel as one unit - the data payload of a `TIM_LIST` or `TMD` chunk inside a [DATA_FIELD stream](data-field.md), or a whole PROT entry on its own. Offsets are counted in 4-byte words, and there is no per-member size: a member ends where the next one starts.

Implementation: [`crates/asset/src/pack.rs`](../../crates/asset/src/pack.rs) (`parse_pack`, `extract_pack`).

## Layout

| Offset | Size | Field | Meaning | Confidence |
|---|---|---|---|---|
| `+0x00` | u32 | `count` | Number of members | Confirmed |
| `+0x04 + i*4` | u32 | `word_offset[i]` | Start of member `i`, in 4-byte words from the start of the pack | Confirmed |
| `word_offset[0] * 4` | - | members | Sub-asset bytes, packed back to back | Confirmed |

Member `i` occupies bytes `[word_offset[i] * 4 .. word_offset[i+1] * 4)`. The last member runs to the end of the pack data.

```
pack data
+-------+-----------+-----------+-----+-------------+-------------+-----+
| count | offset[0] | offset[1] | ... |  member 0   |  member 1   | ... |
+-------+-----------+-----------+-----+-------------+-------------+-----+
0       4           8                 ^             ^
                                      offset[0]*4   offset[1]*4

the same pack behind a DATA_FIELD chunk header (what prot::timpack reads)
+--------------+-------+-----------+-----+-------------+-----+
| 01 | size24  | count | offset[0] | ... |  member 0   | ... |
+--------------+-------+-----------+-----+-------------+-----+
0              4                         ^
                                         offset[0]*4 + 4
```

## Example

A `TIM_LIST` chunk header followed by a 2-TIM pack:

```
chunk header:    6c 02 01 01    type=0x01 (TIM_LIST), size=0x01026C
pack count:      02 00 00 00    count = 2
offset[0]:       03 00 00 00    word offset 3 → byte 12 (= start of TIM 0)
offset[1]:       8b 20 00 00    word offset 0x208B → byte 0x822C (= start of TIM 1)
[then 2 PSX TIMs back-to-back]
```

## One format, two readers

The [standalone TIM-pack](tim-pack.md) reader is not a second format. Its 8-byte "header" (a `byte[3] == 0x01` / `byte[2] < 0x10` discriminator pair, then a `u32` count at `+4`) and its constant `+4` on each word offset are a DATA_FIELD `TIM_LIST` chunk header `(0x01 << 24) | payload_len` followed by this pack, read with the chunk header still attached. Member `i` lands on the same byte either way (`4 + word_offset[i] * 4`).

| Input | Reader |
|---|---|
| A bare pack (a chunk payload, or an entry that starts with the count) | `asset::pack` |
| A whole chunk, header included | `prot::timpack`, or skip 4 bytes and use `asset::pack` |

## See also

- [prot::timpack](tim-pack.md) - the reader for this pack behind a `TIM_LIST` chunk header.
- [field-pack](field-pack.md) - **not** a third pack: the `0x01059B84` word is a DATA_FIELD chunk header wrapping an `asset::pack` of scene TIMs (this page's format).
- [DATA_FIELD streaming](data-field.md) - the streaming container these packs live inside.
