//! Leaf helpers over the 8-slot battle-actor pool (`&DAT_801C9370`) and the
//! action-context target queue. Each is a self-contained function some part of
//! the battle overlay calls - the command SM `FUN_801D0748`, the flow SM
//! `FUN_801D388C`, the turn picker `FUN_801DABA4`, the round reset
//! `FUN_801D88CC`, the pose setter `FUN_801D5854`. None is called by the action
//! SM `FUN_801E295C` itself, and none is a state of it, so they port cleanly as
//! pure functions.
//!
//! REF: FUN_801DB8B4 (first-live-monster slot - the canonical port is
//! `engine-core`'s `BattleRound::first_living_monster`, which is live; the
//! fixed-slot twin below exists for oracle work)
//!
//! Every address is tagged on the function that implements it, not here: a
//! module-level `PORT:` line makes one file-wide anchor per address, and this
//! file now has both wired and inert routines, which a file-wide anchor cannot
//! express.
//!
//! All arithmetic is transcribed from the DISASSEMBLY in
//! `ghidra/scripts/funcs/overlay_battle_action_801db9c4.txt`,
//! `..._801db318.txt`, `..._801d8a88.txt`, `..._801d8d00.txt`,
//! `..._801db124.txt`, `..._801db8b4.txt`, `..._801dba04.txt`,
//! `..._801db81c.txt`, and `ghidra/scripts/funcs/80019b28.txt` - not the C.
//! The pool is 8 slots (0..2 party, 3..7 monsters); slots 0..2 are treated as
//! always-present, slots 3.. are gated on the liveness halfword `actor[+0x14C]`.
//!
//! ## What is on the frame path
//!
//! [`build_attack_target_queue`] and [`cycle_attack_target`] are the retail
//! attack-target ring, and `engine-core`'s `TargetPickerSession` steps its
//! enemy cursor through them - so a Left/Right press moves to the angularly
//! nearest live monster, which is what retail does, instead of to the next
//! slot index. That path also runs [`bearing_12bit`], over
//! [`approx_arctan_lut`] rather than the SCUS table.
//!
//! The other routines are on host paths too: each carries its own `PORT:`
//! tag, and `port-catalog.py --live-audit` reports every one of them live.

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
/// The bits are the near-camera ghost pass's
/// ([`camera_ghost_pass`](crate::battle_action::camera_ghost_pass),
/// `FUN_8004DC68`), carried on
/// [`BattleActor::flag_word`](crate::battle_action::BattleActor::flag_word);
/// the engine's pose hook runs the guard. Whether retail ever reaches the
/// guard is not settled: every `jal 0x801D5854` in the flow and action SMs
/// passes a slot from `ctx[+0x13]`, `actor[+0x1DD]`, a stack local or `0`, and
/// the one source that can hold an all-target code `8` / `9` (`+0x1DD`, at
/// `0x801D43F0`) passes pose `3`, below the `>= 6` test.
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
/// Caller: cases `0` **and** `2` of the battle **flow** SM `FUN_801D388C`
/// (both jump-table slots at `0x801CE880` land on the `jal` at `0x801D3908`,
/// which then re-arms the far framing `FUN_801D5854(0, 9)`). Case `0` is the
/// round start (`0x801D0EE4`, right after the round reset `FUN_801D88CC` and
/// the initiative seeder `FUN_801DA780`), case `2` the ring cancelled back to
/// the round prompt (`0x801D11E0`), so retail re-normalises the formation
/// every round, not once. `engine-core`'s `World::normalize_battle_formation`
/// runs it at both points (`begin_battle_round` and the ring cancel's prompt
/// return) over the live battle positions, mapping the engine's compacted
/// monster slots onto retail's fixed pool slots `3..7`.
///
/// `0x801D388C` has one copy, in PROT 0898: it lies past the end of the arena
/// overlay (PROT 0977 is `0x3800` bytes, ending `0x801D2018`), and
/// `overlay_muscle_dome_801d388c.txt` is the same routine byte-for-byte - the
/// prefix names the **capture**, not the image
/// (`docs/tooling/dump-corpus-integrity.md`).
///
/// The camera-focus pair `focus_x` / `focus_z` is `_DAT_80089118` /
/// `_DAT_80089120`, the **negated** camera target, which is why the recentre
/// adds the centroid to it while subtracting it from the actors: the view
/// holds still across the shift. The case-9 re-frame that follows rebuilds the
/// focus from the live actor table, so the engine passes locals.
///
/// PORT: FUN_801DB318
pub fn normalize_formation_span(
    pos: &mut [FormationPos],
    alive: &[bool],
    focus_x: &mut i32,
    focus_z: &mut i32,
) {
    squash_formation_span(pos, alive, focus_x, focus_z);
    // Pass 3: recompute extents (positions moved), then recentre on the
    // centroid `((max + min) as u32) >> 1` truncated to i16.
    recentre_formation(pos, alive, |_| true, focus_x, focus_z);
}

/// The action seed's formation recentre, `FUN_801EFE44` (state `0x0C`,
/// `jal 0x801efe44` at `0x801E2D44` for every category but Run, and at
/// `0x801E2DE8` on the monster Run arm).
///
/// Passes 1 and 2 are [`normalize_formation_span`]'s: extents over pool slots
/// `0..3` and the `3..7` slots with live HP, then an axis whose span exceeds
/// `0x800` rescaled to `0x800` (focus pair likewise). The recentre box
/// differs: when the acting slot's category is Attack (`+0x1DE == 3`), it
/// takes only the **target** `target`, or the target's whole side when the
/// target is the all-party code `8` (slots `< 3`) or the all-monster code
/// `9` (slots `>= 3`); any other category takes every included slot. The
/// box's centroid `((max + min) as u32) >> 1` is subtracted from every
/// included slot and added onto the focus pair. A melee therefore opens with
/// its target on the stage origin and the rest of the formation translated
/// with it.
///
/// `pos` / `alive` are the retail pool (index = pool slot); `target` is the
/// acting slot's `+0x1DD`, already a pool slot or `8` / `9`.
///
/// PORT: FUN_801EFE44
pub fn seed_formation_recentre(
    pos: &mut [FormationPos],
    alive: &[bool],
    attack: bool,
    target: u8,
    focus_x: &mut i32,
    focus_z: &mut i32,
) {
    squash_formation_span(pos, alive, focus_x, focus_z);
    let in_box = |i: usize| {
        if attack && i as u8 != target {
            match target {
                8 => i < 3,
                9 => i >= 3,
                _ => false,
            }
        } else {
            true
        }
    };
    recentre_formation(pos, alive, in_box, focus_x, focus_z);
}

/// Passes 1 and 2 of [`normalize_formation_span`] alone: the per-axis
/// squash of a span over `0x800`, with no recentre.
fn squash_formation_span(
    pos: &mut [FormationPos],
    alive: &[bool],
    focus_x: &mut i32,
    focus_z: &mut i32,
) {
    let included = |i: usize| i < 3 || alive.get(i).copied().unwrap_or(false);
    let slots = pos.len().min(7);
    let (mut max_x, mut min_x) = (-30000i16, 30000i16);
    let (mut max_z, mut min_z) = (-30000i16, 30000i16);
    for (i, p) in pos.iter().enumerate().take(slots) {
        if !included(i) {
            continue;
        }
        max_x = max_x.max(p.x);
        min_x = min_x.min(p.x);
        max_z = max_z.max(p.z);
        min_z = min_z.min(p.z);
    }
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
}

/// The recentre both formation kernels end on: the centroid `((max + min)
/// as u32) >> 1` of the included slots `in_box` admits, subtracted from
/// every included slot and added onto the focus pair. An empty box
/// (extents still at their `-30000` / `30000` seeds) recentres by zero.
fn recentre_formation(
    pos: &mut [FormationPos],
    alive: &[bool],
    in_box: impl Fn(usize) -> bool,
    focus_x: &mut i32,
    focus_z: &mut i32,
) {
    let included = |i: usize| i < 3 || alive.get(i).copied().unwrap_or(false);
    let slots = pos.len().min(7);
    let (mut max_x, mut min_x) = (-30000i16, 30000i16);
    let (mut max_z, mut min_z) = (-30000i16, 30000i16);
    for (i, p) in pos.iter().enumerate().take(slots) {
        if !included(i) || !in_box(i) {
            continue;
        }
        max_x = max_x.max(p.x);
        min_x = min_x.min(p.x);
        max_z = max_z.max(p.z);
        min_z = min_z.min(p.z);
    }
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
/// - Magic (`2`): when the spell record's cast-class byte `+0`
///   (`0x800754C8[param0 * 12]`, passed in as `spell_tier`) is `>= 0xA`, or
///   when the current target is an enemy slot (`target_slot >= 3`). Every
///   real cast class (`0x14` plain, `0x32` summon, `0x63` capture) clears
///   `0xA`; only the low-class records (the internal `0x00..=0x24` tiers and
///   the monster attacks `0x2E` / `0x2F`, class `0`) aimed at a party slot
///   are left alone.
/// - Item (`1`): only for item ids `0xFE` or `0x98`.
///
/// The roll picks a **living** slot on the current target's side: party
/// (`target_slot < 3`) rolls `rng() % party_count`; enemy rolls
/// `rng() % monster_count + 3`. It retries until it lands on a living slot, so
/// `is_alive` must eventually return true for some slot on that side (retail
/// loops forever otherwise).
///
/// Retail has **three** call sites, and the port drives the one whose gates
/// it can honour. The AI queue assembler `FUN_801F0450` pushes its freshly
/// rolled attack target straight through this at `0x801F0574`, with no
/// command-flow gate in front of it at all; that site is
/// `battle_action::dispatch`'s `auto_fill_party_queues`, so this runs
/// wherever the auto-fill arm does.
///
/// The other two are the turn picker `FUN_801DABA4`'s party arm
/// (`0x801DAF14`) and monster arm (`0x801DAF50`). The party arm is gated on
/// the **command-flow byte `ctx[+0x06] == 0xFF`** (`lbu v1,0x6(a0)` /
/// `bne v1,v0` at `0x801DAF04..0x801DAF0C`) - the round's execution band,
/// the only band `engine-core` dispatches party turns from - so the engine
/// drives it at each party dispatch (`World::redirect_dead_battle_target`,
/// over its compacted seating). The monster arm follows the enemy-AI pick
/// `FUN_801E9FD4` with no gate at all, so it covers a monster's strike
/// (category `3`) and its single-target cast (category `2`) alike - the
/// engine drives it from `World::take_monster_turn` for both.
///
/// PORT: FUN_801DB124
/// REF: FUN_801F0450 (the call site this is driven from),
/// REF: FUN_801DABA4 (the turn picker's two call sites)
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

/// Roster character id that marks the **AI-controlled companion** seat.
///
/// `DAT_8007BD10` is the 3-byte per-slot roster character id (`01 02 03` =
/// Vahn / Noa / Gala; the array is indexed past its own length by every retail
/// scan that walks monster slots, which is retail's own over-read into the
/// adjacent globals). Id `4` is the AI-controlled companion seat - see
/// `docs/subsystems/battle-action.md`, where the same `== 4` test appears in
/// the RunBegin arm and in `FUN_801EED1C`'s auto-fight block.
///
/// It is **not** an "action state" and `4` is not "removed / done"; an earlier
/// reading of the two scans below said so and was wrong.
pub const AI_COMPANION_CHAR_ID: u8 = 4;

/// The lowest pool slot the **player** may still command. `FUN_801DBA04`.
///
/// Scans slots `0..actor_count` and returns the first `i` where all three hold:
/// the slot's roster character id is not [`AI_COMPANION_CHAR_ID`]
/// (`(&DAT_8007BD10)[i] != 4`, `0x801DBA44`), the actor is alive (`+0x14C`,
/// `0x801DBA54`), and it carries no can't-select ailment
/// (`+0x16E & 0xF84 == 0`, `0x801DBA64`). Returns `actor_count` when none
/// qualifies, and `0` when `actor_count == 0` (retail's `uVar1` seed).
///
/// Wired: `engine-core`'s `World::next_member_owing_command` opens the round's
/// command ring with this scan and steps it with [`next_selectable_actor`],
/// on both playable hosts (the ring is shared world state).
///
/// The model question that kept it inert is settled, and the answer was a
/// **bridge, not a choice**. The port holds an actor's ailments twice - the
/// raw `+0x16E` word [`crate::battle_action::BattleActor::field_flags`], which
/// the cast band writes, and a typed `StatusEffect` list, which the turn loop
/// reads - so adopting one and retiring the other would have lost whatever the
/// other carried. `World::raw_status_word` instead composes them: the typed
/// list packs to its retail bits through
/// [`crate::status_effects::pack_display_flags`] (bit map pinned in
/// `crate::status_effects::display_flags`) and the raw word ORs in unchanged,
/// which is bit-exact to the single word retail holds and keeps the `0x380`
/// delegation group the typed list has no kind for.
///
/// The round-flow half stays outside the kernel, where retail keeps it: a
/// member that has already committed this round is skipped by re-entering the
/// scan, because retail walks its cursor on after each commit rather than
/// testing a committed flag here.
///
/// PORT: FUN_801DBA04
pub fn first_selectable_target(pool: &[PoolActor], roster_char_ids: &[u8], actor_count: u8) -> u8 {
    if actor_count == 0 {
        return 0;
    }
    let mut i = 0u8;
    while (i as usize) < actor_count as usize {
        if roster_char_ids[i as usize] != AI_COMPANION_CHAR_ID
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
/// Wired through the same caller as [`first_selectable_target`]: it opens the
/// ring, this steps it, and both read the composed `+0x16E` word
/// `World::raw_status_word` builds.
///
/// Do not reach for this as the engine's turn-advance kernel. The engine's
/// `World::next_living_combatant` is a **wrapping round-robin over every**
/// actor, party and monster alike, and drives turn order; this is a
/// **non-wrapping forward scan over player-commandable slots only** and drives
/// a cursor. Substituting one for the other stops monsters taking turns.
///
/// PORT: FUN_801DB81C, overlay_battle_action_0898_801db81c
pub fn next_selectable_actor(
    pool: &[PoolActor],
    roster_char_ids: &[u8],
    actor_count: u8,
    current_index: u8,
) -> u8 {
    let mut i = current_index.wrapping_add(1);
    while (i as usize) < actor_count as usize {
        if roster_char_ids[i as usize] != AI_COMPANION_CHAR_ID
            && pool[i as usize].alive
            && (pool[i as usize].status & 0xf84) == 0
        {
            return i;
        }
        i += 1;
    }
    i
}

/// Entries in the retail arctan LUT at `0x8006F4C8` - the index space
/// `bearing_12bit` divides into is `(min << 11) / max`, so `0..=0x800`.
pub const ARCTAN_LUT_LEN: usize = 0x801;

/// A port-side stand-in for the retail arctan LUT at `0x8006F4C8`.
///
/// The retail table is Sony data and no engine boot path extracts it, so this
/// is the same function computed from first principles: entry `i` is
/// `atan(i / 2048)` expressed in 12-bit turn units (`0x1000` = one turn),
/// which fixes `lut[0] == 0` and `lut[0x800] == 0x200` (45 degrees) - the two
/// values [`bearing_12bit`]'s quadrant algebra depends on.
///
/// Substituting the table leaves the *arithmetic* of `FUN_80019B28`
/// untouched: quadrant folding, the `(min << 11) / max` divide and the
/// per-octant constants are the ported ones. Only the sampled arctan differs,
/// by at most a unit of the 12-bit angle, which is below the resolution any
/// consumer of a heading here cares about.
pub fn approx_arctan_lut() -> &'static [i16; ARCTAN_LUT_LEN] {
    use std::sync::OnceLock;
    static LUT: OnceLock<[i16; ARCTAN_LUT_LEN]> = OnceLock::new();
    LUT.get_or_init(|| {
        let mut t = [0i16; ARCTAN_LUT_LEN];
        for (i, slot) in t.iter_mut().enumerate() {
            let ratio = i as f64 / 2048.0;
            *slot = (ratio.atan() * 4096.0 / std::f64::consts::TAU).round() as i16;
        }
        t
    })
}

/// [`bearing_12bit`] over [`approx_arctan_lut`] - the form a caller with no
/// disc-extracted arctan table uses.
pub fn bearing_12bit_approx(p1z: i16, p1x: i16, p2z: i16, p2x: i16) -> u16 {
    bearing_12bit(approx_arctan_lut(), p1z, p1x, p2z, p2x)
}

/// Convert two world points to a 12-bit heading (`0x000..0xFFF`), with
/// `0x000` toward `+Z`, `0x400` toward `+X`, `0x800` toward `-Z` and `0xC00`
/// toward `-X`. `FUN_80019B28`.
///
/// This is the faithful port of the retail atan2: it takes the displacement
/// `(dz, dx) = (p2z - p1z, p2x - p1x)`, folds it into one of four quadrants by
/// the signs, divides the shorter leg into the longer (`(min << 11) / max`) and
/// indexes the retail arctan LUT at `0x8006F4C8` (`atan_lut[idx]`, `i16`
/// entries, `0x801` long, `atan_lut[0] == 0`). The per-quadrant / per-octant
/// constant (`0x000/0x400/0x800/0xC00` base, added or subtracted) reassembles
/// the full circle. Every result is masked to `0xFFF`.
///
/// The LUT is data supplied by the caller. No Sony table bytes are embedded
/// here: a host with the disc-extracted `0x8006F4C8` table passes it, and a
/// host without one passes [`approx_arctan_lut`] (which is what
/// [`bearing_12bit_approx`], and through it the engine's enemy target cursor,
/// does). The motion VM keeps a separate `f32` approximation of the whole
/// function for its face-target ramp; this is the faithful integer form.
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
mod tests;
