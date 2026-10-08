//! The slot-B summon modules' **own camera** and the **countdown** that paces
//! their arms - the half of a player Seru-magic choreography the phase-chain
//! ports in [`crate::cast_seru_ticks_a`] / [`crate::cast_seru_ticks_b`] leave
//! out.
//!
//! From the summon band's actor freeze `0x34` on, the battle action SM calls
//! no framing case of its own: `0x35` and `0x36` only re-enter the paged
//! module through `FUN_801F1ED4` (`docs/subsystems/battle-action.md`). The
//! camera in those states is therefore whatever the module arms, and every
//! player-Seru module arms it the same way - two shared kernels, parameterised
//! per arm.
//!
//! ## The shot kernel
//!
//! Each camera arm fills three stack trios and hands them to the tween builder
//! `FUN_801D829C` ([`crate::battle_camera::build_camera_angle_tween`]):
//!
//! ```text
//! sp+0x20  pitch, yaw, roll          ; roll is zeroed in the prologue
//! sp+0x28  TR x, TR y, TR z          ; TR z raw, the builder prescales it
//! sp+0x30  focus x, y, z             ; the negated world point
//! a3       duration, display frames
//! ```
//!
//! [`ModuleShot`] is that call. What varies per arm is only where the nine
//! halfwords come from: immediates, the negated position of the creature seat
//! `actor_table[7]`, a yaw folded off that seat's heading (`K - facing`), or a
//! heading taken between the caster and its victim through `FUN_80019B28`.
//!
//! ## The countdown kernel
//!
//! Each module keeps one countdown word in its own image (`0x801F7960` in
//! PROT 0903). An arm arms it as a multiple of the speed scalar
//! `*(0x1F80037D)` ([`SPEED_SCALAR`]), and every counted arm then drains it by
//! the product `*(0x1F80037D) * *(0x1F800393)` - scalar times the frame delta -
//! holding while the word is above that arm's threshold. The engine ticks once
//! per displayed frame, so its per-tick drain is [`MODULE_DRAIN_PER_TICK`]:
//! the retail product divided by the two vsyncs a retail battle frame spans.
//! [`ModuleCountdown`] carries the word and the three gate shapes the arms
//! use.
//!
//! What is here decides *when* an arm completes and *what the camera does* on
//! it; the arm's simulation writes stay in its phase-chain port, which the
//! engine calls only on the tick a gate here lets through.
//!
//! Provenance: the disassembly of each module at the slot-B base `0x801F69D8`
//! (`see ghidra/scripts/funcs/overlay_summon_<label>_<entry>_<va>.txt`).
//!
//! REF: FUN_801D829C (the tween every shot arms), FUN_80019B28 (the headings)

use crate::battle_action::{bearing_12bit_approx, motion::trig12};
use crate::battle_cam_script::{BattleCamPose, prescale_tr_z};

/// `*(0x1F80037D)` in battle - the speed scalar the battle seating seeds
/// ([`crate::battle_anim_rate::RATE_NORMAL`]).
pub const SPEED_SCALAR: i32 = crate::battle_anim_rate::RATE_NORMAL as i32;

/// The countdown drain per engine tick. Retail drains `scalar * delta` per
/// battle frame, and a battle frame spans `delta` vsyncs; the engine ticks
/// once per vsync, so one tick drains the scalar alone.
pub const MODULE_DRAIN_PER_TICK: i32 = SPEED_SCALAR;

/// One `FUN_801D829C` call out of a module arm, in retail's own value space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleShot {
    /// `sp+0x20..0x24`: pitch, yaw, roll (12-bit units).
    pub angles: [i16; 3],
    /// `sp+0x28..0x2C`: the translation trio, TR z un-prescaled.
    pub tr: [i16; 3],
    /// `sp+0x30..0x34`: the focus trio, negated world position.
    pub focus: [i16; 3],
    /// `a3`: the tween duration in display frames.
    pub frames: u16,
}

impl ModuleShot {
    /// The shot as the battle camera holds a pose: yaw masked to 12 bits,
    /// TR z prescaled, the focus un-negated. Returns the raw TR z beside it,
    /// which is what the tween builder is handed.
    pub fn pose(&self) -> (BattleCamPose, i32) {
        let raw_z = i32::from(self.tr[2]);
        (
            BattleCamPose {
                pitch: f32::from(self.angles[0]),
                yaw: f32::from(self.angles[1] & 0xFFF),
                tr: [
                    f32::from(self.tr[0]),
                    f32::from(self.tr[1]),
                    prescale_tr_z(raw_z),
                ],
                focus: [
                    -f32::from(self.focus[0]),
                    -f32::from(self.focus[1]),
                    -f32::from(self.focus[2]),
                ],
            },
            raw_z,
        )
    }
}

/// A seat's world position and battle heading (`+0x34`, `+0x36`, `+0x38`,
/// `+0x46`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModuleSeat {
    pub x: i16,
    pub y: i16,
    pub z: i16,
    pub facing: u16,
}

/// The seats a module's camera arms read, resolved the way every module's
/// prologue resolves them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModuleCamSeats {
    /// `actor_table[ctx + 0x13]`.
    pub caster: ModuleSeat,
    /// `actor_table[caster + 0x1DD]`, as the cast latched it on its first
    /// pass (PROT 0908 keeps its own copy in a module word; a retarget later
    /// in the cast does not move the framing).
    pub victim: ModuleSeat,
    /// `ctx[+0x6D8]` - the summon band's own frame timer, which the actor
    /// freeze `0x34` arms and the sustain `0x35` counts down.
    pub band_timer: i32,
    /// The formation's first monster id `0x8007BD0C`, which a capture body
    /// can fork its framing on (PROT 0962's `0xA5` arm 0).
    pub first_monster: u8,
    /// The caster's own formation monster id, `0x8007BD0C[ctx+0x13 - 3]` -
    /// what PROT 0940's Glare forks its framing on (`0` for a party caster).
    pub caster_monster: u8,
    /// The caster's queued action id `+0x1DF` - which choreography a
    /// two-spell module runs (PROT 0946's Call Wave / Big Wave).
    pub action: u8,
    /// `ctx[+0x6D0]`, the framing depth a capture shot can scale.
    pub depth_raw: i32,
    /// The caster's battle-scoped latch word `0x801C8FE0 + (ctx[+0x13] + 1)
    /// * 4` - the monster AI's ability cooldown `dat[m + 4]`, which PROT
    /// 0953 reads as its charge flag. `0` for a party caster.
    pub caster_latch: i32,
}

/// The module-resident state the camera arms carry between ticks: the
/// countdown word and the creature seat's pose as the module placed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModuleCamState {
    /// The module's countdown word.
    pub countdown: ModuleCountdown,
    /// The creature seat `actor_table[7]` once an arm has placed it.
    pub creature: Option<ModuleSeat>,
    /// The creature's live seat, as the host last moved it (its walk-in).
    /// Fed in by the host each tick; `None` before it is seated.
    pub creature_live: Option<ModuleSeat>,
    /// Whether the creature's walk has reached the victim - the host's
    /// answer to the range test `FUN_8004E2F0(7, victim)` a walk arm polls.
    pub creature_arrived: bool,
    /// `ctx[+0x6DA]` as the module drives it: the yaw base the action
    /// framing it hands back to (`FUN_801D5854` case 6) subtracts the
    /// creature's heading from.
    pub yaw_base: i32,
    /// The victim seat the cast latched on its first pass.
    pub victim_slot: Option<u8>,
    /// A module-side mirror of the TR y global, for an arm that gates on the
    /// camera it is climbing (PROT 0917's arm 1).
    pub tr_y: i32,
}

/// `FUN_801D5854(7, 6)` out of a module arm: the action SM's own case-6
/// framing aimed at the creature seat, re-armed every pass so it chases the
/// walking creature. The camera fills in the rest of case 6's inputs
/// (`ctx[+0xD]` style) from its own live context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleFollow {
    /// The seat case 6 frames.
    pub seat: ModuleSeat,
    /// `ctx[+0x6DA]`.
    pub yaw_base: i32,
    /// `ctx[+0x6D0]` - the raw eye depth.
    pub depth_raw: i32,
}

/// A module's countdown word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModuleCountdown(pub i32);

impl ModuleCountdown {
    /// `sw (scalar << shift), countdown` - the arming store.
    pub fn arm(&mut self, shift: u32) {
        self.0 = SPEED_SCALAR << shift;
    }

    /// `countdown += scalar * n` - the re-arm the later arms use.
    pub fn add(&mut self, n: i32) {
        self.0 += SPEED_SCALAR * n;
    }

    /// One drain.
    pub fn drain(&mut self) {
        self.0 -= MODULE_DRAIN_PER_TICK;
    }

    /// Drain, then report whether the word is still above `threshold` - the
    /// `subu; slt/bgtz` gate every counted arm opens with.
    pub fn drain_above(&mut self, threshold: i32) -> bool {
        self.drain();
        self.0 > threshold
    }

    /// Drain, then report whether the word is still non-negative - the
    /// `subu; bgez` form of the gate (PROT 0905), which holds one pass longer
    /// than [`Self::drain_above`]`(0)` on a word that lands on zero.
    pub fn drain_non_negative(&mut self) -> bool {
        self.drain();
        self.0 >= 0
    }
}

/// What one module arm decided this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ArmDirection {
    /// `true` when the arm holds: its phase-chain body must not run.
    pub hold: bool,
    /// The camera shot the arm armed, if any.
    pub shot: Option<ModuleShot>,
    /// The case-6 follow the arm re-armed, if any.
    pub follow: Option<ModuleFollow>,
    /// The drift the arm wrote straight into the camera globals, if any.
    pub nudge: Option<ModuleNudge>,
    /// `true` on an arm a camera-only director does not cover: the module
    /// phase stays where it is and the camera keeps the last framing.
    pub park: bool,
    /// The `FUN_80021B04` calls the arm made on this pass, in call order.
    pub spawns: &'static [ModuleSpawn],
    /// The `MoveImage` (`FUN_80058490`) the arm issued on this pass, if any.
    pub vram_move: Option<ModuleVramMove>,
    /// The text the arm put up on this pass (`FUN_8003541C`), if any.
    pub caption: Option<ModuleCaption>,
    /// The full-screen fades the arm spawned on this pass
    /// (`FUN_80024E80(0x801C9070, id)` over the template it wrote), in call
    /// order, each `(template, id)`.
    pub fades: &'static [(crate::battle_action::SummonFadeTemplate, i16)],
    /// The arm killed the module's earlier fades (`ori 0x8` into each fade
    /// actor's flag word) before it spawned its own.
    pub kills_fades: bool,
}

/// A line of text a module arm prints through `FUN_8003541C(0, 0, str, x,
/// 0x96, 0, 0, 0)`: a frameless text actor at `y = 150`, centred by its
/// measured width (`FUN_80035F04`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleCaption {
    /// The cast's spell name, the spell table's `+8` pointer for the
    /// caster's `+0x1DF`.
    SpellName,
    /// The cast's attack name, the streamed actor record's `rec[0]` string
    /// (`*(0x801C9348 + 0x10)`), after `FUN_800319A8(0)` has cleared the
    /// spell name.
    AttackName,
}

/// Where one module spawn call seats its record (`a0` / `a1` of
/// `FUN_80021B04(pos, rot, record, 0x1000)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnAnchor {
    /// The creature seat `actor_table[7]`: `a0 = creature + 0x34`,
    /// `a1 = creature + 0x44`.
    Creature,
    /// The arm's own shot trios: `a0 = sp+0x30` (the negated focus),
    /// `a1 = sp+0x20` (the shot angles). Every record a module spawns this
    /// way is camera-relative (`+0x52 & 0x780`), so its program's own
    /// `WORLD_SET` places it and the anchor is only its seed.
    ShotFocus,
    /// The arm's shot focus turned back into a world point - the stack trio
    /// `sp+0x30` / `sp+0x34` negated again (`subu v0,zero,v0`) - at the
    /// literal height `y` stored over `sp+0x32`, with the shot's yaw zeroed
    /// (`sh zero,0x22(sp)`). PROT 0905 arm 0 (`0x801F6BF4..0x801F6C1C`).
    ShotPoint { y: i16 },
    /// The cast's victim seat `actor_table[caster + 0x1DD]`:
    /// `a0 = victim + 0x34`, `a1 = victim + 0x44`.
    Victim,
    /// A point `4096 / div` units along the **victim's** heading `+0x46`
    /// from `base` - `base + trunc(sin(h) / div)` on X, the same with `cos`
    /// on Z, through the SCUS tables `0x8007B81C` / `0x8007B7F8` - at the
    /// literal height `y`, with no rotation (the arm's zeroed stack angles).
    /// PROT 0905 arms 4 (`/ 32`, `0x801F707C..0x801F710C`) and 5 (`/ 24` on
    /// the creature, `0x801F7340..0x801F7404`).
    AlongVictimHeading { base: HeadingBase, div: i16, y: i16 },
}

/// The seat a [`SpawnAnchor::AlongVictimHeading`] point starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadingBase {
    Victim,
    Creature,
}

/// Resolve one spawn call's `(a0, a1)` - the position trio and the angle
/// trio `FUN_80021B04` seats the record on - from the seats the arm read and
/// the shot it armed this pass. `None` when the anchor's seat or shot is not
/// there.
pub fn spawn_anchor_point(
    anchor: SpawnAnchor,
    creature: Option<ModuleSeat>,
    victim: Option<ModuleSeat>,
    shot: Option<ModuleShot>,
) -> Option<([i16; 3], [i16; 3])> {
    let seat_at = |s: ModuleSeat| ([s.x, s.y, s.z], [0, s.facing as i16, 0]);
    Some(match anchor {
        SpawnAnchor::Creature => seat_at(creature?),
        SpawnAnchor::Victim => seat_at(victim?),
        SpawnAnchor::ShotFocus => {
            let s = shot?;
            (s.focus, s.angles)
        }
        SpawnAnchor::ShotPoint { y } => {
            let s = shot?;
            (
                [s.focus[0].wrapping_neg(), y, s.focus[2].wrapping_neg()],
                [s.angles[0], 0, s.angles[2]],
            )
        }
        SpawnAnchor::AlongVictimHeading { base, div, y } => {
            let v = victim?;
            let from = match base {
                HeadingBase::Victim => v,
                HeadingBase::Creature => creature?,
            };
            let (sin, cos) = trig12(v.facing & 0xFFF);
            let d = i32::from(div);
            (
                [
                    (i32::from(from.x) + i32::from(sin) / d) as i16,
                    y,
                    (i32::from(from.z) + i32::from(cos) / d) as i16,
                ],
                [0; 3],
            )
        }
    })
}

/// Which record one module spawn call hands `FUN_80021B04` as `a2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnRecord {
    /// A record in the module's own image, by slot-B VA (`lui`/`addiu`).
    Module(u32),
    /// A record the battle overlay (PROT 0898, slot A) points at: `a2` is
    /// loaded from the pointer word at this VA (`lw a2, lo(hi)`), an entry of
    /// the effect-prototype table `0x801F6324`.
    BattleProto(u32),
}

/// One `FUN_80021B04` call out of a module arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleSpawn {
    pub record: SpawnRecord,
    pub anchor: SpawnAnchor,
}

/// One `FUN_80058490(&rect, dst_x, dst_y)` - a VRAM-to-VRAM `MoveImage` - out
/// of a module arm. The rect is the four halfwords the arm builds in the
/// packet buffer `*(0x1F8003A0)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleVramMove {
    pub src: (u16, u16),
    pub size: (u16, u16),
    pub dst: (u16, u16),
}

/// A module arm's direct writes into the live camera globals - pitch
/// `0x8007B790`, TR y `0x800840BC`, TR z `0x800840C0` - added every pass,
/// held or not, on top of whatever the last shot landed. The TR z delta is in
/// the global's own (prescaled) units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModuleNudge {
    pub pitch: i16,
    pub tr_y: i16,
    pub tr_z: i16,
}

impl ArmDirection {
    pub(super) const HOLD: Self = Self {
        hold: true,
        shot: None,
        follow: None,
        nudge: None,
        park: false,
        spawns: &[],
        vram_move: None,
        caption: None,
        fades: &[],
        kills_fades: false,
    };
    pub(super) const PASS: Self = Self {
        hold: false,
        shot: None,
        follow: None,
        nudge: None,
        park: false,
        spawns: &[],
        vram_move: None,
        caption: None,
        fades: &[],
        kills_fades: false,
    };
    pub(super) const PARK: Self = Self {
        hold: true,
        shot: None,
        follow: None,
        nudge: None,
        park: true,
        spawns: &[],
        vram_move: None,
        caption: None,
        fades: &[],
        kills_fades: false,
    };
    pub(super) fn shot(shot: ModuleShot) -> Self {
        Self {
            shot: Some(shot),
            ..Self::PASS
        }
    }
    pub(super) fn nudged(self, nudge: ModuleNudge) -> Self {
        Self {
            nudge: Some(nudge),
            ..self
        }
    }
}

/// `FUN_80019B28(a.z, a.x, b.z, b.x)` - the heading from `a` to `b`.
pub fn heading(a: ModuleSeat, b: ModuleSeat) -> u16 {
    bearing_12bit_approx(a.z, a.x, b.z, b.x)
}

/// Half a trig sample, rounded toward zero - the `srl 0x1f; addu; sra 1`
/// idiom every placement arm uses.
pub(super) fn half(v: i16) -> i32 {
    i32::from(v) / 2
}

/// A yaw folded off a heading: `(k - heading) & 0xFFF`.
pub(super) fn yaw_from(k: i32, heading: u16) -> i16 {
    ((k - i32::from(heading)) & 0xFFF) as i16
}

/// The focus trio on a seat at floor height: `(-x, 0, -z)`.
pub(super) fn focus_on(seat: ModuleSeat) -> [i16; 3] {
    [seat.x.wrapping_neg(), 0, seat.z.wrapping_neg()]
}

pub mod capture;
pub mod capture_countdown;
pub mod creature;
mod evil_seru_magic;
mod seru;
pub use capture::*;
pub use capture_countdown::{ArmCountdown, CountdownWrite, arm_countdown, capture_arm_countdowns};
pub use creature::*;
pub use evil_seru_magic::*;
pub use seru::*;

pub(super) fn gate(hold: bool) -> ArmDirection {
    if hold {
        ArmDirection::HOLD
    } else {
        ArmDirection::PASS
    }
}

/// One directed module arm: its camera / countdown half, run before the
/// phase-chain body.
pub type ModuleDirector = fn(&mut ModuleCamState, u8, ModuleCamSeats) -> ArmDirection;

/// What the host needs to know about a directed module besides its arms.
#[derive(Debug, Clone, Copy)]
pub struct ModuleProfile {
    /// The arms.
    pub direct: ModuleDirector,
    /// The phase arm the module lands its outcome in: once the phase has
    /// passed it, the hit / restore has been applied. `None` for a
    /// **camera-only** director - one over a module whose tick body is not
    /// ported ([`creature`]): it owns the module phase for the arms it
    /// covers and parks on the first it does not, and the band's length
    /// stays the engine stager's.
    pub hit_arm: Option<u8>,
    /// The phase arm the module walks its creature in, if it walks one: the
    /// host starts the creature's walk there and reports its arrival back
    /// through [`ModuleCamState::creature_arrived`]. A module with none keeps
    /// its creature where it seated it.
    pub walk_arm: Option<u8>,
    /// Whether a camera-only director owns the module phase. `false` for a
    /// director that runs **beside** a ported tick body
    /// ([`ModuleProfile::camera_beside`]): it reads the phase the body is
    /// about to run, never holds it, and only arms the camera.
    pub owns_phase: bool,
    /// Whether the director reports the module's own spawn calls per arm
    /// ([`ArmDirection::spawns`]). The host then seats each record on the
    /// pass its arm makes the call, instead of seating the module's whole
    /// record set on the stager's first tick.
    pub stages_spawns: bool,
    /// The phase arm whose pass seats the creature (`jal 0x801F19EC`,
    /// [`module_seat_arm`]). The host seats it once the module's phase
    /// reaches this arm rather than on the stager's first tick. `None` where
    /// the module seats it at once, or where a camera-only director parks
    /// before the arm.
    pub seat_arm: Option<u8>,
}

/// The directed profile of a player-Seru module, by owning PROT entry.
/// `None` for a module whose camera arms are not ported: its phase chain then
/// runs ungated, as before.
pub fn module_profile(prot_entry: u32) -> Option<ModuleProfile> {
    match prot_entry {
        903 => Some(ModuleProfile {
            direct: gimard_direct,
            hit_arm: Some(GIMARD_WALK_ARM),
            walk_arm: Some(GIMARD_WALK_ARM),
            owns_phase: true,
            stages_spawns: true,
            seat_arm: module_seat_arm(903),
        }),
        // PROT 0904's tick body paces the band through its ported ramps
        // (arms 10..12, `crate::cast_seru_ticks_a::theeder_tick`); its camera
        // arms are not ported, so the director only passes and case 6 keeps
        // the caster. The outcome lands once the sweep has turned.
        904 => Some(ModuleProfile {
            direct: theeder_direct,
            hit_arm: Some(crate::cast_seru_ticks_a::THEEDER_SWEEP_ARM),
            walk_arm: None,
            owns_phase: false,
            stages_spawns: false,
            seat_arm: module_seat_arm(904),
        }),
        905 => Some(ModuleProfile {
            direct: vera_direct,
            hit_arm: Some(VERA_RESTORE_ARM),
            walk_arm: None,
            owns_phase: true,
            stages_spawns: true,
            seat_arm: module_seat_arm(905),
        }),
        908 => Some(ModuleProfile {
            direct: zenoir_direct,
            hit_arm: Some(ZENOIR_FINISH_ARM),
            walk_arm: None,
            owns_phase: true,
            stages_spawns: false,
            seat_arm: module_seat_arm(908),
        }),
        // A camera-only director seats on its module's seat arm only where
        // it reaches it: PROT 0917, 0928, 0929 and 0931's park short of
        // theirs, so their creature is seated on the stager's first tick.
        914 => Some(ModuleProfile::camera_only(gola_gola_direct).seated_at(914)),
        915 => Some(ModuleProfile::camera_only(mushura_direct).seated_at(915)),
        917 => Some(ModuleProfile::camera_only(barra_direct)),
        920 => Some(ModuleProfile::camera_only(slippery_direct).seated_at(920)),
        923 => Some(ModuleProfile::camera_only(gilium_direct).seated_at(923)),
        928 => Some(ModuleProfile::camera_only(palma_direct)),
        930 => Some(ModuleProfile::camera_only(horn_direct).seated_at(930)),
        931 => Some(ModuleProfile::camera_only(jedo_direct)),
        916 => Some(ModuleProfile::camera_only(aluru_direct).seated_at(916)),
        921 => Some(ModuleProfile::camera_only(iota_direct).seated_at(921)),
        929 => Some(ModuleProfile::camera_only(mule_direct)),
        932 => Some(ModuleProfile::camera_only(meta_direct).seated_at(932)),
        933 => Some(ModuleProfile::camera_only(terra_direct).seated_at(933)),
        934 => Some(ModuleProfile::camera_only(ozma_direct).seated_at(934)),
        913 => Some(ModuleProfile::camera_beside(nova_direct).seated_at(913)),
        _ => None,
    }
}

/// PROT 0904's director: no camera arm is ported, so every arm passes and
/// the ported tick body alone paces the band.
fn theeder_direct(_: &mut ModuleCamState, _: u8, _: ModuleCamSeats) -> ArmDirection {
    ArmDirection::PASS
}

impl ModuleProfile {
    const fn camera_only(direct: ModuleDirector) -> Self {
        Self {
            direct,
            hit_arm: None,
            walk_arm: None,
            owns_phase: true,
            stages_spawns: false,
            seat_arm: None,
        }
    }

    const fn seated_at(mut self, prot_entry: u32) -> Self {
        self.seat_arm = module_seat_arm(prot_entry);
        self
    }

    const fn camera_beside(direct: ModuleDirector) -> Self {
        Self {
            direct,
            hit_arm: None,
            walk_arm: None,
            owns_phase: false,
            stages_spawns: false,
            seat_arm: None,
        }
    }

    /// Whether this director paces the band (the module's own tick body is
    /// ported and its holds hold `0x36`).
    pub const fn paces_band(&self) -> bool {
        self.hit_arm.is_some()
    }
}

/// The phase arm of a summon module's tick body that seats its creature:
/// the arm holding `jal 0x801F19EC` (which installs the streamed creature
/// as actor slot 7), read off each image's own phase dispatch at slot-B base
/// `0x801F69D8`. Where the arm also polls the stream (`FUN_8003F2B8(1)`)
/// the seat is on its first pass with the stream resident; elsewhere an
/// earlier arm polls and exits, and the seat arm runs after it. The engine's
/// stream is resident at once, so the creature appears as the module enters
/// this arm. `zenoir_summon_mid_cast` holds phase 3 with slot 7
/// still empty (`+0x14C == 0`, the seat at `(0, 0, 0)`), one arm short.
///
/// PROT 0909 (Viguro) is absent: its seat lives in `FUN_801F7AF4`, which
/// no reference in its image calls.
///
/// REF: FUN_801F19EC
pub const fn module_seat_arm(prot_entry: u32) -> Option<u8> {
    Some(match prot_entry {
        903 => 3, // 0x801F6D3C
        904 => 4, // 0x801F6EC4
        905 => 5, // 0x801F7284
        906 => 4, // 0x801F6D10
        907 => 6, // 0x801F76C0
        908 => 4, // 0x801F6FC0
        910 => 2, // 0x801F6D10
        911 => 3, // 0x801F70B0
        912 => 2, // 0x801F6C34
        913 => 2, // 0x801F6D24
        914 => 3, // 0x801F6CA8
        915 => 5, // 0x801F6E5C
        916 => 4, // 0x801F7074
        917 => 3, // 0x801F6E38
        919 => 2, // 0x801F6DA8
        920 => 2, // 0x801F6CFC
        921 => 3, // 0x801F6FA4
        922 => 3, // 0x801F6DFC
        923 => 3, // 0x801F6DB0
        927 => 2, // 0x801F6D9C
        928 => 4, // 0x801F744C
        929 => 5, // 0x801F7080
        930 => 3, // 0x801F6E98
        931 => 4, // 0x801F6E28
        932 => 4, // 0x801F6EB0
        933 => 2, // 0x801F6D70
        934 => 5, // 0x801F7034
        _ => return None,
    })
}

/// The director for a player-Seru module, by owning PROT entry.
pub fn module_director(prot_entry: u32) -> Option<ModuleDirector> {
    module_profile(prot_entry).map(|p| p.direct)
}

#[cfg(test)]
mod tests;
