//! Enemy per-turn multi-action budget - clean-room port of the AGL-gauge
//! spending loop inside the monster action picker `FUN_801E9FD4`
//! (`ghidra/scripts/funcs/overlay_battle_action_801e9fd4.txt`, the physical
//! branch at `0x801EA2E4..0x801EA3CC`).
//!
//! ## What retail does
//!
//! When the picker decides a monster attacks physically (rather than casting),
//! it does **not** queue a single swing - it fills the actor's action stream
//! `+0x1DF..` with as many swings as the monster's **per-round AGL gauge**
//! (`actor+0x154`, seeded from record `+0x0E` = the AGL stat, reset to base
//! each round by `FUN_801D88CC`) can afford. Each swing-action record carries a
//! per-action AGL cost at `+0x74`; the loop:
//!
//! 1. Gathers the monster's candidate swing actions (records whose tag byte
//!    `+0x0` is in `0x0C..=0x1F` and whose `+0x74` cost is not the `0xFF`
//!    "not-an-attack" sentinel). This gather is done by the caller - the
//!    engine passes the resulting `action_costs`.
//! 2. Repeatedly rolls `rand() % candidate_count` to pick a random candidate,
//!    and, when the current gauge can still pay that candidate's cost, queues
//!    it and subtracts the cost from the gauge; otherwise it counts a miss.
//! 3. Stops at **15 queued swings** (`s6 < 0xF`) or **16 misses** (`s2 < 0x10`).
//!    The miss counter only advances on an unaffordable pick and is never
//!    reset, so once the gauge can no longer pay the cheapest swing every
//!    subsequent roll misses and the loop drains out within 16 rolls.
//!
//! So a monster's swing count per turn is AGL-driven: a high-AGL monster with
//! cheap swings gets several hits, a low-AGL one gets one. This is the enemy
//! analogue of the party-side Arts AP gauge (see
//! `docs/subsystems/arts-command-gauge.md`).
//!
//! The whole stream is executed inside a *single* action in retail (the
//! attack-chain state `0x1E` walks `+0x1DF..` until its `0x00` terminator), so
//! the count returned here is the number of swings the monster lands on its one
//! turn, not a re-selection count.

/// Retail cap on queued swings per turn (`s6 < 0xF`).
pub const MAX_ENEMY_ACTIONS: usize = 15;

/// Retail cap on unaffordable picks before the loop gives up (`s2 < 0x10`).
pub const MAX_ENEMY_PICK_MISSES: u32 = 16;

/// Faithful port of the AGL-gauge spending loop: given the monster's per-round
/// AGL `gauge` and the AGL `action_costs` of its candidate swing actions, roll
/// the deterministic battle `rng` to build the queued swing stream (indices
/// into `action_costs`). Returns the queued indices in queue order; its length
/// is the number of swings the monster lands this turn.
///
/// Each iteration draws exactly one `rng()` value (the candidate roll), matching
/// retail's one `rand()` per loop pass. Returns an empty stream (no RNG drawn)
/// when there are no candidate actions - the caller then falls back to a single
/// swing.
///
/// PORT: FUN_801E9FD4 (physical multi-action budget branch)
pub fn enemy_action_budget(
    gauge: u16,
    action_costs: &[u8],
    rng: &mut dyn FnMut() -> u32,
) -> Vec<u8> {
    let mut queue: Vec<u8> = Vec::new();
    if action_costs.is_empty() {
        return queue;
    }
    let count = action_costs.len() as u32;
    let mut remaining = gauge;
    let mut misses = 0u32;
    while queue.len() < MAX_ENEMY_ACTIONS && misses < MAX_ENEMY_PICK_MISSES {
        let pick = (rng() % count) as usize;
        let cost = action_costs[pick];
        // `0xFF` is retail's "not an attack action" sentinel (the caller should
        // already have dropped those; guarded here too). An affordable pick is
        // queued and the gauge pays for it; anything else is a miss.
        if cost != 0xFF && remaining >= cost as u16 {
            queue.push(pick as u8);
            remaining -= cost as u16;
        } else {
            misses += 1;
        }
    }
    queue
}

/// Convenience over [`enemy_action_budget`]: the number of swings a monster
/// lands this turn for the given AGL gauge + swing-cost set. Draws the same RNG
/// as [`enemy_action_budget`].
pub fn enemy_action_count(gauge: u16, action_costs: &[u8], rng: &mut dyn FnMut() -> u32) -> u8 {
    enemy_action_budget(gauge, action_costs, rng).len() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic RNG that replays a fixed sequence (then repeats the last
    /// value), so each test pins the exact candidate rolls.
    fn seq_rng(vals: Vec<u32>) -> impl FnMut() -> u32 {
        let mut i = 0usize;
        move || {
            let v = *vals.get(i).unwrap_or(vals.last().unwrap_or(&0));
            i += 1;
            v
        }
    }

    #[test]
    fn no_candidates_yields_no_swings_and_no_rng() {
        let mut drew = false;
        let mut rng = || {
            drew = true;
            0
        };
        let q = enemy_action_budget(60, &[], &mut rng);
        assert!(q.is_empty());
        assert!(!drew, "empty candidate set must not draw RNG");
    }

    #[test]
    fn gauge_bounds_the_swing_count() {
        // One candidate costing 20 AGL, gauge 60 -> exactly 3 swings, then the
        // gauge (0) can't pay the 4th so the miss counter drains to 16.
        let mut rng = seq_rng(vec![0]);
        let q = enemy_action_budget(60, &[20], &mut rng);
        assert_eq!(q.len(), 3, "60 / 20 = 3 swings");
        assert!(q.iter().all(|&i| i == 0));
    }

    #[test]
    fn caps_at_fifteen_swings_when_actions_are_free() {
        // A zero-cost swing never drains the gauge, so the 15-swing cap stops it.
        let mut rng = seq_rng(vec![0]);
        let q = enemy_action_budget(100, &[0], &mut rng);
        assert_eq!(q.len(), MAX_ENEMY_ACTIONS);
    }

    #[test]
    fn low_agl_monster_gets_one_swing() {
        // Gauge affords exactly one 30-cost swing; the second is unaffordable and
        // the miss counter drains the loop.
        let mut rng = seq_rng(vec![0]);
        let n = enemy_action_count(30, &[30], &mut rng);
        assert_eq!(n, 1);
    }

    #[test]
    fn unaffordable_first_pick_still_lands_a_cheaper_one() {
        // Two candidates: cost 50 (index 0) and cost 10 (index 1). Gauge 40.
        // Rolls pick index 0 (unaffordable -> miss) then index 1 (affordable).
        // After that the gauge is 30, still affords index 1, so index-1 rolls
        // keep landing until 16 misses / the gauge empties.
        let mut rng = seq_rng(vec![0, 1, 1, 1, 1]); // then repeats 1
        let q = enemy_action_budget(40, &[50, 10], &mut rng);
        // 40 / 10 = 4 affordable index-1 swings; index-0 is never affordable.
        assert_eq!(q.iter().filter(|&&i| i == 1).count(), 4);
        assert!(
            !q.contains(&0),
            "the 50-cost swing is never affordable at gauge<=40"
        );
    }

    #[test]
    fn ff_cost_sentinel_is_never_queued() {
        let mut rng = seq_rng(vec![0]);
        let q = enemy_action_budget(100, &[0xFF], &mut rng);
        assert!(q.is_empty(), "0xFF-cost actions are not real swings");
    }
}
