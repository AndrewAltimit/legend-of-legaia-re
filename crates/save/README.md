# `legaia-save`

Per-character record schema for Legend of Legaia.

## Scope

The 0x414-byte character record at runtime address `0x80084708 + n * 0x414` —
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

## What this is NOT

- The PSX memory-card `.mcs` save-file format. That wrapper holds one or
  more character records plus runtime globals (party leader, inventory,
  story flags, current scene). Once captured it lands as a sibling
  module here.
- The page-banked inventory at `[0x80085718..0x80085918)` — that's
  global state, not per-character.

## Reference

[`docs/subsystems/battle.md` — Character record layout](../../docs/subsystems/battle.md#character-record-layout).
