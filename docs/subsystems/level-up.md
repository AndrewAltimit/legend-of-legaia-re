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
- Level 5: 281
- Level 10: 765
- Level 20: 2780
- Level 50: 17925
- Level 99: 61444

(Rounded; computed from the prefix sum of the SCUS_942.54 increment table.)

## Stat gains

Per-level HP / MP increments are in the level-up overlay's DATA segment —
not in `SCUS_942.54` code and therefore not extractable from function dumps
alone. The mc4 capture covers the post-battle animation overlay; a full dump of
the stat-distribution screen (mid-stat-distribution mednafen save) is needed
to pin the per-character growth curves.

`StatGain::default()` uses placeholder flat rates: +10 HP / +5 MP per level
for all characters. Different characters (Vahn, Noa, Gala) have distinct curves
in retail.

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

## Open items

- **Per-character stat curves.** Vahn / Noa / Gala have distinct HP/MP growth
  rates in retail. Locating those values requires a full binary dump of the
  stat-distribution sub-screen from the level-up overlay (pending mednafen
  save state at the mid-stat-distribution screen; see §3.1 of the PRD).
- **XP share formula.** Retail may divide the monster XP pool by active party
  size before calling the per-character threshold check. The current port grants
  the full `xp_reward` to each party member independently.
- **Overlay display.** The retail level-up overlay shows per-stat increments
  (STR, INT, VIT, etc.) with an animated counter. Only HP/MP are tracked in the
  current port; other stats are handled by the per-character record's stat
  aggregator (`FUN_80042558`).
