//! New Game+'s enemy curve.
//!
//! A New Game+ party arrives with the levels of a cleared game, so a flat
//! multiplier would either leave the early bosses trivial or make the late
//! game unwinnable. The curve has two terms: one per cycle, and one that
//! grows with the party's strongest level, so a fight stays a fight wherever
//! the party is in the story. Story bosses take a larger share of the level
//! term than ordinary monsters, which keeps a set piece a challenge at a
//! high level. The unwinnable sparring partner is pinned, as the patcher
//! pins it.

use legaia_randomizer::monsters::{SCALE_PINNED_MONSTER_IDS, STORY_BOSS_MONSTER_IDS};

/// Permille added per New Game+ cycle.
pub const PER_CYCLE: u32 = 250;
/// Permille added per party level above 1, for an ordinary monster.
pub const PER_LEVEL: u32 = 12;
/// Permille added per party level above 1, for a story boss.
pub const PER_LEVEL_BOSS: u32 = 20;
/// The curve's ceiling.
pub const MAX_PERMILLE: u32 = 5000;

/// The New Game+ enemy scale for `monster_id` on cycle `cycle` with the
/// party's strongest member at `party_level`. `1000` outside New Game+
/// (`cycle == 0`) and for a pinned monster.
pub fn ng_plus_permille(cycle: u8, party_level: u8, monster_id: u16) -> u16 {
    if cycle == 0 || SCALE_PINNED_MONSTER_IDS.contains(&monster_id) {
        return 1000;
    }
    let per_level = if STORY_BOSS_MONSTER_IDS.contains(&monster_id) {
        PER_LEVEL_BOSS
    } else {
        PER_LEVEL
    };
    let lvl = u32::from(party_level.saturating_sub(1));
    (1000 + PER_CYCLE * u32::from(cycle) + per_level * lvl).min(MAX_PERMILLE) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_grows_with_cycle_and_level_and_favours_bosses() {
        assert_eq!(ng_plus_permille(0, 99, 73), 1000);
        assert_eq!(ng_plus_permille(3, 99, 79), 1000, "Tetsu is pinned");
        let regular = ng_plus_permille(1, 40, 30);
        let boss = ng_plus_permille(1, 40, 73);
        assert!(boss > regular && regular > 1000);
        assert!(ng_plus_permille(2, 40, 30) > regular);
        assert!(ng_plus_permille(1, 60, 30) > regular);
        assert_eq!(ng_plus_permille(9, 99, 73), MAX_PERMILLE as u16);
    }
}
