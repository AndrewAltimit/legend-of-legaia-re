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
LEGAIA_UPDATE_FIXTURES=1 cargo test -p legaia-save --test integration schema_fixture::
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
| `ext.money` | `0x09` | `0x045C` | 4 B | Party gold - the pinned retail slot (`game_data+0x25C`, mirrors RAM `0x8008459C`). Both directions wired: `write_into_retail_sc_block` writes it, `from_retail_sc_block` reads it. The sibling casino coin bank (SC `0x0464`) rides in `ext.minigames` (below). |
| `inv_count` | `0x0D` | - | 1 B | LGSF v2: variable-length list. |
| `ext.inventory` pairs | `0x0E` (2 × `N` bytes) | `0x1818` (256 × 2 bytes) | varies | The SC array is the whole 256-slot bag; `(0, 0)` empty slots are dropped on read so the LGSF list is compact. Written through retail's add + normalize (`compose_window`) unless `ext.item_slots` is populated. |
| `party_count` | after inventory | - | 1 B | |
| `party.members[i].raw` | follows | `0x0200 + 0x3C8 + i*0x414` | `0x414` per record | Both writers preserve the record's raw bytes verbatim; offsets within the record are documented in `docs/subsystems/battle.md`. The record base is `game+0x3C8` (live RAM `0x80084708`); the display name is at record `+0x2A7`, so the names appear at `0x0200 + 0x66F + i*0x414`. |
| `LGX2` ext block | after party records | - | varies | LGSF v2 only |
| `ext_v2.play_time_seconds` | inside LGX2 | engine-only | 4 B | |
| `ext_v2.active_party` | inside LGX2 | `0x454` count (u8), `0x457` leader, `0x458` member list (`u8[4]`) | 1 B + N | Retail's present party (`0x80084594` / `0x80084598`). The lift reads it from the block unless the `LGXE` blob names one; the composer writes it for a 1..=4-member list. |
| `ext.item_slots` | `LGX6` (optional) | `0x1818` (256 × 2 bytes) | varies | The bag's physical slot array, holes included; when present the SC writer lays it in verbatim instead of composing `ext.inventory`. |
| `ext.minigames` | `LGX7` (optional, 36 B) | `0x30C..0x32C`, `0x464`, `0x474` | 9 × 4 B | Fishing point record, casino coin bank, Point Card bank ([`MinigameSave`](../../src/minigame_save.rs)); emitted only when non-zero. |
| `ext_v2.field_position` | `LGX8` (optional, 4 B) | `0x428` / `0x42C` (two sign-extended `i32`) | 4 B | Field position snapshot (`0x80084568` / `0x8008456C`) a card load seats the party at. |
| `ext_v2.audio_levels` | `LGX9` (optional, 8 B) | `0x43C` / `0x440` | 2 × 4 B | Configured audio level + voice volume. |
| `ext_v2.per_char[*].learned_arts_mask` | inside LGX2 | engine-only | 4 B / char | Distinct from the character record's `+0x13C` ability bitmap. |
| `ext_v2.per_char[*].spells` | inside LGX2 | engine-only | 1 + S B | Mirrors the per-character spell list. |
| `ext_v2.per_char[*].seru_captures` | inside LGX2 | engine-only | 1 + 4 × T B | |
| `ext_v2.per_char[*].active_chains` | inside LGX2 | engine-only | 16 B / char | |
| `ext_v2.saved_chains` | inside LGX2 | engine-only | varies | Cross-character chain library. |
| `LGX3` ext block | after LGX2 | - | varies | LGSF v3 only |
| `ext.story_flag_bits` | inside LGX3 | `0x14C0` (`0x358` bytes) | `0x358` B | Window mirrors live RAM `0x80085600..0x80085958`; the system-flag bank is its `+0x158..` (all `0x200` bytes). `from_retail_sc_block` returns the full slice; LGSF v3 stores it with a `u16 LE` length prefix. |
| `LGX4` ext block | after LGX3 | - | varies | LGSF v4 only |
| `ext_v2.per_char[*].shiny_spells` | inside LGX4 | engine-only | 1 + S B / char | Spell ids learned from a shiny Seru (+35% damage). Only chars with ≥1 shiny spell are listed. Retail encodes this in the `+0x161` spell-level high bit. |
| (no `SaveFile` field) | - | `0x1FFC` | 4 B | Block checksum. Not a save *field* - a derived word every SC writer restamps, so it drifts whenever any region above it does. See [`card::sc_block_checksum`](../../src/card.rs). |

## Engine-only fields inside an SC block

"engine-only" rows have no retail slot, but they still survive an SC export:
`SaveFile::write_engine_ext_into_retail_sc_block` writes the `LGX2` / `LGX4`
bodies as a magic-guarded `LGXE` blob in the block's unread tail (from
`0x1A18` up to the checksum word), which a retail console ignores and
`from_retail_sc_block` reads back.

## Fields outside this fixture

The resume point (`SaveResume`: banner / location name at SC `+0x200`, CDNAME
scene label at `+0x408`) is the optional `LGX5` trailer, written by
`write_with_resume` rather than `write`, so it is not in these fixtures. Every
other byte of the SC block the writer has no field for is left as the caller's
block held it.
