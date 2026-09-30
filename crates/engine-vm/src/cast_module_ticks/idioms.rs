//! The idioms every routine in the band is built out of.
//! Split out of `cast_module_ticks.rs`.

use super::*;

// ---------------------------------------------------------------------------
// The idioms every routine in the band is built out of
// ---------------------------------------------------------------------------

/// The phase advance: `lbu v0, 0x279(ctx); addiu v0, v0, 1; sb v0, 0x279(ctx)`.
///
/// At most one taken arm per tick performs it, which is what makes the module
/// phase walk one step per frame. Wrapping is retail's: the byte is a `u8`
/// and the store is an `sb`.
pub fn advance_phase(ctx: &mut CastModuleCtx) {
    ctx.phase = ctx.phase.wrapping_add(1);
}

/// The paired stage/restage idiom: store the clip id to `+0x1DA` and bump the
/// restage counter `+0x1DC`.
///
/// Most `+0x1DA` writes in the band are paired this way, but not all - PROT
/// 0952's arm 3 writes `+0x1DA = 0` at `0x801F6EA4` with no `+0x1DC` bump
/// anywhere near it, so a plain assignment is a distinct operation and is
/// spelled out where it occurs.
pub fn stage_clip(actor: &mut CastActorState, clip: u8) {
    actor.staged_anim = clip;
    actor.restage = actor.restage.wrapping_add(1);
}

/// Apply shape **A** - the kill-capable clamp (PROT 0945 / 0957 / 0958 /
/// 0960).
///
/// ```text
/// a0 = victim[+0x14C]
/// sltu v0, a0, dmg        ; UNSIGNED
/// if v0 { dmg = a0 }
/// victim[+0x10]  += dmg
/// victim[+0x14C] -= dmg
/// ```
///
/// `roll` is the wrapper's raw signed return, and the comparison is unsigned,
/// so a negative roll compares greater than any HP and the clamp rewrites it
/// to the victim's whole HP - the victim dies. That is the retail behaviour,
/// not a port artefact; it is why this is kept apart from
/// [`apply_hit_floor_one`].
///
/// Returns the damage actually applied.
pub fn apply_hit_floor_zero(victim: &mut CastActorState, roll: i32) -> u32 {
    let hp = u32::from(victim.hp);
    let mut dmg = roll as u32;
    if hp < dmg {
        dmg = hp;
    }
    victim.hp_bar_delta = victim.hp_bar_delta.wrapping_add(dmg as i32);
    victim.hp = u32::from(victim.hp).wrapping_sub(dmg) as u16;
    dmg
}

/// Apply shape **B** - the never-kill clamp (PROT 0927 / 0966).
///
/// ```text
/// v0 = victim[+0x14C]
/// v1 = v0 - 1
/// slt v0, v1, dmg         ; SIGNED
/// if v0 { dmg = v1 }
/// victim[+0x10]  += dmg
/// victim[+0x14C] -= dmg
/// ```
///
/// The comparison is signed, so a negative roll passes through unclamped and
/// the subtract *raises* HP. The cap is `HP - 1`, so a live victim is left at
/// 1 HP at worst - neither of the two band-wide AoE stagers can kill.
///
/// Returns the (signed) amount applied.
pub fn apply_hit_floor_one(victim: &mut CastActorState, roll: i32) -> i32 {
    let cap = i32::from(victim.hp).wrapping_sub(1);
    let dmg = if cap < roll { cap } else { roll };
    victim.hp_bar_delta = victim.hp_bar_delta.wrapping_add(dmg);
    victim.hp = i32::from(victim.hp).wrapping_sub(dmg) as u16;
    dmg
}

/// Should an AoE stager's loop body run for this seat? Both `FUN_801F85A8`
/// and `FUN_801F8D64` open with the same two guards: skip a dead seat
/// (`+0x14C == 0`) and skip a non-targetable one (`+0x16E & 4`).
pub fn aoe_seat_is_hittable(actor: &CastActorState) -> bool {
    actor.hp != 0 && (actor.flags & FLAG_NON_TARGETABLE) == 0
}
