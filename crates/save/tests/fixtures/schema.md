# Save-file schema fixture

This directory pins the byte-level mapping between the engine's `LGSF v2`
save file ([`legaia_save::SaveFile`]) and the retail PSX `SC` save block
(`game_data` region at offset `0x200` inside an 8 KiB block).

The two binary fixtures (`schema_synthetic.lgsf.bin`,
`schema_synthetic.sc.bin`) are written by [`schema_fixture::tests`] from
the deterministic `build_synthetic_save` helper. Any change to either
format's writer makes the fixture bytes drift and breaks the test
loudly.

The fixtures contain only synthesised data - no Sony bytes. The
character record bodies are zero except for a 1-byte sentinel at
`raw[0]` and the typed setters the helper invokes through the public
API; the story-flag bitmap is a `(i * 0x33) & 0xFF` sweep; the
inventory is three hand-picked `(item_id, count)` pairs.

## Regenerate

If the writer changes intentionally, regenerate both fixtures by
running the test with `LEGAIA_UPDATE_FIXTURES=1`:

```
LEGAIA_UPDATE_FIXTURES=1 cargo test -p legaia-save --test schema_fixture
```

Then `git diff` and `git add` the changed `.bin` files alongside the
writer change.

## Field map: `SaveFile` → retail SC + LGSF v2

`SC offset` columns are relative to the start of the 8 KiB save block
(SC magic at `+0x0000`, `game_data` region at `+0x0200`). `LGSF v2
offset` is relative to the start of the LGSF v2 buffer. An offset of
`engine-only` means the field has no retail SC representation: the
field round-trips through LGSF v2 but is dropped when the save lands
in an SC block. The retail layout has no slot for it, so engines that
want it durable need to keep emitting LGSF v2 alongside the SC export.

| `SaveFile` field | LGSF v2 offset | SC offset | Width | Note |
|------------------|----------------|-----------|-------|------|
| `LGSF` magic | `0x00` | - | 4 B | LGSF v2 only |
| version byte | `0x04` | - | 1 B | `4` for v4-shaped writers (LGX4 shiny block) |
| `ext.story_flags` | `0x05` | `0x14C0` (low u32 of bitmap) | 4 B | LGSF u32 LE; SC's first four bitmap bytes form the same scratchpad word on `from_retail_sc_block`. |
| `ext.money` | `0x09` | engine-only | 4 B | `RETAIL_GAME_DATA_OFFSET + 0x025C` exists in retail RAM but `write_into_retail_sc_block` doesn't expose a write helper - the engine save is the source of truth. |
| `inv_count` | `0x0D` | - | 1 B | LGSF v2: variable-length list. |
| `ext.inventory` pairs | `0x0E` (2 × `N` bytes) | `0x1818` (72 × 2 bytes) | varies | SC layout is fixed 72-slot; `(0, 0)` empty slots are dropped on read so LGSF order/length is `compact`. |
| `party_count` | after inventory | - | 1 B | |
| `party.members[i].raw` | follows | `0x0200 + 0x3C8 + i*0x414` | `0x414` per record | Both writers preserve the record's raw bytes verbatim; offsets within the record are documented in `docs/subsystems/battle.md`. The record base is `game+0x3C8` (live RAM `0x80084708`); the display name is at record `+0x2A7`, so the names appear at `0x0200 + 0x66F + i*0x414`. |
| `LGX2` ext block | after party records | - | varies | LGSF v2 only |
| `ext_v2.play_time_seconds` | inside LGX2 | engine-only | 4 B | |
| `ext_v2.active_party` | inside LGX2 | engine-only | 1 B + N | |
| `ext_v2.per_char[*].learned_arts_mask` | inside LGX2 | engine-only | 4 B / char | Distinct from the character record's `+0x13C` ability bitmap. |
| `ext_v2.per_char[*].spells` | inside LGX2 | engine-only | 1 + S B | Mirrors the per-character spell list. |
| `ext_v2.per_char[*].seru_captures` | inside LGX2 | engine-only | 1 + 4 × T B | |
| `ext_v2.per_char[*].active_chains` | inside LGX2 | engine-only | 16 B / char | |
| `ext_v2.saved_chains` | inside LGX2 | engine-only | varies | Cross-character chain library. |
| `LGX3` ext block | after LGX2 | - | varies | LGSF v3 only |
| `ext.story_flag_bits` | inside LGX3 | `0x14C0` (`0x200` bytes) | `0x200` B | Bitmap mirrors live RAM `0x80085600..0x80085800`. `from_retail_sc_block` returns the full slice; LGSF v3 stores it with a `u16 LE` length prefix. |
| `LGX4` ext block | after LGX3 | - | varies | LGSF v4 only |
| `ext_v2.per_char[*].shiny_spells` | inside LGX4 | engine-only | 1 + S B / char | Spell ids learned from a shiny Seru (+35% damage). Only chars with ≥1 shiny spell are listed. Retail encodes this in the `+0x161` spell-level high bit. |

## `SC`-block-only fields the engine doesn't read

The retail SC block also stores a display header at `+0x200..+0x5C8`
(location name, primary display name, recent CDNAME labels, party gold).
The
engine's `SaveFile` doesn't model these yet: the writer zero-pads the
region and the reader skips it. When a future engine pass adds those
fields, extend this schema doc and the K2 test in lockstep.
