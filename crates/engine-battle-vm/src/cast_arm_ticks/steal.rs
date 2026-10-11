//! PROT 0941 Steal: the bag-slot draw, the single-victim tick and the sweep tick.

use super::*;

// ---------------------------------------------------------------------------
// PROT 0941 - cast_steal
// ---------------------------------------------------------------------------

/// One bag slot as PROT 0941's Steal reads it: the two bytes at
/// `0x80085958 + slot * 2`, `[item id, count]`.
pub type StealBagSlot = (u8, u8);

/// What PROT 0941's `0x51` body decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StealOutcome {
    /// The victim is a party seat: one of this item came out of the bag
    /// (`jal 0x80042310` with `(id, 1)`, the inventory consume-by-id helper).
    FromBag { item: u8 },
    /// The victim is a party seat but `0x400` draws found no bag slot whose
    /// id, count and item-record shop price are all non-zero, so nothing is
    /// taken.
    BagEmpty,
    /// The victim is a monster seat: the static steal table
    /// `0x80077828 + monster_id * 2` decided, fields `[chance, item]`.
    FromMonster {
        /// The table's `+0` chance byte, compared against `rand() % 100`.
        chance: u8,
        /// The table's `+1` item id, handed over only on a hit.
        item: u8,
        /// `rand() % 100`.
        roll: u8,
        /// `roll < chance`.
        hit: bool,
    },
}

/// The bag draw PROT 0941's Steal makes, spelled out because it is a
/// **rejection sampler** and not a scan.
///
/// ```text
/// tries = 0
/// do {
///     slot = rand() % 0x100
///     while party_seat[1] == 4 && slot < *(0x8007B5EA) { slot = rand() % 0x100 }
/// } while (bag[slot].id == 0 || bag[slot].count == 0 || !item_valid(bag[slot].id))
///          && ++tries < 0x400
/// ```
///
/// The index is `(slot << 16) >> 15`, i.e. `slot * 2` - a byte offset into the
/// 256-slot, two-bytes-per-slot bag at `0x80085958`
/// (`docs/subsystems/inventory.md`). The inner `0x8007B5EA` floor arms on
/// `lbu v1, 1(s5)` at `0x801F77E8` with `s5 = 0x8007BD10` (`0x801F77B0`), i.e.
/// on `DAT_8007BD10[1] == 4` - the per-seat **character id** table, so the
/// test is "battle seat 1 holds roster character 4", not a battle-context
/// field. PROT 0941 makes no access at `+0x11` at all. That is the split-bag
/// condition (`docs/subsystems/inventory.md`): the floor halfword `0x8007B5EA`
/// is `gp[+0x2D2]` (`gp = 0x8007B318`) - the active window's **start** - so
/// the arm confines the draw to the window's own half.
///
/// Returns the chosen slot, or `None` when the `0x400` budget ran out.
///
/// [`steal_bag_slot_from_draw`] is the one-draw half, for a caller whose RNG
/// cannot be handed out as a closure: the draw count is observable (retail
/// advances the shared `FUN_80056798` cursor once per rejected slot), so a
/// host must not pre-draw a batch it might not consume.
pub fn steal_pick_bag_slot(
    bag: &[StealBagSlot],
    min_slot: Option<u8>,
    mut rand: impl FnMut() -> u32,
    item_valid: impl Fn(u8) -> bool,
) -> Option<u8> {
    for _ in 0..STEAL_DRAW_BUDGET {
        let mut slot = (rand() % 0x100) as u8;
        if let Some(floor) = min_slot {
            let mut guard = 0u32;
            while slot < floor && guard < STEAL_DRAW_BUDGET {
                slot = (rand() % 0x100) as u8;
                guard += 1;
            }
        }
        if let Some(hit) = steal_bag_slot_from_draw(bag, slot, &item_valid) {
            return Some(hit);
        }
    }
    None
}

/// The `0x400` rejection budget the loop counts down
/// (`addiu v0, v0, 1; sltiu v0, v0, 0x400`).
pub const STEAL_DRAW_BUDGET: u32 = 0x400;

/// One draw of [`steal_pick_bag_slot`]'s loop: is the bag slot this `rand()`
/// landed on acceptable?
///
/// The test is `bag[slot].id != 0 && bag[slot].count != 0 && item_valid(id)`,
/// where retail's third leg is the item record's **shop price** - the halfword
/// at `0x80074368 + id*0xC + 2`, read at `0x801F789C` - so an item with no
/// price (a quest or found-only one) is unstealable. Split out so a host whose
/// RNG cannot be borrowed into a closure still spends exactly the draws retail
/// spends.
pub fn steal_bag_slot_from_draw(
    bag: &[StealBagSlot],
    slot: u8,
    item_valid: impl Fn(u8) -> bool,
) -> Option<u8> {
    let &(id, count) = bag.get(slot as usize)?;
    (id != 0 && count != 0 && item_valid(id)).then_some(slot)
}

/// PROT 0941 (Steal) tick body for action `0x51` - the **enemy steal**.
///
/// A `beq`/`slti` chain over `{0, 1, 2, 3, 0xFF}` (`0x801F73AC`..`0x801F73F8`).
///
/// * arm `0` - `ctx[+0x0D] = 1`, halve the framing halfword `ctx[+0x6D0]`,
///   write the per-seat dodge angles `ctx[+0x6E6 + i*2]` for the seven seats
///   that are neither caster nor victim, stage the run clip
///   `FUN_80050E2C(record+0x4C, 1, record[+0x4A])` with the **OR** restage
///   `+0x1DC |= 1` (`0x801F7598`). Advances;
/// * arm `1` - walk the caster to the victim (`FUN_8004E2F0` arrival test),
///   then resolve the steal (below), write the message id `ctx[+0x18] = 0x5B`
///   and double `ctx[+0x6D0]` back. Advances;
/// * arm `2` - `ctx[+0x0D] = 0`, face away, `+0x1DA = 0` with `+0x1DC |= 1`
///   (`0x801F7BCC`/`0x801F7BD4`), `ctx[+0x0D] = 1`. Advances;
/// * arm `3` - latches `ctx[+0x279] = 0xFF` (`0x801F7CFC`) and stays Busy;
/// * arm `0xFF` - clears the busy register.
///
/// The resolution splits on the victim's seat:
///
/// * a **party** seat (`< 3`) - draw a bag slot ([`steal_pick_bag_slot`]),
///   stash the id at `0x801C8FE0 + (caster_seat - 3) * 4`, build the "stole
///   X" line and call the inventory consume helper `FUN_80042310(id, 1)`;
/// * a **monster** seat - `rand() % 100 < steal_table[monster_id * 2]` decides,
///   the item is `steal_table[monster_id * 2 + 1]`, and a hit also bumps the
///   per-seat counter at `0x801C8FE0 + (caster_seat + 5) * 4`.
///
/// The steal table is the same `0x80077828` one `docs/formats/steal-table.md`
/// documents, fields `[chance, item]` - so the enemy-side steal and the
/// player-side one read one table.
///
/// This body reaches **no** damage wrapper. Its ten distinct `jal` targets are
/// `0x80019B28` (angle), `0x8003CA78` / `0x8003CAC4` (string build),
/// `0x80042310` (inventory consume), `0x8004E2F0` (arrival),
/// `0x8004FE5C` (voice), `0x80050E2C` (clip pick), `0x80056798` (rand),
/// `0x801D5854` (camera) and `0x801D8DE8` (message).
///
/// Not ported: the walk, the angles, the string build and the camera.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F730C (PROT 0941 action 0x51; phase machine + staging + the
/// steal resolution; walk / camera / string arms unported)
pub fn steal_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    run_clip: u8,
    outcome: Option<StealOutcome>,
) -> (CastTickStep, Option<StealOutcome>) {
    let mut taken = None;
    let step = run_chain_tick(ctx, |c| match c.phase {
        0 => {
            c.ctx_0d = 1;
            stage_clip_or(caster, run_clip);
            CastArmStep::Advance
        }
        1 => {
            taken = outcome;
            CastArmStep::Advance
        }
        2 => {
            c.ctx_0d = 0;
            stage_clip_or(caster, 0);
            c.ctx_0d = 1;
            CastArmStep::Advance
        }
        3 => {
            c.phase = LATCHED_DONE_PHASE;
            CastArmStep::Hold
        }
        LATCHED_DONE_PHASE => CastArmStep::Finish,
        // A phase the chain does not name: Done, not retail's Busy.
        _ => CastArmStep::Finish,
    });
    (step, taken)
}

/// PROT 0941 (Steal) tick body for action `0xB9` - a **whole-row sweep**, and
/// a different routine from the `0x51` arm despite sharing the image.
///
/// Five phase arms behind `sltiu v0, v1, 5` (`0x801F6A84`) through the head
/// table at `0x801F69D8` (arm targets `0x801F6AB0`, `6B38`, `6E38`, `7150`,
/// `7290`).
///
/// * arm `1` - hides every seat `0 .. ctx[+0]` (`+0x21C = 0xFF`), then
///   re-shows them (`+0x21C = 0`) once the countdown expires;
/// * arm `2` - the sweep. For each seat `0 .. ctx[+0]` that is alive and not
///   carrying `+0x16E & 4`: `FUN_801DD4B0(0xC0, ctx[+0x13], seat)`, the
///   damage-number popup `FUN_801F44A0`, the **unsigned** clamp against
///   `+0x14C` (`sltu v0, a0, s1` at `0x801F6FD8` - this sweep kills), `+0x10`
///   accumulate, HP write, `+0x1DA = +0x1F1` with `+0x1DC += 1`, and
///   `+0x21D = 2` on **every** seat including the ones it skipped;
/// * arm `4` - terminal: `ctx[+0x0D] = 0`, `ctx[+0x6DA] = 0x780`, busy
///   cleared.
///
/// Not ported: the two `FUN_80024E80` screen-prim fills that bracket the
/// sweep, the `FUN_801D829C` framing and the countdown.
///
/// `damage` is the caller's per-seat roll of the module's own baked power
/// ([`arm_damage_shape_for`]); `sweep` is false on every phase but `2`, which
/// is what keeps the roll off the frames retail never draws.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A04 (PROT 0941 action 0xB9; phase machine + whole-row sweep;
/// packet / camera arms unported)
pub fn steal_sweep_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    sweep: bool,
    damage: impl Fn(u8) -> i32,
) -> (CastTickStep, Vec<SweepHit>) {
    let mut hits = Vec::new();
    let step = run_arm_tick(ctx, 5, |c| match c.phase {
        1 => {
            // The arm hides every seat (`+0x21C = 0xFF`) for the length of
            // its countdown and shows them again (`+0x21C = 0`) when it
            // expires. The port does not model the countdown, so it applies
            // the arm's **exit** state - the hide is one frame of
            // presentation and leaving it latched would blank the field.
            for seat in 0..c.party_count {
                if let Some(s) = seats.get_mut(seat as usize) {
                    s.render_flag = 0;
                }
            }
            CastArmStep::Advance
        }
        2 => {
            if sweep {
                for seat in 0..c.party_count {
                    let Some(s) = seats.get_mut(seat as usize) else {
                        continue;
                    };
                    if aoe_seat_is_hittable(s) {
                        let applied = apply_arm_hit(s, damage(seat), s.restage.wrapping_add(1), 2);
                        hits.push(SweepHit { seat, applied });
                    }
                    s.anim_rate = 2;
                }
            }
            CastArmStep::Advance
        }
        4 => {
            c.ctx_0d = 0;
            CastArmStep::Finish
        }
        _ => CastArmStep::Advance,
    });
    (step, hits)
}
