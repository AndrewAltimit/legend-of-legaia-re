//! The encounter guards every formation randomizer respects: the monsters
//! that never take part in a random encounter, and the species ceiling a
//! random formation stays under.

/// 1-based `battle_data` monster ids that must never take part in a **random**
/// encounter - neither donated into one nor replaced out of an existing fight -
/// *on top of* the formations the patcher's `random_formation_mask` already marks scripted.
///
/// The region-rate heuristic correctly classifies every story boss's formation
/// as scripted, with one exception: the early **Gimard** Seru-boss fight sits at
/// a formation index a rate>0 region's range spans, so the heuristic alone would
/// treat it as a random encounter. Left unguarded a randomized run could replace
/// that mandatory tutorial fight (soft-locking a fresh save) or donate Gimard - a
/// boss-tier enemy - into an ordinary early encounter the party can't yet beat.
/// Listing its id forces every formation that holds it back to scripted (see
/// `legaia-patcher`'s `SceneEncounters::locate`) and keeps it out of every donor pool, so the
/// fight is preserved no matter how the scene's regions are laid out - the same
/// intent as the matching stat-side guard (`monsters::PROTECTED_MONSTER_IDS`).
///
/// The first wild Piura are deliberately **not** listed: they are genuine random
/// encounters, and the heuristic already leaves their scripted appearances fixed.
pub const PROTECTED_FORMATION_IDS: &[u8] = &[
    10, // Gimard - the early scripted Seru-boss fight.
];

/// Whether `id` is a [`PROTECTED_FORMATION_IDS`] enemy (one that must never be a
/// random encounter).
pub fn is_protected_formation_id(id: u8) -> bool {
    PROTECTED_FORMATION_IDS.contains(&id)
}

/// The most **distinct** monster species a random formation may hold: **2**,
/// retail's own authoring ceiling - and a hard engine invariant, not a style
/// choice. The battle setup `FUN_80055B6C` (at `0x80055C80..0x80055D2C`)
/// classifies the formation cells into "the first species" (`cells[0]`, with a
/// copy count) and "the other species" - held in a **single register** - then,
/// behind a 50% RNG coin flip (`FUN_80056798() & 1`), rebuilds the cell array
/// as `[other x other_count, first x first_count]` (the species-order variety
/// shuffle). With three distinct species the single "other" register is
/// overwritten by each later species, so `[a, b, c]` rebuilds as `[c, c, a]`:
/// the middle species silently vanishes and the last is duplicated. The other
/// half of the flip loads the formation verbatim - all three distinct blocks
/// stream, which is what makes an over-budget trio a *probabilistic* freeze.
/// Capping distinct species at 2 keeps every formation inside the invariant
/// the rebuild was written for (where it is an exact multiset-preserving
/// species swap). Pinned live: forced installs of `[133,151,94]` /
/// `[94,133,151]` / `[151,133,94]` / `[32,34,14]` (map03 + rikuroa contexts)
/// each read back `[c2, c2, c0]` at battle main, with the write PC at
/// `0x80055D14/18` and the per-seat record pointers showing seat 1 sharing
/// seat 0's block.
pub const MAX_DISTINCT_SPECIES: usize = 2;

/// The monsters a random formation of `slots` fights instead, for the site
/// `(scene, formation_id)` of run `seed`: each seat drawn from `pool` (the
/// scene's own random-encounter monsters, so every pick is one the scene
/// loads), the seat count kept, at most [`MAX_DISTINCT_SPECIES`] species.
/// A formation holding a protected monster, or an empty pool, comes back as
/// it was; protected ids never enter the draw.
pub fn reroll_formation(
    seed: u64,
    scene: &str,
    formation_id: u16,
    slots: &[u16],
    pool: &[u16],
) -> Vec<u16> {
    let protected = |id: u16| u8::try_from(id).is_ok_and(is_protected_formation_id);
    if slots.iter().any(|&id| protected(id)) {
        return slots.to_vec();
    }
    let pool: Vec<u16> = pool.iter().copied().filter(|&id| !protected(id)).collect();
    if pool.is_empty() {
        return slots.to_vec();
    }
    let mut key = scene.as_bytes().to_vec();
    key.extend_from_slice(&formation_id.to_le_bytes());
    let mut rng = crate::sites::site_rng(seed, "formation", &key);
    let mut species: Vec<u16> = Vec::new();
    slots
        .iter()
        .map(|_| {
            let id = if species.len() >= MAX_DISTINCT_SPECIES {
                species[rng.below(species.len())]
            } else {
                pool[rng.below(pool.len())]
            };
            if !species.contains(&id) {
                species.push(id);
            }
            id
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rerolls_keep_the_seat_count_the_species_cap_and_the_guard() {
        let pool = [1u16, 2, 3, 4, 5, 6];
        for f in 0..64u16 {
            let r = reroll_formation(9, "map01", f, &[7, 7, 8, 8], &pool);
            assert_eq!(r.len(), 4);
            assert!(r.iter().all(|id| pool.contains(id)));
            let mut d = r.clone();
            d.sort_unstable();
            d.dedup();
            assert!(d.len() <= MAX_DISTINCT_SPECIES, "{r:?}");
            assert_eq!(r, reroll_formation(9, "map01", f, &[7, 7, 8, 8], &pool));
        }
        assert_eq!(reroll_formation(9, "town0b", 2, &[10], &pool), vec![10]);
        assert!(reroll_formation(9, "m", 0, &[1], &[10]) == vec![1]);
    }
}
