//! The slot-B summon modules' **own camera** and the **countdown** that paces
//! their arms - the half of a player Seru-magic choreography the phase-chain
//! ports in [`crate::seru_magic_ticks`] / [`crate::seru_magic_ticks`] leave
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
//! `FUN_801D829C` ([`legaia_engine_camera::battle_camera::build_camera_angle_tween`]):
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

use legaia_engine_battle_vm::battle_action::bearing_12bit_approx;
use legaia_engine_battle_vm::battle_cam_script::ModuleShot;
use legaia_engine_system::trig::trig12;

/// `*(0x1F80037D)` in battle - the speed scalar the battle seating seeds
/// ([`legaia_engine_battle_vm::battle_anim_rate::RATE_NORMAL`]).
pub const SPEED_SCALAR: i32 = legaia_engine_battle_vm::battle_anim_rate::RATE_NORMAL as i32;

/// The countdown drain per engine tick. Retail drains `scalar * delta` per
/// battle frame, and a battle frame spans `delta` vsyncs; the engine ticks
/// once per vsync, so one tick drains the scalar alone.
pub const MODULE_DRAIN_PER_TICK: i32 = SPEED_SCALAR;

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
    /// The victim's body radius `*(victim[+0x22C]) + 0x58`, which a shot can
    /// size its depth by (PROT 0929's arm-16 exit: `10 * r`, at most
    /// `0x1800`).
    pub victim_radius: i16,
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
    /// Whether the side-band applier stage `ctx[+0x276]` is still up - the
    /// per-turn `summon.dat` / `readef.DAT` read a module arm can wait on
    /// ([`sideband_wait_arm`]). The engine streams are resident, so play
    /// leaves it clear and the arm passes at once; a capture replay holds
    /// it for as long as retail's read held its arm.
    pub sideband_busy: bool,
    /// `ctx[+0x6DA]` as the module drives it: the yaw base the action
    /// framing it hands back to (`FUN_801D5854` case 6) subtracts the
    /// creature's heading from.
    pub yaw_base: i32,
    /// The victim seat the cast latched on its first pass.
    pub victim_slot: Option<u8>,
    /// A module-side mirror of the TR y global, for an arm that gates on the
    /// camera it is climbing (PROT 0917's arm 1).
    pub tr_y: i32,
    /// A model word an arm counts up or down as its wait
    /// ([`summon_holds::Gate::RampUp`] / [`summon_holds::Gate::RampDown`]):
    /// PROT 0918's creature record `+0x72`, PROT 0922's clip frame `+0x68`.
    pub ramp: i32,
    /// Whether every monster seat has settled - a living one back on idle,
    /// a dead one faded out - fed in by the host each tick for a settle
    /// gate ([`summon_holds::Gate::Settle`]).
    pub row_settled: bool,
    /// The creature seat's playing clip `+0x1D9`, as the host last saw it
    /// (fed each tick, like [`Self::creature_live`]). `None` before the seat
    /// exists or where the host plays no clip on it: a clip gate then
    /// passes rather than park.
    pub creature_clip: Option<u8>,
    /// The creature seat's clip loop counter `+0x21B`, as the host last saw
    /// it (fed each tick beside [`Self::creature_clip`]): the committed
    /// entry's `+0x84` count, taken off a `0xFF` sentinel the first time the
    /// cursor reaches the window's end. `None` where the host plays no clip
    /// on the seat.
    pub creature_loops: Option<u8>,
    /// The host's answer to `FUN_8004E2F0(7, victim) == 0` - the creature in
    /// reach of the cast's victim. `None` where the host cannot answer.
    pub creature_in_range: Option<bool>,
    /// `FUN_801DCEAC(9, ..)` over the live enemy row, as the host last
    /// measured it: the negated centroid (a focus trio's `x` / `z`) and the
    /// extent. `None` with no live monster.
    pub row_aim: Option<legaia_engine_battle_vm::battle_target_group::GroupAim>,
    /// The creature render node's `+0x42` (`creature[+0x22C] + 0x42`) as
    /// the module's arms last wrote it: `true` draws the body clipped at
    /// the stage floor (`World::battle_actor_floor_clip`), so a creature
    /// seated under the floor rises out of it. PROT 0903 raises it in arm 3
    /// and drops it on arm 5's exit, PROT 0908 in arm 4 / arm 5's exit and
    /// again on arm 11's hit.
    pub creature_floor_clip: bool,
    /// A clip id an arm staged onto the creature's `+0x1DA` on this pass
    /// while it held - a write the phase-chain body, which does not run on
    /// a held pass, cannot make. The host takes it once.
    pub creature_stage: Option<u8>,
    /// An arm cleared the creature's loop counter on this pass, held or not
    /// (`sb zero,0x21b` / `sh zero,0x176`): the park of the playing clip is
    /// released. The host takes it once.
    pub creature_loop_release: bool,
    /// `FUN_801DCEAC(caster[+0x1DD], ..)` - the same centroid and extent
    /// over the cast's own target code (a seat, or the group code `8` /
    /// `9`), as the host last measured it. PROT 0911's arms 9 and 10 frame
    /// the healed party through it.
    pub target_aim: Option<legaia_engine_battle_vm::battle_target_group::GroupAim>,
    /// The live yaw global `0x8007B792` as the host last saw it, for an arm
    /// that re-arms a shot on the yaw it has been drifting or stores a yaw
    /// straight into the global (PROT 0909 arm 13, PROT 0911 arm 10, PROT
    /// 0913 arms 3 and 4). `None` where the host has no camera.
    pub live_yaw: Option<u16>,
    /// Module-image words a director reads that the module's ported tick
    /// body owns, fed by the host each tick: PROT 0910's hit counter
    /// `0x801F8DAC` and fourth-slash progress `0x801F8DA8`, which pick its
    /// arm-10 sway framing.
    pub module_words: [i32; 2],
    /// The victim's `+0x16E & 4` (non-targetable) as the host last read it.
    /// PROT 0908's arm 11 raises its creature's floor clip only inside the
    /// branch a targetable victim takes (`bne v0,zero,0x801F7D9C` at
    /// `0x801F7C04`).
    pub victim_untargetable: bool,
    /// A module's arm-0 kill verdict as the host drew it, held for the whole
    /// cast: `Some(true)` on the kill path. PROT 0916's word `0x801F9010`
    /// (`0` kills) picks between arm `0x11`'s instant kill and arm `0x12`'s
    /// damage. `None` until the host has drawn it.
    pub kill: Option<bool>,
    /// The spread the module's inline light bands draw at on this pass
    /// (PROT 0916's arms `0x11..=0x13`), `None` on a pass that draws none.
    /// The director rewrites it every pass.
    pub band_spread: Option<i16>,
    /// The creature seat's `+0x1BA` colour word as the module's arms own it
    /// (PROT 0916 arms 4..6 assemble the creature), `None` while no arm has
    /// written it. The host mirrors it onto the seat.
    pub creature_shatter: Option<u16>,
    /// What the arm adds to the victim's `+0x1BA` on this pass (PROT 0916's
    /// arm `0x13` breaks a killed victim up). Rewritten every pass.
    pub victim_shatter_step: u16,
    /// The host's answer to a victim-settle poll (PROT 0916's arm `0x14`):
    /// a dead victim's playing clip back to `0`, a living one's tint word
    /// `+0x04` cleared.
    pub victim_settled: bool,
    /// The caster's target code `+0x1DD` as the host last read it: a seat
    /// below `8`, a group code from `8` (PROT 0914 and 0929 frame a group
    /// through `FUN_801DCEAC` and a seat otherwise). `None` where the host
    /// cannot read it.
    pub target_code: Option<u8>,
    /// The creature's anim-node cursor `+0x68` in its 12.4 unit, as the host
    /// last saw it: the frame waits of PROT 0914's arms 4..6 and 8 and PROT
    /// 0931's arm 22 read it. `None` where the host plays no clip on the
    /// seat (a frame wait then passes).
    pub creature_frame: Option<u16>,
    /// An arm re-armed the creature's loop window on this pass (`sb 0xFF,
    /// 0x21B` / `sh 0xFF0, 0x176`: PROT 0914 arms `0xC` / `0xE`, PROT 0923
    /// arms 9..`0xB`, PROT 0928 arms 8 / 9). The host takes it once.
    pub creature_loop_arm: bool,
    /// A cursor store an arm made on the creature's node this pass (PROT
    /// 0928's `sh 0xF0, 0x68`). The host takes it once.
    pub creature_seek: Option<u16>,
    /// An anim-rate store `+0x21D` an arm made on the creature this pass
    /// (PROT 0914's arm `0xC` ramp). The host takes it once.
    pub creature_rate: Option<u8>,
    /// The arm a bounded host wait (a frame or park wait) is counting,
    /// and how many passes it has held.
    pub wait: (u8, u16),
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
    /// `true` when the arm framed the active actor through the action SM's
    /// case 8 (`FUN_801D5854(ctx[+0x13], 8)`) on this pass - the post-action
    /// framing on the caster and its target, whose dead-target arm also
    /// zeroes the yaw ladder `ctx[+0x6DA]`.
    pub end_frame: bool,
    /// The drift the arm wrote straight into the camera globals, if any.
    pub nudge: Option<ModuleNudge>,
    /// The `FUN_80021B04` calls the arm made on this pass, in call order.
    pub spawns: &'static [ModuleSpawn],
    /// The point and angle trio the arm built on its stack for a spawn whose
    /// anchor is [`SpawnAnchor::ArmPoint`] - an arm whose `a0` / `a1` are
    /// arithmetic over several seats and the band's own words.
    pub arm_point: Option<([i16; 3], [i16; 3])>,
    /// The `MoveImage` (`FUN_80058490`) the arm issued on this pass, if any.
    pub vram_move: Option<ModuleVramMove>,
    /// The text the arm put up on this pass (`FUN_8003541C`), if any.
    pub caption: Option<ModuleCaption>,
    /// The full-screen fades the arm spawned on this pass
    /// (`FUN_80024E80(0x801C9070, id)` over the template it wrote), in call
    /// order, each `(template, id)`.
    pub fades: &'static [(
        legaia_engine_battle_vm::battle_action::SummonFadeTemplate,
        i16,
    )],
    /// The arm killed the module's earlier fades (`ori 0x8` into each fade
    /// actor's flag word) before it spawned its own.
    pub kills_fades: bool,
    /// The phase a passing arm stores instead of `phase + 1` (a `sb` of a
    /// literal into `ctx+0x279`), for a director that owns the phase.
    pub jump: Option<u8>,
    /// The arm's direct writes into the camera globals, yaw `0x8007B792`
    /// included, folded into a shot in flight like a capture arm's
    /// ([`CaptureDrift`]): added every pass, held or not.
    pub drift: Option<CaptureDrift>,
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
    /// No text: the arm called `FUN_800319A8(0)` on its own, freeing the text
    /// actor it put up and printing nothing after it. Every player Seru
    /// module ends its line this way ahead of the band's exit (PROT 0903
    /// `0x801F74C8`, 0905 `0x801F802C`, 0906 `0x801F75D4`, 0910
    /// `0x801F7BC4`, 0911 `0x801F7ED0`, 0913 `0x801F8460`).
    Cleared,
    /// A string in the module's own image, by slot-B VA: PROT 0927 arm 21
    /// prints `0x801F69D8`, the bytes ahead of its arm table
    /// (`lui a2,0x801F; addiu a2,a2,0x69D8` at `0x801F7708`). The host reads
    /// it off the resident image.
    ModuleString(u32),
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
    /// `roll` is a literal the arm stored over the angle trio's `z`
    /// (`sp+0x24`) before the call, kept by every later call off the same
    /// stack: PROT 0905's fourth glow is seated a quarter of a right angle
    /// round (`li v0,0x200` / `sh v0,0x24(sp)` in the delay slot of the
    /// `jal` at `0x801F6C7C`), so its four-point star crosses the third's
    /// and the two read as one eight-point star.
    ShotPoint { y: i16, roll: Option<i16> },
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
    /// The creature seat's plane pair at the literal height `y`, with no
    /// rotation: the arm copied the creature's `+0x34` / `+0x38` (or its body
    /// pair) into a zeroed stack trio and stored `y` over its middle
    /// halfword. PROT 0908 arms 4, 6 and 11.
    CreatureGround { y: i16 },
    /// The creature seat's plane pair moved by `(dx, dz)`, at the literal
    /// height `y`, on the literal angle trio `rot`: the arm copied the seat's
    /// `+0x34..+0x3B` into its stack trio (`lwl` / `lwr`), stored `y` over
    /// the middle halfword and stepped the plane pair between calls, with the
    /// angle trio its own. PROT 0912 arms 2, 4 and 12.
    CreatureOffset {
        dx: i16,
        y: i16,
        dz: i16,
        rot: [i16; 3],
    },
    /// The creature seat itself (`a0 = creature + 0x34`, `a1 = creature +
    /// 0x44`, facing kept), read before the arm's later store over its
    /// height: the plane pair the arm has just written, at the literal `y`
    /// the seat still held at the call. PROT 0903 arm 3 makes its three calls
    /// (`0x801F6E04..0x801F6E34`) after storing `+0x34` / `+0x38` / `+0x46`
    /// and before `sh 0x200,0x36(s3)` (`0x801F6E48`), so the parts sit at the
    /// height `FUN_801F19EC` seated the creature at: the caster's `+0x36`,
    /// the floor.
    CreatureBeforeHeight { y: i16 },
    /// The creature seat lifted by `dy` (`creature + 0x34` copied to the
    /// stack, `dy` added to its middle halfword), on the creature's angle
    /// trio `+0x44`. PROT 0911 arm 5's pass, `dy = -0x140`
    /// (`0x801F76B4..0x801F7700`).
    CreatureRaised { dy: i16 },
    /// The caster seat `actor_table[ctx + 0x13]` at the literal height `y`,
    /// on the caster's angle trio `+0x44`. PROT 0911 arm 2: the plane pair
    /// copied off the caster, `-0x3E8` stored over the height
    /// (`0x801F6C14..0x801F6C74`).
    Caster { y: i16 },
    /// The caster seat's display trio `+0x3C..+0x40` (the live pair plus the
    /// pose centroid) on its angle trio `+0x44`: `a0 = caster + 0x3C`,
    /// `a1 = caster + 0x44`. PROT 0946's Call Wave arm 0 (`0x801F6C38`).
    CasterBody,
    /// The caster seat's `+0x34..+0x3B` copied to the stack (`lwl` / `lwr`)
    /// with `dy` / `dz` added to its height and depth halfwords, on the
    /// caster's angle trio `+0x44`: PROT 0961 arms 2 and 3
    /// (`0x801F6F34..0x801F6F70`, `0x801F7148..0x801F7184`), which lower
    /// the point by `0x200` on both in the evolved Cort's formation.
    CasterShifted { dy: i16, dz: i16 },
    /// The arm's shot focus turned back into a world point at the literal
    /// height `y`, turned to the creature's facing plus `yaw`: PROT 0911 arm
    /// 9 negates the `FUN_801DCEAC` centroid it framed (`0x801F7968..
    /// 0x801F7980`) and stores `creature[+0x46] + 0x800` as the yaw
    /// (`0x801F7984..0x801F7998`).
    ShotPointFacing { y: i16, yaw: i16 },
    /// A literal point, rotation and scale (`FUN_80050ED4`'s `a3`): a stager
    /// call that forwards its caller's `+0x72` (PROT 0911's arm `1`,
    /// `lh a3,0x72(v1)` at `0x801F8028`).
    LiteralScaled {
        pos: [i16; 3],
        rot: [i16; 3],
        scale: u16,
    },
    /// The caster seat's position at the literal height `y`, with the zeroed
    /// angle trio the arm's prologue left on its stack: PROT 0913 arm 2 seats
    /// its two vortex records on the creature the installer `FUN_801F19EC`
    /// has just stood on the caster (`a0 = creature + 0x34`, `a1 = sp+0x20`,
    /// `0x801F6D2C..0x801F6D54`).
    CasterUnturned { y: i16 },
    /// The creature seat turned to its facing plus `yaw` (`sh v1,0x22(sp)`
    /// of `creature[+0x46] + yaw`): PROT 0913 arm 10's cloud banks
    /// (`0x801F7824..0x801F787C`).
    CreatureYaw { yaw: i16 },
    /// A point in the creature's own frame: its plane pair less
    /// `trunc(sin(h) / div)` / `trunc(cos(h) / div)` along its heading `h`,
    /// moved `trunc(cos(h) / side)` / `-trunc(sin(h) / side)` across it
    /// (`side == 0`: not), then `trunc(sin(h) / extra)` / `trunc(cos(h) /
    /// extra)` further back (`extra == 0`: not), at the literal height `y`,
    /// on the creature's angle trio turned by `yaw`, with `roll` stored over
    /// the trio's `z` (`sp+0x24`). PROT 0910 arm 5 builds its five points
    /// this way (`0x801F71FC..0x801F7628`: `/ 64` for the ring's emitter,
    /// `/ 12` and `/ 40` for the hands, `/ 32` past them); PROT 0913 arm 8
    /// its twelve (`0x801F7310..0x801F76BC`: `/ 26`, then `/ 12` and `/ 11`
    /// each pair's last two rolled).
    CreatureFrame {
        div: i16,
        side: i16,
        extra: i16,
        y: i16,
        yaw: i16,
        roll: i16,
    },
    /// The victim seat at the framing depth `ctx[+0x6D0]` as the scale
    /// (`lh a3,0x6D0(v0)`): PROT 0913 arms 13, 18 and 19.
    VictimDepthScaled,
    /// Ring `index` of PROT 0913 arm 13's sixteen-ring whirlwind
    /// (`0x801F7B90..0x801F7C5C`): the victim's plane pair at
    /// `y = -0x40 * index`, a random yaw (`rand() % 0xFFF`), and the scale
    /// `(|cos(0x380 + 16 index)| + 0x800) * ctx[+0x6D0] >> 12` off the SCUS
    /// table `*0x8007B7F8` (`lh v0,0x700(v0)` over `index << 5`).
    VictimColumn { index: u8 },
    /// Party seat `n` (`actor_table[n]`, `a0 = seat + 0x34`, `a1 = seat +
    /// 0x44`), resolved only while the seat holds a live member
    /// (`+0x14C != 0`). PROT 0911 arm 9's loop over the party
    /// (`0x801F7A1C..0x801F7A40`).
    PartySeat(u8),
    /// A literal point and rotation the arm built on its stack: PROT 0909
    /// arm 2's two parts at `(0, -0x100, 0x600)` (`0x801F6C40..0x801F6C70`).
    Literal { pos: [i16; 3], rot: [i16; 3] },
    /// The point and angle trio the director computed for this pass
    /// ([`ArmDirection::arm_point`]): PROT 0907's arms, whose spawn points
    /// are the grid cell off the victim, a point ahead of the creature, or a
    /// point back from the victim by the band's depth.
    ArmPoint,
    /// The creature seat (`a0 = creature + 0x34`, `a1 = creature + 0x44`) at
    /// the scale `a3`: PROT 0923 arm 4's four calls (`0x801F7118..
    /// 0x801F7168`, three at `0x400`).
    CreatureScaled { scale: u16 },
    /// The victim's plane pair moved by `(dx, dz)` at the literal height `y`,
    /// with the arm's zeroed angle trio: PROT 0915 arm 3 copies the victim's
    /// `+0x34..+0x3B` (`lwl` / `lwr`), stores `-0x190` over the height and
    /// pulls `z` back `0x800` (`0x801F6D40..0x801F6D78`).
    VictimOffset { dx: i16, y: i16, dz: i16 },
    /// The victim seat's position (`a0 = victim + 0x34`) on the arm's zeroed
    /// angle trio: PROT 0917 arm 13's five calls (`0x801F7EB8..0x801F7F18`).
    VictimFlat,
    /// The caster seat's position on the creature's angle trio (`a0 =
    /// caster + 0x34`, `a1 = creature + 0x44`): PROT 0914 arm `0xB`
    /// (`0x801F7300..0x801F7324`).
    CasterCreatureRot,
}

/// The seat a [`SpawnAnchor::AlongVictimHeading`] point starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadingBase {
    Victim,
    Creature,
}

/// The seats an anchor may name beyond the creature and the victim: the
/// caster and the live party members (`None` for an empty or dead seat).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnchorSeats {
    pub caster: Option<ModuleSeat>,
    /// The caster's display trio `+0x3C..+0x40`, facing its `+0x46`.
    pub caster_body: Option<ModuleSeat>,
    pub party: [Option<ModuleSeat>; 3],
    /// The framing depth `ctx[+0x6D0]`, the scale PROT 0913 passes.
    pub depth: i16,
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
    spawn_anchor_point_with(anchor, creature, victim, shot, AnchorSeats::default())
}

/// [`spawn_anchor_point`] with the caster and party seats as well.
pub fn spawn_anchor_point_with(
    anchor: SpawnAnchor,
    creature: Option<ModuleSeat>,
    victim: Option<ModuleSeat>,
    shot: Option<ModuleShot>,
    seats: AnchorSeats,
) -> Option<([i16; 3], [i16; 3])> {
    let seat_at = |s: ModuleSeat| ([s.x, s.y, s.z], [0, s.facing as i16, 0]);
    Some(match anchor {
        SpawnAnchor::CreatureRaised { dy } => {
            let c = creature?;
            ([c.x, c.y.wrapping_add(dy), c.z], [0, c.facing as i16, 0])
        }
        SpawnAnchor::Caster { y } => {
            let c = seats.caster?;
            ([c.x, y, c.z], [0, c.facing as i16, 0])
        }
        SpawnAnchor::CasterBody => seat_at(seats.caster_body?),
        SpawnAnchor::CasterShifted { dy, dz } => {
            let c = seats.caster?;
            (
                [c.x, c.y.wrapping_add(dy), c.z.wrapping_add(dz)],
                [0, c.facing as i16, 0],
            )
        }
        SpawnAnchor::ShotPointFacing { y, yaw } => {
            let s = shot?;
            let f = creature.map_or(0, |c| c.facing as i16);
            (
                [s.focus[0].wrapping_neg(), y, s.focus[2].wrapping_neg()],
                [0, f.wrapping_add(yaw), 0],
            )
        }
        SpawnAnchor::PartySeat(n) => seat_at((*seats.party.get(usize::from(n))?)?),
        SpawnAnchor::LiteralScaled { pos, rot, .. } => (pos, rot),
        SpawnAnchor::CasterUnturned { y } => {
            let c = seats.caster?;
            ([c.x, y, c.z], [0; 3])
        }
        SpawnAnchor::CreatureYaw { yaw } => {
            let c = creature?;
            ([c.x, c.y, c.z], [0, (c.facing as i16).wrapping_add(yaw), 0])
        }
        SpawnAnchor::VictimDepthScaled => seat_at(victim?),
        SpawnAnchor::CreatureFrame {
            div,
            side,
            extra,
            y,
            yaw,
            roll,
        } => {
            let c = creature?;
            let (sin, cos) = trig12(c.facing & 0xFFF);
            let (sin, cos) = (i32::from(sin), i32::from(cos));
            let q = |v: i32, d: i16| if d == 0 { 0 } else { v / i32::from(d) };
            let x = i32::from(c.x) - q(sin, div) - q(cos, side) - q(sin, extra);
            let z = i32::from(c.z) - q(cos, div) + q(sin, side) - q(cos, extra);
            (
                [x as i16, y, z as i16],
                [0, (c.facing as i16).wrapping_add(yaw), roll],
            )
        }
        SpawnAnchor::VictimColumn { index } => {
            let v = victim?;
            (
                [v.x, -0x40 * i16::from(index), v.z],
                [0, nova_column_yaw(index), 0],
            )
        }
        SpawnAnchor::Creature => seat_at(creature?),
        SpawnAnchor::Victim => seat_at(victim?),
        SpawnAnchor::ShotFocus => {
            let s = shot?;
            (s.focus, s.angles)
        }
        SpawnAnchor::ShotPoint { y, roll } => {
            let s = shot?;
            (
                [s.focus[0].wrapping_neg(), y, s.focus[2].wrapping_neg()],
                [s.angles[0], 0, roll.unwrap_or(s.angles[2])],
            )
        }
        SpawnAnchor::CreatureGround { y } => {
            let c = creature?;
            ([c.x, y, c.z], [0; 3])
        }
        SpawnAnchor::CreatureOffset { dx, y, dz, rot } => {
            let c = creature?;
            ([c.x.wrapping_add(dx), y, c.z.wrapping_add(dz)], rot)
        }
        SpawnAnchor::CreatureBeforeHeight { y } => {
            let c = creature?;
            ([c.x, y, c.z], [0, c.facing as i16, 0])
        }
        SpawnAnchor::Literal { pos, rot } => (pos, rot),
        SpawnAnchor::CreatureScaled { .. } => seat_at(creature?),
        SpawnAnchor::VictimOffset { dx, y, dz } => {
            let v = victim?;
            ([v.x.wrapping_add(dx), y, v.z.wrapping_add(dz)], [0; 3])
        }
        SpawnAnchor::VictimFlat => {
            let v = victim?;
            ([v.x, v.y, v.z], [0; 3])
        }
        SpawnAnchor::CasterCreatureRot => {
            let c = seats.caster?;
            let f = creature.map_or(0, |k| k.facing as i16);
            ([c.x, c.y, c.z], [0, f, 0])
        }
        // The host resolves it from the arm's own direction.
        SpawnAnchor::ArmPoint => return None,
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

/// The scale one spawn call seats its record at (`a3`, stored at `+0x72` by
/// `FUN_80021B04` at `0x80021DAC`): `0x1000` at nearly every call; PROT
/// 0911 arm 9 passes the healed group's extent (`lh a3,0x2C(sp)`, the TR z
/// of the shot it armed), and a [`SpawnAnchor::LiteralScaled`] call its own.
pub fn spawn_anchor_scale(
    anchor: SpawnAnchor,
    shot: Option<ModuleShot>,
    seats: AnchorSeats,
) -> u16 {
    match anchor {
        SpawnAnchor::VictimDepthScaled => seats.depth as u16,
        SpawnAnchor::VictimColumn { index } => {
            let (_, cos) = trig12(0x380 + 16 * u16::from(index));
            let k = i32::from(cos).abs() + 0x800;
            ((k * i32::from(seats.depth)) >> 12) as u16
        }
        SpawnAnchor::ShotPointFacing { .. } => shot
            .map(|s| s.tr[2])
            .filter(|&z| z > 0)
            .map_or(0x1000, |z| z as u16),
        SpawnAnchor::LiteralScaled { scale, .. } => scale,
        SpawnAnchor::CreatureScaled { scale } => scale,
        _ => 0x1000,
    }
}

/// The yaw PROT 0913 arm 13 gives whirlwind ring `index`: retail draws
/// `rand() % 0xFFF` per ring (`0x801F7BBC..0x801F7BF4`); the port runs the
/// BIOS `rand` shape from a fixed seed so the stack's spread is stable.
pub fn nova_column_yaw(index: u8) -> i16 {
    let mut state = 0x3039u32.wrapping_add(u32::from(index).wrapping_mul(0x9E37));
    let r = legaia_engine_system::rand::psyq_rand_step(&mut state);
    (u32::from(r) % 0xFFF) as i16
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
/// `0x8007B790`, TR x `0x800840B8`, TR y `0x800840BC`, TR z `0x800840C0` -
/// added every pass, held or not, on top of whatever the last shot landed.
/// The TR z delta is in the global's own (prescaled) units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModuleNudge {
    pub pitch: i16,
    pub tr_x: i16,
    pub tr_y: i16,
    pub tr_z: i16,
}

impl ArmDirection {
    pub(super) const HOLD: Self = Self {
        hold: true,
        shot: None,
        follow: None,
        end_frame: false,
        nudge: None,
        spawns: &[],
        arm_point: None,
        vram_move: None,
        caption: None,
        fades: &[],
        kills_fades: false,
        jump: None,
        drift: None,
    };
    pub(super) const PASS: Self = Self {
        hold: false,
        shot: None,
        follow: None,
        end_frame: false,
        nudge: None,
        spawns: &[],
        arm_point: None,
        vram_move: None,
        caption: None,
        fades: &[],
        kills_fades: false,
        jump: None,
        drift: None,
    };
    pub(super) fn shot(shot: ModuleShot) -> Self {
        Self {
            shot: Some(shot),
            ..Self::PASS
        }
    }
    pub(super) fn drifted(self, drift: CaptureDrift) -> Self {
        Self {
            drift: Some(drift),
            ..self
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

mod aluru;
mod body_kit;
mod body_paced;
pub mod capture;
pub mod capture_countdown;
mod capture_spawns;
pub mod creature;
mod creature_spawns;
mod evil_seru_magic;
mod freed;
mod gilium_palma_horn;
mod gola_mushura_barra;
mod jedo_iota_mule;
mod juggernaut;
mod kemaro;
mod meta_terra_ozma;
mod nighto;
mod nova;
mod orb;
mod puera;
mod seru;
pub mod summon_holds;
mod swordie;
mod theeder_gizam;
mod viguro;
pub use aluru::*;
pub use capture::*;
pub use capture_countdown::{ArmCountdown, CountdownWrite, arm_countdown, capture_arm_countdowns};
pub use capture_spawns::*;
pub use creature::*;
pub use creature_spawns::*;
pub use evil_seru_magic::*;
pub use freed::*;
pub use gilium_palma_horn::*;
pub use gola_mushura_barra::*;
pub use jedo_iota_mule::*;
pub use juggernaut::*;
pub use kemaro::*;
pub use meta_terra_ozma::*;
pub use nighto::*;
pub use nova::*;
pub use orb::*;
pub use puera::*;
pub use seru::*;
pub use swordie::*;
pub use theeder_gizam::*;
pub use viguro::*;

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
    /// passed it, the hit / restore has been applied. `None` for a director
    /// that only arms the camera over arms it does not pace: the band's
    /// length then stays the engine stager's.
    pub hit_arm: Option<u8>,
    /// The phase arm the module walks its creature in, if it walks one: the
    /// host starts the creature's walk there and reports its arrival back
    /// through [`ModuleCamState::creature_arrived`]. A module with none keeps
    /// its creature where it seated it.
    pub walk_arm: Option<u8>,
    /// Whether the director owns the module phase. `false` for a
    /// director that runs **beside** a ported tick body
    /// ([`ModuleProfile::body_paced`]): it reads the phase the body is
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
    /// the module seats it at once.
    pub seat_arm: Option<u8>,
    /// Whether the director **is** the module's phase chain: a module whose
    /// arms are countdown gates and camera writes, with no tick body ported
    /// beside it. The director then owns the phase (a pass advances it, or
    /// stores [`ArmDirection::jump`]), its holds hold `0x36` like a ported
    /// body's ([`Self::hit_arm`] is set), and the band runs until the phase
    /// reaches `0xFF`.
    pub director_body: bool,
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
            director_body: false,
        }),
        // PROT 0904's director gates arms 2..6 and 14 and arms the camera
        // beside the ported tick body, which paces arms 7..13 itself
        // (`crate::seru_magic_ticks::theeder_tick`). The outcome lands once
        // the sweep has turned.
        904 => Some(ModuleProfile {
            direct: theeder_direct,
            hit_arm: Some(crate::seru_magic_ticks::THEEDER_SWEEP_ARM),
            walk_arm: None,
            owns_phase: false,
            stages_spawns: false,
            seat_arm: module_seat_arm(904),
            director_body: false,
        }),
        // PROT 0906's director gates every counted arm beside the ported
        // tick body; its phase-5 clock stands in for the spawned record's
        // stager call that releases the phase hole. It reports arms 4, 7
        // and 12's spawn calls.
        906 => Some(ModuleProfile {
            direct: gizam_direct,
            hit_arm: Some(crate::seru_magic_ticks::GIZAM_SWEEP_ARM),
            walk_arm: None,
            owns_phase: false,
            stages_spawns: true,
            seat_arm: module_seat_arm(906),
            director_body: false,
        }),
        905 => Some(ModuleProfile {
            direct: vera_direct,
            hit_arm: Some(VERA_RESTORE_ARM),
            walk_arm: None,
            owns_phase: true,
            stages_spawns: true,
            seat_arm: module_seat_arm(905),
            director_body: false,
        }),
        // PROT 0907's countdowns, clip gate, camera and spawn calls, beside
        // its ported tick body; the outcome lands once the fork arm has
        // passed.
        907 => Some(ModuleProfile {
            direct: nighto_direct,
            hit_arm: Some(crate::seru_magic_ticks::NIGHTO_FORK_ARM),
            walk_arm: None,
            owns_phase: true,
            stages_spawns: true,
            seat_arm: module_seat_arm(907),
            director_body: false,
        }),
        908 => Some(ModuleProfile {
            direct: zenoir_direct,
            hit_arm: Some(ZENOIR_FINISH_ARM),
            walk_arm: None,
            owns_phase: true,
            stages_spawns: true,
            seat_arm: module_seat_arm(908),
            director_body: false,
        }),
        // PROT 0909's countdowns and camera ride beside its ported tick body;
        // its two rendezvous arms are released by the stager its own parts
        // call, so the parts are seated on the arms that spawn them.
        909 => Some(ModuleProfile {
            direct: viguro_direct,
            hit_arm: Some(crate::seru_magic_ticks::VIGURO_DAMAGE_PHASE),
            walk_arm: None,
            owns_phase: true,
            stages_spawns: true,
            seat_arm: None,
            director_body: false,
        }),
        // The summon-creature modules' directors carry their whole phase
        // chain (`gola_mushura_barra`, `gilium_palma_horn`, `jedo_iota_mule`,
        // `meta_terra_ozma`): each paces the band and lands the outcome once
        // the phase passes its hit arm.
        914 => Some(
            ModuleProfile::director_body(gola_gola_direct, GOLA_GOLA_HIT_ARM)
                .with_staged_spawns()
                .seated_at(914),
        ),
        915 => Some(
            ModuleProfile::director_body(mushura_direct, MUSHURA_HIT_ARM)
                .with_staged_spawns()
                .seated_at(915),
        ),
        917 => Some(
            ModuleProfile::director_body(barra_direct, BARRA_HIT_ARM)
                .with_staged_spawns()
                .seated_at(917),
        ),
        // PROT 0920's arms are countdown gates and camera writes end to end,
        // so its director carries the whole phase chain and paces the band:
        // the rain the draw tick runs between arms 5 and `0xFF` needs the
        // band to last that long.
        920 => {
            Some(ModuleProfile::director_body(slippery_direct, SLIPPERY_DAMAGE_ARM).seated_at(920))
        }
        923 => Some(
            ModuleProfile::director_body(gilium_direct, GILIUM_HIT_ARM)
                .with_staged_spawns()
                .seated_at(923),
        ),
        928 => Some(
            ModuleProfile::director_body(palma_direct, PALMA_HIT_ARM)
                .with_staged_spawns()
                .seated_at(928),
        ),
        930 => Some(
            ModuleProfile::director_body(horn_direct, HORN_HEAL_ARM)
                .with_staged_spawns()
                .seated_at(930),
        ),
        931 => Some(
            ModuleProfile::director_body(jedo_direct, JEDO_HIT_ARM)
                .with_staged_spawns()
                .seated_at(931),
        ),
        // PROT 0916's director carries the whole chain; the damage path's
        // hit lands once the phase passes arm `0x12`, and the kill path's
        // HP store is the host's at arm `0x11`.
        916 => Some(
            ModuleProfile::director_body(aluru_direct, ALURU_HIT_ARM)
                .with_staged_spawns()
                .seated_at(916),
        ),
        921 => Some(
            ModuleProfile::director_body(iota_direct, IOTA_HIT_ARM)
                .with_staged_spawns()
                .seated_at(921),
        ),
        929 => Some(
            ModuleProfile::director_body(mule_direct, MULE_HIT_ARM)
                .with_staged_spawns()
                .seated_at(929),
        ),
        932 => Some(
            ModuleProfile::director_body(meta_direct, META_HIT_ARM)
                .with_staged_spawns()
                .seated_at(932),
        ),
        933 => Some(
            ModuleProfile::director_body(terra_direct, TERRA_HIT_ARM)
                .with_staged_spawns()
                .seated_at(933),
        ),
        934 => Some(
            ModuleProfile::director_body(ozma_direct, OZMA_HIT_ARM)
                .with_staged_spawns()
                .seated_at(934),
        ),
        // PROT 0910..0913's tick bodies hold their own arms (countdowns,
        // ramps and seat polls, `crate::seru_magic_ticks`), so each paces the
        // band like PROT 0904's: `0x36` holds on the body, the creature walks
        // nowhere the module does not move it, and the outcome lands once the
        // phase has passed the body's hit arm. Each director arms the camera
        // beside its body on the arm clock of `body_paced`.
        910 => Some(
            ModuleProfile::body_paced(swordie_direct, crate::seru_magic_ticks::SWORDIE_SLASH_ARM)
                .with_staged_spawns()
                .seated_at(910),
        ),
        911 => Some(
            ModuleProfile::body_paced(orb_direct, crate::seru_magic_ticks::ORB_HEAL_ARM)
                .with_staged_spawns()
                .seated_at(911),
        ),
        // PROT 0912's spawn calls are seated on the body's own passes
        // ([`freed_arm_spawns`]), not off the director.
        912 => Some(ModuleProfile {
            stages_spawns: true,
            ..ModuleProfile::body_paced(freed_direct, crate::seru_magic_ticks::FREED_DAMAGE_PHASE)
                .seated_at(912)
        }),
        913 => Some(
            ModuleProfile::body_paced(nova_direct, crate::seru_magic_ticks::NOVA_DAMAGE_PHASE)
                .with_staged_spawns()
                .seated_at(913),
        ),
        // Ported tick bodies paced by their countdown holds alone.
        918 | 919 | 922 | 924 | 925 | 927 => summon_holds::summon_hold_profile(prot_entry),
        _ => None,
    }
}

impl ModuleProfile {
    const fn director_body(direct: ModuleDirector, hit_arm: u8) -> Self {
        Self {
            direct,
            hit_arm: Some(hit_arm),
            walk_arm: None,
            owns_phase: true,
            stages_spawns: false,
            seat_arm: None,
            director_body: true,
        }
    }

    /// A director beside a ported tick body whose own holds pace the band
    /// (PROT 0904's shape): the body owns the phase, the director only arms
    /// the camera, and the outcome lands once the phase passes `hit_arm`.
    const fn body_paced(direct: ModuleDirector, hit_arm: u8) -> Self {
        Self {
            direct,
            hit_arm: Some(hit_arm),
            walk_arm: None,
            owns_phase: false,
            stages_spawns: false,
            seat_arm: None,
            director_body: false,
        }
    }

    /// The director reports its arms' spawn calls
    /// ([`ArmDirection::spawns`]), so the stager seats no record set of its
    /// own.
    const fn with_staged_spawns(mut self) -> Self {
        self.stages_spawns = true;
        self
    }

    const fn seated_at(mut self, prot_entry: u32) -> Self {
        self.seat_arm = module_seat_arm(prot_entry);
        self
    }

    /// Whether this director paces the band (the module's own tick body is
    /// ported and its holds hold `0x36`).
    pub const fn paces_band(&self) -> bool {
        self.hit_arm.is_some()
    }
}

/// The arm of a summon module's tick body that holds on the creature
/// stream's CD read, `FUN_8003F2B8(1)` non-zero branching to the busy
/// return: PROT 0910's and 0913's arm 2 (`jal 0x8003F2B8` at `0x801F6CDC` /
/// `0x801F6D0C`, then `bne v0,zero`), behind arm 1's seek
/// `FUN_8003EAE4(0, 0xC)`. Retail spends as many vsyncs there as the drive
/// takes; the engine's stream is resident, so the arm passes at once. A
/// capture taken on the arm is placed by the arm alone
/// (`legaia_engine_capture`'s phase gate).
///
pub const fn stream_poll_arm(prot_entry: u32) -> Option<u8> {
    match prot_entry {
        910 | 913 => Some(2),
        _ => None,
    }
}

/// Every arm of a summon module's tick body that polls the creature
/// stream's CD read `FUN_8003F2B8(1)`: [`stream_poll_arm`]'s, and PROT
/// 0905's arm 3 (`jal 0x8003F2B8` at `0x801F6E60`, then `bne v0,zero`),
/// behind arm 2's seek `FUN_8003EAE4(0, 7)`. PROT 0905's arm-1 shot walks on
/// through the wait, so a capture there stands as far into the shot as
/// retail's read lasted. Unlike PROT 0910 / 0913's, the flash's age still
/// places such a frame (`vera_summon_mid_cast`: gated on the arm alone, the
/// frame read `.971` against `.996`).
pub const fn creature_poll_arm(prot_entry: u32) -> Option<u8> {
    match prot_entry {
        905 => Some(3),
        _ => stream_poll_arm(prot_entry),
    }
}

/// The arm of a summon module's tick body that waits on the side-band
/// applier stage `ctx[+0x276]` ([`ModuleCamState::sideband_busy`]): PROT
/// 0930's arm 2 returns busy while the byte is up (`lbu v0,0x276(v0)` /
/// `bne v0,zero` at `0x801F6D60..0x801F6D68`) and, once it drops, issues its
/// creature seek `FUN_8003EAE4(0, 0xE)` and steps on. Retail spends as many
/// vsyncs there as the turn's side-band read takes -
/// `horn_summon_mid_cast` sits in it 28 vsyncs into the sustain, three
/// sparkle volleys in - while the engine's read is resident. PROT 0929's
/// arm 1 makes the same test (`0x801F6D94..0x801F6DA8`) behind its anchor
/// push, then seeks `FUN_8003EAE4(0, 0xE)`; PROT 0928's arm 1 the same
/// (`0x801F6D58..0x801F6D60`), and PROT 0931's arm 2
/// (`0x801F6D18..0x801F6D20`): `jedo_summon_mid_cast` holds that arm one
/// pass into the sustain with the byte at `8`.
pub const fn sideband_wait_arm(prot_entry: u32) -> Option<u8> {
    match prot_entry {
        928 | 929 => Some(1),
        930 | 931 => Some(2),
        _ => None,
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
