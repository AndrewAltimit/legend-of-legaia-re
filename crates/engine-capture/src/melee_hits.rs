//! The hits a captured melee action had already landed, replayed with
//! retail's amounts (`LEGAIA_BATTLE_MELEE_HITS`).
//!
//! The action seed `0x0C` zeroes the damage-number ring's push count
//! `ctx[+0x273]`, and every hit the melee kernel resolves pushes its amount
//! to `ctx[+0x83C + i*4]` and the struck pool slot to `ctx[+0x318 + i*2]`.
//! An amount reads `0` again once its number has left the screen, so only
//! the newest pushes still carry theirs. The combo's running total is
//! `DAT_8007BD14`, which the kernel's HP write stores
//! (`0x801EEA64..0x801EEA78`) and the `HIT` / `TOTAL` cluster draws.
//!
//! The replay plays the action from its prompt on its own stream, whose
//! damage rolls are not retail's. Seeded with the ring and the total
//! ([`legaia_engine_core::world::BattleState::forced_melee_hits`] /
//! `forced_melee_total`), each replayed party swing on the victim takes the
//! push it replays when the ring still holds it, and the hit that lands the
//! total takes whatever brings the string to retail's total.

use crate::battle_drive::engine_seat;

/// A melee capture's landed hits: the ring's pushes in order as
/// `(retail pool slot, amount)` (`0` = an amount the ring no longer holds)
/// and the string's total on its victim.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MeleeHitSeed {
    pub hits: Vec<(u8, u16)>,
    pub total: Option<(u8, u32)>,
}

impl MeleeHitSeed {
    pub fn is_empty(&self) -> bool {
        self.hits.is_empty() && self.total.is_none()
    }
}

/// Seed the replay with `seed` (retail pool slots).
pub fn seed_melee_hits(world: &mut legaia_engine_core::world::World, seed: &MeleeHitSeed) {
    let pc = world.party.party_count.clamp(1, 3);
    world.battle.forced_melee_hits = seed
        .hits
        .iter()
        .map(|&(slot, amount)| (engine_seat(slot, pc), amount))
        .collect();
    world.battle.forced_melee_total = seed
        .total
        .map(|(slot, total)| (engine_seat(slot, pc), total));
}

/// `slot:amount,...[|slot:total]` for `LEGAIA_BATTLE_MELEE_HITS`.
pub fn melee_hits_to_env(seed: &MeleeHitSeed) -> String {
    let hits = seed
        .hits
        .iter()
        .map(|(s, a)| format!("{s}:{a}"))
        .collect::<Vec<_>>()
        .join(",");
    match seed.total {
        Some((s, t)) => format!("{hits}|{s}:{t}"),
        None => hits,
    }
}

/// Parse [`melee_hits_to_env`]'s form; a malformed entry is skipped.
pub fn melee_hits_from_env(s: &str) -> MeleeHitSeed {
    let (hits, total) = match s.split_once('|') {
        Some((h, t)) => (h, Some(t)),
        None => (s, None),
    };
    MeleeHitSeed {
        hits: hits
            .split(',')
            .filter_map(|e| {
                let (slot, amount) = e.trim().split_once(':')?;
                Some((slot.parse().ok()?, amount.parse().ok()?))
            })
            .collect(),
        total: total.and_then(|t| {
            let (slot, v) = t.trim().split_once(':')?;
            Some((slot.parse().ok()?, v.parse().ok()?))
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_env_form_round_trips() {
        let seed = MeleeHitSeed {
            hits: vec![(3, 0), (3, 15)],
            total: Some((3, 29)),
        };
        assert_eq!(melee_hits_from_env(&melee_hits_to_env(&seed)), seed);
        let bare = MeleeHitSeed {
            hits: vec![(4, 7)],
            total: None,
        };
        assert_eq!(melee_hits_from_env(&melee_hits_to_env(&bare)), bare);
        assert!(melee_hits_from_env("").is_empty());
    }
}
