# Level-Up Subsystem

Covers XP distribution after a battle win, the per-level stat-gain table, and
the banner display. Post-battle level-up logic is driven by
`engine-core::levelup::LevelUpTracker`. Retail display lives in the level-up
overlay (partial coverage).

## XP table

**The previously-documented "XP increment table at `0x8007123C`" is FALSIFIED
by inspection of the SCUS bytes.** The address was an off-by-`0x800` (PSX EXE
header size) confusion between the file offset `0x6123C` and the virtual
address `0x80070A3C`; and the bytes at the *correct* virtual address are not
an XP table at all — they are deep samples (entries `0x408..0x469`) of the
shared **4096-entry full-circle sin LUT** at `0x80070A2C` that the GTE
rotation builders `RotMatrixX/Y/Z` (`0x800461A4` / `0x8004629C` / `0x8004638C`,
documented under [`reference/functions.md`](../reference/functions.md)) and
the cutscene camera (`FUN_8001CF50`,
[`subsystems/cutscene.md`](cutscene.md#camera-rotation-build)) consume. Every
one of the 4096 u16 entries at `0x80070A2C` matches `round(4096 × sin(i/4096
× 2π))` to within ±1; the quadrant breakpoints land exactly (`[0x000]=0`,
`[0x400]=4096`, `[0x800]=0`, `[0xC00]=-4096`), `cos[i] = sin[i + 0x400]`.

`engine_core::levelup::retail_xp_table()` therefore embeds a 98-entry slice of
that sin LUT (`sin[0x408..0x46A]` = `50, 56, 62, 69, 75, … 650, 656`) and
prefix-sums it as cumulative XP thresholds. The data behaves consistently
under the engine's self-tests (50 XP reaches L2, 106 reaches L3, …) but its
provenance as retail XP is **unproven**: an in-engine save-pair diff against
a real retail level-up has not been done, no retail reader has been found
loading this address as XP, and the L99 cumulative the slice produces
(34 663 XP) is conspicuously small for a 1998-era JRPG. The two non-exclusive
possibilities remain open:

1. **Designed reuse** — Legaia's level designers picked the early sin LUT
   slice deliberately because `4096 × sin(small angle)` is near-linear with
   mild curvature (≈ `idx × 6.25 + small_quadratic_term`), which is a
   reasonable XP curve shape. JRPGs of that era frequently shared static
   tables across subsystems to save ROM.
2. **Engine fabrication** — the 50/56/62/… numbers were read from this sin
   slot by an earlier extraction pass that mistook the address for an XP
   table, and the real XP curve lives elsewhere (likely an uncaptured
   overlay; static SCUS + every captured overlay carries no reader for
   either the wrong address `0x8007123C` or the sin-LUT-slice address
   `0x80070A3C`, regardless of LUI+ADDIU / gp-rel / LUI+LHU encoding).

`LevelUpTracker::default()` uses the slice. Override via `with_xp_table(Vec<u32>)`
when the real source lands. Total XP to reach level N (from the slice):
L2 = 50, L3 = 106, L5 = 312, L10 = 949, L20 = 3093, L50 = 14 655, L99 = 34 663.

## Stat gains

Retail HP / MP growth does not come from a per-character per-level table in
the overlay binary — a writer-search across every captured
`overlay_magic_level_up_*` dump returns no `sb` / `sh` writes targeting the
record stat window (`+0x10E`, `+0x11C..+0x12C`, `+0x130`, `+0x161`).

**FALSIFIED: the "Seru struct `+0x74`" growth hypothesis.** An earlier reading
held that a Seru gaining a level applied a per-Seru `+0x74` "HP grant" to the
battle actor. That is wrong — the only `+0x74` reads in the captured overlay
surface a 32-bit battle-state flag the SCUS-side handler `FUN_800480D8` stamps
with the constant `0x80808080`, not a stat-growth value. The real grant table
likely lives in a still-uncaptured overlay (battle-data init or the Seru-equip
path) or is encoded inline in a Seru PROT entry the current capture set doesn't
surface. So per-character growth remains **uncaptured**; `engine-core::levelup`
ships placeholder flat rates (see below) and exposes
`LevelUpTracker::with_seru_roster` / `SeruStatTable::insert` for explicit curves
once the writer is pinned. (Battle actor base: `DAT_801C9370[slot]`, 8 slots —
party 0..2, monsters 3..7; current HP at `+0x14C`.)

The level-up overlay data section (`overlay_magic_level_up_full.bin`,
`0x801C0000–0x801FFFFF`, full 256 KB) contains:

| Address | Content |
|---|---|
| `0x801F4B8C` | 4-byte display row-ID table for magic slots (indices 12–17) |
| `0x801F4B98` | Magic-type name strings: Spirit / Defense / Meta / Terra / Ozma |
| `0x801F4C28+` | Battle-result text strings (win / annihilated / escaped / …) |
| `0x801F5CF8`, `0x801F5D90` | Binary animation tables passed to particle spawner `FUN_80050ED4` |
| `0x801F6000+` | Live animation state globals (runtime values; zero at rest) |

No static HP/MP/STR/DEF increment table was found in the overlay. The remaining
capture path is an empirical one: diff a pre-/post-level-up save pair across the
character-record window (`LevelUpObservation` + the `mednafen-state diff`
toolkit) to recover the *observed* per-level deltas, since the producing writer
isn't in the captured overlays. Only one early-game card save exists today, so
the per-character curves stay unmeasured.

`StatGain::default()` uses placeholder flat rates: +10 HP / +5 MP per level for
all characters. Retail almost certainly varies growth per character (Vahn, Noa,
Gala), but the source of that variation is **not** the falsified `+0x74` path
above — don't fabricate numbers; populate a measured curve via
`with_xp_table` / `SeruStatTable` once a capture lands.

The tracker supports per-slot overrides via `with_stat_gains([StatGain; 4])`.

## Captured per-character level-up footprint

Three per-character 4-level-jump observations have been captured from the
mednafen save corpus. Each one is a settled pre→post diff over the
character record window; the underlying captures are pre / mid / post
save triplets at battle scene `map01`.

| Character | Slot | XP delta (u16 LE at `+0x004`) | HP_max | MP_max | SP_max |
|---|---:|---|---:|---:|---:|
| Vahn (legacy) | 0 | 365 → 730 (+365) | (`+0x126` wrap, +38) | +8 | +8 |
| Noa | 1 | 102 → 336 (+234) | +32 | +6 | **+40** |
| Gala | 2 | 140 → 394 (+254) | +44 | +8 | **0** |

Codified in [`engine_core::levelup::observations`](../../crates/engine-core/src/levelup.rs):
- `vahn_4_level_jump` (legacy historical fact - the source saves were rotated
  out of the active corpus when the Noa / Gala triplets shipped).
- `noa_4_level_jump` (settled delta across Noa's 3-phase split).
- `gala_4_level_jump` (settled delta across Gala's 2-phase split).

Each `LevelUpObservation::stat_deltas` is an 18-byte window covering
`+0x11C..+0x12D` (9 u16 LE values: HP_max, MP_max, per-stat cap (always 100),
six record-side stats). `LevelUpObservation::record_stats_u16()` lifts the
window as `[u16; 9]`.

### Phase split (multi-frame writes)

The level-up event splits the character record write across multiple frames.
For Noa the captured triplet pins three phases:

| Phase | Window | Writes |
|---|---|---|
| Record write | pre → mid₁ | `+0x11C..+0x12D` (record stat window), `+0x004..+0x005` (XP), `+0x130` (rank counter +1) |
| Live copy | mid₁ → mid₂ | `+0x104..+0x11B` (HP_cur, MP_cur, six u16 live stats) |
| Settle | mid₂ → post | `+0x106 / +0x10A / +0x10E` (live HP_max / MP_max / SP_max settle) |

Gala's level-up runs in two phases (record write, then live copy + settle
collapsed into one frame).

The slot indices that hold each frame in the active corpus live in
[`scripts/scenarios.toml`](../../scripts/scenarios.toml);
they rotate as the corpus is re-captured for new investigations.

The phase split + per-character record bases (Vahn `0x80084708`,
Noa `0x80084B1C`, Gala `0x80084F30`, slot 3 `0x80085344`, stride `0x414`)
are documented in [`engine_core::capture_observations::char_level_up`](../../crates/engine-core/src/capture_observations.rs)
with helpers `read_record_stats` / `read_rank_counter` / `read_xp_u16`.

### Per-character semantic findings

- **Noa grants `+40` SP_max** at `+0x10E`. Noa is a Seru-magic user; level-ups
  scale her Spirit gauge.
- **Gala grants `0` SP_max** across the entire triplet. Gala uses physical
  Tactical Arts (no Seru magic), so the level-up event leaves `+0x10E`
  untouched. Engines that copy Vahn's curve to Gala mis-grant SP.
- **The `+0x120` u16 LE field is a per-stat cap constant `100`**, not SP_max.
  Pinned across every captured save (Vahn, Noa, Gala) and every state. The
  earlier `legaia_save::character::CharacterRecord::stat_cap` accessor
  reading `+0x11A` is misnamed — `+0x11A` is one of the live stat slots and
  is mutated on level-up. Engines should read the cap from `+0x120` instead.
- **Rank counter at `+0x130`** increments by `+1` per level-up event,
  independent of `levels_gained` (Noa and Gala both jumped four levels but
  bumped this byte by one).

### Cross-character delta search (negative finding)

A grep across `extracted/PROT.DAT` for u8 sequences matching the observed
Vahn / Noa / Gala stat-delta tuples surfaces a 128-byte stride table at
PROT.DAT byte offset `0x033E9000`. Inspection shows records with
ramp-up-peak-ramp-down patterns (`06 06 07 08 09 0A 0B 0C 0D 0E 0F 0F 0F 0E
0D 0C 0B 0A 09 08 07`) characteristic of **per-effect animation curves**
(particle weight tables / attack timing curves), **not** stat-grant data.
The matched stat-shape patterns (Vahn `04 04 02 02 04 04`, Gala `02 04 04
02 02 02`) sit inside these animation curves as coincidental byte runs.

Net: cross-character u8-pattern search does not surface a stat-grant table.
The next viable approach is locating the battle-data init overlay slice that
loads the Seru PROT entries and tracing the stat read at the moment a level-
up event applies.

## Level-up flow

After a battle win with `BattleEndCause::MonsterWipe`:

1. The engine calls `World::apply_battle_xp(xp_reward)`.
2. `apply_battle_xp` enumerates the surviving party members (slots whose
   `BattleActor::hp > 0`) and divides `xp_reward` equally among them
   (integer divide, remainder dropped on the floor). Dead members
   receive zero XP.
3. `apply_battle_xp` calls `LevelUpTracker::grant_xp(char_id, share)`
   for each surviving party member.
4. `grant_xp` accumulates XP and checks the retail XP table for threshold
   crossings. Multi-level jumps collapse into a single `LevelUpResult` with
   summed HP/MP gains.
5. For each level-up: `LevelUpTracker::apply_to_record(result, record)` bumps
   `hp_max` and `mp_max`, restores `hp_cur` and `mp_cur` to the new maxima
   (retail restores full HP/MP on level-up), and writes `result.new_level`
   back to the record's `+0x100` byte via `CharacterRecord::set_level`.
6. `BattleEvent::LevelUp { char_id, new_level, hp_gained, mp_gained }` is pushed
   to `World::battle_events`.
7. `World::current_level_up_banner` is set to the last character who levelled up.

### Hydration on load

`World::load_full` syncs `LevelUpTracker::level[]` from each loaded
character record's `+0x100` byte. Without this, a reloaded party would
keep the tracker's default `1` per slot even when the saved records hold
the party at level 30; the next XP grant would then roll the party back
to level 1 + N.

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

## Fire Book I - captured write footprint

A pre/post save pair (battle command menu parked on Fire Book I → Fire Book I just used on Vahn) pins the per-character record write footprint of an in-battle Fire Book usage. The `mednafen-state diff` over Vahn's character record (`0x80084708..+0x414`) surfaces **exactly one 3-byte region** at `+0x185..+0x188`:

| Offset | Pre-event | Post-event | Read |
|---|---|---|---|
| `+0x185` | `0x01` | `0x02` | length-prefix byte (+1) |
| `+0x186` | `0x0C` | `0x03` | first list entry - new entry inserted at front |
| `+0x187` | `0x00` | `0x0C` | second list entry - pre-event entry shifted right |

Pattern: a length-prefixed list at `+0x185` grew by one entry. The new entry was inserted at position 0; the existing entry at position 0 moved to position 1.

### Reader resolved

A grep across the captured menu overlays (`overlay_menu_801d33d8.txt` and the identical save_ui / shop_save copies) for any read at `+0x185(reg)` surfaces exactly one reader cluster at `0x801D4440..0x801D44A4`:

```text
801d4440  lbu t2,0x185(t2)        ; load count from char_rec[+0x185]
801d4454  lbu v0,0x185(t1)
801d445c  slt v0,s6,v0            ; loop while s6 < count
801d4480  addu a0,t1,s6
801d4498  lbu v1,0x1(s2)          ; spell-table[+1] = id
801d449c  lbu v0,0x186(a0)        ; load id from char_rec[+0x186 + s6]
801d44a4  beq v1,v0,...           ; match id against spell-table entry
```

The structure is `[u8 count at +0x185][u8 ids[N] at +0x186..]`. The menu's spell-table at `0x801E472C` is indexed by these IDs (stride `0x14`; `record[+0]` = sort key, `record[+1]` = ID, `record[+0xC]` = name pointer). Display is capped at 7 by `slti v0,t2,0x7` later in the loop, but the on-record array fits 16 bytes (the gap to the equipment-slot field at `+0x196`).

The pre/post Fire Book I capture is a head-insert into this list: the menu's displayed-skill roster grew by one new entry. The values are skill-table indices, not action-queue constants - so the earlier "0x03 = Attack" reading is moot. Engines now read this through a typed accessor `legaia_save::character::CharacterRecord::displayed_skills` (`DisplayedSkillList { count: u8, ids: [u8; MAX_DISPLAYED_SKILLS = 16] }`); `engine_core::capture_observations::vahn_fire_book_use` gains `MENU_READER_ADDR` (`0x801D4440`) + `MENU_OVERLAY_FN` (`0x801D33D8`) constants pointing at the resolved reader.

No `sb` / `sh` writers to `+0x185` exist in any captured overlay. The learn-write path lives in an overlay we haven't dumped (likely the item-use battle event, accessed via the menu rather than the action SM); when that overlay lands, the writer is the next thing to pin.

A disc-gated test in [`crates/mednafen/tests/real_saves.rs`](../../crates/mednafen/tests/real_saves.rs) (`fire_book_use_diff_pins_vahn_record_write`) asserts exactly one record-internal region at the documented offset against the real save pair. Three new unit tests in `legaia_save::character::displayed_skills_*` exercise the typed accessor's BEFORE/AFTER round-trip + the `MAX_DISPLAYED_SKILLS` clamp.

## Open items

- **Per-Seru stat grants.** Vahn / Noa / Gala have distinct HP/MP growth via
  their Seru rosters. **Status:** writer-search across the captured
  `magic_level_up` overlay returned **negative** for code-side `sb` / `sh`
  writes targeting the destination offsets (`+0x10E`, `+0x11C..+0x12C`,
  `+0x130`, `+0x161`). A follow-up grep for any read at `+0x74(reg)` across
  the same overlay surfaces five hits - but each one is reading a 32-bit
  battle-state flag the SCUS-side handler `FUN_800480D8` writes with the
  constant `0x80808080` (`lui v0, 0x80; ori v0, v0, 0x8080; sw v0, 0x74(s0)`),
  not a stat-grant pointer. The "Seru struct +0x74 pointer dereference"
  hypothesis is **not supported** by the current capture set. Cross-character
  delta search across PROT.DAT (using the three pinned per-character triplets
  for orientation) also fails to surface a stat-grant table — the candidate
  cluster at PROT.DAT byte offset `0x033E9000` is animation-curve data, not
  stat grants. A new battle scene-init save pair (mc1 = `map01` field with
  encounter armed; mc2 = `map01` battle initiated) pins the **post-load
  residency window** of the battle scene-init pipeline (codified as
  `engine_core::capture_observations::battle_init_overlay`), but the loader
  function itself - the helper that reads PROT entry `0x05C4` + sibling
  Seru blobs and decompresses the per-Seru stat-grant data into RAM - has
  finished by the time the post-load save is captured. The loader still
  resides in an overlay slice that requires a mid-execution capture
  (between the field→battle game-mode flip and the post-load state) which
  the current Mednafen workflow can't generate without manual
  frame-stepping. Engine-side scaffold lives at
  [`crates/engine-core/src/seru_stats.rs`]: `SeruStatGrant` +
  `SeruStatTable` + `LevelUpTracker::with_seru_roster` install a flat curve
  summed across the equipped Seru. Replace with `StatGrowthCurve::PerLevel`
  when the table is pinned.
- **`+0x120` u16 LE field renaming.** The captured triplets pin
  `record[+0x120]` as a constant 100 across every save / character. The
  existing `legaia_save::character::CharacterRecord::stat_cap` accessor reads
  `+0x11A` instead, which is a live stat byte (mutated by level-up). The
  accessor needs renaming or relocation. Tracked separately to keep this
  level-up doc focused.
- **Battle actor struct fields `+0x14C`–`+0x176`.** The battle actor (pointed
  to by `DAT_801C9370[slot]`) holds HP at `+0x14C`, max HP at `+0x14E`, and
  additional stats at `+0x150`/`+0x152`/`+0x154`/`+0x156`; full field mapping
  has not been traced from the stat aggregator.
- **Find the real retail XP table source.** The current port divides the
  monster XP pool by `alive_count` (surviving party members) and credits each
  surviving slot the floored share. The thresholds it grants come from a
  98-entry slice of the shared sin LUT (see *XP table* above) whose
  provenance as retail XP is unproven. A LUI+ADDIU / gp-relative / LUI+LHU
  sweep across `SCUS_942.54` and every captured overlay
  (`overlay_magic_level_up`, `overlay_battle_action`, …) finds **zero**
  readers for either the wrong address `0x8007123C` or the sin-LUT-slice
  address `0x80070A3C` in any encoding, so either the reader lives in an
  uncaptured overlay slice, or the table is genuinely the sin LUT (read by
  the cutscene/GTE consumer at `0x800461C4` for unrelated rotation math,
  with the level-up consumer indexing a different sub-slice through the
  same base). The scanner
  [`scripts/find_xp_table_readers.py`](../../ghidra/scripts/find_xp_table_readers.py)
  + [`scripts/find_xp_table_all_overlays.py`](../../ghidra/scripts/find_xp_table_all_overlays.py)
  were targeting `0x8007123C` (the wrong address); update or replace them
  before re-running.
- **Overlay display.** The retail level-up overlay shows per-stat increments
  (STR, INT, VIT, etc.) with an animated counter. Only HP/MP are tracked in the
  current port; other stats are handled by the per-character record's stat
  aggregator (`FUN_80042558`).

## See also

**Reference** —
[Battle scene](battle.md) ·
[Battle formulas](battle-formulas.md) ·
[Shop UI](shop.md) ·
[Game-data tables](../reference/gamedata.md)
