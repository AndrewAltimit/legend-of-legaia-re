//! PROT 0966 (Cort's Evil Seru Magic) tick body `0x801F6A74`: the half of
//! each arm that writes the seats. The camera, the countdown and the phase
//! chain are [`crate::cast_module_camera::evil_seru_magic_camera`], which
//! decides whether an arm passes; this is what the arm then does to the
//! party row `actor_table[0 .. ctx[+0]]` and the caster `actor_table[ctx+0x13]`.

use super::{
    CastActorState, CastDamageShape, CastWrapper, StagerCaller, StagerSpawn, SweepHit,
    aoe_seat_is_hittable, apply_hit_floor_zero, stage_clip,
};

/// The neutral tint word the body shows a seat with (`lui 0x2008; ori 0x200`).
pub const ESM_NEUTRAL_TINT: u32 = 0x2008_0200;

/// The `+0x21C` value that hides a seat (`li 0xFF; sb 0x21c`).
pub const ESM_HIDDEN: u8 = 0xFF;

/// The arm-26 party hit's damage shape: `FUN_801DD4B0(0x327, ctx[+0x13],
/// seat)` at `0x801F8610`, the unsigned clamp to HP at `0x801F863C` - shape A,
/// so it **kills**. It is a separate hit from the module's move-VM stager
/// (`0x801F8D64`, power `0x100`, the never-kill clamp), which the band's fold
/// still applies.
pub const EVIL_SERU_MAGIC_SWEEP_SHAPE: CastDamageShape = CastDamageShape {
    prot_entry: 966,
    routine: 0x801F_6A74,
    wrapper: CastWrapper::Respect,
    never_kills: false,
    powers: &[0x327],
};

/// The first of the four arms (`12..=15`, `0x801F73E4` / `0x801F75C8` /
/// `0x801F7798` / `0x801F7964`) that open on the party release loop.
pub const ESM_RELEASE_ARM_FIRST: u8 = 12;
/// The last arm carrying the release loop.
pub const ESM_RELEASE_ARM_LAST: u8 = 15;

/// Whether the release loop of arms `12..=15` cuts this party seat's get-up
/// short this pass.
///
/// Each of the four arms opens on the same walk over the party row (bound
/// `ctx[+0]`), ahead of its countdown, so it runs on every pass the arm
/// holds:
///
/// ```text
/// actor = table[i]
/// if actor[+0x1D9] == 0                      { next }     ; idle
/// if actor[+0x1D9] == actor[+0x1F2]                       ; the get-up
///    && (node[+0x68] << 16 >> 20) >= stream[1] - 2:       ; two frames left
///     actor[+0x1DA] = 0; actor[+0x1DC] += 1               ; stage idle, bump
/// ```
///
/// (`0x801F7418..0x801F7490` in arm 12; the copies at `0x801F75FC`,
/// `0x801F77CC`, `0x801F7998`). The test reads the cursor the module sees,
/// before the pass's anim tick, which then advances it once more and takes
/// the bump's bit 0 to the cut (`FUN_80047430` `0x800478F4` ->
/// `0x80047A3C`): the get-up ends on keyframe `frames - 1`, a pass ahead of
/// its natural end, so the effect record gated on `frames + 1` never fires.
/// The Evil Seru Magic recording stores the release at `0x801F765C` /
/// `0x801F7674` with `stream[1] - 2` = `23`, `26`, `13` for the three seats'
/// 25-, 28- and 15-keyframe get-ups, and the cut at `0x80047A40` the same
/// pass. The loop's other half, the `+0x38` slide by `dt *
/// 0x801FA460[id]` and Noa's `-0x10` on release, is [`esm_get_up_slide`].
// PORT: FUN_801F6A74, overlay_cast_evil_seru_magic_0966_801f6a74 (PROT 0966; `0x801F7418..0x801F7490`, the arms 12..15 release loop)
pub fn esm_releases_get_up(s: &CastActorState) -> bool {
    s.playing_anim != 0
        && s.playing_anim == s.reaction_gate
        && i32::from(s.anim_frame) >= i32::from(s.anim_frames) - 2
}

/// Arm 0's row depth: every party seat stands at `+0x38 = 0x190`
/// (`li v0,0x190` / `sh v0,0x38` at `0x801F6C64` / `0x801F6C68`).
pub const ESM_ROW_Z: i16 = 0x190;

/// The render scale arm 0 gives each party seat, and arm 21 the caster:
/// node `+0x72 = 0x800`, half size (`0x801F6CA0`, `0x801F7EB4`); arm 27 puts
/// `0x1000` back (`0x801F886C`, `0x801F8904`).
pub const ESM_ROW_SCALE: u16 = 0x800;

/// Arm 21's caster seat: `+0x38 = 0x400`, `+0x34 = 0`, `+0x36 = -0x380`,
/// `+0x46 = 0x800` (`0x801F7E80..0x801F7EA4`), above the row and facing it.
/// The recording's caster stands at `z = 1024` from arm 21 on.
pub const ESM_CASTER_Y: i16 = -0x380;
/// Arm 21's caster depth (`li v0,0x400` / `sh v0,0x38(s2)` at `0x801F7E80`).
pub const ESM_CASTER_Z: i16 = 0x400;
/// Arm 21's caster heading.
pub const ESM_CASTER_FACING: u16 = 0x800;

/// The cast whose `summon.dat` group holds the creature arm 10 seats:
/// Juggernaut's (`0x99`, group `72`), the body PROT 0966 shares its arm
/// layout and stager with (PROT 0927). The seat installer `FUN_801F19EC`
/// (`jal` at `0x801F70E4`) installs the streamed actor record as seat 7,
/// and the recording's caption reads "Juggernaut".
pub const ESM_CREATURE_SPELL: u8 = 0x99;

/// Arm 10's creature seat after the installer: `+0x38 = 0x1C0`, `+0x34 =
/// 0`, `+0x46 = 0x800`, rate `+0x21D = 0` (`0x801F7134..0x801F7148`), the
/// node's floor clip `+0x42 = 2` (`0x801F7110`) and object-effect row 1's
/// angles zero with bounds `(-0x300, 0)` (`0x801F7114..0x801F7130`).
pub const ESM_CREATURE_Z: i16 = 0x1C0;
/// Arm 10's creature heading.
pub const ESM_CREATURE_FACING: u16 = 0x800;

/// Arm 18's creature seat on its pass: `+0x36 = 0x100`, `+0x38 = 0x800`
/// (`0x801F7BF4..0x801F7BFC`), beside object-effect row 1's angles
/// `(0x400, 0, 0)` and bounds `(-0x800, 0x800)` (`0x801F7BD0..0x801F7BF0`).
pub const ESM_CREATURE_RISE: (i16, i16) = (0x100, 0x800);

/// The creature's clip and rate stores of one pass of arm `phase`
/// (`s4 = actor_table[7]`, loaded at `0x801F6AEC`), on a view of seat 7.
/// `cursor` is the creature node's clip cursor `+0x68` the module reads,
/// `delta` the frame delta `0x1F800393` the call drains (the engine ticks
/// the body once a vsync, so `1`). Returns the depth the arm walks the
/// creature by this call.
///
/// | arm | every pass | on pass |
/// |---|---|---|
/// | 10 | - | rate `0` behind the installer (`0x801F7140`) |
/// | 11 | - | rate `8` (`0x801F73E0`) |
/// | 12 | rate `0` once `+0x68 >= 0x240` (`0x801F7510..0x801F752C`) | - |
/// | 13 | - | rate `8`, `+0x1DA` and `+0x1DC` stepped (`0x801F7774..0x801F7794`) |
/// | 14 | rate `0` once `+0x68 >= 0x2D0` (`0x801F78C4..0x801F78E0`) | - |
/// | 18 | - | rate `8`, `+0x1DA` and `+0x1DC` stepped (`0x801F7BA4..0x801F7BCC`) |
/// | 19 | `+0x38 -= 3 * delta`, `+0x1DA = 3` bare (`0x801F7C94..0x801F7CAC`) | - |
/// | 23 | - | rate `2`, `+0x1DA` and `+0x1DC` stepped (`0x801F82A8..0x801F82C4`) |
/// | 24 | `+0x1DA = 5` bare (`0x801F82DC`) | - |
///
/// The installer leaves rate `2`; arm 10 stores `0` on the same pass, and
/// arm 11 holds it there until its own pass, so a host that seats the
/// creature behind the pass stores `0` on every pass of both arms ahead of
/// arm 11's.
// PORT: FUN_801F6A74, overlay_cast_evil_seru_magic_0966_801f6a74 (PROT 0966; the seat-7 clip, rate and walk stores of arms 10..24)
pub fn esm_creature_writes(
    phase: u8,
    passed: bool,
    cursor: i32,
    delta: u8,
    s: &mut CastActorState,
) -> i16 {
    match phase {
        10 => s.anim_rate = 0,
        11 if !passed => s.anim_rate = 0,
        12 if cursor >= 0x240 => s.anim_rate = 0,
        14 if cursor >= 0x2D0 => s.anim_rate = 0,
        19 => s.staged_anim = 3,
        24 => s.staged_anim = 5,
        _ => {}
    }
    if passed {
        match phase {
            11 => s.anim_rate = 8,
            13 | 18 | 23 => {
                s.anim_rate = if phase == 23 { 2 } else { 8 };
                let next = s.staged_anim.wrapping_add(1);
                stage_clip(s, next);
            }
            _ => {}
        }
    }
    if phase == 19 { 3 * i16::from(delta) } else { 0 }
}

/// The arms 12..15 slide a pass by character id, the bytes `0x801FA460`
/// (`lbu` off `t0 = 0x801FA460` indexed by `DAT_8007BD10[seat]`,
/// `0x801F74B8..0x801F74C8`): Vahn and Noa `1`, Gala `2`.
pub const ESM_GET_UP_SLIDE: [u8; 4] = [0, 1, 1, 2];

/// What Noa's release takes off her row depth (`addiu v0,v0,-0x10` at
/// `0x801F74B4`, behind `li v0,0x2` / `bne` on her character id).
pub const ESM_NOA_RELEASE_PULL: i16 = 0x10;

/// The party action entry whose end-of-clip step arm 0 halves: the sixth
/// pointer of each member's entry table `0x801C9360[i]` (`lw v0,0x14(v0)`
/// at `0x801F6CD0`), its `+0x0E` halfword (`lhu` / `sll 16` / `sra 0x11`,
/// `0x801F6CD8..0x801F6D04`): the displacement the commit paths apply in
/// proportion to how far the clip got, halved with the model.
pub const ESM_END_STEP_ENTRY: usize = 5;

/// Arm 0's half of an entry's end-of-clip step: an arithmetic shift, so a
/// negative step rounds toward minus infinity.
pub const fn esm_half_end_step(step: i16) -> i16 {
    step >> 1
}

/// Arm 0's row column for party seat `i` of `n`: the formation point's x
/// `*(0x800775E0 + (n * 3 + i) * 8)` over three, toward zero
/// (`mult 0x2AAAAAAB` / `mfhi` less the sign, `0x801F6C20..0x801F6C5C`).
/// The other three writes of the loop are fixed: `+0x38 = ESM_ROW_Z`,
/// `+0x46 = 0`, node `+0x72 = ESM_ROW_SCALE` (and the node's radius `+0x58`
/// halved). The pair, heading and radius go to `0x801FA474` / `0x801FA494`
/// for arm 27's restore, which arm 28 repeats every pass.
// PORT: FUN_801F6A74, overlay_cast_evil_seru_magic_0966_801f6a74 (PROT 0966; `0x801F6BE0..0x801F6D1C`, arm 0's party row)
pub fn esm_row_x(formation_x: i16) -> i16 {
    (i32::from(formation_x) / 3) as i16
}

/// The arms 12..15 loop's depth step for one party seat this pass (it runs
/// ahead of the countdown, so on every pass the arm holds): nothing on an
/// idle seat; on a seat [`esm_releases_get_up`] releases, Noa's `-0x10` and
/// nothing for the others; on any other clip `frame_step *
/// ESM_GET_UP_SLIDE[char_id]` (`0x801F7428..0x801F74E8`).
// PORT: FUN_801F6A74, overlay_cast_evil_seru_magic_0966_801f6a74 (PROT 0966; `0x801F7494..0x801F74E8`, the arms 12..15 slide)
pub fn esm_get_up_slide(s: &CastActorState, char_id: u8, frame_step: u8) -> i16 {
    if s.playing_anim == 0 {
        return 0;
    }
    if esm_releases_get_up(s) {
        return if char_id == 2 {
            -ESM_NOA_RELEASE_PULL
        } else {
            0
        };
    }
    let per = ESM_GET_UP_SLIDE
        .get(usize::from(char_id))
        .copied()
        .unwrap_or(0);
    i16::from(frame_step) * i16::from(per)
}

/// The seat writes of one pass of PROT 0966's arm `phase`. `passed` is
/// whether the arm's countdown gate let it through this pass (the camera
/// director's answer); the writes an arm makes before its gate run either
/// way.
///
/// | arm | every pass | on pass |
/// |---|---|---|
/// | 0 | party `+0x04 = 0`, `+0x21C = 0xFF` (`0x801F6C80` / `0x801F6C8C`) | - |
/// | 1 | the same, again (`0x801F6D8C` / `0x801F6D98`) | caster stages clip `6`, rate `2` (`0x801F6DE4` / `0x801F6DF0`) |
/// | 10 | - | caster hidden (`0x801F70E0` / `0x801F70E8`); party shown, neutral tint (`0x801F7310` / `0x801F731C`) |
/// | 12..15 | a party seat on its get-up `+0x1F2` with two keyframes left stages idle with the bump ([`esm_releases_get_up`]) | - |
/// | 18 | - | party hidden again (`0x801F7C34` / `0x801F7C40`) |
/// | 21 | - | caster stages clip `0`, shown, neutral tint (`0x801F7EA8..0x801F7EC8`); party shown (`0x801F7EF8` / `0x801F7F04`) |
/// | 23 | - | caster `+0x21C = 2`, the defeat-fade tint state (`0x801F8280`) |
/// | 26 | - | the party hit, see [`EVIL_SERU_MAGIC_SWEEP_SHAPE`] |
/// | 27 | party rate `2` (`0x801F8718`), then `0` on a seat whose committed clip `+0x1D9` is not its knockdown `+0x1F1` (`beq` at `0x801F8730` over the store at `0x801F8738`) | caster shown, neutral tint (`0x801F890C` / `0x801F8910`) |
/// | 28 | party rate `2` (`0x801F89E8`) | - |
///
/// The hit (`0x801F85D4..0x801F86C8`) walks the party row, skips a dead or
/// non-targetable seat, takes `roll(seat)` as the wrapper's signed net, clamps
/// it to the live HP, and stages the seat's own knockdown `+0x1F1` at rate
/// `2` - the stager's writes with the other clamp.
///
/// The seat moves (arm 0's half-size row, the arms 12..15 slide, arm 21's
/// caster seat, the arm 27 / 28 restore) are the host's, over these
/// kernels' constants ([`esm_row_x`], [`esm_get_up_slide`],
/// [`esm_half_end_step`]). The creature seat 7's stores are
/// [`esm_creature_writes`]. Not carried here: the effect records and the
/// arm-20 banner (the screen fades are the band seam's,
/// `World::run_cast_module_code`).
///
/// PORT: FUN_801F6A74, overlay_cast_evil_seru_magic_0966_801f6a74 (PROT 0966; the party and caster writes and the arm-26 hit)
pub fn evil_seru_magic_seat_writes(
    phase: u8,
    passed: bool,
    party_count: u8,
    caster: u8,
    seats: &mut [CastActorState],
    mut roll: impl FnMut(u8) -> i32,
) -> Vec<SweepHit> {
    let mut hits = Vec::new();
    let party = party_count as usize;
    let each_party = |seats: &mut [CastActorState], f: &mut dyn FnMut(&mut CastActorState)| {
        for s in seats.iter_mut().take(party) {
            f(s);
        }
    };
    let hide = |s: &mut CastActorState| {
        s.present_04 = 0;
        s.render_flag = ESM_HIDDEN;
    };
    let show = |s: &mut CastActorState| {
        s.present_04 = ESM_NEUTRAL_TINT;
        s.render_flag = 0;
    };
    match phase {
        0 | 1 => each_party(seats, &mut |s| hide(s)),
        27 => each_party(seats, &mut |s| {
            s.anim_rate = if s.playing_anim == s.knockdown_anim {
                2
            } else {
                0
            };
        }),
        28 => each_party(seats, &mut |s| s.anim_rate = 2),
        ESM_RELEASE_ARM_FIRST..=ESM_RELEASE_ARM_LAST => each_party(seats, &mut |s| {
            if esm_releases_get_up(s) {
                stage_clip(s, 0);
            }
        }),
        _ => {}
    }
    if !passed {
        return hits;
    }
    let caster_seat = |seats: &mut [CastActorState], f: &dyn Fn(&mut CastActorState)| {
        if let Some(c) = seats.get_mut(caster as usize) {
            f(c);
        }
    };
    match phase {
        1 => caster_seat(seats, &|c| {
            stage_clip(c, 6);
            c.anim_rate = 2;
        }),
        10 => {
            caster_seat(seats, &|c| hide(c));
            each_party(seats, &mut |s| show(s));
        }
        18 => each_party(seats, &mut |s| hide(s)),
        21 => {
            caster_seat(seats, &|c| {
                stage_clip(c, 0);
                show(c);
            });
            each_party(seats, &mut |s| show(s));
        }
        23 => caster_seat(seats, &|c| c.render_flag = 2),
        26 => {
            for seat in 0..party_count {
                let Some(v) = seats.get_mut(seat as usize) else {
                    break;
                };
                if !aoe_seat_is_hittable(v) {
                    continue;
                }
                let applied = apply_hit_floor_zero(v, roll(seat));
                let knockdown = v.knockdown_anim;
                stage_clip(v, knockdown);
                v.anim_rate = 2;
                hits.push(SweepHit { seat, applied });
            }
        }
        27 => caster_seat(seats, &|c| show(c)),
        _ => {}
    }
    hits
}

/// The records whose programs call the stager's two sweep arms, with the
/// tick arm on whose pass each call falls when no part runs the program
/// (`0x801F937C`: `WAIT 0x7F`, op `0x20` arm 4; `0x801F94D0`: `WAIT 0x187`,
/// op `0x20` arm 3). Arm 10 seats both (`0x801F7164`, `0x801F71C4`).
pub const EVIL_SERU_MAGIC_SWEEP_CALLERS: [(u32, u8); 2] = [(0x801F_937C, 11), (0x801F_94D0, 13)];

/// The records PROT 0966's stager `FUN_801F8D64` arm `arm` seats on its
/// caller (`a0 = part + 0x14`, `a1 = part + 0x24`, scale `0x1000`, every call
/// through the pool wrapper `FUN_80050ED4`), behind `sltiu a1, 9` and the
/// table at `0x801F6A50`:
///
/// | arm | records |
/// |---|---|
/// | 0 (`0x801F8DAC`) | `0x801F9538` |
/// | 1 (`0x801F8DC0`) | `0x801F93A0` |
/// | 2 (`0x801F8DD4`) | `0x801F96E0`, `0x801F9778`, `0x801F97BC`, `0x801F9808` |
/// | 6 (`0x801F9024`) | `0x801F98D0` |
/// | 8 (`0x801F910C`) | `0x801FA2FC`, `0x801FA340` |
///
/// The other arms are PROT 0927's, instruction for instruction over this
/// module's own words: arm 3 (`0x801F8E38`) adds `0xC` to the shake word
/// `0x801FA468` and falls into arm 4 (`0x801F8E4C`), which adds `0x14`, spawns
/// the `0x40`-vsync salmon fade and sweeps the party row
/// ([`super::evil_seru_magic_stager`]); arm 5 (`0x801F8FD0`) spawns the
/// `0x100`-vsync fade; arm 7 (`0x801F9038`) jitters the caller's `+0x84` /
/// `+0x28` ([`super::juggernaut_stager_fade`], [`super::juggernaut_stager_shake`],
/// [`super::juggernaut_stager_jitter`]).
///
/// So the cast lands **two** never-kill sweeps before arm 26's hit: record
/// `0x801F937C` calls arm 4 `0x7F` vsyncs after arm 10 seats it, and record
/// `0x801F94D0` calls arm 3 `0x187` vsyncs after.
///
/// PORT: FUN_801F8D64 (PROT 0966 stager; the spawn arms 0, 1, 2, 6 and 8)
/// REF: FUN_80050ED4
pub fn evil_seru_magic_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    let records: &[u32] = match arm {
        0 => &[0x801F_9538],
        1 => &[0x801F_93A0],
        2 => &[0x801F_96E0, 0x801F_9778, 0x801F_97BC, 0x801F_9808],
        6 => &[0x801F_98D0],
        8 => &[0x801F_A2FC, 0x801F_A340],
        _ => &[],
    };
    records
        .iter()
        .map(|&record| StagerSpawn {
            record,
            pos: caller.pos,
            rot: caller.rot,
            pokes: Vec::new(),
            patches: Vec::new(),
            halve_speed: false,
            scale: 0x1000,
        })
        .collect()
}

#[cfg(test)]
mod esm_tests {
    use super::*;

    fn seat(hp: u16) -> CastActorState {
        CastActorState {
            hp,
            knockdown_anim: 0x0B,
            anim_rate: 8,
            present_04: ESM_NEUTRAL_TINT,
            ..Default::default()
        }
    }

    /// Arm 0's row takes a third of the formation point's x, toward zero,
    /// and the arms 12..15 slide steps a struck seat back by character.
    #[test]
    fn the_row_and_the_get_up_slide() {
        assert_eq!(esm_row_x(600), 200);
        assert_eq!(esm_row_x(-600), -200);
        assert_eq!(esm_row_x(-301), -100);
        // The entry-5 end step halves on an arithmetic shift.
        assert_eq!(esm_half_end_step(64), 32);
        assert_eq!(esm_half_end_step(-3), -2);
        let mut s = seat(100);
        // Idle slides nothing.
        assert_eq!(esm_get_up_slide(&s, 3, 4), 0);
        // A reaction slides by the frame step times the character's byte.
        s.playing_anim = 0x0B;
        assert_eq!(esm_get_up_slide(&s, 1, 4), 4);
        assert_eq!(esm_get_up_slide(&s, 3, 4), 8);
        // A released get-up pulls Noa forward and leaves the others.
        s.playing_anim = 0x0C;
        s.reaction_gate = 0x0C;
        s.anim_frames = 25;
        s.anim_frame = 23;
        assert!(esm_releases_get_up(&s));
        assert_eq!(esm_get_up_slide(&s, 2, 4), -ESM_NOA_RELEASE_PULL);
        assert_eq!(esm_get_up_slide(&s, 1, 4), 0);
        // Short of its last two keyframes the get-up still slides.
        s.anim_frame = 20;
        assert_eq!(esm_get_up_slide(&s, 2, 2), 2);
    }

    #[test]
    fn the_stager_seats_its_records_on_the_caller() {
        let caller = StagerCaller {
            pos: [0, -768, 2048],
            rot: [1, 2, 3],
            ..Default::default()
        };
        let s = evil_seru_magic_stager_spawns(2, caller);
        assert_eq!(
            s.iter().map(|s| s.record).collect::<Vec<_>>(),
            [0x801F_96E0, 0x801F_9778, 0x801F_97BC, 0x801F_9808]
        );
        assert!(
            s.iter()
                .all(|s| s.pos == caller.pos && s.rot == caller.rot && s.scale == 0x1000)
        );
        assert_eq!(evil_seru_magic_stager_spawns(8, caller).len(), 2);
        // The sweep, fade and jitter arms seat nothing.
        for arm in [3, 4, 5, 7, 9] {
            assert!(evil_seru_magic_stager_spawns(arm, caller).is_empty());
        }
        // Both sweep callers are records arm 10 seats.
        assert_eq!(EVIL_SERU_MAGIC_SWEEP_CALLERS.map(|c| c.1), [11, 13]);
    }

    /// Arm 27 keeps a struck seat's knockdown playing at rate `2` and
    /// freezes only a seat that is on another clip: a retail recording walks
    /// the party's cursors through the arm and reads rate `0` from each
    /// clip's end.
    #[test]
    fn arm_27_freezes_only_a_seat_off_its_knockdown() {
        let mut seats = vec![seat(500), seat(500), seat(500), seat(9000)];
        seats[0].playing_anim = 0x0B;
        seats[1].playing_anim = 0;
        evil_seru_magic_seat_writes(27, false, 3, 3, &mut seats, |_| 0);
        assert_eq!(seats[0].anim_rate, 2);
        assert_eq!(seats[1].anim_rate, 0);
        assert_eq!(seats[3].anim_rate, 8, "the caster is outside the row");
    }

    /// Arms 12..15 stage idle with the bump on a seat whose get-up `+0x1F2`
    /// has two keyframes left, held or passed; the Evil Seru Magic
    /// recording's Gala (28 keyframes) is released on the pass its module
    /// reads keyframe 26, Vahn (25) on 23.
    #[test]
    fn arms_12_to_15_release_a_get_up_two_frames_short() {
        let get_up = |frame: i16, frames: u8| CastActorState {
            playing_anim: 5,
            staged_anim: 5,
            reaction_gate: 5,
            anim_frame: frame,
            anim_frames: frames,
            ..seat(500)
        };
        for arm in ESM_RELEASE_ARM_FIRST..=ESM_RELEASE_ARM_LAST {
            for passed in [false, true] {
                let mut seats = vec![get_up(26, 28), get_up(22, 25), get_up(23, 25), seat(9000)];
                seats[3].playing_anim = 5;
                seats[3].reaction_gate = 5;
                seats[3].anim_frames = 25;
                seats[3].anim_frame = 24;
                evil_seru_magic_seat_writes(arm, passed, 3, 3, &mut seats, |_| 0);
                assert_eq!((seats[0].staged_anim, seats[0].restage), (0, 1));
                assert_eq!(
                    (seats[1].staged_anim, seats[1].restage),
                    (5, 0),
                    "three left"
                );
                assert_eq!((seats[2].staged_anim, seats[2].restage), (0, 1));
                assert_eq!(seats[3].restage, 0, "the caster is outside the row");
            }
        }
        // Only the get-up is released: the knockdown `+0x1F1` and the idle
        // play on, and the arms either side of the band leave it alone.
        let mut seats = vec![get_up(26, 28)];
        seats[0].playing_anim = 4;
        evil_seru_magic_seat_writes(13, true, 1, 3, &mut seats, |_| 0);
        assert_eq!(seats[0].restage, 0);
        let mut seats = vec![get_up(26, 28)];
        evil_seru_magic_seat_writes(16, true, 1, 3, &mut seats, |_| 0);
        assert_eq!(seats[0].restage, 0);
    }

    #[test]
    fn arm_26_hits_the_party_and_can_kill() {
        let mut seats = vec![seat(500), seat(0), seat(40), seat(9000)];
        let hits = evil_seru_magic_seat_writes(26, true, 3, 3, &mut seats, |_| 100);
        assert_eq!(hits.len(), 2, "the dead seat draws nothing");
        assert_eq!(seats[0].hp, 400);
        assert_eq!(seats[2].hp, 0, "shape A clamps to HP and kills");
        assert_eq!(seats[0].staged_anim, 0x0B);
        assert_eq!(seats[0].anim_rate, 2);
        assert_eq!(seats[3].hp, 9000, "the caster is outside the party row");
    }

    #[test]
    fn a_held_arm_makes_only_its_pre_gate_writes() {
        let mut seats = vec![seat(500), seat(500), seat(500), seat(9000)];
        assert!(evil_seru_magic_seat_writes(26, false, 3, 3, &mut seats, |_| 100).is_empty());
        assert_eq!(seats[0].hp, 500);
        evil_seru_magic_seat_writes(1, false, 3, 3, &mut seats, |_| 0);
        assert_eq!(seats[0].render_flag, ESM_HIDDEN);
        assert_eq!(
            seats[3].staged_anim, 0,
            "the caster stage waits for the gate"
        );
        evil_seru_magic_seat_writes(1, true, 3, 3, &mut seats, |_| 0);
        assert_eq!(seats[3].staged_anim, 6);
        assert_eq!(seats[3].anim_rate, 2);
    }

    #[test]
    fn the_party_is_shown_again_before_the_body_ends() {
        let mut seats = vec![seat(500), seat(500), seat(500), seat(9000)];
        for (p, passed) in [(0, true), (10, true), (18, true), (21, true)] {
            evil_seru_magic_seat_writes(p, passed, 3, 3, &mut seats, |_| 0);
        }
        assert!(seats[..3].iter().all(|s| s.render_flag == 0));
        assert!(seats[..3].iter().all(|s| s.present_04 == ESM_NEUTRAL_TINT));
        assert_eq!(seats[3].render_flag, 0);
    }

    #[test]
    fn the_creature_plays_the_juggernaut_clips() {
        let mut s = seat(9000);
        s.anim_rate = 2;
        esm_creature_writes(10, true, 0, 4, &mut s);
        assert_eq!(s.anim_rate, 0, "arm 10 freezes the installer's rate");
        esm_creature_writes(11, false, 0, 4, &mut s);
        assert_eq!(s.anim_rate, 0);
        esm_creature_writes(11, true, 0, 4, &mut s);
        assert_eq!(s.anim_rate, 8);
        esm_creature_writes(12, false, 0x23F, 4, &mut s);
        assert_eq!(s.anim_rate, 8);
        esm_creature_writes(12, false, 0x240, 4, &mut s);
        assert_eq!(s.anim_rate, 0);
        esm_creature_writes(13, true, 0x240, 4, &mut s);
        assert_eq!((s.staged_anim, s.restage, s.anim_rate), (1, 1, 8));
        assert_eq!(esm_creature_writes(19, false, 0, 4, &mut s), 12);
        assert_eq!(s.staged_anim, 3, "arm 19 stores clip 3 bare");
        esm_creature_writes(23, true, 0, 4, &mut s);
        assert_eq!((s.staged_anim, s.anim_rate), (4, 2));
        esm_creature_writes(24, false, 0, 4, &mut s);
        assert_eq!(s.staged_anim, 5);
    }
}
