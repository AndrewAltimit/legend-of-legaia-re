# `legaia-save`

Per-character record schema for Legend of Legaia.

## Scope

The 0x414-byte character record at runtime address `0x80084708 + n * 0x414` -
typed accessors for every documented field, a round-trip-safe `parse` /
`write` pair that preserves every untouched byte, and a `Party` wrapper for
N-character rosters.

## Format coverage

| Offset | Field | Source |
|---|---|---|
| `+0xF4..0x100` | active-abilities bitfield | `FUN_80042558` |
| `+0x104..0x110` | HP / MP / SP (cur, max each) | `FUN_80042558` |
| `+0x11A` | stat cap (clamped to `0x3E7`) | `FUN_80042558` |
| `+0x13C..0x184` | spell list (count + IDs + levels, 36 entries) | `FUN_80042DBC` |
| `+0x196..0x19D` | equipment slots (8 bytes) | inventory consumers |
| `+0x2B0..0x380` | active-spell slots (stride 0x14) | `FUN_800432BC` |

Untyped offsets pass through unchanged via [`CharacterRecord::raw`].

## Composition

```rust
use legaia_save::{CharacterRecord, HpMpSp, STAT_CAP};

let mut r = CharacterRecord::zeroed();
r.set_hp_mp_sp(HpMpSp { hp_cur: 250, hp_max: 999, mp_cur: 30, mp_max: 80, sp_cur: 10, sp_max: 50 });
r.set_stat_cap(STAT_CAP);
let bytes = r.write();  // 0x414 bytes
let r2 = CharacterRecord::parse(&bytes)?;
assert_eq!(r.hp_mp_sp(), r2.hp_mp_sp());
```

## Retail SC-block bridges

The retail save block stores party records, the 512-byte story-flag
bitmap, and the 72-slot inventory at fixed offsets. `SaveFile` round-trips
through both directions:

```rust
use legaia_save::{SaveFile, RETAIL_MAX_CHAR_RECORDS};

// Disc → engine state
let parsed = SaveFile::from_retail_sc_block(&sc_block, RETAIL_MAX_CHAR_RECORDS)?;

// Engine state → disc
let mut sc_block = vec![0u8; legaia_save::BLOCK_SIZE];
parsed.write_into_retail_sc_block(&mut sc_block)?;
```

The companion helpers in [`card`](src/card.rs) (`read_retail_*` /
`write_retail_*`) expose each region individually for callers that don't
want the whole `SaveFile`. Story-flag rendering is the wide 512-byte
bitmap mirroring RAM `0x80085600..0x80085800` - the engine carries it in
[`SaveExt::story_flag_bits`] alongside the narrower 32-bit scratchpad
word at `_DAT_1F800394`.

## What this is NOT

- A drop-in replacement for the retail save format. Money + the LGSF v2
  / v3 extension blocks (party metadata, learned arts, saved chains) are
  the engine's own layer; only party records + story flags + inventory
  cross the retail-SC boundary today.

## Reference

[`docs/subsystems/battle.md` - Character record layout](../../docs/subsystems/battle.md#character-record-layout).
