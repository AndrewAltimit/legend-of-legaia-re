//! The idioms every routine in the band is built out of.

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

/// PROT 0957 `0x76`'s picked card on its two seats, past arm 9's gate.
///
/// | card | `0x801F...` | stores |
/// |---|---|---|
/// | victim falls | `922C` | victim HP `0` with the whole bar as damage, its `+0x1F1` staged and bumped |
/// | caster falls | `935C` | caster HP `0` (no damage word), capture state `+0x225 = 2` by the host, its `+0x1F1` staged and bumped |
/// | both bleed | `9390` | each loses `(hp + 3) >> 2`, its `+0x1F1` staged and bumped |
/// | both heal | `95D0` | each gains `(max + 3) >> 2`, capped at its missing HP; the victim's clip stands |
///
/// PORT: FUN_801F798C (`0x801F922C..0x801F9794`, the four results)
pub fn death_game_outcome(
    outcome: crate::cast_module_camera::DeathGameOutcome,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
    max_hp: [u16; 2],
) {
    use crate::cast_module_camera::DeathGameOutcome as O;
    let fall = |a: &mut CastActorState| {
        let clip = a.knockdown_anim;
        stage_clip(a, clip);
    };
    let bleed = |a: &mut CastActorState| {
        let hp = u32::from(a.hp);
        let dmg = ((hp + 3) >> 2).min(hp);
        a.hp_bar_delta = a.hp_bar_delta.wrapping_add(dmg as i32);
        a.hp = (hp - dmg) as u16;
    };
    let heal = |a: &mut CastActorState, max: u16| {
        let (hp, max) = (u32::from(a.hp), u32::from(max));
        let gain = ((max + 3) >> 2).min(max.saturating_sub(hp));
        a.hp_bar_delta = a.hp_bar_delta.wrapping_sub(gain as i32);
        a.hp = (hp + gain) as u16;
    };
    match outcome {
        O::VictimFalls => {
            victim.hp_bar_delta = victim.hp_bar_delta.wrapping_add(i32::from(victim.hp));
            victim.hp = 0;
            fall(victim);
        }
        O::CasterFalls => {
            caster.hp = 0;
            fall(caster);
        }
        O::BothBleed => {
            bleed(victim);
            fall(victim);
            bleed(caster);
            fall(caster);
        }
        O::BothHeal => {
            heal(caster, max_hp[0]);
            heal(victim, max_hp[1]);
        }
    }
}

/// The effect pool's sprite scale (pool head `+0x2`, `0xA00` from the pool
/// init) that Theeder's arm-12 hits store (`li v0,0x800; sh v0,0x2(v1)` at
/// `0x801F7DD0..0x801F7DD4`, on `_DAT_8007BD30`).
pub const THEEDER_HIT_SPRITE_SCALE: u16 = 0x800;

/// The pool's default sprite scale, which a module's way out restores.
pub const DEFAULT_SPRITE_SCALE: u16 = 0xA00;

/// The store a module arm makes on the effect pool's sprite scale (pool head
/// `+0x2`, read by pass 2 as `atlas w/h * scale >> 8`), if any. Two player
/// Seru modules shrink the effect-pool sprites their creature sheds and put
/// the scale back on their `0xFF` arm:
///
/// - PROT 0904 (Theeder): arm 0 `0x400` (`0x801F6C28..0x801F6C34`), arm
///   `0xFF` `0xA00` (`0x801F80F4..0x801F8104`); its arm-12 hits store
///   [`THEEDER_HIT_SPRITE_SCALE`].
/// - PROT 0909 (Viguro): arm 0 `0x600` (`0x801F6BC4..0x801F6BCC`), arm `0xFF`
///   `0xA00` (`0x801F7908..0x801F7910`).
///
/// No other dumped routine stores into the pool head after the init.
pub fn effect_sprite_scale_store(prot_entry: u32, phase: u8) -> Option<u16> {
    match (prot_entry, phase) {
        (904, 0) => Some(0x400),
        (909, 0) => Some(0x600),
        (904 | 909, 0xFF) => Some(DEFAULT_SPRITE_SCALE),
        _ => None,
    }
}
