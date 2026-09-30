//! The seven state-touching spawn stagers (`0x801F6734` row, move-VM op `0x2F`).
//! Split out of `cast_module_ticks.rs`.

use super::*;

// ---------------------------------------------------------------------------
// The seven state-touching spawn stagers (`0x801F6734` row, move-VM op 0x20)
// ---------------------------------------------------------------------------

/// PROT 0949 (Water Crystals) spawn stager - the freeze ramp.
///
/// Eight arms behind `sltiu a1, 8` through the table at `0x801F69F0`, and
/// every one is two stores on the **victim** (`actor_table[caster[+0x1DD]]`,
/// derived in the prologue at `0x801F75BC..0x801F75F4`). The routine holds no
/// `jal` at all:
///
/// ```text
/// arm n:  sw (n + 1) * 0x200, 0x0C(victim)
///         sb 7 - n,           0x21D(victim)
/// ```
///
/// so `+0x0C` climbs `0x200 .. 0x1000` while the animation rate falls `7 .. 0`:
/// the victim freezes as the magnitude peaks. An `a1` of 8 or more falls
/// through to the epilogue and writes nothing.
///
/// `docs/subsystems/cast-module.md` grades this **PORT** for the `+0x0C`
/// write and does not mention `+0x21D`; both stores are here.
///
/// The eight arms are eight distinct entry addresses, not one body with a
/// computed constant, so the catalog tracks each one. Arm 0 is the
/// fall-through inside this routine's own extent; arms 1..7 are frameless
/// leaves the jump table reaches and nothing else references:
///
/// | table slot | entry | `+0x0C` | `+0x21D` |
/// |---:|---|---:|---:|
/// | 0 | `0x801F761C` (interior of `0x801F75BC`) | `0x200` | 7 |
/// | 1 | `0x801F7630` | `0x400` | 6 |
/// | 2 | `0x801F7644` | `0x600` | 5 |
/// | 3 | `0x801F7658` | `0x800` | 4 |
/// | 4 | `0x801F766C` | `0xA00` | 3 |
/// | 5 | `0x801F7680` | `0xC00` | 2 |
/// | 6 | `0x801F7694` | `0xE00` | 1 |
/// | 7 | `0x801F76A8` | `0x1000` | 0 |
///
/// Arms 1..6 are 20 bytes each - five instructions, the `sb` in the `jr ra`
/// delay slot. Arm 7 is 12 bytes and has no `jr ra` of its own: it stores
/// `0x1000` and `sb $zero` and falls into the routine's shared epilogue at
/// `0x801F76B4`, which is also where the out-of-range `beqz` lands. The table
/// itself is eight words at `0x801F69F0`, six words into the image's leading
/// VA run, and `0x801F7644` is one of the VAs PROT 0901 also uses for a
/// world-map draw leaf - different image, different bytes.
///
/// Wired: `World::run_cast_module_code`, at the cast band's staging seam.
///
/// PORT: FUN_801F75BC, FUN_801F7630, FUN_801F7644, FUN_801F7658, FUN_801F766C, FUN_801F7680, FUN_801F7694, FUN_801F76A8 (PROT 0949; the stager and its seven ramp arms)
pub fn water_crystals_stager(victim: &mut CastActorState, arm: u8) {
    if arm >= 8 {
        return;
    }
    victim.root_speed = (i32::from(arm) + 1) * 0x200;
    victim.anim_rate = 7 - arm;
}

/// PROT 0922 (Puera) spawn stager - eight instructions and one byte.
///
/// `bnez a1` skips the whole body, so only arm `0` does anything, and what it
/// does is `ctx[+0x278] = 3`. The frame (`addiu sp,sp,-8`) opens in the
/// branch's delay slot, which is why a prologue scan puts the entry four
/// bytes late - the routine really starts at `0x801F90E4`.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F90E4
pub fn puera_stager(ctx: &mut CastModuleCtx, arm: u8) {
    if arm != 0 {
        return;
    }
    ctx.ctx_278 = 3;
}

/// PROT 0923 (Gilium) spawn stager.
///
/// Three arms reached by a `beq`/`slti` chain rather than a table:
///
/// * arm `0` - `FUN_80021B04(actor+0x14, actor+0x24, 0x801F9450, 0x1000)`,
///   stash the handle at `0x801FA4D0`, then `ctx[+0x278] = 3`;
/// * arm `1` - OR bit `8` into the `+0x10` word of three live handles and
///   allocate a screen prim through `FUN_80024E80`;
/// * arm `2` - `FUN_80021B04(actor+0x14, actor+0x24, 0x801FA414, actor[+0x72])`.
///
/// The spawn calls are the DATA layer the pool already stages; the only
/// simulation write in the routine is arm 0's `ctx[+0x278]`.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F8B90 (state half; the three spawn sites are
/// `legaia_asset::cast_effect_pool`'s records)
pub fn gilium_stager(ctx: &mut CastModuleCtx, arm: u8) {
    if arm == 0 {
        ctx.ctx_278 = 3;
    }
}

/// PROT 0906 (Gizam) spawn stager.
///
/// Seven arms behind `sltiu a1, 7` through the head table at `0x801F69D8`
/// (arm targets `0x801F7788`, `780C`, `783C`, `785C`, `7920`, `7964`,
/// `79A8`). Two arms carry state:
///
/// * arm `1` (`0x801F780C..0x801F783C`) poses the summon seat
///   `actor_table[7]`: `+0x21C = 0` (visible), `+0x04 = 0x3FF00000` (the
///   render blend), `+0x0C = 0x1000`;
/// * arm `2` (`0x801F783C..0x801F785C`) is nothing but the phase advance.
///
/// The rest are `FUN_80024E80` prim fills, two `FUN_80056798` rand draws and
/// one `FUN_80021B04` spawn - the DATA layer.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F7740 (state half)
pub fn gizam_stager(ctx: &mut CastModuleCtx, summon_seat: &mut CastActorState, arm: u8) {
    match arm {
        1 => {
            summon_seat.render_flag = 0;
            summon_seat.root_speed = 0x1000;
        }
        2 => advance_phase(ctx),
        _ => {}
    }
}

// --- W1-C ---
/// PROT 0909's stager arm that stages a clip and advances the phase - the
/// second of its two state-touching arms.
pub const VIGURO_STAGER_STAGE_ARM: u8 = 1;
/// The clip that arm stores to the summon seat's `+0x1DA`
/// (`addiu a1,zero,1` at `0x801F7BE8`, `move v0,a1` then `sb v0,0x1da(v1)` at
/// `0x801F7BF8`), with no `+0x1DC` bump anywhere in the arm.
pub const VIGURO_STAGER_ARM1_CLIP: u8 = 1;
// --- end W1-C ---

/// PROT 0909 (Viguro) spawn stager.
///
/// Seven arms behind `sltiu a1, 7` through the head table at `0x801F69D8`
/// (arm targets `0x801F7B2C`, `7BCC`, `7C64`, `7C78`, `7C8C`, `7CB8`,
/// `7CA0`; arm 5 is the epilogue itself, i.e. a no-op). **Two** arms touch
/// state, not one - arm `0` is the seat pose:
///
/// ```text
/// jal FUN_801F19EC                        ; module init
/// summon = actor_table[7]
/// summon[+0x21C] = 0                      ; visible
/// summon[+0x0C]  = 0x1000
/// saved          = summon[+0x1DD]         ; stashed at 0x8019835C
/// summon[+0x1DD] = 9                      ; the enemy-row group code
/// ctx[+0x279]   += 1
/// ```
///
/// The `+0x34` / `+0x38` / `+0x46` / `+0x04` / `+0x21F` stores in the same arm
/// are pose and render fields, left to the host's own seat placement.
///
// --- W1-C ---
/// Arm `1` (`0x801F7BCC`) is the second, and it is what releases the tick
/// body's phase-`8` rendezvous ([`crate::cast_seru_ticks_b`]):
///
/// ```text
/// summon[+0x176] = 0                      ; pose, unported
/// summon[+0x21B] = 0                      ; pose, unported
/// summon[+0x1DA] = 1                      ; 0x801F7BF8, no +0x1DC bump
/// jal FUN_80024E80 -> ctx[+0x102C]        ; the pool's
/// ctx[+0x279]   += 1                      ; 0x801F7C54
/// summon[+0x21F] = 0                      ; pose, unported
/// ```
// --- end W1-C ---
///
/// Returns the `+0x1DD` value the arm displaced, which retail stashes for a
/// later arm to restore.
///
/// Wired: `World::run_cast_module_code`.
///
// --- W1-C ---
/// REF: FUN_80024E80
// --- end W1-C ---
/// PORT: FUN_801F7AF4 (state half)
pub fn viguro_stager(
    ctx: &mut CastModuleCtx,
    summon_seat: &mut CastActorState,
    arm: u8,
) -> Option<u8> {
    // --- W1-C ---
    if arm == VIGURO_STAGER_STAGE_ARM {
        summon_seat.staged_anim = VIGURO_STAGER_ARM1_CLIP;
        advance_phase(ctx);
        return None;
    }
    // --- end W1-C ---
    if arm != 0 {
        return None;
    }
    let saved = summon_seat.target_code;
    summon_seat.render_flag = 0;
    summon_seat.root_speed = 0x1000;
    summon_seat.target_code = TARGET_CODE_ENEMY_ROW;
    advance_phase(ctx);
    Some(saved)
}

/// One seat's outcome inside an AoE stager's loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AoeHit {
    /// Absolute actor slot the hit landed on.
    pub seat: u8,
    /// Amount applied after the module's clamp (signed: shape B can heal).
    pub applied: i32,
}

/// PROT 0927 (Juggernaut) spawn stager - the enemy-row sweep.
///
/// Nine arms behind `sltiu a1, 9` through the table at `0x801F6A60`. The
/// working arm walks the actor table from `+0xC` (seat
/// [`FIRST_MONSTER_SEAT`]) for `ctx[+1]` seats:
///
/// ```text
/// for i in 0 .. ctx[+1]:
///     victim = actor_table[3 + i]
///     if victim[+0x14C] == 0    { continue }        ; dead
///     if victim[+0x16E] & 4     { continue }        ; non-targetable
///     dmg = FUN_801DD0AC(0x12, 7, 3 + i)            ; SHARED kernel, summon branch
///     cap = victim[+0x14C] - 1
///     if (i32)cap < (i32)dmg { dmg = cap }          ; never kills
///     victim[+0x10]  += dmg
///     victim[+0x14C] -= dmg
/// ```
///
/// The `a1 = 7` is the shared kernel's summon-branch selector, so this module
/// is the band's one user of `FUN_801DD0AC` - and it is a *summon* module
/// (PROT 0903..0934), which is why it does not contradict
/// `docs/subsystems/battle-formulas.md`'s "a capture-class cast never reaches
/// `FUN_801DD0AC` at all". Unlike Evil Seru Magic it stages no reaction clip
/// and leaves the animation rate alone.
///
/// `roll` supplies one wrapper result per hittable seat, in seat order; the
/// caller owns the RNG cursor so a host can keep retail's draw order.
///
/// Wired: `World::run_cast_module_aoe`.
///
/// PORT: FUN_801F85A8 (state half; the seven spawn sites are the pool's)
pub fn juggernaut_stager(
    ctx: &CastModuleCtx,
    seats: &mut [CastActorState],
    arm: u8,
    mut roll: impl FnMut(u8) -> i32,
) -> Vec<AoeHit> {
    let mut hits = Vec::new();
    if arm >= 9 {
        return hits;
    }
    for i in 0..ctx.monster_count {
        let seat = FIRST_MONSTER_SEAT.saturating_add(i);
        let Some(victim) = seats.get_mut(seat as usize) else {
            break;
        };
        if !aoe_seat_is_hittable(victim) {
            continue;
        }
        let applied = apply_hit_floor_one(victim, roll(seat));
        hits.push(AoeHit { seat, applied });
    }
    hits
}

/// PROT 0966 (Evil Seru Magic) spawn stager - the whole-table sweep.
///
/// Nine arms behind `sltiu a1, 9` through the table at `0x801F6A50`. Its
/// working arm (arm `4`, `0x801F8E4C`) walks the actor table from seat `0`
/// for `ctx[+0]` seats, and unlike Juggernaut it also stages the reaction:
///
/// ```text
/// for i in 0 .. ctx[+0]:
///     victim = actor_table[i]
///     if victim[+0x14C] == 0 || victim[+0x16E] & 4 { continue }
///     dmg = FUN_801DD4B0(0x100, ctx[+0x13], i)      ; the RESPECTING wrapper
///     cap = victim[+0x14C] - 1
///     if (i32)cap < (i32)dmg { dmg = cap }          ; never kills
///     victim[+0x10]  += dmg
///     victim[+0x14C] -= dmg
///     victim[+0x1DA]  = victim[+0x1F1]              ; its own knockdown clip
///     victim[+0x1DC] += 1
///     victim[+0x21D]  = 2                           ; slow-motion
/// ```
///
/// So Cort's ESM hits **every seat in the table**, party and monsters alike,
/// and leaves each at 1 HP at worst.
///
/// Wired: `World::run_cast_module_aoe`.
///
/// PORT: FUN_801F8D64 (state half; the seven spawn sites are the pool's)
pub fn evil_seru_magic_stager(
    ctx: &CastModuleCtx,
    seats: &mut [CastActorState],
    arm: u8,
    mut roll: impl FnMut(u8) -> i32,
) -> Vec<AoeHit> {
    let mut hits = Vec::new();
    if arm >= 9 {
        return hits;
    }
    for seat in 0..ctx.party_count {
        let Some(victim) = seats.get_mut(seat as usize) else {
            break;
        };
        if !aoe_seat_is_hittable(victim) {
            continue;
        }
        let applied = apply_hit_floor_one(victim, roll(seat));
        let knockdown = victim.knockdown_anim;
        stage_clip(victim, knockdown);
        victim.anim_rate = 2;
        hits.push(AoeHit { seat, applied });
    }
    hits
}
