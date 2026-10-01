//! The battle camera's state, glides and per-frame inputs.
//! Split out of `battle_cam_script.rs`.

use super::*;

/// Raw eye-space Z of the swing pose, before the projection prescale.
pub(super) const SWING_TR_Z_RAW: i32 = 0x800;

/// Over-the-shoulder swing pose the submenu exit passes through
/// (trace frames 163..173; yaw target is `4096` = `0` unwrapped upward from
/// `2288`). Retail case `1` orbits the acting actor like the close-up does,
/// so the focus is filled in per swing rather than baked here.
pub(super) const SWING_POSE: BattleCamPose = BattleCamPose {
    pitch: 256.0,
    yaw: 4096.0,
    tr: [0.0, 1536.0, prescale_tr_z(SWING_TR_Z_RAW)],
    focus: [0.0; 3],
};

/// Idle-orbit yaw decrement per camera step (`-4` units per 2 vsyncs
/// = -120 units/s; the mednafen menu state's yaw 3372 is an orbit sample).
pub(super) const ORBIT_STEP: f32 = 4.0;
/// Dialogue-dismiss glide rates (per step, clamped per component).
pub(super) const DIALOGUE_EXIT_PITCH_RATE: f32 = 6.0;
pub(super) const DIALOGUE_EXIT_Z_RATE: f32 = 864.0;
/// Step counts for the linear (arrive-together) glides.
pub(super) const SUBMENU_ENTER_STEPS: u32 = 6;
pub(super) const SUBMENU_SWING_STEPS: u32 = 6;
pub(super) const SWING_RETURN_STEPS: u32 = 7;

/// One glide segment: per-component per-step rates toward `target`, each
/// component clamping independently. `yaw_glides` routes yaw through the
/// glide (shortest-arc, pre-unwrapped into `target.yaw`); when `false` the
/// idle orbit keeps owning yaw during the glide (the dialogue-dismiss law).
#[derive(Debug, Clone, Copy)]
pub(super) struct Glide {
    pub(super) target: BattleCamPose,
    /// Per-step absolute rates, in the order `FUN_801D829C` walks its nine
    /// components: `[pitch, yaw, tr.x, tr.y, tr.z, focus.x, focus.y, focus.z]`
    /// (roll is never driven, so it is dropped).
    pub(super) rate: [f32; 8],
    pub(super) yaw_glides: bool,
    /// `Some(n)`: an arrive-together glide over exactly `n` steps (the final
    /// step lands every component ON the target, so float rounding in the
    /// per-step rates can't leave a residue). `None`: a rate-clamped glide
    /// (each component clamps independently; done when all are at target).
    pub(super) steps_left: Option<u32>,
}

impl Glide {
    /// Linear glide: every component arrives together after `steps` steps.
    ///
    /// The rate table is **retail's**. `build_camera_angle_tween` is
    /// `FUN_801D829C`, and its fourth argument is the glide's duration: it
    /// emits one integer per-frame increment per component
    /// (`ceil(|current - target| / duration)`) plus the 12-bit shortest-arc
    /// adjustment on the rotation pair, which is precisely the arrive-together
    /// law this walker wants. Calling it here means the engine holds one
    /// implementation of the stepping arithmetic instead of two, and steps on
    /// retail's rounding rather than on an exact float divide.
    ///
    /// `target_tr_z_raw` is the target's eye-space Z in **world** units - the
    /// space retail's framing cases pass, and the one input the builder
    /// converts (`(z << 8) / 0xA0`). The converted value is what the glide
    /// converges on, so the caller's `target.tr[2]` is overwritten with it.
    ///
    /// `from` is `&mut` for the same reason retail's `current` side is: the
    /// shortest-arc unwrap can move the *current* angle a full turn rather
    /// than the target, and the pose the walker steps has to see that.
    ///
    /// REF: FUN_801D829C
    pub(super) fn linear(
        from: &mut BattleCamPose,
        mut target: BattleCamPose,
        target_tr_z_raw: i32,
        steps: u32,
        yaw: bool,
    ) -> Self {
        use crate::battle_camera::{CameraAngles, build_camera_angle_tween};

        let trio = |v: [f32; 3]| [v[0] as i16, v[1] as i16, v[2] as i16];
        let mut cur = CameraAngles {
            rotation: [from.pitch as i16, from.yaw as i16, 0],
            shake: trio(from.tr),
            focus: trio(from.focus),
        };
        let mut tgt = CameraAngles {
            rotation: [target.pitch as i16, target.yaw as i16, 0],
            shake: [
                target.tr[0] as i16,
                target.tr[1] as i16,
                target_tr_z_raw as i16,
            ],
            focus: trio(target.focus),
        };
        let table = build_camera_angle_tween(&mut cur, &mut tgt, steps.max(1) as u16);
        // Step 1 of the builder converted TR.z into projection units in place.
        target.tr[2] = tgt.shake[2] as f32;
        if yaw {
            // Both ends come back from the builder's wrap-adjust; the segment
            // that leaves yaw to the idle orbit keeps its own value instead.
            from.yaw = cur.rotation[1] as f32;
            target.yaw = tgt.rotation[1] as f32;
        }
        // The builder's slot order is rotation, translation, focus; roll
        // (slot 2) is never driven here.
        let rate = [
            table[0].step as f32,
            table[1].step as f32,
            table[3].step as f32,
            table[4].step as f32,
            table[5].step as f32,
            table[6].step as f32,
            table[7].step as f32,
            table[8].step as f32,
        ];
        Glide {
            target,
            rate,
            yaw_glides: yaw,
            steps_left: Some(steps.max(1)),
        }
    }
}

/// Step `v` toward `target` by at most `rate`, clamping at the target.
pub(super) fn step_toward(v: f32, target: f32, rate: f32) -> f32 {
    let d = target - v;
    if d.abs() <= rate {
        target
    } else {
        v + rate.copysign(d)
    }
}

/// Action-SM state bands the yaw-counter ladder keys on (`ctx[7]`, the
/// [`crate::battle_action`] state byte): the seed pass, the Attack band's
/// bounds, the strike loop, and the Done band that closes every category.
pub(super) const ACTION_SEED_STATE: u8 = 0x0C;
pub(super) const ATTACK_BAND_FIRST: u8 = 0x14;
pub(super) const ATTACK_BAND_LAST: u8 = 0x20;
pub(super) const STRIKE_LOOP_STATE: u8 = 0x1E;
pub(super) const ACTION_DONE_STATE: u8 = 0x50;

/// The phase-scripted battle camera state. Created on battle entry, stepped
/// once per 2 retail display frames (`World::clock.display_frames`), dropped on exit.
#[derive(Debug)]
pub struct BattleCamera {
    pub(super) phase: BattleCamPhase,
    pub(super) pose: BattleCamPose,
    /// Chained glide segments (front = active).
    pub(super) glides: std::collections::VecDeque<Glide>,
    /// `field_frames` value already consumed, for the 2-vsync step cadence.
    pub(super) last_frames: u64,
    /// Sub-step vsync accumulator (steps fire every 2 frames).
    pub(super) frame_accum: u64,
    /// The acting actor the submenu close-up frames. Defaults to the
    /// measured solo-Vahn case; hosts that track the live battle actor call
    /// [`BattleCamera::set_actor`] so non-Vahn seats frame correctly.
    pub(super) actor: BattleCamActor,
    /// The formation the far menu framing encloses. `None` (an un-wired host)
    /// falls back to retail's degenerate case: minimum depth, origin focus.
    pub(super) formation: Option<FormationBox>,
    /// The acting actor's target, for cases `7` / `8`.
    pub(super) target: Option<PostActionTarget>,
    /// Case-6 context inputs for the [`BattleCamPhase::Action`] framing.
    pub(super) action: ActionFraming,
    /// Retail `ctx[+0x6DA]`, the action SM's free-running yaw counter
    /// (`0x801E29E4..0x801E2A24`). Advanced one unit per display frame for
    /// as long as the battle runs, exactly like the SM's own prologue.
    pub(super) action_yaw: i32,
    /// The last `ctx[7]` [`Self::observe_action_state`] saw, so the yaw
    /// counter's per-action seeds fire on the state **edges** the way
    /// retail's arms store them once on entry.
    pub(super) last_action_state: u8,
    /// The acting actor's body pair `+0x3C` / `+0x40` (live pair plus the
    /// facing-rotated pose centroid), which the summon close-up focuses.
    pub(super) acting_body: Option<[f32; 2]>,
    /// Latch for the swing-clip commit's `ctx[+0xD] = 0`
    /// (`sb zero,0xd(v1)` at `0x8004E2B4`, in `FUN_8004E13C`'s party arm
    /// beside the `ctx[+0x6DA]` seed).
    ///
    /// It is a **latch** rather than a write because retail's is a write to
    /// the shared context byte, which then stands until the next action
    /// seed re-rolls it - while the host re-supplies
    /// [`Self::action`] every frame from the live byte. Setting
    /// `self.action.style = 0` on the edge alone would be overwritten on the
    /// very next frame; this survives instead, and clears on the edge out of
    /// the action bands, which is where the next seed happens.
    pub(super) strike_style_zeroed: bool,
    /// Live screen shake (`FUN_801D9D30`), held beside the pose.
    pub(super) shake: ShakeState,
    /// The per-art attack camera's channel: the disc track table, the battle
    /// context's own counters, and the acting actor's three bytes.
    pub(super) attack: AttackChannel,
    /// The summon module's own shot (`FUN_801D829C` out of a slot-B arm),
    /// which owns the camera through the summon band's `0x35` / `0x36`
    /// ([`BattleCamera::arm_module_shot`]).
    pub(super) module_glide: Option<Glide>,
    /// Set once a successful flee arms its shot
    /// ([`BattleCamera::arm_escape_shot`]): from then on the shot owns the
    /// camera for the rest of the battle, whatever the phase or state.
    pub(super) escape_shot: bool,
    /// The last [`BattleCamInputs::active_commits`] seen; `None` until the
    /// first drive, so a camera created mid-action does not reset on its
    /// first frame.
    pub(super) last_active_commits: Option<u32>,
}

/// [`BattleCamera`]'s per-art attack-camera state - retail's `ctx[+0x26D]` /
/// `+0x26E` / `+0x26F` / `+0x87C` plus the disc table the arms fold.
#[derive(Debug, Default)]
pub(super) struct AttackChannel {
    /// The parsed `0x801F4E10` table. `None` on a disc-free host, which
    /// leaves the whole channel inert (retail cannot run without it either).
    pub(super) tracks: Option<legaia_asset::battle_attack_camera_table::AttackCameraTracks>,
    /// The battle context's ramp / cursor / latch quartet.
    pub(super) ctx: crate::battle_attack_camera::AttackCamCtx,
    /// The acting actor's channels this frame.
    pub(super) actor: Option<AttackCamChannels>,
    /// The `rand()` state the per-action cursor coin flip draws from.
    /// Retail's `FUN_8004E13C` draws from the process-wide PsyQ `rand()`; the
    /// engine keeps its own so a battle is reproducible.
    pub(super) seed: u32,
}

/// The `FUN_801D9D30` shake, as the engine holds it.
///
/// Retail has one storage slot - the camera translation pair itself - so its
/// routine subtracts the previous jitter back out of `0x800840B8/BC` before
/// re-rolling and adding the new one. The engine keeps the accumulator
/// *separate* from the phase-script pose and adds it at [`BattleCamera::pose`]
/// instead, because the pose is also the target a rate-clamped glide walks
/// toward: folding a per-step random offset into it would leave a glide that
/// can never reach its endpoint. The kernel call is unchanged - the
/// accumulator simply starts at zero, so it carries exactly the jitter retail
/// adds on top of the framing.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct ShakeState {
    /// Retail `0x800840B8/BC`'s jitter contribution.
    pub(super) accum: [i32; 2],
    /// Retail `DAT_801C6EA4 + 0x18/+0x1C` - the offset last applied.
    pub(super) offset: [i32; 2],
    /// Retail `_DAT_8007B630`.
    pub(super) amplitude: u32,
    /// PsyQ `rand()` state (retail `FUN_80056798`'s seed).
    pub(super) seed: u32,
}

/// Seed the shake RNG with something other than zero so the very first roll
/// is not degenerate. Any constant works - retail's seed is the process-wide
/// `rand()` state, which the engine does not share.
pub(super) const SHAKE_SEED: u32 = 0x0BAD_5EED;

/// Sibling seed for the per-action camera-track coin flip (retail's
/// `FUN_8004E13C` draws it from the same shared `rand()`; the engine keeps a
/// second stream so a shake and a swing angle are independently reproducible).
pub(super) const ATTACK_CURSOR_SEED: u32 = 0x0CA3_5EED;

/// One **measured** sample of retail's free-running battle azimuth, in 12-bit
/// units - the yaw a mednafen battle save state reads while the fight idles at
/// the far Begin/Run framing.
///
/// It is not "the" resting yaw and nothing in retail makes it special: five
/// battle states caught at the same framing read `224`, `2632`, `3136`, `3808`
/// and `3882`, because `_DAT_8007B792` free-runs and a fight inherits whatever
/// the field camera left. What every one of them *is* is **far from the seat
/// axis**, and that is the property this constant is used for - see
/// [`battle_entry_yaw`].
pub const BATTLE_ENTRY_YAW_SAMPLE: f32 = 3372.0;

/// How close to the seat axis an entry azimuth may be before
/// [`battle_entry_yaw`] replaces it, in 12-bit units (`192` = ~17 degrees).
///
/// The bound is geometric, not fitted: the retail seats are `(0, +-800)`
/// (`legaia_engine_core::battle_seats`), so at azimuth `t` the two rows are
/// separated on screen by roughly `1600 * sin(t)` battle-world units against
/// character meshes ~400 units wide (`docs/formats/character-mesh.md`). Inside
/// ~17 degrees the separation is under one character width and the near row
/// still covers the far one. It is a threshold on a continuum, and it is a
/// **port judgement** - retail needs none because its azimuth free-runs.
/// Sanity check rather than derivation: all five captured retail battle yaws
/// (`224`, `2632`, `3136`, `3808`, `3882`) sit outside it.
pub const DEGENERATE_YAW_WINDOW: u16 = 192;

/// The azimuth a fight inherits on entry, given the live field-camera
/// compass word (`_DAT_8007B792`, the port's
/// `World::locomotion.camera_azimuth`).
///
/// Retail passes the shared rotation global straight through. The problem is
/// that the port's mirror is not free-running: the field is framed by a
/// **fixed follow camera** whose free-roam reset snaps the controller back
/// every frame, so the compass publishes a constant `0` for the entire time
/// the player is not manually orbiting.
///
/// `0` is the one azimuth a battle must not start at, for the reason
/// [`BattleCamInputs::entry_yaw`] gives. The test is against
/// [`DEGENERATE_YAW_WINDOW`] rather than against zero because the live
/// compass reaches the battle with small non-zero values too: measured in
/// `town01` with a seeded party it reads `160` on the frame the Tetsu fight
/// opens - 14 degrees off the seat axis, inside the overlap window, and it
/// framed both combatants at the same screen X.
///
/// This lives beside the script rather than in a host because it is an input
/// to [`drive`], and an input only one host applies is a camera the two hosts
/// do not share: the browser play page fed the raw compass word here and
/// opened its fights down the seat axis whenever the native window did not.
pub fn battle_entry_yaw(camera_azimuth: u16) -> f32 {
    let live = camera_azimuth & 0xFFF;
    // Distance to the nearer end of the seat axis (`0` and `2048` are the two
    // azimuths that put the eye on it).
    let off_axis = live.min(4096 - live).min(live.abs_diff(2048));
    if off_axis < DEGENERATE_YAW_WINDOW {
        BATTLE_ENTRY_YAW_SAMPLE
    } else {
        f32::from(live)
    }
}

/// Everything one frame of battle state tells the camera. Bundled so the two
/// hosts pass the same record to [`drive`] and a new channel cannot be added
/// to one host and forgotten on the other.
#[derive(Debug, Clone, Copy, Default)]
pub struct BattleCamInputs {
    /// The framing phase ([`phase_for`]).
    pub phase: BattleCamPhase,
    /// The acting battle actor, when one owns the framing.
    pub acting: Option<BattleCamActor>,
    /// The acting actor's **target** (`actor[+0x1DD]` through the actor
    /// table), which cases `7` and `8` frame against. `None` degenerates to
    /// retail's own actor-only arms.
    pub target: Option<PostActionTarget>,
    /// The live formation the far framing encloses.
    pub formation: Option<FormationBox>,
    /// Case-6 context inputs for the action framing.
    pub action: ActionFraming,
    /// `_DAT_8007B630` - the screen-shake amplitude the field VM last wrote.
    pub shake_amplitude: u8,
    /// The per-art attack camera's per-actor channels
    /// ([`AttackCamChannels`]), or `None` when that channel is not armed.
    pub attack: Option<AttackCamChannels>,
    /// The idle orbit's azimuth **on battle entry**, in 12-bit units.
    ///
    /// `_DAT_8007B792` is one global: the field camera and the battle camera
    /// share the rotation trio `0x8007B790/92/94`, and nothing on the battle
    /// entry path zeroes it - case 9 passes it straight through and the
    /// action SM only decrements it. A fight therefore *inherits* whatever
    /// azimuth the field camera left, which is why five retail battle save
    /// states caught at the same framing (`ctx[7] == 0x00`, pitch `32`,
    /// `TR (0, 1280, 7680)`, focus at the origin) read five different yaws -
    /// `224`, `2632`, `3136`, `3808`, `3882`. No captured value is *the*
    /// resting yaw; the resting yaw is the free-running orbit.
    ///
    /// Seeding it matters because `0` is the one degenerate azimuth: the
    /// retail seats are `(0, +-800)`, so at yaw `0` the eye looks straight
    /// down the seat axis and the two rows project to the same screen X,
    /// each occluding the other. Retail cannot start there; a host that
    /// seeds `0` does, for the ~6 seconds the `-4`/step orbit needs to leave.
    pub entry_yaw: f32,
    /// The live action-SM state `ctx[7]`, the same byte [`phase_for_state`]
    /// classifies. The camera reads it for the edges that re-seed the yaw
    /// counter `ctx[+0x6DA]` ([`BattleCamera::observe_action_state`]).
    pub action_state: u8,
    /// The active actor's clip-commit count
    /// (`BattleActionCtx::active_clip_commits`): a change re-zeroes the
    /// ramp / accumulator / latch the way the commit `FUN_8004AD80` does.
    pub active_commits: u32,
    /// The acting actor's **body pair** `+0x3C` / `+0x40` - the live pair
    /// plus the facing-rotated pose centroid the pose decoder `FUN_8004998C`
    /// rewrites each drawn frame - as `(x, z)`. The summon close-up
    /// (`FUN_801DC0A0` case `0x12`) focuses it, not the live pair. `None`
    /// falls back to [`BattleCamActor::world`].
    pub acting_body: Option<[f32; 2]>,
}

/// Drive one host's battle camera for a frame - the single shared entry both
/// hosts call so the create / retarget / phase-change / step ordering cannot
/// drift between them. `slot` is the host's per-battle camera state
/// (dropped whenever `active` is false so the next battle re-snaps);
/// `active` is "a stage-dome battle owns the 3D frame"; `frames` is the
/// world's retail display-frame counter (`World::clock.display_frames`, one camera
/// step per 2 frames).
///
/// A battle that opens on dialogue snaps to the held close-up; any other
/// battle snaps to the far menu framing (retail's loading pose resolves
/// there) and glides out to whichever phase is already live. The entry snap
/// takes the LIVE formation: retail's case 9 always runs against the live
/// actor table, so a battle that opens on the far framing sizes its depth
/// and centres its focus immediately rather than sitting at the degenerate
/// minimum until the first phase transition re-derives it.
/// `tracks` is the disc-parsed per-art camera table
/// (`legaia_asset::battle_attack_camera_table`). It is a parameter rather than
/// a field on [`BattleCamInputs`] because the record is `Copy` and the table
/// is not; a host that has no disc data passes `None` and the per-art channel
/// stays inert, which is the same framing the port had before it existed.
pub fn drive(
    slot: &mut Option<BattleCamera>,
    active: bool,
    inputs: BattleCamInputs,
    frames: u64,
    tracks: Option<&legaia_asset::battle_attack_camera_table::AttackCameraTracks>,
) {
    if !active {
        *slot = None;
        return;
    }
    let entry = if inputs.phase == BattleCamPhase::Dialogue {
        BattleCamPhase::Dialogue
    } else {
        BattleCamPhase::Menu
    };
    let cam = slot.get_or_insert_with(|| {
        BattleCamera::new_with_formation(entry, inputs.formation, inputs.entry_yaw, frames)
    });
    if let Some(actor) = inputs.acting {
        cam.set_actor(actor);
    }
    cam.acting_body = inputs.acting_body;
    cam.set_post_action_target(inputs.target);
    cam.set_formation(inputs.formation);
    cam.set_action_framing(inputs.action);
    cam.observe_action_state(inputs.action_state);
    cam.observe_active_commits(inputs.active_commits);
    cam.set_shake_amplitude(inputs.shake_amplitude);
    cam.set_attack_channels(inputs.attack, tracks);
    cam.set_phase(inputs.phase);
    cam.advance_to(frames);
}
