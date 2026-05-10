# Level-Up Subsystem

Covers XP distribution after a battle win, the per-level stat-gain table, and
the banner display. Post-battle level-up logic is driven by
`engine-core::levelup::LevelUpTracker`. Retail display lives in the level-up
overlay (mc4 capture; partial coverage).

## XP table

The cumulative XP thresholds for levels 2–99 are derived by prefix-summing the
98 u16 LE per-level increment values stored at `SCUS_942.54` address `0x8007123C`:

| Levels | Increment range |
|---|---|
| L1→2 | 50 |
| L2→3 | 56 |
| L3→4 | 62 |
| … | … |
| L98→99 | 656 |

`retail_xp_table()` in `engine-core::levelup` builds the cumulative table by
prefix-summing these 98 values. `LevelUpTracker::default()` uses it; the XP
table can be overridden via `with_xp_table(Vec<u32>)`.

Total XP to reach level N (from level 1):
- Level 2: 50
- Level 3: 106
- Level 5: 312
- Level 10: 949
- Level 20: 3093
- Level 50: 14655
- Level 99: 34663

(Computed by prefix-summing the 98 u16 LE increments at SCUS_942.54
`0x8007123C`. Verified by `legaia_save::RETAIL_XP_CUMULATIVE` and
`level_for_cumulative_xp` round-trip.)

## Stat gains

Retail HP / MP growth does not come from a per-character per-level table in
the overlay binary. Stat increments are sourced from **per-Seru structs** loaded
from PROT entries at runtime: when a Seru gains a level, the Seru struct field
at `+0x74` (HP grant) and sibling fields are applied to the battle actor's stat
block. The battle actor base lives at `DAT_801C9370[slot]` (8 slots: party 0..2,
monsters 3..7); its current HP sits at `+0x14C`.

The level-up overlay data section (`overlay_magic_level_up_full.bin`,
`0x801C0000–0x801FFFFF`, full 256 KB) contains:

| Address | Content |
|---|---|
| `0x801F4B8C` | 4-byte display row-ID table for magic slots (indices 12–17) |
| `0x801F4B98` | Magic-type name strings: Spirit / Defense / Meta / Terra / Ozma |
| `0x801F4C28+` | Battle-result text strings (win / annihilated / escaped / …) |
| `0x801F5CF8`, `0x801F5D90` | Binary animation tables passed to particle spawner `FUN_80050ED4` |
| `0x801F6000+` | Live animation state globals (runtime values; zero at rest) |

No static HP/MP/STR/DEF increment table was found in the overlay. To extract
per-Seru stat grants, dump the Seru struct data from a live PROT entry load
(see [`formats/field-pack.md`](../formats/field-pack.md) for the Seru data
container and the Seru struct layout investigation).

`StatGain::default()` uses placeholder flat rates: +10 HP / +5 MP per level
for all characters. Different characters (Vahn, Noa, Gala) have distinct curves
in retail (derived from their respective Seru rosters).

The tracker supports per-slot overrides via `with_stat_gains([StatGain; 4])`.

## Level-up flow

After a battle win with `BattleEndCause::MonsterWipe`:

1. The engine calls `World::apply_battle_xp(xp_reward)`.
2. `apply_battle_xp` calls `LevelUpTracker::grant_xp(char_id, share)` for
   each active party member.
3. `grant_xp` accumulates XP and checks the retail XP table for threshold
   crossings. Multi-level jumps collapse into a single `LevelUpResult` with
   summed HP/MP gains.
4. For each level-up: `LevelUpTracker::apply_to_record(result, record)` bumps
   `hp_max` and `mp_max`, then restores `hp_cur` and `mp_cur` to the new maxima
   (retail restores full HP/MP on level-up).
5. `BattleEvent::LevelUp { char_id, new_level, hp_gained, mp_gained }` is pushed
   to `World::battle_events`.
6. `World::current_level_up_banner` is set to the last character who levelled up.

## Level-up banner

`LevelUpBanner` carries `char_id`, `new_level`, `hp_gained`, `mp_gained`, and a
`frames_remaining` countdown (default 180 frames = 3 s at 60 Hz).

`World::tick` decrements `frames_remaining` each frame and clears the banner
when it reaches zero. The same tick pattern is used for `ArtLearnedBanner`
(Tactical Arts learning).

`level_up_draws_for(banner, world)` in `engine-render` returns two text draw
calls:
- Line 1 (yellow): `LEVEL UP! (char N -> Lv M)`
- Line 2 (green): `HP +X  MP +Y`

Wired into `PlayWindowApp::build_text_overlay` at anchor `(8, 60)` in
`legaia-engine play-window`.

## Key types

### `LevelUpTracker` (`engine-core::levelup`)

| Field | Type | Meaning |
|---|---|---|
| `xp` | `[u32; 4]` | Accumulated XP per party slot |
| `level` | `[u8; 4]` | Current level per party slot (1-based) |
| `xp_table` | `Vec<u32>` | Cumulative XP thresholds (len = MAX_LEVEL − 1 = 98) |
| `stat_gains` | `[StatGain; 4]` | HP/MP increments per level per slot |

### `LevelUpResult`

| Field | Type | Meaning |
|---|---|---|
| `char_id` | `u8` | Party slot |
| `old_level` | `u8` | Level before XP grant |
| `new_level` | `u8` | Level after XP grant (may skip multiple levels) |
| `xp_gained` | `u32` | XP granted in this call |
| `hp_gained` | `u16` | Total HP max increase (sum across all levels gained) |
| `mp_gained` | `u16` | Total MP max increase |

### `LevelUpBanner`

| Field | Type | Meaning |
|---|---|---|
| `char_id` | `u8` | Character who levelled up |
| `new_level` | `u8` | New level |
| `hp_gained` | `u16` | HP max increase (for display) |
| `mp_gained` | `u16` | MP max increase (for display) |
| `frames_remaining` | `u16` | Counts down from 180; cleared when zero |

## Fire Book I — captured write footprint

The user's `mc4` (battle command menu parked on Fire Book I) → `mc5` (Fire Book I just used on Vahn) save pair pins the per-character record write footprint of an in-battle Fire Book usage. The `mednafen-state diff` over Vahn's character record (`0x80084708..+0x414`) surfaces **exactly one 3-byte region** at `+0x185..+0x188`:

| Offset | Pre-event (mc4) | Post-event (mc5) | Read |
|---|---|---|---|
| `+0x185` | `0x01` | `0x02` | length-prefix byte (+1) |
| `+0x186` | `0x0C` | `0x03` | first list entry — new entry inserted at front |
| `+0x187` | `0x00` | `0x0C` | second list entry — pre-event entry shifted right |

Pattern: a length-prefixed list at `+0x185` grew by one entry. The new entry was inserted at position 0; the existing entry at position 0 moved to position 1.

The byte values do **not** match retail learned-art constants (those occupy `0x1B..=0x32`; see `legaia_art::tables`). `0x03` is the action constant for `Attack`; `0x0C` for direction `Left`. Two consistent interpretations remain:

1. The 3-byte cluster is a transient command-history buffer that the item-use animation populated, unrelated to the permanent Hyper-Art unlock flag — meaning the actual learn write lives outside Vahn's character record (e.g. a global story-flag word at `_DAT_1F800394` or a mask field elsewhere in main RAM).
2. The cluster is a per-character recent-action buffer the runtime pre-fills before the Fire Book animation plays, with the new entry encoded under a different scheme than the retail Learned Art Constant table.

A reader-search through the captured battle-action overlay (`overlay_battle_action_*.txt`) for `LUI` + `ORI` pairs targeting `0x80084708 + 0x185` (or stride `+0x414` adjacent slots) would disambiguate. Until then, the field is treated as **pinned but uninterpreted** — codified in `engine_core::capture_observations::vahn_fire_book_use` as the `BEFORE` / `AFTER` byte triples plus the absolute address.

A disc-gated test in [`crates/mednafen/tests/real_saves.rs`](../../crates/mednafen/tests/real_saves.rs) (`fire_book_use_diff_pins_vahn_record_write`) asserts exactly one record-internal region at the documented offset against the real save pair.

## Open items

- **Per-Seru stat grants.** Vahn / Noa / Gala have distinct HP/MP growth via
  their Seru rosters. The per-Seru HP-grant field is at `+0x74` in the Seru
  struct; remaining grant fields (MP, STR, DEF, INT, LUCK) are at sibling
  offsets not yet traced. Extraction requires a live capture of the Seru struct
  data during a level-up save state. **Status:** writer-search across the
  captured `magic_level_up` overlay returned **negative** for code-side `sb` /
  `sh` writes targeting the destination offsets (`+0x10E`, `+0x11C..+0x12C`,
  `+0x130`, `+0x161`) — the per-Seru lookup table is read at runtime through a
  pointer set at scene-load time, not in any captured static body. Engine-side
  scaffold lives at [`crates/engine-core/src/seru_stats.rs`]: `SeruStatGrant` +
  `SeruStatTable` + `LevelUpTracker::with_seru_roster` install a flat curve
  summed across the equipped Seru. Replace with `StatGrowthCurve::PerLevel`
  when the runtime trace lands.
- **Battle actor struct fields `+0x14C`–`+0x176`.** The battle actor (pointed
  to by `DAT_801C9370[slot]`) holds HP at `+0x14C`, max HP at `+0x14E`, and
  additional stats at `+0x150`/`+0x152`/`+0x154`/`+0x156`; full field mapping
  has not been traced from the stat aggregator.
- **XP share formula.** Retail may divide the monster XP pool by active party
  size before calling the per-character threshold check. The current port grants
  the full `xp_reward` to each party member independently.
- **Overlay display.** The retail level-up overlay shows per-stat increments
  (STR, INT, VIT, etc.) with an animated counter. Only HP/MP are tracked in the
  current port; other stats are handled by the per-character record's stat
  aggregator (`FUN_80042558`).
