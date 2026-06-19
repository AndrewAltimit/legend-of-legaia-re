# Level-Up Subsystem

Covers XP distribution after a battle win, the per-level stat-gain table, and
the banner display. Post-battle level-up logic is driven by
`engine-core::levelup::LevelUpTracker`. Retail applies XP, stat growth, and the
level bump in the overlay-resident function `FUN_801E9504`.

## XP table

The retail XP-to-next-level curve is a **static `SCUS_942.54` table plus a
scaling formula**, applied by the level-up function `FUN_801E9504`
(overlay-resident; dumped as `overlay_battle_action_801e9504` and aliased into
`overlay_magic_level_up` / `overlay_magic_capture` / `overlay_muscle_dome`,
all the same code). The battle-victory reward resolver `FUN_8004E568` calls it
at `0x8004F34C` (`jal 0x801E9504`, argument = active-party slot − 1) once it has
divided the monster XP pool by the alive-party count.

The curve source:

- **Per-level XP-delta table `DAT_80076AF4`** (u16 entries, referenced literally
  as `&DAT_80076AF4` at `0x801E9588`/`0x801E9594`). It is static `SCUS_942.54`
  data - below the `0x801C0000` overlay boundary and clear of the sin LUT range
  (`0x80070A2C..0x80072A2C`). The threshold for the current level is the running
  sum `sum = Σ DAT_80076AF4[0 .. level]`.
- **Scaling formula** (`0x801E95D0`–`0x801E9624`): for `level < 0x11` (17),
  `threshold = (sum × 9_999_999) / 0x140FE` (≈ `sum × 121.69`); for
  `level ≥ 0x11`, `threshold = sum × 0x79` (121).
- **Per-character correction** for slots 1 and 2 (Noa subtracts, Gala adds)
  `(threshold × 0x14) / s`, where `s` is a runtime per-character scalar.
- **Level-up loop**: a `do … while (threshold ≤ record cumulative XP)` (the
  `sltu` at `0x801E9714` / `0x801E9F70`) bumps the record level field and applies
  one round of stat growth per crossed threshold, so a single large XP award can
  advance several levels at once.

The only readers of `DAT_80076AF4` anywhere in the corpus are the four alias
copies of `FUN_801E9504`, confirming it is the canonical XP curve.

**This supersedes the earlier sin-LUT reading.** A prior extraction pass
mistook a 98-entry slice of the shared 4096-entry sin LUT at `0x80070A2C`
(`sin[0x408..0x46A]` = `50, 56, 62, …`) for the XP table, after an
off-by-`0x800` file/virtual-address confusion (`0x6123C` vs `0x80070A3C`). That
slice is genuinely sin-LUT data consumed by the GTE rotation builders
`RotMatrixX/Y/Z` (`0x800461A4` / `0x8004629C` / `0x8004638C`) and the cutscene
camera (`FUN_8001CF50`), not XP. The engine now extracts the real curve at boot:
`legaia_asset::level_up_tables::xp_thresholds_from_scus` reads `DAT_80076AF4` +
the formula from the user's `SCUS_942.54` (no Sony bytes committed) and
`legaia_engine_shell::BootSession` installs it over `LevelUpTracker::xp_table`
(byte-validated: L2 = 365, L3 = 730 against a captured retail level-up). The
`retail_xp_table()` sin-LUT slice is retained only as the disc-less fallback.

Provenance: `FUN_801E9504` in
`ghidra/scripts/funcs/overlay_battle_action_801e9504.txt`; caller `FUN_8004E568`
at `0x8004F34C` (`ghidra/scripts/funcs/8004e568.txt`).

## Stat gains

Per-character stat growth is **also `FUN_801E9504`'s job, sourced from static
`SCUS_942.54` tables** - the writer the earlier capture work could not find is
`FUN_801E9504` itself (a victory-path overlay function, not the
`overlay_magic_level_up` display code that was searched). It takes the 0-based
party slot (`a0 = active-slot − 1`; slot 3 returns immediately), indexes the
character record at `0x80084140 + slot×0x414`, and runs a `do { … } while
(threshold ≤ record_xp)` multi-level loop, so one large XP award advances several
levels in a single call. Each iteration grows **eight stats** at record
`+0x6E4..+0x6F4` (HP `+0x6E4`, MP `+0x6E6`, then six battle stats at
`+0x6EA..+0x6F4`, skipping the `+0x6E8` 100-cap slot), then increments the level
byte at `+0x6F8`.

### Tables

- **Per-stat growth curves at `DAT_800769CC`** (`lui v0,0x8007; addiu
  s4,v0,0x69CC`): [`GROWTH_ROW_COUNT`] = 3 rows, stride `0x62` (= 98 =
  `MAX_LEVEL − 1`), each a monotonic ramp (row 0 = `0x50, 0x52, 0x54, …`) settling
  to a `0x40` plateau byte at high levels.
- **A per-character parameter block at `DAT_80076918`** (`lui a0,0x8007; addiu
  a0,a0,0x6918`): stride `0x3C`, one record per Vahn / Noa / Gala (the 4th slot
  is never grown). Each record is **8 contiguous 6-byte sub-records**
  `{u16 start, u16 max, u8 jitter, u8 row}`. `start` is the stat's base
  (level-1) value - **validated against the new-game starting template**
  (`legaia_asset::new_game`): **Gala's record matches the template on all 8
  stats**, Vahn/Noa on HP/MP/AGL (their late-join templates are lightly
  retuned). `max` is the level-99 ceiling, `row` selects a curve, `jitter` the
  spread. (The leading `0x00B4` of the block is Vahn's HP `start` = 180, **not**
  a length word - an earlier note mislabeled it.)

### Per-level gain arithmetic (decoded + validated)

Per stat per level (disassembly `0x801E9758..0x801E97F8` in
`overlay_magic_level_up_801e9504.txt`):

```
jitter_val = rand() % (2*jitter + 1)                  ; BIOS A(0x2F) rand, 0..2*jitter
byte       = curve[row][level - 1]                    ; 0x62-stride row index
gain       = (max - start) * byte / 0x24C0            ; the 0x6F74AE27 >> (32+12) magic divide
gain       = gain + jitter_val - jitter               ; recenter jitter to [-jitter, +jitter]
gain       = max(1, gain)                             ; +1 floor
record[stat] += gain                                  ; then caps: HP ≤ 9999, MP ≤ 999, SP ≤ 0x118, others ≤ 999
```

Slots 1/2 also apply a per-character XP-threshold correction
(`±(threshold×0x14)/divisor`; Noa subtracts, Gala adds). The divisor is the
`i16` at `table + level×0x28` (indexed by the character's *current* level)
through the pointer global `_DAT_8007B81C` - which is **constant in retail**:
a pure-Rust read across all 45 library save states finds `0x80070A2C` in every
one, so the "runtime struct" is plain static `SCUS_942.54` data (the head of
the GTE sin LUT sampled at a `0x28` stride - sin data doubling as a divisor
curve: 125, 251, 376, … by level, so the correction shrinks from ~16% at L1
toward ~0.5% mid-game). Parsed by
`legaia_asset::level_up_tables::xp_correction_divisors_from_scus`, applied by
`LevelUpTracker::threshold_for` (slot 1 earlier, slot 2 later), installed at
boot alongside the XP curve; the disc-gated `level_up_tables_real` test pins
the first divisors plus the captured-L3 example (365 ± 29).

**The divisor `0x24C0` is the curve normalizer.** Each of the three growth
curves sums to *exactly* `0x24C0` (= 9408), so the per-level term
`(max−start)×byte/Σcurve` accumulates to exactly `(max−start)` across all 98
levels - every stat lands precisely on its `max` at level 99. The record-write
offsets are confirmed too: the applier's `+0x6E4` block is the same RAM as the
`+0x11C..+0x12D` record stat window (the two bases differ by a constant `0x5C8`
at the same `0x414` stride).

**VALIDATED against a single-level capture.** The `noa_levelup_field_pre` /
`noa_levelup_field_post` library saves (Noa, growth slot 1, **L2 → L3**, the
`noa_levelup_*` scenarios in `scripts/scenarios.toml`) give byte-exact
single-level deltas: HP +39, MP +5, and the six record stats +2 / +4 / +4 / +3
/ +4 / +3. Leveling **from** L2 reads `curve[row][1]`; every one of the 8 deltas
lands within `[core − jitter, core + jitter]` of the formula above (e.g. HP
core `(4500−150)×82/9408 = 37`, observed +39, jitter half-range 4). So the
arithmetic is correct as written - the earlier "~4.3..4.8x overshoot" reading
was an artifact of the *multi-level* corpus observations
(`noa_4_level_jump` / `gala_4_level_jump`), whose stated HP deltas (≈+32 over a
claimed 4 levels) are impossible under the validated ≈+38/level rate; those
captures are unreliable for per-level growth (they still pin the write
*footprint* + phase split, below). Provenance:
`ghidra/scripts/funcs/overlay_magic_level_up_801e9504.txt` (+ identical
`overlay_battle_action_801e9504.txt` / `overlay_muscle_dome_801e9504.txt`
aliases); decoded + checked in `legaia_asset::level_up_tables`
(`GrowthTables::char_params` / `level_gain_core`) by the disc-gated
`crates/asset/tests/level_up_tables_real.rs`.

**Engine wiring (deterministic core - done, all 8 stats).** `StatGain` carries
the full eight-stat gain (HP, MP, AGL, ATK, UDF, LDF, SPD, INT).
`LevelUpTracker::with_growth_tables` builds a per-character
`StatGrowthCurve::PerLevel` from the parsed SCUS tables (the jitter-free
`level_gain_core` for each of the 8 stats), and `BootSession` installs it from
the user's `SCUS_942.54` at boot (alongside the XP curve), replacing the flat
10 HP / 5 MP placeholder for Vahn/Noa/Gala. `apply_to_record` bumps HP/MP maxima
(restoring cur to max) and grows the six battle stats in the record-side window
(`+0x11C..+0x12D`), then **mirrors** them into the live window
(`+0x110..+0x11B`) - matching the applier's write-then-mirror. Disc-gated
`boot_installs_the_real_per_character_growth_curves_from_disc` checks Noa's curve
produces the validated L2→L3 core (HP 37, MP 6).

**Jitter (modeled, opt-in).** The per-level `rand() % (2×jitter+1) − jitter`
spread is implemented as an **opt-in** layer:
`LevelUpTracker::with_level_up_jitter(seed)` seeds a faithful PSX BIOS-rand LCG
(`BiosRand`: `seed = seed×0x41C6_4E6D + 0x3039; (seed>>16)&0x7FFF`) and the
level-up pass then draws **one** `rand()` per stat per level - in the applier's
stat order (HP, MP, AGL, ATK, UDF, LDF, SPD, INT), *including* the draw when
`jitter == 0` (`rand() % 1 == 0`) - applying the spread to the **unfloored**
core (`level_gain_core_raw`) before the `max(1, …)` floor, exactly as
`FUN_801E9504` does. It is **off by default**: with no jitter RNG installed the
tracker applies only the deterministic core and draws zero `rand()`, so every
replay/determinism oracle stays bit-identical. The *algorithm* is faithful; a
bit-exact reproduction of a *specific* retail level-up additionally needs the
BIOS-rand state at that moment (runtime, not recoverable from disc), so the
default engine path stays jitter-free (the jitter mean is 0 ⇒ unbiased totals).

**FALSIFIED (still): the "Seru struct `+0x74`" growth hypothesis.** An earlier
reading held that a Seru gaining a level applied a per-Seru `+0x74` "HP grant"
to the battle actor. That is wrong, and this finding confirms it from the other
side: growth comes from the `DAT_800769CC` / `DAT_80076918` static tables, not a
Seru `+0x74` dereference. (The only `+0x74` reads in the captured overlays
surface a 32-bit battle-state flag the SCUS handler `FUN_800480D8` stamps with
`0x80808080`.) Battle actor base for reference: `DAT_801C9370[slot]`, 8 slots -
party 0..2, monsters 3..7; current HP at `+0x14C`.

The level-up overlay data section (`overlay_magic_level_up_full.bin`,
`0x801C0000–0x801FFFFF`, full 256 KB) contains:

| Address | Content |
|---|---|
| `0x801F4B8C` | 4-byte display row-ID table for magic slots (indices 12–17) |
| `0x801F4B98` | Magic-type name strings: Spirit / Defense / Meta / Terra / Ozma |
| `0x801F4C28+` | Battle-result text strings (win / annihilated / escaped / …) |
| `0x801F5CF8`, `0x801F5D90` | Binary animation tables passed to particle spawner `FUN_80050ED4` |
| `0x801F6000+` | Live animation state globals (runtime values; zero at rest) |

No increment table lives in this *display* overlay - the growth tables
(`DAT_800769CC` / `DAT_80076918`) are in static `SCUS_942.54`, read by the
victory-path applier `FUN_801E9504` (above). The captured per-character triplets
below (Vahn / Noa / Gala observed deltas) remain useful as an empirical
cross-check: a future engine port that reads the SCUS curves can validate
against them.

`StatGain::default()` uses placeholder flat rates: +10 HP / +5 MP per level for
all characters. Retail varies growth per character via the `DAT_80076918`
parameter block; until the SCUS tables are wired into the engine, don't
fabricate numbers - populate a measured curve via `with_stat_gains` /
`SeruStatTable`, or extract the real `DAT_800769CC` curve at runtime.

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
  reading `+0x11A` is misnamed - `+0x11A` is one of the live stat slots and
  is mutated on level-up. Engines should read the cap from `+0x120` instead.
- **Displayed character level at `+0x130`.** This is the byte the status
  screen reads as "LV" and the `Level 99` GameShark code targets - **boot-confirmed
  via the starting-level randomizer**: a New Game record with level-10 cumulative
  experience (`+0x0`), level-10 stats, and the correct next-level threshold (`+0x4`)
  but `+0x130 == 1` still displays **LV 1**, and setting `+0x130 = 10` makes it
  display **LV 10**. So the shown level is *not* re-derived from experience at a New
  Game - it is read from `+0x130` directly (the new-game seed writes it; see
  [`new-game-table.md`](../formats/new-game-table.md)). The retail level-up applier
  maintains it by **incrementing it `+1` per level-up event** (the captured 4-level
  jumps bumped it by one, so it can momentarily lag the XP-derived level after a rare
  multi-level grant), but for single-level play and the new-game seed it equals the
  level. This supersedes the earlier "`+0x130` = magic rank, level derived from
  cumulative XP" reading for the *level* question; whether a separate magic-rank byte
  lives at the adjacent `+0x131` (which the seed also inits to 1) is unconfirmed.
  NB the engine port tracks its own level at `+0x100` (always zero in retail, where
  the live byte is `+0x130`) - self-consistent for the port's own LGSF saves, a
  deliberate divergence from the retail byte, not a mirror of it.

### Cross-character delta search (negative finding)

A grep across `extracted/PROT.DAT` for u8 sequences matching the observed
Vahn / Noa / Gala stat-delta tuples surfaces a 128-byte stride table at
PROT.DAT byte offset `0x033E9000`. Inspection shows records with
ramp-up-peak-ramp-down patterns (`06 06 07 08 09 0A 0B 0C 0D 0E 0F 0F 0F 0E
0D 0C 0B 0A 09 08 07`) characteristic of **per-effect animation curves**
(particle weight tables / attack timing curves), **not** stat-grant data.
The matched stat-shape patterns (Vahn `04 04 02 02 04 04`, Gala `02 04 04
02 02 02`) sit inside these animation curves as coincidental byte runs.

Net: cross-character u8-pattern search does not surface a stat-grant table -
because the grant table is not in PROT.DAT at all. It is the static-SCUS pair
`DAT_800769CC` / `DAT_80076918` read by `FUN_801E9504` (see *Stat gains*).

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

- **Per-character stat grants - source RESOLVED.** Vahn / Noa / Gala have
  distinct HP/MP/stat growth. The growth source is the static SCUS pair
  `DAT_800769CC` (per-stat 98-entry curves, stride `0x62`, indexed by level) +
  `DAT_80076918` (per-stat parameter block selecting curve rows), read and
  applied by `FUN_801E9504` (see *Stat gains* above). The earlier negative
  results came from searching the wrong code: the `magic_level_up` overlay is
  the display path, not the writer; the `+0x74` reads are a `0x80808080`
  battle-state flag (`FUN_800480D8`), not a grant; and the PROT.DAT
  `0x033E9000` cluster is animation-curve data. Remaining work is an **engine
  port**: extract the two tables from the user's `SCUS_942.54` at runtime and
  drive the per-level growth, replacing the placeholder flat curve in
  [`crates/engine-core/src/seru_stats.rs`] (`SeruStatGrant` / `SeruStatTable` /
  `LevelUpTracker::with_seru_roster`) with `StatGrowthCurve::PerLevel`.
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
- **Real retail XP table source - RESOLVED + PORTED.** The curve is the static
  SCUS table `DAT_80076AF4` + the scaling formula, read by `FUN_801E9504` (see
  *XP table* above). The prior sweeps targeting `0x8007123C` / `0x80070A3C`
  found nothing because both are wrong addresses. The engine now extracts it at
  boot - parser `legaia_asset::level_up_tables::xp_thresholds_from_scus`
  (disc-gated `level_up_tables_real`), installed by `BootSession` (disc-gated
  `new_game_seed::boot_installs_the_real_retail_xp_curve_from_disc`). The stale
  scanners [`scripts/find_xp_table_readers.py`](../../ghidra/scripts/find_xp_table_readers.py)
  / [`scripts/find_xp_table_all_overlays.py`](../../ghidra/scripts/find_xp_table_all_overlays.py)
  (targeting `0x8007123C`) are superseded.
- **Overlay display.** The retail level-up overlay shows per-stat increments
  (STR, INT, VIT, etc.) with an animated counter. Only HP/MP are tracked in the
  current port; other stats are handled by the per-character record's stat
  aggregator (`FUN_80042558`).

## See also

**Reference** -
[Battle scene](battle.md) ·
[Battle formulas](battle-formulas.md) ·
[Shop UI](shop.md) ·
[Game-data tables](../reference/gamedata.md)
