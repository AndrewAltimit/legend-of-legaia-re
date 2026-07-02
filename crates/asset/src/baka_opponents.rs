//! Baka Fighter **per-fighter roster table** (overlay VA `0x801D769C`).
//!
//! Each of the minigame's 17 roster fighters has a `0x6c`-byte record. The
//! match code reaches the same record through two bases: the **stat pointer**
//! (`DAT_801dc060[slot] = 0x801d769c + id*0x6c`, installed by the fighter-setup
//! path in `FUN_801d0fe4`/siblings) and the historical "+0x20 table" view at
//! `0x801d76bc` used by the gold read. The record base is `0x801d769c`; all
//! offsets below are relative to it.
//!
//! | Offset | Field | Consumer |
//! |---|---|---|
//! | `+0x20` | gold reward (u32) | `FUN_801d0fe4` win payout → tally → `_DAT_80084440` |
//! | `+0x24` | damage modifier (i32) | `FUN_801d3b18` defense term `mod + mod*def/100` |
//! | `+0x28/+0x2c/+0x30` | DEF tier % (i32×3, HP-keyed high/mid/low) | `FUN_801d3b18` (defender side; the `def %d` debug operand) |
//! | `+0x34` | critical chance % (i32) | `FUN_801d6660` comeback-crit roll (`rand()%100 < chance`, HP band `(0, 0x280)`) |
//! | `+0x38/+0x3c/+0x40` | ATK tier % (i32×3, HP-keyed high/mid/low) | `FUN_801d3b18` (attacker side; the `atk %d` debug operand) |
//! | `+0x44` | actor anchor (i16) | read `+ 200` as a spawn Y in `FUN_801d0fe4` |
//! | `+0x4c` | AI move-pattern (NUL-terminated `1`/`2`/`3` symbols) | `FUN_801d487c` move picker |
//!
//! The HP keying picks tier `[0]` while the fighter's HP is `>= 0x8c1`, `[1]`
//! in `[0x3c1, 0x8c0]`, `[2]` below - fighters hit and guard differently as
//! their HP drops.
//!
//! ## AI pattern - consumed backward
//!
//! `FUN_801d487c` rolls `rand() % 6`. While no pattern run is active a roll
//! `< 3` returns a uniformly random attack (`roll % 3`); a roll `>= 3` seeds
//! the pattern cursor to the pattern **length** and each subsequent pick steps
//! the cursor *down*, returning `pattern[cursor-1] - 1` (`% 3`) - the scripted
//! loop plays back-to-front to exhaustion, then the picker is free again.
//! [`BakaOpponent::attack_at`] is the forward-indexed convenience view; the
//! faithful backward walk lives in the engine port
//! (`legaia_engine_core::baka_fighter`).
//!
//! ## Action tables (power + keyframes)
//!
//! Per-exchange damage takes its base power from the winner's **action
//! record**: a per-fighter table of [`ACTIONS_PER_FIGHTER`] = 9 `0x60`-byte
//! action records reached through the pointer array `PTR_DAT_801db8b8[fighter]`
//! ([`ACTION_PTR_TABLE_VA`]). `FUN_801d3b18` indexes it with the fighter's
//! current-action id (`+0x10`): records **1/2/3 are the three attacks**
//! (positive `+0x18` power corpus-wide) and **4 is the special** (power 0 -
//! its payoff is the full-charge round win, gated on the `+0x1c` keyframe
//! count). The *display*-anim id space (`actor+0x5c = char*9 + frame`) sits
//! one higher (attacks `+2..+4`, special `+5`, knockdowns `+6..+8`); don't
//! conflate the two. Per record the fight code reads `+0x18` (base attack
//! power) and `+0x1c` (sub-keyframe count). [`parse_actions`] decodes the 17
//! tables.
//!
//! ## Extent - 17 fighters
//!
//! The table holds [`OPPONENT_COUNT`] = 17 records (the same `0x11` count the
//! action-table dump loop walks, see `docs/subsystems/minigame-baka-fighter.md`).
//! Both duel slots index this one roster: the player fights *as* a roster
//! fighter too (stats + actions), with the pad replacing the AI picker.
//!
//! ## Match length - best of 3
//!
//! Independent of this table, the match is **first to [`ROUND_WIN_TARGET`] = 2
//! round wins** (best of 3): `FUN_801cf00c` inits `DAT_801dbed0 = 2`, and the
//! match-over check in `FUN_801d0fe4` ends the match when a fighter's round-win
//! count (`DAT_801dbff0` / `DAT_801dc098`) equals `DAT_801dbed0`.
//!
//! ## Provenance - baked overlay data, pinned on disc
//!
//! Both tables are static `.rodata` in the Baka Fighter overlay (PROT entry
//! **0976**, base [`BAKA_OVERLAY_BASE_VA`]); reproducible from the user's
//! `PROT.DAT` (disc-gated `baka_opponents_real`). No Sony bytes are committed -
//! the values decode from the user's disc.

/// CDNAME / PROT index of the Baka Fighter overlay (`data\OTHER5`).
pub const BAKA_OVERLAY_PROT_INDEX: usize = 976;

/// Load base of the Baka Fighter overlay (the shared slot-A minigame base).
pub const BAKA_OVERLAY_BASE_VA: u32 = 0x801C_E818;

/// Runtime VA of the roster-record base (`0x801d769c`, the stat-pointer base;
/// the historical "+0x20" gold view is `DAT_801d76bc`).
pub const OPPONENT_TABLE_VA: u32 = 0x801D_769C;

/// File offset of the roster table within the as-loaded overlay image.
pub const OPPONENT_TABLE_FILE_OFFSET: usize = (OPPONENT_TABLE_VA - BAKA_OVERLAY_BASE_VA) as usize;

/// Per-fighter record stride (`id * 0x6c` index math).
pub const OPPONENT_RECORD_STRIDE: usize = 0x6C;

/// Byte offset of the gold-reward field within a record.
pub const RECORD_GOLD_OFFSET: usize = 0x20;

/// Byte offset of the damage-modifier field (the `mod` of the defense term).
pub const RECORD_DAMAGE_MOD_OFFSET: usize = 0x24;

/// Byte offset of the three HP-keyed DEF tier percentages.
pub const RECORD_DEF_TIERS_OFFSET: usize = 0x28;

/// Byte offset of the critical-chance percentage.
pub const RECORD_CRIT_CHANCE_OFFSET: usize = 0x34;

/// Byte offset of the three HP-keyed ATK tier percentages.
pub const RECORD_ATK_TIERS_OFFSET: usize = 0x38;

/// Byte offset of the AI move-pattern string within a record (`DAT_801d76e8`).
pub const RECORD_AI_PATTERN_OFFSET: usize = 0x4C;

/// Number of roster fighters (the `0x11` records the action-table loop walks).
pub const OPPONENT_COUNT: usize = 17;

/// Round wins needed to win a match (`DAT_801dbed0`): first to 2 = best of 3.
pub const ROUND_WIN_TARGET: u32 = 2;

/// Runtime VA of the per-fighter action-table pointer array
/// (`PTR_DAT_801db8b8`).
pub const ACTION_PTR_TABLE_VA: u32 = 0x801D_B8B8;

/// Action records per fighter (idle, walk, 3 attacks, special, 3 knockdowns).
pub const ACTIONS_PER_FIGHTER: usize = 9;

/// Byte stride of one action record.
pub const ACTION_RECORD_STRIDE: usize = 0x60;

/// Byte offset of the base attack power within an action record.
pub const ACTION_POWER_OFFSET: usize = 0x18;

/// Byte offset of the sub-keyframe count within an action record.
pub const ACTION_KEYFRAME_COUNT_OFFSET: usize = 0x1C;

/// Action record of the first of the three attacks (types 1/2/3 → records
/// 1/2/3 in the current-action id space the damage kernel indexes).
pub const ACTION_ATTACK_BASE: usize = 1;

/// Action record of the special / guard-break attack (type 4).
pub const ACTION_SPECIAL: usize = 4;

/// One decoded Baka Fighter roster record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BakaOpponent {
    /// Fighter id = index into the table.
    pub index: usize,
    /// `+0x20` - gold credited to the party on beating this fighter.
    pub gold_reward: u32,
    /// `+0x24` - damage modifier, the `mod` of `mod + mod*def/100`.
    pub damage_mod: i32,
    /// `+0x28..` - DEF tier % at HP high / mid / low.
    pub def_tiers: [i32; 3],
    /// `+0x34` - comeback-critical chance % (`rand()%100 < chance`).
    pub crit_chance: i32,
    /// `+0x38..` - ATK tier % at HP high / mid / low.
    pub atk_tiers: [i32; 3],
    /// `+0x4c` - the CPU attack-type loop (symbols `1`/`2`/`3`), NUL-terminated.
    pub ai_pattern: Vec<u8>,
}

impl BakaOpponent {
    /// The attack type (`0`/`1`/`2`) at a forward cycle cursor,
    /// `pattern[cursor % len] - 1`. Convenience view - the retail picker
    /// consumes the pattern backward (see the module notes).
    pub fn attack_at(&self, cursor: usize) -> Option<u8> {
        if self.ai_pattern.is_empty() {
            return None;
        }
        self.ai_pattern
            .get(cursor % self.ai_pattern.len())
            .map(|&s| s - 1)
    }
}

/// One fighter's decoded action table: per-slot base power + keyframe count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BakaActionSet {
    /// Fighter id = index into the pointer array.
    pub index: usize,
    /// `+0x18` per action record - the damage formula's base power.
    pub power: [i32; ACTIONS_PER_FIGHTER],
    /// `+0x1c` per action record - sub-keyframe count.
    pub keyframes: [i32; ACTIONS_PER_FIGHTER],
}

impl BakaActionSet {
    /// Base power of attack type `1..=3` (action slots 2/3/4) or the special
    /// (type `4`, slot 5). `None` for other type values.
    pub fn attack_power(&self, attack_type: u8) -> Option<i32> {
        match attack_type {
            1..=3 => Some(self.power[ACTION_ATTACK_BASE + (attack_type as usize - 1)]),
            4 => Some(self.power[ACTION_SPECIAL]),
            _ => None,
        }
    }
}

fn read_i32(overlay: &[u8], off: usize) -> i32 {
    i32::from_le_bytes([
        overlay[off],
        overlay[off + 1],
        overlay[off + 2],
        overlay[off + 3],
    ])
}

/// Parse the [`OPPONENT_COUNT`] roster records out of the as-loaded Baka
/// Fighter overlay image (PROT entry [`BAKA_OVERLAY_PROT_INDEX`]).
pub fn parse(overlay: &[u8]) -> Option<Vec<BakaOpponent>> {
    parse_at(overlay, OPPONENT_TABLE_FILE_OFFSET, OPPONENT_COUNT)
}

/// Parse `count` records starting at file offset `off`.
pub fn parse_at(overlay: &[u8], off: usize, count: usize) -> Option<Vec<BakaOpponent>> {
    if overlay.len() < off + count * OPPONENT_RECORD_STRIDE {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let b = off + i * OPPONENT_RECORD_STRIDE;
        let gold = u32::from_le_bytes([
            overlay[b + RECORD_GOLD_OFFSET],
            overlay[b + RECORD_GOLD_OFFSET + 1],
            overlay[b + RECORD_GOLD_OFFSET + 2],
            overlay[b + RECORD_GOLD_OFFSET + 3],
        ]);
        let damage_mod = read_i32(overlay, b + RECORD_DAMAGE_MOD_OFFSET);
        let def_tiers = [
            read_i32(overlay, b + RECORD_DEF_TIERS_OFFSET),
            read_i32(overlay, b + RECORD_DEF_TIERS_OFFSET + 4),
            read_i32(overlay, b + RECORD_DEF_TIERS_OFFSET + 8),
        ];
        let crit_chance = read_i32(overlay, b + RECORD_CRIT_CHANCE_OFFSET);
        let atk_tiers = [
            read_i32(overlay, b + RECORD_ATK_TIERS_OFFSET),
            read_i32(overlay, b + RECORD_ATK_TIERS_OFFSET + 4),
            read_i32(overlay, b + RECORD_ATK_TIERS_OFFSET + 8),
        ];
        // AI pattern: NUL-terminated, bounded by the record tail.
        let pat_start = b + RECORD_AI_PATTERN_OFFSET;
        let pat_end = b + OPPONENT_RECORD_STRIDE;
        let mut ai_pattern = Vec::new();
        for &byte in &overlay[pat_start..pat_end] {
            if byte == 0 {
                break;
            }
            ai_pattern.push(byte);
        }
        out.push(BakaOpponent {
            index: i,
            gold_reward: gold,
            damage_mod,
            def_tiers,
            crit_chance,
            atk_tiers,
            ai_pattern,
        });
    }
    Some(out)
}

/// Decode the 17 per-fighter action tables through the `PTR_DAT_801db8b8`
/// pointer array. `None` when the array or any pointed-to table falls outside
/// the image.
pub fn parse_actions(overlay: &[u8]) -> Option<Vec<BakaActionSet>> {
    let ptr_off = (ACTION_PTR_TABLE_VA - BAKA_OVERLAY_BASE_VA) as usize;
    if overlay.len() < ptr_off + OPPONENT_COUNT * 4 {
        return None;
    }
    let mut out = Vec::with_capacity(OPPONENT_COUNT);
    for i in 0..OPPONENT_COUNT {
        let p = ptr_off + i * 4;
        let va = u32::from_le_bytes([overlay[p], overlay[p + 1], overlay[p + 2], overlay[p + 3]]);
        if va < BAKA_OVERLAY_BASE_VA {
            return None;
        }
        let table = (va - BAKA_OVERLAY_BASE_VA) as usize;
        if overlay.len() < table + ACTIONS_PER_FIGHTER * ACTION_RECORD_STRIDE {
            return None;
        }
        let mut power = [0i32; ACTIONS_PER_FIGHTER];
        let mut keyframes = [0i32; ACTIONS_PER_FIGHTER];
        for (a, (pw, kf)) in power.iter_mut().zip(keyframes.iter_mut()).enumerate() {
            let r = table + a * ACTION_RECORD_STRIDE;
            *pw = read_i32(overlay, r + ACTION_POWER_OFFSET);
            *kf = read_i32(overlay, r + ACTION_KEYFRAME_COUNT_OFFSET);
        }
        out.push(BakaActionSet {
            index: i,
            power,
            keyframes,
        });
    }
    Some(out)
}

/// Whether a decoded AI pattern is a valid non-empty attack loop (every symbol
/// is one of the three attack types `1`/`2`/`3`). A real opponent always has
/// one; the field bounds the table.
pub fn is_valid_pattern(pattern: &[u8]) -> bool {
    !pattern.is_empty() && pattern.iter().all(|&s| (1..=3).contains(&s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_offset_and_consts() {
        assert_eq!(OPPONENT_TABLE_FILE_OFFSET, 0x8E84);
        assert_eq!(OPPONENT_RECORD_STRIDE, 0x6C);
        assert_eq!(OPPONENT_COUNT, 17);
        assert_eq!(ROUND_WIN_TARGET, 2);
        // The historical "+0x20 view" base used by the gold read.
        assert_eq!(OPPONENT_TABLE_VA + RECORD_GOLD_OFFSET as u32, 0x801D_76BC);
        // The AI-pattern VA the move picker reads (`DAT_801d76e8`).
        assert_eq!(
            OPPONENT_TABLE_VA + RECORD_AI_PATTERN_OFFSET as u32,
            0x801D_76E8
        );
    }

    #[test]
    fn parse_gold_stats_and_pattern() {
        let off = 0x10;
        let mut buf = vec![0u8; off + 2 * OPPONENT_RECORD_STRIDE];
        // record 1: gold 25, stats, pattern [1,2,3] then NUL.
        let b = off + OPPONENT_RECORD_STRIDE;
        buf[b + RECORD_GOLD_OFFSET..b + RECORD_GOLD_OFFSET + 4]
            .copy_from_slice(&25u32.to_le_bytes());
        buf[b + RECORD_DAMAGE_MOD_OFFSET..b + RECORD_DAMAGE_MOD_OFFSET + 4]
            .copy_from_slice(&40i32.to_le_bytes());
        for (t, v) in [(0usize, 10i32), (1, 20), (2, 30)] {
            buf[b + RECORD_DEF_TIERS_OFFSET + t * 4..b + RECORD_DEF_TIERS_OFFSET + t * 4 + 4]
                .copy_from_slice(&v.to_le_bytes());
            buf[b + RECORD_ATK_TIERS_OFFSET + t * 4..b + RECORD_ATK_TIERS_OFFSET + t * 4 + 4]
                .copy_from_slice(&(v * 2).to_le_bytes());
        }
        buf[b + RECORD_CRIT_CHANCE_OFFSET..b + RECORD_CRIT_CHANCE_OFFSET + 4]
            .copy_from_slice(&15i32.to_le_bytes());
        buf[b + RECORD_AI_PATTERN_OFFSET..b + RECORD_AI_PATTERN_OFFSET + 3]
            .copy_from_slice(&[1, 2, 3]);
        let recs = parse_at(&buf, off, 2).expect("parses");
        assert_eq!(recs[1].gold_reward, 25);
        assert_eq!(recs[1].damage_mod, 40);
        assert_eq!(recs[1].def_tiers, [10, 20, 30]);
        assert_eq!(recs[1].atk_tiers, [20, 40, 60]);
        assert_eq!(recs[1].crit_chance, 15);
        assert_eq!(recs[1].ai_pattern, vec![1, 2, 3]);
        assert!(is_valid_pattern(&recs[1].ai_pattern));
        // cursor wraps and subtracts 1 → attack type.
        assert_eq!(recs[1].attack_at(0), Some(0));
        assert_eq!(recs[1].attack_at(3), Some(0)); // wraps
        assert_eq!(recs[1].attack_at(2), Some(2));
        // record 0 empty pattern → invalid / no attack.
        assert!(!is_valid_pattern(&recs[0].ai_pattern));
        assert_eq!(recs[0].attack_at(0), None);
    }

    #[test]
    fn parse_actions_through_pointer_array() {
        let base = BAKA_OVERLAY_BASE_VA;
        let ptr_off = (ACTION_PTR_TABLE_VA - base) as usize;
        let table_off = ptr_off + OPPONENT_COUNT * 4;
        let mut buf = vec![0u8; table_off + OPPONENT_COUNT * ACTIONS_PER_FIGHTER * 0x60];
        for i in 0..OPPONENT_COUNT {
            let t = table_off + i * ACTIONS_PER_FIGHTER * ACTION_RECORD_STRIDE;
            let va = base + t as u32;
            buf[ptr_off + i * 4..ptr_off + i * 4 + 4].copy_from_slice(&va.to_le_bytes());
            for a in 0..ACTIONS_PER_FIGHTER {
                let r = t + a * ACTION_RECORD_STRIDE;
                buf[r + ACTION_POWER_OFFSET..r + ACTION_POWER_OFFSET + 4]
                    .copy_from_slice(&((i * 10 + a) as i32).to_le_bytes());
                buf[r + ACTION_KEYFRAME_COUNT_OFFSET..r + ACTION_KEYFRAME_COUNT_OFFSET + 4]
                    .copy_from_slice(&(a as i32 + 1).to_le_bytes());
            }
        }
        let sets = parse_actions(&buf).expect("parses");
        assert_eq!(sets.len(), OPPONENT_COUNT);
        assert_eq!(sets[1].power[2], 12);
        assert_eq!(sets[1].attack_power(1), Some(11));
        assert_eq!(sets[1].attack_power(3), Some(13));
        assert_eq!(sets[1].attack_power(4), Some(14));
        assert_eq!(sets[1].attack_power(0), None);
        assert_eq!(sets[2].keyframes[ACTION_SPECIAL], 5);
    }

    #[test]
    fn too_short_is_none() {
        assert!(parse_at(&[0u8; 4], 0, 1).is_none());
        assert!(parse_actions(&[0u8; 4]).is_none());
    }
}
