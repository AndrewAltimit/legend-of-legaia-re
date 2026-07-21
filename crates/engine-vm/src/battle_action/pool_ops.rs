//! Leaf helpers over the 8-slot battle-actor pool (`&DAT_801C9370`) and the
//! action-context target queue. Each is a self-contained function the battle
//! action state machine (`FUN_801E295C`) and its round driver call; none is a
//! state of the SM itself, so they port cleanly as pure functions.
//!
//! PORT: FUN_801DB9C4 (end-of-action flag scrub)
//! PORT: FUN_801DB318 (formation span-normalise + recentre)
//! PORT: FUN_801D8D00 (target-cycle accessor)
//! PORT: FUN_801DB124 (dead-target redirect roll)
//!
//! All arithmetic is transcribed from the DISASSEMBLY in
//! `ghidra/scripts/funcs/overlay_battle_action_801db9c4.txt`,
//! `..._801db318.txt`, `..._801d8d00.txt`, `..._801db124.txt` - not the C.
//! The pool is 8 slots (0..2 party, 3..7 monsters); slots 0..2 are treated as
//! always-present, slots 3.. are gated on the liveness halfword `actor[+0x14C]`.

/// The `+0x8` actor flag-word bits state `0x5A` keeps: it clears
/// `0x83000000` (bit 31 and bits 25/24). `FUN_801DB9C4`.
pub const END_OF_ACTION_FLAG_KEEP: u32 = 0x7cff_ffff;

/// End-of-action per-actor flag scrub (state `0x5A`). AND-masks the `+0x8`
/// flag word of the first **7** pool slots with [`END_OF_ACTION_FLAG_KEEP`].
///
/// The retail loop is a fixed `while (i < 7)` over `&DAT_801C9370[i]`, so it
/// touches slots 0..=6 (not slot 7); a shorter slice is truncated to match.
///
/// PORT: FUN_801DB9C4
pub fn clear_end_of_action_flags(flag_words: &mut [u32]) {
    for w in flag_words.iter_mut().take(7) {
        *w &= END_OF_ACTION_FLAG_KEEP;
    }
}

/// A battle-actor's planar position: `+0x34` world X, `+0x38` world Z, both
/// `i16`. Used by [`normalize_formation_span`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FormationPos {
    /// `actor[+0x34]` world X.
    pub x: i16,
    /// `actor[+0x38]` world Z.
    pub z: i16,
}

/// Squash a battle formation whose X or Z extent exceeds `0x800` back inside
/// the camera frame, then recentre it on the origin, shifting the two
/// camera-focus accumulators (`_DAT_80089118` X / `_DAT_80089120` Z) to
/// compensate. `FUN_801DB318`.
///
/// The retail walk covers pool slots 0..=6 (`while (i < 7)`); slots 0..2 are
/// always included, slots 3.. only when the matching `alive[i]` is set. Three
/// passes, faithful to the disassembly:
///
/// 1. min/max of X and Z over the included slots (seeded ±30000).
/// 2. if `max_x - min_x > 0x800`, rescale every included X by
///    `(x << 11) / span_x` and the X focus likewise; same for Z with its own
///    `> 0x800` gate. `span` is the low-16 (sign-extended) of the extent, the
///    quirk that shows up only for absurd (`> 0x7FFF`) formations.
/// 3. recompute min/max, then subtract the centroid `((max + min) >>u 1)` from
///    every included slot and add it back onto the focus accumulators.
///
/// PORT: FUN_801DB318
pub fn normalize_formation_span(
    pos: &mut [FormationPos],
    alive: &[bool],
    focus_x: &mut i32,
    focus_z: &mut i32,
) {
    let included = |i: usize| i < 3 || alive.get(i).copied().unwrap_or(false);
    let slots = pos.len().min(7);

    // extents(): min/max of X and Z over the included slots (seeded ±30000).
    let extents = |pos: &[FormationPos]| {
        let (mut max_x, mut min_x) = (-30000i16, 30000i16);
        let (mut max_z, mut min_z) = (-30000i16, 30000i16);
        for (i, p) in pos.iter().enumerate().take(slots) {
            if !included(i) {
                continue;
            }
            if max_x < p.x {
                max_x = p.x;
            }
            if p.x < min_x {
                min_x = p.x;
            }
            if max_z < p.z {
                max_z = p.z;
            }
            if p.z < min_z {
                min_z = p.z;
            }
        }
        (max_x, min_x, max_z, min_z)
    };

    // Pass 1 + 2: extents, then per-axis span-normalise. The comparison uses
    // the full i32 extent; the divisor is that extent narrowed to i16
    // (retail's `(short)(max - min)`).
    let (max_x, min_x, max_z, min_z) = extents(pos);
    if max_x as i32 - min_x as i32 > 0x800 {
        let span = max_x.wrapping_sub(min_x) as i32;
        for (i, p) in pos.iter_mut().enumerate().take(slots) {
            if included(i) {
                p.x = ((p.x as i32).wrapping_shl(11) / span) as i16;
            }
        }
        *focus_x = focus_x.wrapping_shl(11) / span;
    }
    if max_z as i32 - min_z as i32 > 0x800 {
        let span = max_z.wrapping_sub(min_z) as i32;
        for (i, p) in pos.iter_mut().enumerate().take(slots) {
            if included(i) {
                p.z = ((p.z as i32).wrapping_shl(11) / span) as i16;
            }
        }
        *focus_z = focus_z.wrapping_shl(11) / span;
    }

    // Pass 3: recompute extents (positions moved), then recentre on the
    // centroid `((max + min) as u32) >> 1` truncated to i16.
    let (max_x, min_x, max_z, min_z) = extents(pos);
    let cx = (((max_x as i32 + min_x as i32) as u32) >> 1) as i16;
    let cz = (((max_z as i32 + min_z as i32) as u32) >> 1) as i16;
    for (i, p) in pos.iter_mut().enumerate().take(slots) {
        if included(i) {
            p.x = p.x.wrapping_sub(cx);
            p.z = p.z.wrapping_sub(cz);
        }
    }
    *focus_x = focus_x.wrapping_add(cx as i32);
    *focus_z = focus_z.wrapping_add(cz as i32);
}

/// Which way the player's target cursor steps through the multi-target attack
/// queue built by `FUN_801D8A88`. `FUN_801D8D00`'s `param_1`: `0` = the
/// "next" arm, `1` = the "previous" arm. (Retail's `default` arm returns an
/// uninitialised register, so it is never invoked - hence only the two real
/// modes are modelled.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetCycle {
    /// `param_1 == 0`.
    Next,
    /// `param_1 == 1`.
    Prev,
}

/// Step the attack target cursor to the neighbour of `active_target` in the
/// context target queue. `FUN_801D8D00`.
///
/// `queue` is the context byte window based at `ctx[+0x244]`: `queue[0]` = the
/// live-target count, `queue[1]` = `ctx[+0x245]` (the ordered ring's wrap
/// slot), `queue[2..]` = `ctx[+0x246..]` the ordered target slots. The routine
/// first finds where `active_target` sits in the ring, then:
///
/// - [`TargetCycle::Next`]: returns `queue[idx + 2]`, or wraps to `queue[1]`
///   when `idx == count - 1`.
/// - [`TargetCycle::Prev`]: returns `queue[idx]`, or wraps to `queue[count]`
///   when `idx == 0`.
///
/// PORT: FUN_801D8D00
pub fn cycle_attack_target(queue: &[u8], active_target: u8, mode: TargetCycle) -> u8 {
    let count = queue[0] as usize;
    // Locate active_target in the ring; `idx` mirrors retail's `uVar3`.
    let mut idx: usize = 0;
    if count != 0 {
        let mut uvar1 = 1usize;
        loop {
            let uvar2 = uvar1;
            if active_target == queue[uvar2] {
                break;
            }
            uvar1 = uvar2 + 1;
            idx = uvar2;
            if uvar2 >= count {
                break;
            }
        }
    }
    match mode {
        TargetCycle::Next => {
            if idx == count.wrapping_sub(1) {
                queue[1]
            } else {
                queue[idx + 2]
            }
        }
        TargetCycle::Prev => {
            if idx == 0 {
                queue[count]
            } else {
                queue[idx]
            }
        }
    }
}

/// The queued-action fields [`redirect_dead_target`] inspects on the acting
/// battle actor.
#[derive(Debug, Clone, Copy)]
pub struct RedirectQuery {
    /// `actor[+0x1DD]` - the currently chosen target slot.
    pub target_slot: u8,
    /// `actor[+0x1DE]` - action category (1=Item, 2=Magic, 3=Attack).
    pub category: u8,
    /// `actor[+0x1DF]` - the first action-param byte (spell id / item id).
    pub param0: u8,
}

/// Re-roll a queued action's target when the chosen target has died, keeping
/// the roll on the same side of the field. `FUN_801DB124`.
///
/// Returns `Some(new_slot)` when the action is redirected, `None` when it is
/// left alone. A redirect happens only when the current target is **dead**
/// (`!is_alive(target_slot)`) and the category qualifies:
///
/// - Attack (`3`): always.
/// - Magic (`2`): when the spell's table class byte `spell_tier(param0) >= 0xA`
///   (a status/utility spell), or when the current target is an enemy slot
///   (`target_slot >= 3`); an offensive spell already aimed at a party slot is
///   left alone.
/// - Item (`1`): only for item ids `0xFE` or `0x98`.
///
/// The roll picks a **living** slot on the current target's side: party
/// (`target_slot < 3`) rolls `rng() % party_count`; enemy rolls
/// `rng() % monster_count + 3`. It retries until it lands on a living slot, so
/// `is_alive` must eventually return true for some slot on that side (retail
/// loops forever otherwise).
///
/// PORT: FUN_801DB124
pub fn redirect_dead_target(
    q: RedirectQuery,
    party_count: u8,
    monster_count: u8,
    mut rng: impl FnMut() -> i32,
    is_alive: impl Fn(u8) -> bool,
    spell_tier: impl Fn(u8) -> u8,
) -> Option<u8> {
    if q.target_slot >= 8 {
        return None;
    }
    if is_alive(q.target_slot) {
        return None;
    }
    let reroll = match q.category {
        3 => true,
        2 => spell_tier(q.param0) >= 0xa || q.target_slot >= 3,
        1 => q.param0 == 0xfe || q.param0 == 0x98,
        _ => false,
    };
    if !reroll {
        return None;
    }
    if q.target_slot < 3 {
        loop {
            let t = (rng() % party_count as i32) as u8;
            if is_alive(t) {
                return Some(t);
            }
        }
    } else {
        loop {
            let t = (rng() % monster_count as i32) as u8 + 3;
            if is_alive(t) {
                return Some(t);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_scrub_masks_first_seven_slots() {
        let mut words = [0xFFFF_FFFFu32; 8];
        clear_end_of_action_flags(&mut words);
        for w in words.iter().take(7) {
            assert_eq!(*w, 0x7cff_ffff);
        }
        // Slot 7 is untouched by the `< 7` loop.
        assert_eq!(words[7], 0xFFFF_FFFF);
    }

    #[test]
    fn flag_scrub_clears_exactly_bits_31_25_24() {
        let mut words = [0x8300_0001u32];
        clear_end_of_action_flags(&mut words);
        assert_eq!(words[0], 0x0000_0001);
    }

    #[test]
    fn flag_scrub_truncates_short_slice() {
        let mut words = [0xFFFF_FFFFu32; 3];
        clear_end_of_action_flags(&mut words);
        assert!(words.iter().all(|w| *w == 0x7cff_ffff));
    }

    #[test]
    fn formation_within_frame_only_recenters() {
        // Span 400 in X (< 0x800), 0 in Z: no normalise pass, just recentre.
        let mut pos = [
            FormationPos { x: 100, z: 0 },
            FormationPos { x: 500, z: 0 },
            FormationPos { x: 300, z: 0 },
        ];
        let alive = [true, true, true];
        let (mut fx, mut fz) = (0i32, 0i32);
        normalize_formation_span(&mut pos, &alive, &mut fx, &mut fz);
        // centroid X = (500 + 100) >> 1 = 300.
        assert_eq!(pos[0].x, -200);
        assert_eq!(pos[1].x, 200);
        assert_eq!(pos[2].x, 0);
        assert_eq!(fx, 300);
        assert_eq!(fz, 0);
    }

    #[test]
    fn formation_wide_x_is_squashed_then_centered() {
        // X span 4000 (> 0x800). span = 4000, scale = (x << 11) / 4000.
        let mut pos = [
            FormationPos { x: -2000, z: 0 },
            FormationPos { x: 2000, z: 0 },
            FormationPos { x: 0, z: 0 },
        ];
        let alive = [true, true, true];
        let (mut fx, mut fz) = (0i32, 0i32);
        normalize_formation_span(&mut pos, &alive, &mut fx, &mut fz);
        // After scale: -2000 -> (-2000<<11)/4000 = -1024; 2000 -> 1024; 0 -> 0.
        // Then centroid = (1024 + -1024) >> 1 = 0, so no shift.
        assert_eq!(pos[0].x, -1024);
        assert_eq!(pos[1].x, 1024);
        assert_eq!(pos[2].x, 0);
        assert_eq!(fx, 0);
    }

    #[test]
    fn formation_skips_dead_monster_slots() {
        // Slot 3 (a monster) is dead and must not move the extents.
        let mut pos = [
            FormationPos { x: 0, z: 0 },
            FormationPos { x: 0, z: 0 },
            FormationPos { x: 0, z: 0 },
            FormationPos { x: 9000, z: 0 }, // dead - ignored
        ];
        let alive = [true, true, true, false];
        let (mut fx, mut fz) = (0i32, 0i32);
        normalize_formation_span(&mut pos, &alive, &mut fx, &mut fz);
        // Extents only over slots 0..2 (all x=0): centroid 0, nothing moves.
        assert_eq!(pos[0].x, 0);
        assert_eq!(pos[3].x, 9000);
        assert_eq!(fx, 0);
    }

    #[test]
    fn target_cycle_mid_ring() {
        // count=3, queue[1]=10 (wrap slot), queue[2..]=[20,30,40].
        // active_target=20 matches at uvar2=2 -> idx=1 (idx = match_pos - 1).
        let queue = [3u8, 10, 20, 30, 40];
        // Next: idx==count-1? 1==2 no -> queue[idx+2] = queue[3] = 30.
        assert_eq!(cycle_attack_target(&queue, 20, TargetCycle::Next), 30);
        // Prev: idx==0? no -> queue[idx] = queue[1] = 10.
        assert_eq!(cycle_attack_target(&queue, 20, TargetCycle::Prev), 10);
    }

    #[test]
    fn target_cycle_wraps() {
        let queue = [3u8, 10, 20, 30, 40];
        // active_target=30 matches at uvar2=3 (=count) -> idx=2=count-1.
        // Next wraps to queue[1]=10.
        assert_eq!(cycle_attack_target(&queue, 30, TargetCycle::Next), 10);
        // active_target=10 matches at uvar2=1 -> idx=0. Prev wraps to
        // queue[count]=queue[3]=30.
        assert_eq!(cycle_attack_target(&queue, 10, TargetCycle::Prev), 30);
    }

    #[test]
    fn redirect_skips_when_target_alive() {
        let q = RedirectQuery {
            target_slot: 3,
            category: 3,
            param0: 0,
        };
        let out = redirect_dead_target(q, 1, 2, || 0, |_| true, |_| 0);
        assert_eq!(out, None);
    }

    #[test]
    fn redirect_attack_rerolls_to_living_enemy() {
        let q = RedirectQuery {
            target_slot: 3, // dead enemy
            category: 3,
            param0: 0,
        };
        // monster_count=3 -> slots 3,4,5. Slot 3 dead, 4 alive.
        let mut rolls = [0i32, 1i32].into_iter();
        let out = redirect_dead_target(
            q,
            1,
            3,
            move || rolls.next().unwrap(),
            |slot| slot == 4, // only slot 4 alive
            |_| 0,
        );
        // roll 0 -> slot 3 (dead, retry); roll 1 -> slot 4 (alive).
        assert_eq!(out, Some(4));
    }

    #[test]
    fn redirect_magic_offensive_at_party_target_is_left_alone() {
        // Magic, low-tier (< 0xA), target a dead party slot -> no reroll.
        let q = RedirectQuery {
            target_slot: 1,
            category: 2,
            param0: 0x10,
        };
        let out = redirect_dead_target(q, 3, 2, || 0, |_| false, |_| 5);
        assert_eq!(out, None);
    }

    #[test]
    fn redirect_magic_status_spell_rerolls() {
        // Magic, high-tier (>= 0xA) -> reroll on the party side.
        let q = RedirectQuery {
            target_slot: 1, // dead party slot
            category: 2,
            param0: 0x20,
        };
        let out = redirect_dead_target(q, 3, 2, || 2, |slot| slot == 2, |_| 0xa);
        assert_eq!(out, Some(2));
    }

    #[test]
    fn redirect_item_only_special_ids() {
        let dead = |_: u8| false;
        let base = RedirectQuery {
            target_slot: 4,
            category: 1,
            param0: 0x01,
        };
        // Ordinary item id, dead enemy target -> no reroll.
        assert_eq!(redirect_dead_target(base, 1, 3, || 0, dead, |_| 0), None);
        // Special id 0xFE -> reroll on the enemy side. monster_count=3 covers
        // slots 3,4,5; slot 4 (target) dead, slot 5 alive. rng 2 -> slot 5.
        let q = RedirectQuery {
            param0: 0xfe,
            ..base
        };
        assert_eq!(
            redirect_dead_target(q, 1, 3, || 2, |s| s == 5, |_| 0),
            Some(5)
        );
    }
}
