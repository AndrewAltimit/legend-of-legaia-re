//! Leaf helpers over the 8-slot battle-actor pool (`&DAT_801C9370`) and the
//! action-context target queue. Each is a self-contained function some part of
//! the battle overlay calls - the command SM `FUN_801D0748`, the flow SM
//! `FUN_801D388C`, the turn picker `FUN_801DABA4`, the round reset
//! `FUN_801D88CC`, the pose setter `FUN_801D5854`. None is called by the action
//! SM `FUN_801E295C` itself, and none is a state of it, so they port cleanly as
//! pure functions. The per-routine caller is named in the `NOT WIRED` block.
//!
//! PORT: FUN_801DB9C4 (pool `+0x8` flag-word scrub)
//! PORT: FUN_801DB318 (formation span-normalise + recentre)
//! PORT: FUN_801D8A88 (attack-target-queue builder)
//! PORT: FUN_801D8D00 (target-cycle accessor)
//! PORT: FUN_801DB124 (dead-target redirect roll)
//! REF: FUN_801DB8B4 (first-live-monster slot - the canonical port is
//! `engine-core`'s `BattleRound::first_living_monster`, which is live; the
//! fixed-slot twin below exists for oracle work)
//! PORT: FUN_801DBA04 (first-selectable target)
//! PORT: FUN_801DB81C (next-selectable actor)
//! PORT: FUN_80019B28 (12-bit bearing / atan2, faithful LUT form)
//!
//! All arithmetic is transcribed from the DISASSEMBLY in
//! `ghidra/scripts/funcs/overlay_battle_action_801db9c4.txt`,
//! `..._801db318.txt`, `..._801d8a88.txt`, `..._801d8d00.txt`,
//! `..._801db124.txt`, `..._801db8b4.txt`, `..._801dba04.txt`,
//! `..._801db81c.txt`, and `ghidra/scripts/funcs/80019b28.txt` - not the C.
//! The pool is 8 slots (0..2 party, 3..7 monsters); slots 0..2 are treated as
//! always-present, slots 3.. are gated on the liveness halfword `actor[+0x14C]`.
//!
//! # NOT WIRED
//!
//! No engine caller reaches any of these leaves. The missing prerequisite is
//! per-routine, and in every case it is the routine's own retail **caller**
//! being un-ported - each entry below names that caller from the `jal` site in
//! the battle-overlay dumps, not from a doc:
//!
//! - `FUN_801DB9C4` ([`clear_pool_flag_words`]) scrubs the actor **`+0x8`**
//!   flag word. Its only static caller in the battle overlay is the pose
//!   setter `FUN_801D5854`'s invalid-slot guard at `0x801D58E8` - **not** the
//!   action SM's end-of-action state. `FUN_801E295C` never calls it (zero
//!   `jal 0x801db9c4` in `overlay_battle_action_801e295c.txt`). Wiring it needs
//!   two things: a `+0x8` flag word on `BattleActor` (the port carries `+0x1DC`
//!   `flag_bits` and no `+0x8`), and the guard living inside the engine's
//!   `BattleActionHost::pose` implementation, which is `engine-core`'s.
//! - `FUN_801DB318` ([`normalize_formation_span`]) is case `0` of the battle
//!   **flow** SM `FUN_801D388C` (`jal` at `0x801D3908`, jump table
//!   `0x801CE880`), which is not ported. It also shifts the camera-focus
//!   accumulators `_DAT_80089118` / `_DAT_80089120` to compensate for the
//!   squash, and the engine frames the battle camera by a per-action snap
//!   (`camera_height_for_frame` through `BattleActionHost::camera_bounds`) with
//!   no focus accumulator for that compensation to land in.
//! - `FUN_801D8A88` ([`build_attack_target_queue`]), `FUN_801D8D00`
//!   ([`cycle_attack_target`]), `FUN_801DBA04` ([`first_selectable_target`])
//!   and `FUN_801DB81C` ([`next_selectable_actor`]) are all leaves of the
//!   battle **command / menu** SM `FUN_801D0748` (e.g. `jal 0x801d8a88` at
//!   `0x801D1624` / `0x801D17A4` / `0x801D18F8`, each immediately followed by
//!   the target-cursor stamp `FUN_801DA6B4`). That SM is not ported: the engine
//!   drives target selection through `engine-core::target_picker`, a row/slot
//!   cursor over `SlotState` rows, so nothing holds the `ctx[+0x244..+0x249]`
//!   ring or the `action_state[i] != 4` array these index. **Porting
//!   `FUN_801D0748` is the single prerequisite for all four.**
//! - `FUN_801DB124` ([`redirect_dead_target`]) *is* called by the turn picker
//!   `FUN_801DABA4` - itself ported, as `World::next_combatant_by_initiative` -
//!   at `0x801DAF14` (party arm) and `0x801DAF50` (monster arm). The gates the
//!   port does not carry are the **command-flow byte `ctx[+0x06] == 0xFF`** on
//!   the party arm and the enemy-AI pick `FUN_801E9FD4` that precedes the
//!   monster arm. (`ctx[+0x276]`, the outer gate, *is* modelled - it is
//!   `BattleActionCtx::menu_open`.) Adding the re-roll without `ctx[+0x06]`
//!   would spend RNG draws retail does not always make, which is a simulation
//!   change rather than a wiring fix.
//! - `FUN_801DB8B4` ([`first_live_monster_slot`]) is **already ported and
//!   live** elsewhere: `engine-core`'s `BattleRound::first_living_monster`,
//!   reached from `BattleRound::boundary` (retail's `FUN_801D88CC` loop B, the
//!   `jal` at `0x801D8A44`). The copy here is the fixed-pool twin - retail
//!   scans slots `3..7` unconditionally, the live port scans from
//!   `party_count` because the engine compacts its seating. It is kept as the
//!   byte-exact form for oracle work, not as a second implementation to wire.
//! - `FUN_80019B28` ([`bearing_12bit`]) needs the `SCUS_942.54` arctan LUT at
//!   `0x8006F4C8`. No engine boot path extracts that table, so no caller can
//!   supply the `atan_lut` argument.

/// The `+0x8` actor flag-word bits `FUN_801DB9C4` keeps: it clears
/// `0x83000000` (bit 31 and bits 25/24).
pub const POOL_FLAG_WORD_KEEP: u32 = 0x7cff_ffff;

/// Per-actor `+0x8` flag-word scrub. AND-masks the `+0x8` flag word of the
/// first **7** pool slots with [`POOL_FLAG_WORD_KEEP`].
///
/// The retail loop is a fixed `while (i < 7)` over `&DAT_801C9370[i]`, so it
/// touches slots 0..=6 (not slot 7); a shorter slice is truncated to match.
///
/// The caller is `FUN_801D5854`'s **invalid-slot guard**: when the requested
/// pose is `>= 6` *and* the slot index is `>= 8`, retail forces pose `9` and
/// runs this scrub across the pool (`0x801D58C8..0x801D58E8`). An earlier
/// reading here attributed it to action-SM state `0x5A`; `FUN_801E295C`
/// contains no call to it, so that attribution is withdrawn.
///
/// PORT: FUN_801DB9C4
pub fn clear_pool_flag_words(flag_words: &mut [u32]) {
    for w in flag_words.iter_mut().take(7) {
        *w &= POOL_FLAG_WORD_KEEP;
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

/// A battle-actor's read-only view as the target-selection helpers see it.
/// One per pool slot; `pool[0..3]` are party, `pool[3..7]` are monsters (slot 7
/// exists in the array but the monster-facing scans stop at 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PoolActor {
    /// `actor[+0x14C] != 0` - the liveness halfword.
    pub alive: bool,
    /// `actor[+0x34]` world X (`i16`).
    pub x: i16,
    /// `actor[+0x38]` world Z (`i16`).
    pub z: i16,
    /// `actor[+0x1DD]` - the actor's currently chosen target slot.
    pub target_slot: u8,
    /// `actor[+0x16E]` - the status-ailment flag word. Bits `0xF84`
    /// (`can't-be-selected` ailments) gate a slot out of the selectable scans.
    pub status: u16,
}

/// The 12-bit heading returned by [`build_attack_target_queue`] and consumed by
/// [`cycle_attack_target`]. `count` = live-monster count (`ctx[+0x244]`),
/// `wrap_slot` = the acting actor's current target (`ctx[+0x245]`, the ring's
/// wrap entry), `ordered` = the three nearest alternate monster slots in
/// ascending angular distance (`ctx[+0x246..+0x249]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AttackTargetQueue {
    /// `ctx[+0x244]` - number of live monsters in pool slots 3..=6.
    pub count: u8,
    /// `ctx[+0x245]` - the acting actor's current target slot (ring wrap).
    pub wrap_slot: u8,
    /// `ctx[+0x246..+0x249]` - the three nearest alternate monster slots,
    /// ascending by angular distance from the current-target direction. A slot
    /// stays `0` when fewer than three alternates exist (retail leaves the
    /// matching context byte stale; the port zeroes it instead of reading
    /// uninitialised memory).
    pub ordered: [u8; 3],
}

/// Build the multi-target attack cursor ring for the acting party actor.
/// `FUN_801D8A88`.
///
/// This is the *builder* that feeds the ring [`cycle_attack_target`]
/// (`FUN_801D8D00`) steps through. It runs in four passes, transcribed from the
/// disassembly at `ghidra/scripts/funcs/overlay_battle_action_801d8a88.txt`:
///
/// 1. Count live monsters over pool slots 3..=6 into `count`.
/// 2. Take the acting actor's `+0x1DD` as `wrap_slot` (its current target).
/// 3. For each monster slot 3..=6, compute its bearing from the acting actor
///    (`bearing(m.z, m.x, actor.z, actor.x)`, each `+0x800 & 0xFFF`), express it
///    as a positive angular offset from the current target's bearing, wrapped
///    into `[0, 0x1000)`.
/// 4. Three times, pick the *alive* monster with the smallest remaining offset
///    that is **not** the current target, append its slot to `ordered`, and mark
///    it consumed (offset `= 30000`).
///
/// `bearing` mirrors `FUN_80019B28` (see [`bearing_12bit`]); the queue builder
/// only needs the relative ordering, so the retail arctan LUT is abstracted
/// behind the closure. `pool` must have at least 8 slots and the acting actor's
/// `target_slot` must index a valid slot (retail does no bounds check).
///
/// PORT: FUN_801D8A88
/// REF: FUN_80019B28, FUN_801D8D00
pub fn build_attack_target_queue(
    pool: &[PoolActor],
    acting_index: usize,
    bearing: impl Fn(i16, i16, i16, i16) -> u16,
) -> AttackTargetQueue {
    // Pass 1: live-monster count over slots 3..=6 (retail `while (i < 7)`).
    let count = pool[3..7].iter().filter(|a| a.alive).count() as u8;

    // Pass 2: the acting actor's current target becomes the ring wrap slot.
    let acting = pool[acting_index];
    let wrap_slot = acting.target_slot;

    // Reference bearing: from the acting actor to its current target, `+0x800`.
    let target = pool[acting.target_slot as usize];
    let ref_ang = bearing(target.z, target.x, acting.z, acting.x).wrapping_add(0x800) & 0xfff;

    // Pass 3: per-monster angular offset from the reference, in `[0, 0x1000)`.
    let mut rel = [0i16; 4];
    for (i, r) in rel.iter_mut().enumerate() {
        let m = pool[3 + i];
        let b = bearing(m.z, m.x, acting.z, acting.x).wrapping_add(0x800) & 0xfff;
        let mut v = b as i16;
        // `if ((int)uVar4 < (int)(short)local_20) v += 0x1000;` - ref_ang and b
        // are both in [0, 0xFFF] so the sign-extend is a no-op.
        if (b as i32) < (ref_ang as i16 as i32) {
            v = v.wrapping_add(0x1000);
        }
        *r = v.wrapping_sub(ref_ang as i16);
    }

    // Pass 4: three selection sweeps, each taking the nearest alive non-target
    // monster and consuming it (`local_28[slot-3] = 30000`).
    let mut ordered = [0u8; 3];
    for out in ordered.iter_mut() {
        let mut min = 30000i16;
        let mut found: Option<u8> = None;
        for i in 0..4 {
            if rel[i] < min && pool[3 + i].alive && acting.target_slot as usize != i + 3 {
                found = Some((i + 3) as u8);
                min = rel[i];
            }
        }
        if let Some(slot) = found {
            *out = slot;
            rel[slot as usize - 3] = 30000;
        }
    }

    AttackTargetQueue {
        count,
        wrap_slot,
        ordered,
    }
}

/// The first live monster slot (3..=6), or `7` when none is alive.
/// `FUN_801DB8B4`.
///
/// Scans pool slots 3,4,5,6 in order and returns the first whose `+0x14C`
/// liveness halfword is non-zero; the retail loop's fall-through value is `7`.
///
/// This is the **fixed-slot** twin of the anchor's canonical port,
/// `engine-core`'s `BattleRound::first_living_monster`, which is on the live
/// round-boundary path and scans from `party_count` because the engine
/// compacts its battle seating. Kept here in retail's literal slot-`3..7`
/// form for oracle work; it carries a `REF` rather than a second `PORT` tag
/// so the anchor stays attributed to the live implementation.
///
/// REF: FUN_801DB8B4
pub fn first_live_monster_slot(pool: &[PoolActor]) -> u8 {
    let mut slot = 3u8;
    while slot < 7 {
        if pool[slot as usize].alive {
            return slot;
        }
        slot += 1;
    }
    slot
}

/// The lowest pool slot that is a valid action *target* / turn participant.
/// `FUN_801DBA04`.
///
/// Scans slots `0..actor_count` and returns the first `i` where all three hold:
/// the per-slot action-state byte `action_state[i] != 4` (`4` = removed / done),
/// the actor is alive (`+0x14C`), and it carries no can't-select ailment
/// (`+0x16E & 0xF84 == 0`). Returns `actor_count` when none qualifies, and `0`
/// when `actor_count == 0` (retail's `uVar1` seed).
///
/// PORT: FUN_801DBA04
pub fn first_selectable_target(pool: &[PoolActor], action_state: &[u8], actor_count: u8) -> u8 {
    if actor_count == 0 {
        return 0;
    }
    let mut i = 0u8;
    while (i as usize) < actor_count as usize {
        if action_state[i as usize] != 4
            && pool[i as usize].alive
            && (pool[i as usize].status & 0xf84) == 0
        {
            return i;
        }
        i += 1;
    }
    i
}

/// The next selectable turn participant after `current_index`. `FUN_801DB81C`.
///
/// Starts at `current_index + 1` and applies the same three predicates as
/// [`first_selectable_target`]. When `current_index + 1` is already `>=
/// actor_count` it returns `current_index + 1` without scanning; otherwise it
/// returns the first qualifying slot, or `actor_count` when none qualifies.
///
/// PORT: FUN_801DB81C
pub fn next_selectable_actor(
    pool: &[PoolActor],
    action_state: &[u8],
    actor_count: u8,
    current_index: u8,
) -> u8 {
    let mut i = current_index.wrapping_add(1);
    while (i as usize) < actor_count as usize {
        if action_state[i as usize] != 4
            && pool[i as usize].alive
            && (pool[i as usize].status & 0xf84) == 0
        {
            return i;
        }
        i += 1;
    }
    i
}

/// Convert two world points to a 12-bit clockwise heading (`0x000..0xFFF`, with
/// `0x000` toward `-Z` and `0x400` toward `+X`). `FUN_80019B28`.
///
/// This is the faithful port of the retail atan2: it takes the displacement
/// `(dz, dx) = (p2z - p1z, p2x - p1x)`, folds it into one of four quadrants by
/// the signs, divides the shorter leg into the longer (`(min << 11) / max`) and
/// indexes the retail arctan LUT at `0x8006F4C8` (`atan_lut[idx]`, `i16`
/// entries, `0x801` long, `atan_lut[0] == 0`). The per-quadrant / per-octant
/// constant (`0x000/0x400/0x800/0xC00` base, added or subtracted) reassembles
/// the full circle. Every result is masked to `0xFFF`.
///
/// The LUT is Sony data supplied by the caller (extracted from `SCUS_942.54` at
/// `0x8006F4C8`); no table bytes are embedded here. The motion VM keeps a
/// separate `f32` approximation of this function for its face-target ramp; this
/// is the byte-faithful form.
///
/// PORT: FUN_80019B28
pub fn bearing_12bit(atan_lut: &[i16], p1z: i16, p1x: i16, p2z: i16, p2x: i16) -> u16 {
    let mut dz = p2z as i32 - p1z as i32;
    let mut dx = p2x as i32 - p1x as i32;
    let mut quad = 0u32;
    if dz < 0 {
        dz = -dz;
        quad |= 2;
    }
    if dx < 0 {
        dx = -dx;
        quad |= 1;
    }
    let lut = |numer: i32, denom: i32| -> i32 { atan_lut[((numer << 11) / denom) as usize] as i32 };
    let raw: i32 = match quad {
        0 => {
            if dz >= dx {
                if dx == 0 { 0 } else { lut(dx, dz) }
            } else {
                // dx != 0 here (dz < dx implies dx > 0).
                0x400 - lut(dz, dx)
            }
        }
        1 => {
            if dz >= dx {
                if dx == 0 {
                    0x1000
                } else {
                    0x1000 - lut(dx, dz)
                }
            } else if dz == 0 {
                0xc00
            } else {
                lut(dz, dx) + 0xc00
            }
        }
        2 => {
            if dz >= dx {
                if dx == 0 { 0x800 } else { 0x800 - lut(dx, dz) }
            } else if dz == 0 {
                0x400
            } else {
                lut(dz, dx) + 0x400
            }
        }
        _ => {
            // quad == 3
            if dz >= dx {
                if dx == 0 { 0x800 } else { lut(dx, dz) + 0x800 }
            } else if dz == 0 {
                0xc00
            } else {
                0xc00 - lut(dz, dx)
            }
        }
    };
    (raw & 0xfff) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_scrub_masks_first_seven_slots() {
        let mut words = [0xFFFF_FFFFu32; 8];
        clear_pool_flag_words(&mut words);
        for w in words.iter().take(7) {
            assert_eq!(*w, 0x7cff_ffff);
        }
        // Slot 7 is untouched by the `< 7` loop.
        assert_eq!(words[7], 0xFFFF_FFFF);
    }

    #[test]
    fn flag_scrub_clears_exactly_bits_31_25_24() {
        let mut words = [0x8300_0001u32];
        clear_pool_flag_words(&mut words);
        assert_eq!(words[0], 0x0000_0001);
    }

    #[test]
    fn flag_scrub_truncates_short_slice() {
        let mut words = [0xFFFF_FFFFu32; 3];
        clear_pool_flag_words(&mut words);
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

    /// Build an 8-slot pool: party 0..2 alive, monsters at the given z's.
    fn pool_with_monster_z(zs: [i16; 4], alive: [bool; 4], target: u8) -> Vec<PoolActor> {
        let mut pool = vec![PoolActor::default(); 8];
        for p in pool.iter_mut().take(3) {
            p.alive = true;
        }
        pool[0].target_slot = target;
        for (i, (&z, &a)) in zs.iter().zip(alive.iter()).enumerate() {
            pool[3 + i] = PoolActor {
                alive: a,
                x: 0,
                z,
                target_slot: 0,
                status: 0,
            };
        }
        pool
    }

    // Deterministic stand-in for the bearing: returns `target.z & 0xFFF`, which
    // lets the queue test predict the relative ordering by hand.
    fn z_bearing(tz: i16, _tx: i16, _az: i16, _ax: i16) -> u16 {
        (tz as u16) & 0xfff
    }

    #[test]
    fn attack_queue_orders_alternates_by_angular_distance() {
        // Monsters slots 3,4,5,6 with z = 0x100,0x050,0x200,0x300; actor targets
        // slot 3. ref_ang = (0x100 + 0x800) & 0xFFF = 0x900.
        //   slot3 rel=0 (excluded: it is the current target/wrap)
        //   slot4 rel=0xF50, slot5 rel=0x100, slot6 rel=0x200
        // Nearest-first over {4,5,6}: 5 (0x100), 6 (0x200), 4 (0xF50).
        let pool = pool_with_monster_z([0x100, 0x050, 0x200, 0x300], [true; 4], 3);
        let q = build_attack_target_queue(&pool, 0, z_bearing);
        assert_eq!(q.count, 4);
        assert_eq!(q.wrap_slot, 3);
        assert_eq!(q.ordered, [5, 6, 4]);
    }

    #[test]
    fn attack_queue_composes_with_cycle() {
        // The ring built above, fed straight into cycle_attack_target.
        let pool = pool_with_monster_z([0x100, 0x050, 0x200, 0x300], [true; 4], 3);
        let q = build_attack_target_queue(&pool, 0, z_bearing);
        let ring = [
            q.count,
            q.wrap_slot,
            q.ordered[0],
            q.ordered[1],
            q.ordered[2],
        ];
        // From the current target 3: Next -> 5, and Prev wraps to 4.
        assert_eq!(cycle_attack_target(&ring, 3, TargetCycle::Next), 5);
        assert_eq!(cycle_attack_target(&ring, 3, TargetCycle::Prev), 4);
    }

    #[test]
    fn attack_queue_skips_dead_monsters() {
        // Only the current target (slot 3) is alive; no alternates exist.
        let pool = pool_with_monster_z([0x100, 0, 0, 0], [true, false, false, false], 3);
        let q = build_attack_target_queue(&pool, 0, z_bearing);
        assert_eq!(q.count, 1);
        assert_eq!(q.wrap_slot, 3);
        assert_eq!(q.ordered, [0, 0, 0]);
    }

    #[test]
    fn first_live_monster_scans_slots_3_to_6() {
        let mut pool = vec![PoolActor::default(); 8];
        pool[5].alive = true;
        assert_eq!(first_live_monster_slot(&pool), 5);
        // None alive -> fall-through value 7.
        let empty = vec![PoolActor::default(); 8];
        assert_eq!(first_live_monster_slot(&empty), 7);
        // Slot 7 alive is out of the 3..=6 scan, so still 7.
        let mut only7 = vec![PoolActor::default(); 8];
        only7[7].alive = true;
        assert_eq!(first_live_monster_slot(&only7), 7);
    }

    #[test]
    fn selectable_scans_apply_all_three_predicates() {
        let mut pool = vec![PoolActor::default(); 8];
        for p in pool.iter_mut() {
            p.alive = true;
        }
        // Slot 1 afflicted (0x004 in the 0xF84 mask); slot 2 marked done in the
        // action-state array; slot 3 is the first fully-selectable one.
        pool[1].status = 0x004;
        let action_state = [0u8, 0, 4, 0, 0, 0, 0, 0];
        assert_eq!(first_selectable_target(&pool, &action_state, 6), 0);
        pool[0].alive = false; // slot 0 out -> skip 1 (ailment), 2 (done) -> 3.
        assert_eq!(first_selectable_target(&pool, &action_state, 6), 3);
        // count == 0 returns the 0 seed.
        assert_eq!(first_selectable_target(&pool, &action_state, 0), 0);
    }

    #[test]
    fn next_selectable_starts_after_current() {
        let mut pool = vec![PoolActor::default(); 8];
        for p in pool.iter_mut() {
            p.alive = true;
        }
        let action_state = [0u8; 8];
        // From current index 2, the next is 3.
        assert_eq!(next_selectable_actor(&pool, &action_state, 6, 2), 3);
        // current+1 already >= count -> returns current+1 unscanned.
        assert_eq!(next_selectable_actor(&pool, &action_state, 4, 5), 6);
        // No qualifying slot after current -> returns count.
        pool[3].alive = false;
        pool[4].status = 0x080;
        let action_state2 = [0u8, 0, 0, 0, 0, 4, 0, 0];
        assert_eq!(next_selectable_actor(&pool, &action_state2, 6, 2), 6);
    }

    #[test]
    fn bearing_axis_cases_are_exact() {
        // atan_lut[0] must be 0; the axis cases never touch the LUT interior.
        let lut = vec![0i16; 0x801];
        // dz = p2z - p1z, dx = p2x - p1x.
        // +Z (dz>0, dx=0) -> 0.
        assert_eq!(bearing_12bit(&lut, 0, 0, 100, 0), 0);
        // +X (dz=0, dx>0) -> 0x400.
        assert_eq!(bearing_12bit(&lut, 0, 0, 0, 100), 0x400);
        // -Z (dz<0, dx=0) -> 0x800.
        assert_eq!(bearing_12bit(&lut, 0, 0, -100, 0), 0x800);
        // -X (dz=0, dx<0) -> 0xC00.
        assert_eq!(bearing_12bit(&lut, 0, 0, 0, -100), 0xc00);
    }

    #[test]
    fn bearing_diagonals_hit_lut_end() {
        // Synthetic LUT: linear ramp, lut[i] = i/4, so lut[0x800] = 0x200 (45deg).
        let lut: Vec<i16> = (0..=0x800).map(|i| (i / 4) as i16).collect();
        // dz == dx > 0, quad 0, dz>=dx path: idx = (dx<<11)/dz = 0x800 -> 0x200.
        assert_eq!(bearing_12bit(&lut, 0, 0, 100, 100), 0x200);
        // quad 0, dz < dx: 0x400 - lut[(dz<<11)/dx]. dz=50,dx=100 -> idx=0x400,
        // lut=0x100 -> 0x300.
        assert_eq!(bearing_12bit(&lut, 0, 0, 50, 100), 0x300);
        // quad 1 (dx<0), dz==dx magnitude: 0x1000 - lut[0x800] = 0xE00.
        assert_eq!(bearing_12bit(&lut, 0, 0, 100, -100), 0xe00);
        // quad 3 (both negative), dz>=dx, dx!=0: lut[idx] + 0x800.
        // dz=100,dx=50 -> idx=(50<<11)/100=0x400, lut=0x100 -> 0x900.
        assert_eq!(bearing_12bit(&lut, 0, 0, -100, -50), 0x900);
    }
}
