//! The capture-class trampolines (`0x801CF56C` arm -> tick body) and six more per-module tick bodies.
//! Split out of `cast_module_ticks.rs`.

use super::*;

// ---------------------------------------------------------------------------
// The capture-class trampolines (`0x801CF56C` arm -> tick body)
// ---------------------------------------------------------------------------

/// One capture-class module's trampoline: the routine PROT 0898's
/// `0x801CF56C` arm `jal`s, and the `(queued action id -> tick body)` map it
/// dispatches on.
///
/// The shape is uniform across the band and is 22 to 51 instructions long:
/// materialise the battle ctx `*0x8007BD24`, load the caster
/// `actor_table[ctx+0x13]` out of `0x801C9370`, read its queued action byte
/// `caster[+0x1DF]`, and either compare it against the module's own spell ids
/// in a `beq` chain or index a jump table. An id the map does not name falls
/// straight through to the epilogue with `a0 = 0`, so the module ticks
/// nothing and the drive loop's "tick returned zero" gate lets the battle
/// proceed.
#[derive(Debug, Clone, Copy)]
pub struct CaptureTrampoline {
    /// Extraction PROT entry of the owning module.
    pub prot_entry: u32,
    /// Retail VA of the trampoline itself - the `0x801CF56C` arm's `jal`
    /// target.
    pub trampoline: u32,
    /// `(action id, tick body VA)`, in `beq`-chain / table order.
    pub arms: &'static [(u8, u32)],
}

/// PROT 0955's twenty-word head table, read at file `+0x00..+0x50`: the
/// trampoline bounds `id - 0x60` with `sltiu 0x14` and jumps through it, so
/// the table's index space is action ids `0x60..=0x73`. Fourteen of the
/// twenty words point at the shared epilogue `0x801F9360` and tick nothing;
/// the six below are the module's real bodies.
///
/// This is a **third** owner for a band head table.
/// `docs/subsystems/cast-module.md` resolves head tables as the tick's or the
/// stager's by reading the `sltiu` immediate; 0955's belongs to neither - it
/// is the trampoline's.
pub const WHITE_SHIELD_TRAMPOLINE_ARMS: [(u8, u32); 6] = [
    (0x60, 0x801F_8F0C),
    (0x6E, 0x801F_86A4),
    (0x6F, 0x801F_7FA4),
    (0x70, 0x801F_767C),
    (0x72, 0x801F_7158),
    (0x73, 0x801F_6A28),
];

/// **Every** capture-class trampoline in PROT 0935..0966, read off its
/// **owning** image's bytes at slot-B base `0x801F69D8`.
///
/// Twenty-one of the thirty-two `0x801CF56C` arms are a trampoline; the other
/// eleven (PROT 0935, 0936, 0937, 0939, 0946, 0947, 0948, 0949, 0953, 0954,
/// 0966) point straight at a tick body, which is what
/// `docs/subsystems/cast-module.md` means by "modules whose cell holds a
/// single spell skip the trampoline".
///
/// The `(id -> body)` map matters to the port because a module's own entry is
/// **not** enough to pick a body: PROT 0945 and 0960 each hold two whole
/// choreographies, and a dispatcher keyed on the entry alone runs one of them
/// for both ids. The map also makes the body VA ambiguous on its own - six
/// modules put a body at `0x801F69D8`, the load base - so a consumer has to
/// key on `(entry, body)`, never on the body alone.
///
/// The first `PORT:` tag names the six trampoline VAs that are neither
/// phantom prints nor VA-aliased across images by bare address. The other
/// fifteen, PROT 0957's `0x801F9BA8` included, sit at VAs where other images
/// hold different code (`scripts/ci/port-catalog-ignore.toml` files them under
/// `[worklist_va_aliased]` / `[ghidra_phantoms]`), so the second tag names
/// each by its **dump stem** - `overlay_<label>_<prot>_<addr>`, the trampoline
/// in that one image - which is how the per-image port table tells this
/// port from the unrelated routines at the same VA.
///
/// PORT: FUN_801F7A40, FUN_801F7B1C, FUN_801F7B28, FUN_801F816C, FUN_801F8E60, FUN_801F92A4
/// PORT: overlay_cast_glare_divide_0940_801f8228, overlay_cast_steal_0941_801f7d38,
///       overlay_cast_power_up_0942_801f80a0, overlay_cast_curse_0943_801f7624,
///       overlay_cast_guilty_cross_0944_801f7ebc, overlay_cast_water_column_0945_801f76f4,
///       overlay_cast_rolling_flare_0950_801f8190, overlay_cast_water_hazard_0956_801f7e4c,
///       overlay_summon_effect_table_0957_801f9ba8, overlay_cast_megaton_press_0959_801f87f4,
///       overlay_cast_plasma_strike_0960_801f8638, overlay_cast_dead_end_crisis_0961_801f7a54,
///       overlay_cast_blade_breath_0962_801f8080, overlay_cast_genocidal_cannon_0963_801f8438,
///       overlay_cast_element_change_0964_801f8e3c
pub const CAPTURE_TRAMPOLINES: [CaptureTrampoline; 21] = [
    // `beq v1, 0x4e -> 0x801F726C` / `beq v1, 0xb7 -> 0x801F69EC`.
    CaptureTrampoline {
        prot_entry: 938,
        trampoline: 0x801F_7A40,
        arms: &[(0x4E, 0x801F_726C), (0xB7, 0x801F_69EC)],
    },
    // The band's widest `beq` chain: four ids over three bodies, with `0x50`
    // and `0xAE` sharing `0x801F78B8` (`0x801F825C` and the `bne` at
    // `0x801F8288` both land on `0x801F8290`).
    CaptureTrampoline {
        prot_entry: 940,
        trampoline: 0x801F_8228,
        arms: &[
            (0x3C, 0x801F_69F8),
            (0x50, 0x801F_78B8),
            (0xAC, 0x801F_7240),
            (0xAE, 0x801F_78B8),
        ],
    },
    CaptureTrampoline {
        prot_entry: 941,
        trampoline: 0x801F_7D38,
        arms: &[(0x51, 0x801F_730C), (0xB9, 0x801F_6A04)],
    },
    CaptureTrampoline {
        prot_entry: 942,
        trampoline: 0x801F_80A0,
        arms: &[(0x52, POWER_UP_TICK), (0xAA, 0x801F_69F4)],
    },
    CaptureTrampoline {
        prot_entry: 943,
        trampoline: 0x801F_7624,
        arms: &[(0x40, 0x801F_6EF4), (0xB5, 0x801F_6A04)],
    },
    CaptureTrampoline {
        prot_entry: 944,
        trampoline: 0x801F_7EBC,
        arms: &[(0x37, 0x801F_6A04), (0x53, 0x801F_7470)],
    },
    CaptureTrampoline {
        prot_entry: 945,
        trampoline: 0x801F_76F4,
        arms: &[(0x54, WATER_COLUMN_TICK), (0xBA, ALL_STATS_SURGE_TICK)],
    },
    CaptureTrampoline {
        prot_entry: 950,
        trampoline: 0x801F_8190,
        arms: &[(0x5A, 0x801F_79F8), (0xAB, 0x801F_6A24)],
    },
    // Single arm, spelled as `bne v1, 0xb6 -> epilogue`.
    CaptureTrampoline {
        prot_entry: 951,
        trampoline: 0x801F_816C,
        arms: &[(0x36, 0x801F_6A20), (0x5B, 0x801F_77E8)],
    },
    CaptureTrampoline {
        prot_entry: 952,
        trampoline: 0x801F_7B28,
        arms: &[(0x5C, 0x801F_7118), (0xB8, 0x801F_6A0C)],
    },
    CaptureTrampoline {
        prot_entry: 955,
        trampoline: 0x801F_92A4,
        arms: &WHITE_SHIELD_TRAMPOLINE_ARMS,
    },
    CaptureTrampoline {
        prot_entry: 956,
        trampoline: 0x801F_7E4C,
        arms: &[(0x71, 0x801F_7298), (0x75, 0x801F_69D8)],
    },
    CaptureTrampoline {
        prot_entry: 957,
        trampoline: 0x801F_9BA8,
        arms: &[
            (SUMMON_EFFECT_TICK_B_ID, SUMMON_EFFECT_TICK_B),
            (SUMMON_EFFECT_TICK_A_ID, SUMMON_EFFECT_TICK_A),
        ],
    },
    CaptureTrampoline {
        prot_entry: 958,
        trampoline: 0x801F_8E60,
        arms: &[(0x79, 0x801F_6DD8)],
    },
    CaptureTrampoline {
        prot_entry: 959,
        trampoline: 0x801F_87F4,
        arms: &[(0x7A, 0x801F_69F0)],
    },
    CaptureTrampoline {
        prot_entry: 960,
        trampoline: 0x801F_8638,
        arms: &[(0x7B, PLASMA_STRIKE_TICK), (0xA6, 0x801F_69D8)],
    },
    // Two ids, one body - the only trampoline in the band that maps a pair
    // onto the same routine (`0x801F7A88` and the `bne` at `0x801F7A90` both
    // reach `0x801F7A98`).
    CaptureTrampoline {
        prot_entry: 961,
        trampoline: 0x801F_7A54,
        arms: &[(0xA1, 0x801F_69D8), (0xB4, 0x801F_69D8)],
    },
    CaptureTrampoline {
        prot_entry: 962,
        trampoline: 0x801F_8080,
        arms: &[
            (0xA2, 0x801F_7AE4),
            (0xA3, 0x801F_74A0),
            (0xA4, 0x801F_6D54),
            (0xA5, 0x801F_69D8),
        ],
    },
    CaptureTrampoline {
        prot_entry: 963,
        trampoline: 0x801F_8438,
        arms: &[(0xB3, 0x801F_6A20)],
    },
    // `0xAF` is a `beq`; the second body is reached by a **range** test
    // (`slti v1, 0xaf` then `slti v1, 0xb3` at `0x801F8E88`/`0x801F8E90`), so
    // ids `0xB0..=0xB2` share `0x801F69D8`.
    CaptureTrampoline {
        prot_entry: 964,
        trampoline: 0x801F_8E3C,
        arms: &[
            (0xAF, ELEMENT_CHANGE_TICK),
            (0xB0, 0x801F_69D8),
            (0xB1, 0x801F_69D8),
            (0xB2, 0x801F_69D8),
        ],
    },
    CaptureTrampoline {
        prot_entry: 965,
        trampoline: 0x801F_7B1C,
        arms: &[(0xB6, 0x801F_69D8)],
    },
];

/// PROT 0958's tick body, the arm its trampoline reaches for action `0x79`.
pub const BLAZING_SLASH_TICK: u32 = 0x801F_6DD8;
/// PROT 0942's `0x52` arm - [`power_up_tick`].
pub const POWER_UP_TICK: u32 = 0x801F_7D34;
/// PROT 0964's `0xAF` arm - [`element_change_tick`].
pub const ELEMENT_CHANGE_TICK: u32 = 0x801F_88EC;
/// PROT 0945's `0x54` arm - [`water_column_tick`]. Its `0xBA` arm is a
/// **second** choreography in the same image, which is what makes the
/// `(entry, body)` key load-bearing.
pub const WATER_COLUMN_TICK: u32 = 0x801F_6EDC;
/// PROT 0945's `0xBA` arm - [`all_stats_surge_tick`].
pub const ALL_STATS_SURGE_TICK: u32 = 0x801F_69F8;
/// PROT 0960's `0x7B` arm - [`plasma_strike_tick`]. Its `0xA6` arm (Neo Star
/// Slash) is `0x801F69D8`, the load base, and is unported.
pub const PLASMA_STRIKE_TICK: u32 = 0x801F_74E4;
/// PROT 0957's `0x77` arm - [`summon_effect_tick_a`].
pub const SUMMON_EFFECT_TICK_A: u32 = 0x801F_6A14;
/// PROT 0957's `0x76` arm - [`summon_effect_tick_b`].
pub const SUMMON_EFFECT_TICK_B: u32 = 0x801F_798C;
/// PROT 0952's tick body, the arm its trampoline reaches for action `0xB8`.
pub const ASTRAL_SLASH_TICK: u32 = 0x801F_6A0C;
/// PROT 0938's `0x4E` arm - [`chaos_breath_tick`].
pub const CHAOS_BREATH_TICK: u32 = 0x801F_726C;
/// PROT 0938's `0xB7` arm - [`mystic_circle_tick`].
pub const MYSTIC_CIRCLE_TICK: u32 = 0x801F_69EC;
/// PROT 0951's `0x36` arm - [`chaos_flare_tick`].
pub const CHAOS_FLARE_TICK: u32 = 0x801F_6A20;
/// PROT 0951's `0x5B` arm - [`scythe_wind_tick`].
pub const SCYTHE_WIND_TICK: u32 = 0x801F_77E8;
/// PROT 0952's `0x5C` arm - [`bloody_horns_tick`].
pub const BLOODY_HORNS_TICK: u32 = 0x801F_7118;
/// PROT 0965's `0xB6` arm - [`doomsday_tick`].
pub const DOOMSDAY_TICK: u32 = 0x801F_69D8;
/// PROT 0955's `0x60` arm - [`white_shield_tick`].
pub const WHITE_SHIELD_TICK: u32 = 0x801F_8F0C;
/// PROT 0955's `0x6E` arm - [`kiss_of_death_tick`].
pub const KISS_OF_DEATH_TICK: u32 = 0x801F_86A4;
/// PROT 0955's `0x6F` arm - [`melt_spray_tick`].
pub const MELT_SPRAY_TICK: u32 = 0x801F_7FA4;
/// PROT 0955's `0x70` arm - [`terror_scream_tick`].
pub const TERROR_SCREAM_TICK: u32 = 0x801F_767C;
/// PROT 0955's `0x72` arm - [`power_charge_tick`].
pub const POWER_CHARGE_TICK: u32 = 0x801F_7158;
/// PROT 0955's `0x73` arm - [`void_accessories_tick`].
pub const VOID_ACCESSORIES_TICK: u32 = 0x801F_6A28;
/// The phase PROT 0955's Melt Spray runs its five-stat debuff on
/// (`0x801F828C`, the `slt v1, 4` arm of its chain).
pub const MELT_SPRAY_DEBUFF_ARM: u8 = 3;
/// The phase PROT 0938's Chaos Breath runs its sweep on (`0x801F7750`).
pub const CHAOS_BREATH_SWEEP_ARM: u8 = 2;
/// The phase PROT 0938's Mystic Circle runs its sweep on: table word 3
/// (`0x801F69E4` -> `0x801F6F3C`), the arm the loop at `0x801F70B0` sits in.
pub const MYSTIC_CIRCLE_SWEEP_ARM: u8 = 3;
/// The phase PROT 0965's Doomsday runs its sweep on: the `beq v1, 0x0C` /
/// `slt` pair at `0x801F6AE8` sends `0x0B` to `0x801F7648`, and the loop at
/// `0x801F76E4` sits inside that arm.
pub const DOOMSDAY_SWEEP_ARM: u8 = 0x0B;

/// The trampoline of the module PROT `prot_entry` pages, if it has one.
pub fn capture_trampoline_for(prot_entry: u32) -> Option<&'static CaptureTrampoline> {
    CAPTURE_TRAMPOLINES
        .iter()
        .find(|t| t.prot_entry == prot_entry)
}

/// Resolve one capture-class cast through its module's trampoline: which tick
/// body VA the caster's queued action id `caster[+0x1DF]` reaches.
///
/// `None` is retail's fall-through - the trampoline returns `a0 = 0`, the
/// module ticks nothing, and the drive loop proceeds. That is why a
/// "multi-spell cell" is several whole choreographies in one image rather
/// than one body branching internally: PROT 0955 holds **six**, the widest in
/// the band.
pub fn capture_tick_body(prot_entry: u32, action_id: u8) -> Option<u32> {
    capture_trampoline_for(prot_entry).and_then(|t| {
        t.arms
            .iter()
            .find(|(id, _)| *id == action_id)
            .map(|(_, body)| *body)
    })
}

// ---------------------------------------------------------------------------
// Six more tick bodies (`0x801CF4EC` / `0x801CF56C` arm, per-module)
// ---------------------------------------------------------------------------

/// PROT 0925 (Spikefish) tick body.
///
/// Ten phase arms behind `sltiu a0, 0xa` (`0x801F6A68`) through the head
/// table filling file `0x0..0x28`; `$s3 = ctx + 0x279` is materialised in the
/// same breath at `0x801F6A64`. No damage-wrapper call anywhere in its 1082
/// instructions - the module's whole simulation footprint is one staged clip
/// with its restage bump (`0x801F7448`), two `ctx+0x278` writes
/// (`0x801F6AC4` seeds it, `0x801F7AB0` clears it in the terminal arm), and
/// two animation-rate stores.
///
/// Ported: the bound, the phase walk, the stage/restage pair and the
/// `ctx+0x278` discipline. Not ported: the packet arms, which are most of the
/// body.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A00 (phase machine + staging; packet arms unported)
pub fn spikefish_tick(ctx: &mut CastModuleCtx, caster: &mut CastActorState) -> CastTickStep {
    run_tick(ctx, 10, |c| {
        if c.phase == 0 {
            c.ctx_278 = 0;
        }
        if c.phase == SPIKEFISH_STAGE_ARM {
            stage_clip(caster, caster.staged_anim);
        }
        false
    })
}

/// The arm PROT 0925 stages its clip from (`sb t0, 0x1da(v0)` at
/// `0x801F7448`, inside the arm the head table's word 5 reaches).
pub const SPIKEFISH_STAGE_ARM: u8 = 5;

/// PROT 0924 (Ultimate Rave) tick body.
///
/// Twelve phase arms behind `sltiu a1, 0xc` (`0x801F6AA8`) through the table
/// at `0x801F69E8` - base `+0x10`, i.e. the head table starts four words in,
/// which is why a reader that assumes file `+0` misses it. Five `+0x1DA`
/// stages against four restage bumps (the unpaired one is `0x801F7350`,
/// which writes the staged clip and the animation rate in the same pair of
/// instructions), ten `+0x21D` animation-rate stores, three `ctx+0x278`
/// writes and no damage wrapper.
///
/// Its one HP write is not a hit: `sh zero, 0x14c(s3)` at `0x801F76A0` zeroes
/// the victim's HP outright in the finale arm, next to `+0x225` / `+0x21C`
/// render flags - the seat-0 "declare the victim dead" shape the module docs
/// name, not a roll through a wrapper.
///
/// Ported: the bound, the phase walk, the stage/restage pairs and the finale
/// HP zero. Not ported: the packet and camera arms.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A18 (phase machine + staging + the finale HP zero; packet
/// arms unported)
pub fn ultimate_rave_tick(
    ctx: &mut CastModuleCtx,
    caster: &mut CastActorState,
    victim: &mut CastActorState,
) -> CastTickStep {
    run_tick(ctx, 12, |c| {
        if c.phase == ULTIMATE_RAVE_FINALE_ARM {
            victim.hp = 0;
            victim.render_flag = 2;
            victim.anim_rate = 2;
            let knockdown = victim.knockdown_anim;
            stage_clip(victim, knockdown);
        } else {
            stage_clip(caster, caster.staged_anim);
        }
        false
    })
}

/// The arm PROT 0924 zeroes the victim's HP in (`sh zero, 0x14c(s3)` at
/// `0x801F76A0`).
pub const ULTIMATE_RAVE_FINALE_ARM: u8 = 9;

/// PROT 0922 (Puera) tick body - 2474 instructions, the band's longest.
///
/// Twenty-five phase arms behind `sltiu a0, 0x19` (`0x801F6AB4`) through the
/// head table at file `+0`. One `FUN_801DD0AC` site at `0x801F8E1C` with the
/// baked power `0x12` and `a1 = 7` (the shared kernel's summon branch), under
/// the same two guards the band's AoE stagers use - skip a dead seat
/// (`+0x14C == 0`) and skip a non-targetable one (`+0x16E & 4`) - followed by
/// the **shape-A** clamp at `0x801F8E48` (`sltu a0, s1`, unsigned).
///
/// That corrects a reading `docs/subsystems/cast-module.md` could be taken to
/// imply: PROT 0927's *stager* clamps to `HP - 1` and cannot kill, but the
/// summon-branch wrapper is not itself a never-kill shape - this module calls
/// it with the kill-capable clamp.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A3C (phase machine + damage/staging; packet arms unported)
pub fn puera_tick(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
    hit: Option<i32>,
) -> CastTickStep {
    run_tick(ctx, 25, |_| {
        if let Some(roll) = hit
            && aoe_seat_is_hittable(victim)
        {
            apply_hit_floor_zero(victim, roll);
        }
        false
    })
}

/// PROT 0927 (Juggernaut) tick body - the sibling of the AoE stager
/// [`juggernaut_stager`] and a **second, differently clamped** damage site in
/// the same module.
///
/// Twenty-nine phase arms behind `sltiu a1, 0x1d` (`0x801F6B04`) through the
/// table at `0x801F69E8`. Its `FUN_801DD0AC` site is `0x801F7E0C`, same baked
/// `0x12` and `a1 = 7` as the stager, but the clamp at `0x801F7E38` is
/// `sltu a0, s1` - **shape A**, which can kill - where the stager's
/// `0x801F85C4` clamp is the signed `HP - 1` shape that cannot.
///
/// So "PROT 0927 never kills" is true of its move-VM sweep only. The tick's
/// hit is kill-capable, and a negative wrapper return there kills outright
/// through the unsigned compare.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A84 (phase machine + damage/staging; packet arms unported)
pub fn juggernaut_tick(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
    hit: Option<i32>,
) -> CastTickStep {
    run_tick(ctx, 29, |_| {
        if let Some(roll) = hit
            && aoe_seat_is_hittable(victim)
        {
            apply_hit_floor_zero(victim, roll);
        }
        false
    })
}

/// PROT 0918 (Kemaro) tick body.
///
/// The one body in this set whose head is a `beq`/`slti` chain rather than a
/// word table (`0x801F6D08` onward); its phase literals run `1 ..= 0x14` plus
/// the `0xFF` done marker. One `FUN_801DD0AC` site at `0x801F87A4`, and its
/// power is **not** loaded as its own literal - the arm gate
/// `addiu v0, zero, 0x12; bne v1, v0` compares the phase byte against `0x12`
/// and then reuses the same register as `a0` (`move a0, v0` at `0x801F8798`),
/// so the phase number and the baked power are one constant. A reader that
/// takes `move a0, v0` at face value loses the power entirely.
///
/// Shape-A clamp at `0x801F87C8`, spelled the other way round
/// (`sltu s1, a1` - damage below HP branches *past* the clamp). The arm that
/// does clamp also credits a kill: it increments the word at `+0x664` of the
/// caster's per-character record in the `0x80084140 + n * 0x414` block.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6C70 (phase chain + damage/staging + the kill credit; packet
/// arms unported)
pub fn kemaro_tick(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
    hit: Option<i32>,
) -> Option<CastTickStep> {
    if ctx.phase == KEMARO_DONE_PHASE {
        return Some(CastTickStep::Done);
    }
    if ctx.phase > KEMARO_LAST_PHASE {
        return None;
    }
    if ctx.phase == KEMARO_DAMAGE_PHASE
        && let Some(roll) = hit
    {
        apply_hit_floor_zero(victim, roll);
    }
    advance_phase(ctx);
    Some(CastTickStep::Busy)
}

/// PROT 0918's damage arm, and - the same constant - the `a0` its
/// `FUN_801DD0AC` site bakes.
pub const KEMARO_DAMAGE_PHASE: u8 = 0x12;
/// The widest phase literal PROT 0918's `beq`/`slti` chain compares.
pub const KEMARO_LAST_PHASE: u8 = 0x14;
/// The band's "choreography done" phase marker.
pub const KEMARO_DONE_PHASE: u8 = 0xFF;

/// PROT 0949 (Water Crystals) tick body - the sibling of the freeze-ramp
/// stager [`water_crystals_stager`].
///
/// Six phase arms behind `sltiu v1, 6` (`0x801F6AA0`) through the head table
/// at file `+0`, and the victim is derived the band's way in the prologue
/// (`caster[+0x1DD]` at `0x801F6A5C`, then `actor_table[that]`). One
/// `FUN_801DD4B0` site at `0x801F7318` with the baked power `0xC0`
/// (`0x801F72F8`), then the shape-A clamp at `0x801F733C`, the `+0x10`
/// accumulate, the HP write, a `+0x1DC = 1` restage store and the victim's
/// own `+0x1F1` reaction stage.
///
/// `0xC0` is a power `docs/subsystems/cast-module.md`'s baked-constant table
/// does not carry: that table was read off the routines already on the
/// verdict table, and this tick was not one of them.
///
/// Wired: `World::run_cast_module_code`.
///
/// PORT: FUN_801F6A10 (phase machine + damage/staging; packet arms unported)
pub fn water_crystals_tick(
    ctx: &mut CastModuleCtx,
    victim: &mut CastActorState,
    hit: Option<i32>,
) -> CastTickStep {
    run_tick(ctx, 6, |_| {
        if let Some(roll) = hit {
            apply_hit_floor_zero(victim, roll);
            victim.restage = 1;
            let knockdown = victim.knockdown_anim;
            victim.staged_anim = knockdown;
        }
        false
    })
}
