//! The battle-end presentation's `World`-free half: the results sequencer's
//! phase constants, the [`VictorySequence`] state the engine walks, and the
//! win-pose tier and pick (`0x8004E78C..0x8004EAEC`).
//!
//! The walk itself, over a `World`, is `legaia_engine_core`'s
//! `world::battle::victory`, which also carries the measured retail timeline.
//!
//! REF: FUN_8004e568 (the results sequencer these constants and kernels are read from)

use legaia_engine_battle_vm::battle_action::BattleEndCause;
use legaia_game_tables::victory_pose::VictoryPoseTable;

/// Vsyncs the pose-8 hold lasts while retail streams the hero voice clip and
/// the reward bank (phases `0 -> 2 -> 4 -> 5`). Measured once on
/// `rim_elm_gimard_victory` under PCSX-Redux (signal v322 -> results v402);
/// a real drive varies it, the engine keeps the measured span.
pub const VICTORY_LOAD_FRAMES: u16 = 80;

/// Vsyncs the side-band stream request `ctx[+0x26B]` stays raised at the
/// head of the load hold: `FUN_80055B4C` raises it with the battle-end
/// signal and the stream tick `FUN_801F17F8` clears it on landing, and the
/// sequencer holds at `0x8004E5C0` until it does - the pose-archive stream,
/// before the phase walk's own two CD waits. Measured on
/// `rim_elm_gimard_victory` under PCSX-Redux (`autorun_victory_timeline.lua`:
/// raised v322..v349 with the signal at v322, the phase walk from v350).
pub const VICTORY_STREAM_FRAMES: u16 = 28;

/// The results hold: `gp+0xA54` counts one per vsync and the exit fade is
/// spawned once it reaches `0x100` (`0x8004F778` / `0x8004FAF8`).
pub const VICTORY_RESULTS_HOLD_FRAMES: u16 = 0x100;

/// The phase halfword's exit threshold in `FUN_80046A20`
/// (`slti v0,v0,0x43` at `0x80046DAC`).
pub const VICTORY_EXIT_PHASE: u16 = 0x43;

/// The phase halfword's value on the frame the exit fade is spawned
/// (`li v0,0x2; sh v0,0x6ce(a0)` at `0x8004F7A0` / `0x8004FC44`).
pub const VICTORY_FADE_PHASE_SEED: u16 = 2;

/// The level-up jingle - the single category-`11` descriptor, keyed on PROT
/// 0889 in VAB slot 11 (`FUN_8004FCC8(0x50)` at `0x8004F6E8`).
pub const LEVEL_UP_CUE: u16 = 0x50;

/// Story flag the results frame sets (`FUN_8003CE08(0x35)` at
/// `0x8004EECC`) - a "won a battle" latch in the system-flag bank.
pub const VICTORY_STORY_FLAG: u16 = 0x35;

/// Status bits at actor `+0x16E` that force the weak-pose tier
/// (`andi v0,v0,0x107b` at `0x8004E830`).
pub const WEAK_POSE_STATUS_MASK: u16 = 0x107B;

/// Where the sequence is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VictoryPhase {
    /// Phases `0..=4`: the CD loads. The pose actor holds framing 8.
    Loading { frames_left: u16 },
    /// Phase 5 with `ctx[+0x6CE] == 1`: windows up, pose clip playing,
    /// `hold` = `gp+0xA54`.
    Results { hold: u16 },
    /// `ctx[+0x6CE] >= 2`: the exit fade is running; `phase` is the
    /// halfword the exit gate reads.
    Exit { phase: u16 },
}

/// The armed end-of-battle presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VictorySequence {
    pub cause: BattleEndCause,
    pub phase: VictoryPhase,
    /// Seat that poses (`ctx[+0x13]`; the leader - see the module docs).
    pub pose_actor: usize,
    /// The chosen win-pose action id, `0x11..=0x18` (`gp+0xA4C`), once
    /// phase 0 has picked it. `None` for a wipe / escape or without the
    /// SCUS table.
    pub pose_id: Option<u8>,
    /// Whether the results frame opened its window - the result window
    /// `FUN_801D8DE8(0x41, 0)` on a win (`0x8004F65C`), the loss window
    /// `FUN_801D8DE8(0x42, 0)` on a wipe (`0x8004F900`). A non-zero
    /// special-battle word skips both calls (`0x8004F614` / `0x8004F8F0`),
    /// so an arena leg or a Ra-Seru-forbidden fight ends on the bare scene.
    /// `false` until the results frame runs.
    pub window_opened: bool,
}

impl VictorySequence {
    /// `ctx[+0x26B]` as retail holds it through this sequence: raised for the
    /// first [`VICTORY_STREAM_FRAMES`] vsyncs of a victory's load hold (the
    /// win-pose archive streaming in), idle from then on and on every other
    /// end.
    pub fn side_band_request_up(&self) -> bool {
        matches!(
            (self.cause, self.phase),
            (BattleEndCause::MonsterWipe, VictoryPhase::Loading { frames_left })
                if frames_left > VICTORY_LOAD_FRAMES - VICTORY_STREAM_FRAMES
        )
    }
}

impl VictorySequence {
    /// `true` while the result windows are on screen (results frame through
    /// the exit).
    pub fn results_shown(&self) -> bool {
        matches!(
            self.cause,
            BattleEndCause::MonsterWipe | BattleEndCause::PartyWipe
        ) && matches!(
            self.phase,
            VictoryPhase::Results { .. } | VictoryPhase::Exit { .. }
        )
    }
}

/// The pose **tier** from the pose actor's HP, the round count and its
/// status word (`0x8004E78C..0x8004E83C`): `0` healthy (`hp >= 3/4 max`),
/// `1` (`>= 1/2`), `2` (`>= 1/4`), `3` below that. Two passes then age the
/// tier by one while `round >= tier * 2 + 2` and `tier < 3` - a long fight
/// degrades the pose - and any status in [`WEAK_POSE_STATUS_MASK`] forces
/// tier `4`.
pub fn victory_pose_tier(hp: u16, hp_max: u16, round: u8, status: u16) -> u8 {
    let hp = u32::from(hp);
    let max = u32::from(hp_max);
    let mut tier: u8 = if hp < max >> 1 {
        if hp < max >> 2 { 3 } else { 2 }
    } else if hp < (max * 3) >> 2 {
        1
    } else {
        0
    };
    for _ in 0..2 {
        if u32::from(round) >= u32::from(tier) * 2 + 2 && tier < 3 {
            tier += 1;
        }
    }
    if status & WEAK_POSE_STATUS_MASK != 0 {
        tier = 4;
    }
    tier
}

/// The pose **column** for a tier (`0x8004E870..0x8004EAEC`): each arm draws
/// one `rand()` to choose between its two pairs and a second for the pair's
/// member, so the call order is the retail one.
///
/// | tier | first roll | pair when it passes / fails |
/// |---|---|---|
/// | 0 | `rand & 3 != 0` | healthy / alternate |
/// | 1 | `rand & 1 != 0` | healthy / alternate |
/// | 2 | `rand & 1 != 0` | alternate / weak |
/// | 3 | `rand & 3 != 0` | weak / alternate |
/// | 4 | - | weak |
pub fn victory_pose_column(tier: u8, rng: &mut dyn FnMut() -> u32) -> usize {
    let pair_base = match tier {
        0 => {
            if rng() & 3 != 0 {
                0
            } else {
                2
            }
        }
        1 => {
            if rng() & 1 != 0 {
                0
            } else {
                2
            }
        }
        2 => {
            if rng() & 1 != 0 {
                2
            } else {
                4
            }
        }
        3 => {
            if rng() & 3 != 0 {
                4
            } else {
                2
            }
        }
        _ => 4,
    };
    pair_base + (rng() % 2) as usize
}

/// The win-pose action id for `char_id` (1-based, `DAT_8007BD10[seat]`).
pub fn victory_pose_id(
    table: &VictoryPoseTable,
    char_id: u8,
    tier: u8,
    rng: &mut dyn FnMut() -> u32,
) -> Option<u8> {
    let row = table.get(usize::from(char_id.checked_sub(1)?))?;
    Some(row[victory_pose_column(tier, rng)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_side_band_request_covers_the_head_of_the_load_hold_only() {
        let seq = |cause, phase| VictorySequence {
            cause,
            phase,
            pose_actor: 0,
            pose_id: None,
            window_opened: false,
        };
        let win = |frames_left| {
            seq(
                BattleEndCause::MonsterWipe,
                VictoryPhase::Loading { frames_left },
            )
        };
        // The hold counts VICTORY_LOAD_FRAMES down; the request is up for
        // its first VICTORY_STREAM_FRAMES ticks (v322..v349 of 80).
        let up: Vec<bool> = (1..=VICTORY_LOAD_FRAMES)
            .rev()
            .map(|f| win(f).side_band_request_up())
            .collect();
        assert_eq!(up.iter().filter(|&&u| u).count(), 28);
        assert!(up[..28].iter().all(|&u| u) && up[28..].iter().all(|&u| !u));
        assert!(
            !seq(
                BattleEndCause::MonsterWipe,
                VictoryPhase::Results { hold: 0 }
            )
            .side_band_request_up()
        );
        assert!(
            !seq(BattleEndCause::PartyWipe, VictoryPhase::Results { hold: 0 })
                .side_band_request_up()
        );
    }

    #[test]
    fn tier_follows_hp_quarters_then_ages_with_rounds() {
        assert_eq!(victory_pose_tier(100, 100, 0, 0), 0);
        assert_eq!(victory_pose_tier(75, 100, 0, 0), 0);
        assert_eq!(victory_pose_tier(74, 100, 0, 0), 1);
        assert_eq!(victory_pose_tier(50, 100, 0, 0), 1);
        assert_eq!(victory_pose_tier(49, 100, 0, 0), 2);
        assert_eq!(victory_pose_tier(25, 100, 0, 0), 2);
        assert_eq!(victory_pose_tier(24, 100, 0, 0), 3);
        // Round 2 ages tier 0 to 1; the second pass needs round >= 4.
        assert_eq!(victory_pose_tier(100, 100, 2, 0), 1);
        assert_eq!(victory_pose_tier(100, 100, 4, 0), 2);
        assert_eq!(victory_pose_tier(100, 100, 6, 0), 2);
        // Tier 3 never ages; a status forces 4.
        assert_eq!(victory_pose_tier(1, 100, 9, 0), 3);
        assert_eq!(victory_pose_tier(100, 100, 0, 0x1000), 4);
    }

    #[test]
    fn column_picks_the_pair_then_the_member() {
        // First roll passes (`& 3 != 0`), second roll odd -> healthy col 1.
        let mut rolls = vec![1u32, 1].into_iter();
        let mut rng = || rolls.next().unwrap();
        assert_eq!(victory_pose_column(0, &mut rng), 1);
        // First roll fails -> alternate pair, even member.
        let mut rolls = vec![4u32, 2].into_iter();
        let mut rng = || rolls.next().unwrap();
        assert_eq!(victory_pose_column(0, &mut rng), 2);
        // Tier 3: pass -> weak pair.
        let mut rolls = vec![1u32, 0].into_iter();
        let mut rng = || rolls.next().unwrap();
        assert_eq!(victory_pose_column(3, &mut rng), 4);
        // Tier 4 draws only the member roll.
        let mut rolls = vec![7u32].into_iter();
        let mut rng = || rolls.next().unwrap();
        assert_eq!(victory_pose_column(4, &mut rng), 5);
    }

    #[test]
    fn pose_id_indexes_the_one_based_character_row() {
        let table: VictoryPoseTable = [
            [0x13, 0x14, 0x11, 0x12, 0x15, 0x16],
            [0x11, 0x13, 0x12, 0x14, 0x15, 0x16],
            [0x13, 0x14, 0x11, 0x12, 0x15, 0x16],
            [0x11, 0x12, 0x13, 0x14, 0x15, 0x16],
        ];
        let mut rolls = vec![1u32, 1].into_iter();
        let mut rng = || rolls.next().unwrap();
        assert_eq!(victory_pose_id(&table, 2, 0, &mut rng), Some(0x13));
        let mut rng = || 0;
        assert_eq!(victory_pose_id(&table, 0, 0, &mut rng), None);
        assert_eq!(victory_pose_id(&table, 5, 0, &mut rng), None);
    }
}
