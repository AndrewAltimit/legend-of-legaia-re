//! PROT 0905 Vera (`0x83`): party restore and status-cure tick.

use super::*;

// ---------------------------------------------------------------------------
// PROT 0905 - Vera (spell id 0x83)
// ---------------------------------------------------------------------------

/// PROT 0905's tick entry.
pub const VERA_TICK: u32 = 0x801F_69D8;

/// The arm PROT 0905 clears the target's render flag in (`0x801F6D98`).
pub const VERA_SHOW_ARM: u8 = 1;

/// The arm PROT 0905 hides the target in, behind the cut's flash: render
/// flag `0xFF` and `+0x04 = 0` (`li s2,0xff` / `sb s2,0x21c(s3)` /
/// `sw zero,0x4(s3)` at `0x801F6F3C..0x801F6FD8`). The target stays hidden
/// through the creature's hover until arm 8's exit shows it again.
pub const VERA_HIDE_ARM: u8 = 4;

/// The render flag [`VERA_HIDE_ARM`] stores on the target.
pub const VERA_HIDDEN_RENDER_FLAG: u8 = 0xFF;

/// The arm PROT 0905 poses the summon seat in (`0x801F7300..0x801F733C`).
pub const VERA_POSE_ARM: u8 = 5;

/// The arm PROT 0905 stages the summon seat's cast clips in
/// (`0x801F78E0`, `0x801F7944`).
pub const VERA_CAST_ARM: u8 = 8;

/// The arm PROT 0905 restores HP and cures status in.
pub const VERA_RESTORE_ARM: u8 = 9;

/// The arm PROT 0905 fades out and latches `0xFF` from (`0x801F8034`).
pub const VERA_FADE_ARM: u8 = 10;

/// The magic level PROT 0905's status cure unlocks at
/// (`sltiu v0, v0, 0x3; bne` at `0x801F7D5C`).
pub const VERA_CURE_MIN_LEVEL: u8 = 3;

/// The `+0x16E` keep-masks PROT 0905's four cure tiers `and` with
/// (`0x801F7DB4`, `0x801F7E30`, `0x801F7EAC`, `0x801F7F28`).
///
/// The tier is the battle-overlay word `0x801F6960`, which sits below the
/// slot-B base and so is not the module's own data. It is the **Seru
/// side-effect stager's** output latch: `FUN_801F3D3C` reads an 8-byte record
/// out of the `[element][level band]` table at `0x801F6870`
/// (`0x801F6870 + ((level - 3) >> 1) * 8 + element * 0x20`, built at
/// `0x801F4420..0x801F4440`) and stores its first byte there
/// (`sw v1,0x6960(v0)` at `0x801F4480`). On the light row - element `5`, the
/// row a cure-class summon selects - that byte is
/// [`legaia_game_tables::seru_side_effect::RETAIL_CURE_CLASS_BY_BAND`], i.e. `1..=4`
/// by magic-level band. On the six damaging rows it is a percent
/// (`5 / 10 / 15 / 20`), which matches none of the four arms, so a non-light
/// summon's cast cures nothing without needing a second element test.
pub const VERA_CURE_MASKS: [u16; 4] = [0xFFFC, 0xFF84, 0xFB84, 0xFB84];

/// The cure ladder's own element row in the side-effect table - light.
pub const CURE_ELEMENT: u8 = 5;

/// Tier `4` also **doubles** the target's battle AP gauge `+0x170` and clamps
/// it to `0x64` (`0x801F7F24..0x801F7F48`: `+0x170 <<= 1`, stored, then
/// `0x64` when the stored halfword is `>= 0x65`). All three light-row heal
/// modules carry the same tail - PROT 0905 here, PROT 0911 at
/// `0x801F7E10..0x801F7E3C`, PROT 0919 at `0x801F8394..0x801F83C0` - so a
/// level-9 Vera / Orb / Spoon doubles the AP of every seat it cures. See
/// [`cure_tier4_ap`].
pub const CURE_TIER4_AP_CAP: u16 = 0x64;

/// The cure ladder's tier-`4` AP write: `+0x170 = min(+0x170 << 1, 100)`,
/// the doubling taken on the `u16` halfword the `sh` stores.
///
/// First reported by the_rabidsquirel (community research, save-state
/// testing) as "level-9 healing magic doubles the target's AP"; the three
/// module tails above are the disassembly behind it.
///
/// REF: FUN_801F69D8 (PROT 0905 `0x801F7F24..0x801F7F48`, PROT 0911 `0x801F7E10..0x801F7E3C`, PROT 0919 `0x801F8394..0x801F83C0`)
pub fn cure_tier4_ap(spirit_gauge: u16) -> u16 {
    let doubled = spirit_gauge.wrapping_shl(1);
    if doubled > CURE_TIER4_AP_CAP {
        CURE_TIER4_AP_CAP
    } else {
        doubled
    }
}

/// The `+0x16E` bits whose presence makes PROT 0905's tiers `2..=4` play the
/// cure cue and light the target's `+0x220..+0x223` markers
/// (`andi v0, v0, 0x3c`).
pub const VERA_CURE_CUE_BITS: u16 = 0x003C;

/// What PROT 0905's restore arm needs that the actor mirror does not carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VeraRestore {
    /// `record[+0x729 + slot]` - the caster's magic level for this spell,
    /// found by scanning the 32 learned-spell ids at `record[+0x705 + i]` for
    /// the caster's queued action (`0x801F7C28..0x801F7C50`). The record base
    /// is `0x80084140 + (0x8007BD10[caster_seat] - 1) * 0x414`.
    pub magic_level: u8,
    /// `target[+0x14E]` - max HP, which the clamp needs and the mirror has no
    /// field for.
    pub max_hp: u16,
    /// The cure tier the battle-overlay word `0x801F6960` carries - the Seru
    /// side-effect stager's light-row class `1..=4`. Anything else (a damaging
    /// row's percent, or `0` when nothing staged) cures nothing.
    pub cure_tier: u8,
    /// Does this arm **apply** the HP it computes?
    ///
    /// Retail has one owner for the restore and it is this arm, so a faithful
    /// replay sets this. The engine has a second owner - it folds a cast's HP
    /// outcome exactly once at `World::cast_spell_on_slots_prepaid`, which
    /// routes this module's own magnitude in through
    /// `World::seru_tick_heal_amount` - and the driven seam re-enters this
    /// body every frame, so a driven Vera with this set restores twice.
    /// Clearing it keeps the arm's phase machine, its cure sweep and its
    /// presentation writes while the fold stays the single applier, which is
    /// the posture every other body in the band already takes (they are handed
    /// a neutral magnitude outright).
    pub apply_hp: bool,
}

/// What PROT 0905's restore arm did this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VeraOutcome {
    /// HP actually restored after the missing-HP clamp.
    pub restored: u16,
    /// `true` when the tier was high enough to reach the `+0x16E` mask.
    pub cured: bool,
    /// `true` when the target was carrying one of [`VERA_CURE_CUE_BITS`], so
    /// retail played the cure cue and set `+0x220..+0x223`.
    pub cure_cue: bool,
    /// `true` on tier `4`, where the arm also doubled the target's AP gauge
    /// `+0x170` under [`CURE_TIER4_AP_CAP`] ([`cure_tier4_ap`]).
    pub doubled_ap: bool,
}

/// PROT 0905 (Vera, spell id `0x83`) tick body - 5792 bytes, 1448
/// instructions.
///
/// Eleven phase arms `0..=10` plus `0xFF`, through the chain at
/// `0x801F6A70..0x801F6B10`. It holds **no** damage-wrapper call at all.
///
/// That is the module `static-overlays.toml` still labels
/// `summon_stager_x83`; spell `0x83` is **Vera**, whose SCUS record class is
/// the ally-side `0x03`, and arm 9 is a **restore**, not a hit:
///
/// ```text
/// level = record[+0x729 + slot]              ; slot found by scanning +0x705
/// heal  = level * 0x20 + 0xE0                ; clamped to +0x14E - +0x14C
/// if (target[+0x14C] != 0 && !(target[+0x16E] & 4))
///     target[+0x14C] += heal                 ; 0x801F7CFC
///     target[+0x10]   = -heal                ; 0x801F7D0C, an assignment
/// if (level >= 3)  target[+0x16E] &= VERA_CURE_MASKS[tier - 1]
/// ```
///
/// `+0x10` is **stored**, not accumulated, and it is stored negative - the
/// one site in this set that does not `addu` into the pending HP-bar delta.
///
/// Other simulation state: the target's render flag `= 0` in arm 1 and
/// `= 0xFF` with `+0x04 = 0` in arm 4 ([`VERA_HIDE_ARM`]), the
/// summon seat's `+0x0C = 0x1000` / animation rate `= 8` / render flag `= 0`
/// / clip pair `+= 1` in arm 5, `+0x1DA = 2` then `= 3` with a `+0x1DC` bump
/// and render flag `= 9` in arm 8, and the fade countdown in arm 10 that
/// latches `0xFF`. Arm `0xFF` writes no actor state; it only returns `0`.
///
/// Tier `4` also doubles the target's AP gauge `+0x170`, capped at 100
/// ([`cure_tier4_ap`]).
///
/// Not ported: the packet arms, the cure cue's `+0x220..+0x223` markers, and
/// the battle-overlay tier selector `0x801F6960` (the caller supplies it).
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F69D8 (PROT 0905; phase chain + the HP restore and status cure; packet arms unported)
pub fn vera_tick(
    ctx: &mut CastModuleCtx,
    seats: &mut [CastActorState],
    who: SeruSeats,
    restore: Option<VeraRestore>,
) -> (CastTickStep, Option<VeraOutcome>) {
    let mut outcome = None;
    let step = run_chain(ctx, |c| match c.phase {
        VERA_SHOW_ARM => {
            if let Some(t) = seats.get_mut(who.victim as usize) {
                t.render_flag = 0;
            }
            CastArmStep::Advance
        }
        VERA_HIDE_ARM => {
            if let Some(t) = seats.get_mut(who.victim as usize) {
                t.render_flag = VERA_HIDDEN_RENDER_FLAG;
                t.present_04 = 0;
            }
            CastArmStep::Advance
        }
        VERA_POSE_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.root_speed = SERU_HIT_ROOT_SPEED;
                s.anim_rate = ANIM_RATE_NORMAL;
                s.render_flag = 0;
                s.staged_anim = s.staged_anim.wrapping_add(1);
                s.restage = s.restage.wrapping_add(1);
            }
            // `addiu v0,v0,0x3; sb v0,0x279(s2)` at `0x801F7434`: the arm
            // steps the phase by three, so arms 6 and 7 are never run.
            c.phase = c.phase.wrapping_add(2);
            CastArmStep::Advance
        }
        VERA_CAST_ARM => {
            if let Some(s) = seats.get_mut(who.summon as usize) {
                s.staged_anim = 3;
                s.anim_rate = ANIM_RATE_NORMAL;
                s.restage = s.restage.wrapping_add(1);
                s.render_flag = 9;
            }
            // `lui v1,0x2008; ori v1,0x200` / `sw v1,0x4(s3)` / `sb zero,
            // 0x21c(s3)` (`0x801F79FC..0x801F7A24`): the target arm 4 hid is
            // shown again.
            if let Some(t) = seats.get_mut(who.victim as usize) {
                t.render_flag = 0;
                t.present_04 = GIZAM_ROW_LIT_WORD;
            }
            CastArmStep::Advance
        }
        VERA_RESTORE_ARM => {
            if let Some(r) = restore
                && let Some(t) = seats.get_mut(who.victim as usize)
            {
                let mut out = VeraOutcome::default();
                // The arm's own arithmetic always runs; `apply_hp` decides
                // whether this body is the one that stores it.
                let heal = match r.apply_hp {
                    true => vera_heal_amount(r.magic_level, t.hp, r.max_hp),
                    false => 0,
                };
                if t.hp != 0 && (t.flags & FLAG_NON_TARGETABLE) == 0 {
                    t.hp = t.hp.wrapping_add(heal);
                    // Retail stores, it does not accumulate: `sw -heal`.
                    t.hp_bar_delta = -i32::from(heal as i16);
                    out.restored = heal;
                }
                if r.magic_level >= VERA_CURE_MIN_LEVEL
                    && let Some(&mask) = VERA_CURE_MASKS.get(r.cure_tier.wrapping_sub(1) as usize)
                {
                    out.cure_cue = r.cure_tier >= 2 && (t.flags & VERA_CURE_CUE_BITS) != 0;
                    t.flags &= mask;
                    out.cured = true;
                    if r.cure_tier == 4 {
                        t.spirit_gauge = cure_tier4_ap(t.spirit_gauge);
                        out.doubled_ap = true;
                    }
                }
                outcome = Some(out);
            }
            CastArmStep::Advance
        }
        CHOREOGRAPHY_DONE_PHASE => CastArmStep::Finish,
        p if p <= VERA_FADE_ARM => CastArmStep::Advance,
        _ => CastArmStep::Finish,
    });
    (step, outcome)
}
