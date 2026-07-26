//! The field overlay's **scripted-scene actor**: four voice-over cutscene
//! programs sharing one 38-state jump table.
//!
//! PORT: FUN_801D4A60
//!
//! ## What it is (and what it is not)
//!
//! It is **not** the "scripted actor-approach controller" the docs used to
//! call it. `FUN_801D4A60` opens with `lh v1,0x54(s1)` / `sltiu v1,0x26` /
//! `jr v0` against a 38-word table at `0x801CE960`, and the entry state
//! computes its own successor as
//!
//! ```text
//! actor[+0x54] = (actor[+0x54] + 1) + actor[+0x50] * 10
//! ```
//!
//! (`sll v1,a0,2; addu v1,v1,a0; sll v1,v1,1` = `×10`). So `+0x50` is a
//! **program selector** and the state space is four programs on a stride of
//! ten, entered at `1`, `11`, `21`, `31`. That is exactly where the table's
//! live entries sit, and the gaps `6..=10`, `16..=20`, `27..=30` - fifteen
//! slots all pointing at the epilogue - are the unused tails of each block,
//! not fifteen separate dead states. Reading the table without the `×10`
//! makes the function look like a sparse mess; with it, it is four short
//! programs.
//!
//! What the programs do is stage move-VM effect parts at the player, lock the
//! player, swap the BGM, and play a **CD-XA voice stream**: program 2 reaches
//! `FUN_80019794(0x10)` at `0x801D4FCC`, which `docs/subsystems/audio.md`
//! already catalogues as clip `XA17`, "scripted-scene voice stream", citing
//! this exact call site - an attribution made from the audio side that lands
//! on this function from the other direction.
//!
//! ## Provenance, and why the old reading was wrong
//!
//! The static `overlay_0897_801d4a60.txt` dump stops at **690** instructions.
//! Five independent live-RAM field captures agree on **756**, and so does
//! capstone over `extracted/overlays/overlay_field_0897.bin` at the committed
//! base `0x801CE818` (file `0x006248`), which decodes 756 instructions ending
//! on the `jr ra` epilogue at `0x801D5628`. The 66 instructions the short dump
//! drops are states `0x22`..`0x25` and the shared tail - i.e. most of program
//! 3 - which is how a four-program state machine came to be described as one
//! approach controller.
//!
//! ## Structure
//!
//! Three shapes recur, and naming them is what makes the 23 live arms short:
//!
//! - **snapshot** (prologue, and again inside state `0x18`): copy the player's
//!   `+0x14..+0x1B` and `+0x24..+0x2B` into two stack vectors through
//!   unaligned `lwl`/`lwr` pairs, then bias the position's Y by `-0x40`.
//!   Those two vectors are the `(pos, rot)` arguments of every part stage.
//! - **stage-per-vsync**: `for _ in 0..DAT_1F800393 { FUN_80021B04(pos, rot,
//!   record, 0x1000) }` - one stage per vsync the game tick spans, so the
//!   emission rate is cadence-invariant.
//! - **accumulate**: `actor[+0x9E] += DAT_1F800393`, then compare
//!   `(i16)actor[+0x9E]` against a per-state threshold; below it the arm
//!   returns, at or above it the arm does its one-shot work and advances
//!   through the shared tail `0x801D5594` (`+0x9E = 0`, `+0x54 += 1`).
//!
//! Several arms **fall through** into the next state inside the same call -
//! `1→2→3→4`, `11→12`, `21→22`, `23→24`, `31→32→33` - because they end by
//! bumping `+0x54` without a jump and the arms are laid out in state order.
//! [`step_scene_program`] models that with a loop rather than one arm per call, which is the
//! difference between a program reaching its voice cue on the frame retail
//! reaches it and four frames later.
//!
//! REF: FUN_80021B04 (part stager), FUN_8003CE08 / FUN_8003CE34 / FUN_8003CE64
//! (story-flag set / clear / test), FUN_80035B50 (SFX), FUN_8003D53C (CD-XA
//! one-shot), FUN_80019794 (CD-XA whole-clip stream), FUN_801EE328 (the dev
//! warp applier whose rise-up arm is the same `+0x8E`/`+0x16` idiom)

/// Number of jump-table entries (`sltiu v1,0x26`).
pub const STATE_COUNT: u16 = 0x26;

/// Distance between the entry states of two consecutive programs.
pub const PROGRAM_STRIDE: u16 = 10;

/// Programs the table actually carries (`+0x50` in `0..PROGRAM_COUNT`).
pub const PROGRAM_COUNT: u16 = 4;

/// Y bias applied to the snapshotted player position before it is used as a
/// part-stage origin (`0x801D4AD0`: `addiu v0,v0,-0x40`).
pub const SNAPSHOT_Y_BIAS: i16 = -0x40;

/// The per-frame "ambient" part record every program stages while it runs.
pub const RECORD_TICK: u32 = 0x801F_2658;

/// Part-record pairs the one-shot arms stage. Each is `(first, second)` in
/// retail's emission order.
pub const RECORD_PAIR_A: (u32, u32) = (0x801F_2498, 0x801F_250C);
/// Staged by states `0x0C` and `0x21`.
pub const RECORD_PAIR_B: (u32, u32) = (0x801F_23C8, 0x801F_2430);
/// Staged by states `0x0D` and `0x22`.
pub const RECORD_PAIR_C: (u32, u32) = (0x801F_22F8, 0x801F_2360);
/// Staged by state `0x17`, alongside the voice cue.
pub const RECORD_PAIR_D: (u32, u32) = (0x801F_2580, 0x801F_25EC);

/// Scale argument every `FUN_80021B04` call passes (`a3 = 0x1000`).
pub const STAGE_SCALE: i32 = 0x1000;

/// BGM track the programs request (`_DAT_8007BABC = 0x7F3`).
pub const BGM_TRACK: i32 = 0x7F3;

/// The request values state `0x02` treats as "already one of ours" and only
/// waits on: `0x7F3..=0x7F5` (`addiu v0,v1,-0x7f3; sltiu v0,v0,3`).
pub const BGM_ACCEPTED: std::ops::RangeInclusive<i32> = BGM_TRACK..=(BGM_TRACK + 2);

/// CD-XA clip id the voice programs stream (`XA17`).
pub const VOICE_CLIP: u8 = 0x10;
/// Channel of the one-shot voice cue.
pub const VOICE_CHANNEL: u8 = 7;
/// Duration of the one-shot voice cue.
pub const VOICE_DURATION: u16 = 0x135;

/// SFX fired by state `0x03`.
pub const SFX_OPENING: u16 = 0x200;
/// SFX fired by states `0x0B` and `0x20`.
pub const SFX_BEAT: u16 = 0x1B;

/// Story flags the programs drive (`FUN_8003CE08` / `FUN_8003CE34` ids).
pub const FLAG_PLAYER_BUSY: u8 = 0x0B;
/// Set by program 1's opener, cleared by program 3's closer.
pub const FLAG_PROGRAM_1: u8 = 0x0C;
/// Set by program 0's opener, cleared by program 2's closer.
pub const FLAG_SCENE_ACTIVE: u8 = 0x17;
/// Cleared by both openers; tested by both closers before they release the
/// player.
pub const FLAG_RELEASE_GUARD: u8 = 0x18;

/// Scratchpad story-flag bit (`_DAT_1F800394 & 0x01000000`) state `0x17` sets
/// and state `0x1A` clears.
pub const STORY_FLAG_BIT: u32 = 0x0100_0000;

/// Player `+0x10` bit meaning "engaged by a script" - the same bit the text
/// balloon's early-out watches.
pub const PLAYER_ENGAGED: u32 = 0x0008_0000;
/// Player `+0x10` bit raised while a program owns the player's motion.
pub const PLAYER_MOTION_HELD: u32 = 0x0020_0000;
/// Player `+0x10` bit raised for the lift leg only.
pub const PLAYER_LIFTING: u32 = 0x2000_0000;

/// Target the lift leg drives `player[+0x8E]` to before winding it back down
/// (`negu v0; addiu v0,v0,0x618`).
pub const LIFT_TARGET_BASE: i16 = 0x618;

/// Per-vsync ceiling on the lift wind-down step.
pub const LIFT_STEP_MAX: i16 = 0x10;

/// The literal the clamp test compares against (`slti v0,v1,0x11`): a computed
/// step *below* this is kept, anything else becomes [`LIFT_STEP_MAX`]. Kept as
/// its own constant because the disassembly spells the test `< 0x11` rather
/// than `<= 0x10` - the same predicate, but only one of the two is what the
/// bytes say.
pub const LIFT_STEP_CLAMP_TEST: i16 = 0x11;

/// Rounding bias and shift of the wind-down step (`addiu v0,v0,0x1f; sra
/// v1,v0,5`).
pub const LIFT_STEP_BIAS: i16 = 0x1F;
/// Shift of the wind-down step.
pub const LIFT_STEP_SHIFT: u32 = 5;

/// Spawn descriptor the programs' actor comes from (field `0x023EC0`; its
/// `+8` handler word is `FUN_801D4A60` itself).
pub const SPAWN_DESCRIPTOR: u32 = 0x801F_26D8;

/// The two `(system flag, program)` pairs the scene MAN loader spawns on.
///
/// PORT: FUN_8003AEB0 (`0x8003BAF0..0x8003BB3C`)
///
/// The loader tests two bits of the shared flag bank `DAT_80085758` and calls
/// `FUN_801D5A24(program)` for each that is set. The bank is MSB-first
/// (`byte[idx >> 3] & (0x80 >> (idx & 7))`), so `0x8008575A & 0x01` is flag
/// `0x17` and `0x80085759 & 0x08` is flag `0x0C`.
///
/// Reading the pairs alongside what each program does gives the family its
/// shape: **flag `0x17` is what program 0 sets and program 2 clears, and flag
/// `0x0C` is what program 1 sets and program 3 clears.** So the loader is not
/// starting cutscenes - it is *finishing* ones a scene change interrupted.
/// Programs 0 and 1 are openers, 2 and 3 their closers, and the flag is the
/// handshake that survives the scene boundary.
pub const MAN_LOAD_RESUME: [(u8, u16); 2] = [(FLAG_SCENE_ACTIVE, 2), (FLAG_PROGRAM_1, 3)];

/// Seat a fresh program actor.
///
/// PORT: FUN_801D5A24 (`0x801d5a24..0x801d5a64`)
///
/// Seventeen instructions: allocate from [`SPAWN_DESCRIPTOR`] against the
/// generic effect-actor list, then `+0x54 = 0` and `+0x50 = program`. State
/// zero is the entry arm, so the actor picks its block on its first tick
/// rather than at spawn - which is why the spawner takes the program and not
/// the state.
pub fn spawn_program(program: u16) -> ProgramActor {
    ProgramActor {
        program,
        state: 0,
        ..Default::default()
    }
}

/// A 3-vector as the snapshot builds it.
pub type Vec3 = (i16, i16, i16);

/// The SM's own actor fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProgramActor {
    /// `+0x50` - program selector, read only by the entry state.
    pub program: u16,
    /// `+0x54` - state word.
    pub state: u16,
    /// `+0x9E` - the accumulator every threshold compares.
    pub accum: u16,
    /// `+0x72` - the player's speed multiplier, parked here while a program
    /// holds the player at zero.
    pub saved_speed: u16,
    /// `+0x16` - the player's terrain-conform angle at lift start, kept as the
    /// arrival test of the wind-down leg.
    pub angle: i16,
    /// `+0x10` - flag word (the retire bit lands here).
    pub flags: u32,
}

/// The player-actor fields a program reads and writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProgramPlayer {
    /// `+0x14 / +0x16 / +0x18`.
    pub pos: Vec3,
    /// `+0x24 / +0x26 / +0x28`.
    pub rot: Vec3,
    /// `+0x10` - flag word.
    pub flags: u32,
    /// `+0x72` - speed multiplier.
    pub speed: u16,
    /// `+0x8E` - the lift accumulator.
    pub lift: i16,
}

/// Everything outside the two actors that a program reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramEnv {
    /// `DAT_1F800393` - vsyncs this game tick spans.
    pub frame_delta: u8,
    /// `_DAT_8007BABC` - the requested BGM track.
    pub bgm_request: i32,
    /// `_DAT_8007BAA0` - the track the driver has acknowledged.
    pub bgm_current: i32,
    /// `_DAT_8007B868` - the dev/retail discriminator (retail `0`).
    pub dev_flags: u32,
    /// `_DAT_8007BC20` - non-zero while a CD-XA stream is in flight.
    pub xa_busy: u32,
    /// `_DAT_1F800394` - the scratchpad story-flag word.
    pub story_flags: u32,
    /// `FUN_8003CE64(FLAG_RELEASE_GUARD)` - whether the guard flag is set.
    pub release_guard_set: bool,
}

/// One side effect a step produced, in emission order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramEffect {
    /// `FUN_80021B04(pos, rot, record, 0x1000)`.
    StagePart { record: u32, pos: Vec3, rot: Vec3 },
    /// `FUN_8003CE08(id)`.
    SetFlag(u8),
    /// `FUN_8003CE34(id)`.
    ClearFlag(u8),
    /// `FUN_80035B50(id)`.
    Sfx(u16),
    /// `FUN_8003D53C(clip, chan, dur)`.
    XaCue { clip: u8, chan: u8, dur: u16 },
    /// `FUN_80019794(clip)` - the whole-clip stream request.
    XaStream { clip: u8 },
    /// `_DAT_8007BABC = track`.
    RequestBgm(i32),
}

/// What one call to [`step_scene_program`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramStep {
    /// The SM actor after the step.
    pub actor: ProgramActor,
    /// The player after the step.
    pub player: ProgramPlayer,
    /// `_DAT_1F800394` after the step.
    pub story_flags: u32,
    /// Effects in retail emission order.
    pub effects: Vec<ProgramEffect>,
    /// `true` when the step set the actor's own retire bit (`+0x10 |= 8`).
    pub retired: bool,
}

/// Actor flag bit the closers set on themselves.
const RETIRE_BIT: u32 = 0x8;

/// The prologue snapshot: the player transform as every part stage sees it.
///
/// PORT: FUN_801D4A60 (`0x801d4a7c..0x801d4ad4`)
///
/// The unaligned `lwl`/`lwr` pairs copy eight bytes from each source, so the
/// third component of each vector is genuinely read (the fourth halfword is
/// carried along and never used). Only the position's Y takes the `-0x40`
/// bias; the rotation vector is copied verbatim.
pub fn snapshot(player: &ProgramPlayer) -> (Vec3, Vec3) {
    let pos = (
        player.pos.0,
        player.pos.1.wrapping_add(SNAPSHOT_Y_BIAS),
        player.pos.2,
    );
    (pos, player.rot)
}

/// The successor state the entry arm computes.
///
/// PORT: FUN_801D4A60 (`0x801d4b08..0x801d4b30`)
///
/// `(state + 1) + program * 10`. Called with `state == 0` in practice, which
/// is what maps program `p` onto entry state `1 + 10p`; the `+ 1` is on the
/// *current* state, not a constant, so the arithmetic is kept general.
///
/// NOT WIRED: its only non-test caller is [`step_scene_program`], the entry
/// arm it belongs to, and nothing ticks that - see its note for the specific
/// missing input (the BGM request/acknowledge pair and the CD-XA in-flight
/// counter that three of its states park on). This helper needs no host of its
/// own; it becomes live the moment the program is stepped.
pub fn entry_successor(state: u16, program: u16) -> u16 {
    state
        .wrapping_add(1)
        .wrapping_add(program.wrapping_mul(PROGRAM_STRIDE))
}

/// The wind-down step of the lift leg, per vsync.
///
/// PORT: FUN_801D4A60 (`0x801d50dc..0x801d5108`)
///
/// `step = (player.lift + actor.angle + 0x1F) >> 5`, clamped to
/// [`LIFT_STEP_MAX`]. The clamp test is spelled `< 0x11`
/// ([`LIFT_STEP_CLAMP_TEST`]) rather than `<= 0x10` - the same predicate, but
/// only one of the two is what the bytes say. The shift is arithmetic, so a
/// negative sum steps the lift *up*; retail relies on that not happening
/// because the leg only runs after the lift was seeded positive.
///
/// NOT WIRED: same blocker as [`entry_successor`] above - its only non-test
/// caller is [`step_scene_program`]'s lift leg, and nothing ticks that program.
pub fn lift_step(lift: i16, angle: i16) -> i16 {
    let raw = (i32::from(lift) + i32::from(angle) + i32::from(LIFT_STEP_BIAS)) >> LIFT_STEP_SHIFT;
    let raw = raw as i16;
    if raw < LIFT_STEP_CLAMP_TEST {
        raw
    } else {
        LIFT_STEP_MAX
    }
}

/// Control flow out of one arm.
enum Flow {
    /// Return from the whole function.
    Done,
    /// Continue executing at the *next* state's arm in this same call.
    FallThrough,
    /// `0x801D5594`: `+0x9E = 0`, `+0x54 += 1`, then return.
    Advance,
    /// `0x801D55E0`: test the guard flag, release the player if clear, retire.
    CloseOut,
    /// `0x801D5608`: retire without the guard test.
    Retire,
}

/// Run the scripted-scene actor for one game tick.
///
/// PORT: FUN_801D4A60 (whole function, `0x801d4a60..0x801d562c`, 756
/// instructions)
///
/// A state outside `0..STATE_COUNT` falls straight to the epilogue and nothing
/// happens - retail's `sltiu` bound with no default arm.
///
/// NOT WIRED: the actor is spawned (`World::man_load_actor_reset` runs
/// [`MAN_LOAD_RESUME`] on every scene load, so a resumed program's actor is on
/// the pool carrying [`ActorHandler::ScriptedScene`]), but nothing ticks it.
/// The specific missing input is [`ProgramEnv`]'s middle three fields: the BGM
/// request/acknowledge pair `_DAT_8007BABC` / `_DAT_8007BAA0` and the CD-XA
/// in-flight counter `_DAT_8007BC20` have no `engine-core` counterparts, and
/// states `0x02`, `0x16` and `0x19` each park indefinitely on them - a step
/// driven with invented values would either stall the program forever or run
/// it through its voice line in one frame. The rest is present: the player
/// transform, the flag bank ([`crate::world::World::system_flag_test`]), the
/// cadence byte and the actor's own `+0x50`/`+0x54`/`+0x9E` all have engine
/// homes.
///
/// Named `step_scene_program` and not `step` on purpose, and it must stay that
/// way. `port-catalog.py` never gates a *free* function edge - the receiver
/// gate is defined over `impl_type`, which a free function has none of - so a
/// free `fn step` collects an in-edge from both of the other things in this
/// tree that spell `step`: the live `engine_vm::motion_vm::step`, and every
/// reachable function that merely names a local or parameter `step` (the
/// browser's `LegaiaMinigames::fishing_advance_cast(&mut self, step: i32)` was
/// the one that fired). Under the old name this correct `NOT WIRED:` was
/// reported as a stale disclosure.
///
/// [`ActorHandler::ScriptedScene`]: crate::actor_handler::ActorHandler::ScriptedScene
pub fn step_scene_program(
    actor: ProgramActor,
    player: ProgramPlayer,
    env: ProgramEnv,
) -> ProgramStep {
    let mut a = actor;
    let mut p = player;
    let mut story_flags = env.story_flags;
    let mut fx: Vec<ProgramEffect> = Vec::new();
    let mut retired = false;
    let dt = u16::from(env.frame_delta);
    let (mut pos, mut rot) = snapshot(&p);

    // Stage `record` once per vsync this tick spans. A zero `frame_delta`
    // stages nothing, which is retail's `beqz` skip and not an edge case: the
    // cadence byte is genuinely zero on the frames the game does not advance.
    macro_rules! stage_per_vsync {
        ($record:expr) => {
            for _ in 0..env.frame_delta {
                fx.push(ProgramEffect::StagePart {
                    record: $record,
                    pos,
                    rot,
                });
            }
        };
    }
    macro_rules! stage_once {
        ($record:expr) => {
            fx.push(ProgramEffect::StagePart {
                record: $record,
                pos,
                rot,
            })
        };
    }
    // `+0x9E += dt`, then the signed compare against a threshold. `true` means
    // "not there yet" (retail's taken branch back to the epilogue).
    macro_rules! accumulate_below {
        ($threshold:expr) => {{
            a.accum = a.accum.wrapping_add(dt);
            (a.accum as i16) < $threshold
        }};
    }

    if a.state >= STATE_COUNT {
        return ProgramStep {
            actor: a,
            player: p,
            story_flags,
            effects: fx,
            retired,
        };
    }

    loop {
        let flow = match a.state {
            // Entry: pick the program's block and stop for this frame.
            0x00 => {
                a.accum = 0;
                a.state = entry_successor(a.state, a.program);
                Flow::Done
            }

            // ---- program 0: states 1..=5 ----
            0x01 => {
                fx.push(ProgramEffect::SetFlag(FLAG_SCENE_ACTIVE));
                fx.push(ProgramEffect::ClearFlag(FLAG_RELEASE_GUARD));
                a.state = a.state.wrapping_add(1);
                Flow::FallThrough
            }
            0x02 => {
                // Request the program's BGM, then wait for the driver to
                // acknowledge it. The `0x7F3..=0x7F5` window is "the request
                // already belongs to this family", so a program that follows
                // another one does not re-request.
                if BGM_ACCEPTED.contains(&env.bgm_request) {
                    if env.bgm_request != env.bgm_current {
                        Flow::Done
                    } else {
                        a.state = a.state.wrapping_add(1);
                        Flow::FallThrough
                    }
                } else {
                    if env.bgm_request == env.bgm_current || env.bgm_current == -1 {
                        fx.push(ProgramEffect::RequestBgm(BGM_TRACK));
                    }
                    Flow::Done
                }
            }
            0x03 => {
                fx.push(ProgramEffect::Sfx(SFX_OPENING));
                a.state = a.state.wrapping_add(1);
                Flow::FallThrough
            }
            0x04 => {
                stage_per_vsync!(RECORD_TICK);
                if accumulate_below!(0x28) {
                    Flow::Done
                } else {
                    stage_once!(RECORD_PAIR_A.0);
                    stage_once!(RECORD_PAIR_A.1);
                    fx.push(ProgramEffect::ClearFlag(FLAG_PLAYER_BUSY));
                    Flow::Advance
                }
            }
            0x05 => {
                // Terminal idle: stage the ambient record forever. Retail
                // clobbers `s1` (the actor pointer) with the record base here
                // and never dereferences it again - noted because it is the
                // one arm where the actor register does not survive.
                stage_per_vsync!(RECORD_TICK);
                Flow::Done
            }

            // ---- program 1: states 11..=15 ----
            0x0B => {
                fx.push(ProgramEffect::SetFlag(FLAG_PROGRAM_1));
                fx.push(ProgramEffect::ClearFlag(FLAG_RELEASE_GUARD));
                fx.push(ProgramEffect::Sfx(SFX_BEAT));
                a.state = a.state.wrapping_add(1);
                Flow::FallThrough
            }
            0x0C => {
                stage_per_vsync!(RECORD_TICK);
                if accumulate_below!(0x14) {
                    Flow::Done
                } else {
                    stage_once!(RECORD_PAIR_B.0);
                    stage_once!(RECORD_PAIR_B.1);
                    Flow::Advance
                }
            }
            0x0D => {
                stage_per_vsync!(RECORD_TICK);
                if accumulate_below!(0x32) {
                    Flow::Done
                } else {
                    stage_once!(RECORD_PAIR_C.0);
                    stage_once!(RECORD_PAIR_C.1);
                    Flow::Advance
                }
            }
            0x0E => {
                stage_per_vsync!(RECORD_TICK);
                if accumulate_below!(0x14) {
                    Flow::Done
                } else {
                    fx.push(ProgramEffect::ClearFlag(FLAG_PLAYER_BUSY));
                    p.speed = 0;
                    p.flags |= PLAYER_MOTION_HELD;
                    Flow::Advance
                }
            }
            0x0F => {
                stage_once!(RECORD_TICK);
                if accumulate_below!(0x64) {
                    Flow::Done
                } else {
                    Flow::Retire
                }
            }

            // ---- program 2: states 21..=26 ----
            0x15 => {
                p.flags |= PLAYER_ENGAGED;
                fx.push(ProgramEffect::SetFlag(FLAG_PLAYER_BUSY));
                a.saved_speed = p.speed;
                p.speed = 0;
                p.flags |= PLAYER_MOTION_HELD;
                a.accum = 0;
                a.state = a.state.wrapping_add(1);
                Flow::FallThrough
            }
            0x16 => {
                // The dev build skips the BGM handshake outright; retail waits
                // for the acknowledge, then starts the whole-clip voice stream.
                if env.dev_flags != 0 || env.bgm_request == env.bgm_current {
                    fx.push(ProgramEffect::XaStream { clip: VOICE_CLIP });
                    Flow::Advance
                } else {
                    Flow::Done
                }
            }
            0x17 => {
                if accumulate_below!(0x28) {
                    Flow::Done
                } else {
                    story_flags |= STORY_FLAG_BIT;
                    p.flags |= PLAYER_LIFTING;
                    // Latch the player's angle, seed the lift, and hand the
                    // player back the speed this program parked.
                    a.angle = p.pos.1;
                    p.lift = LIFT_TARGET_BASE.wrapping_sub(p.pos.1);
                    p.speed = a.saved_speed;
                    stage_once!(RECORD_PAIR_D.0);
                    stage_once!(RECORD_PAIR_D.1);
                    fx.push(ProgramEffect::XaCue {
                        clip: VOICE_CLIP,
                        chan: VOICE_CHANNEL,
                        dur: VOICE_DURATION,
                    });
                    a.accum = 0;
                    a.state = a.state.wrapping_add(1);
                    Flow::FallThrough
                }
            }
            0x18 => {
                // Wind the lift down, one clamped step per vsync, mirroring
                // its negation into the player's angle - the player rises.
                for _ in 0..env.frame_delta {
                    let s = lift_step(p.lift, a.angle);
                    p.lift = p.lift.wrapping_sub(s);
                }
                p.pos.1 = p.lift.wrapping_neg();
                // Retail re-snapshots the transform here rather than reusing
                // the prologue's, so the parts staged below follow the rising
                // player instead of trailing a frame behind.
                let s = snapshot(&p);
                pos = s.0;
                rot = s.1;
                stage_per_vsync!(RECORD_TICK);
                if a.angle != p.pos.1 {
                    Flow::Done
                } else {
                    fx.push(ProgramEffect::ClearFlag(FLAG_PLAYER_BUSY));
                    p.flags &= !PLAYER_MOTION_HELD;
                    p.flags &= !PLAYER_LIFTING;
                    Flow::Advance
                }
            }
            0x19 => {
                stage_once!(RECORD_TICK);
                a.accum = a.accum.wrapping_add(dt);
                if env.xa_busy != 0 || (a.accum as i16) < 0x30 {
                    Flow::Done
                } else {
                    Flow::Advance
                }
            }
            0x1A => {
                fx.push(ProgramEffect::ClearFlag(FLAG_SCENE_ACTIVE));
                story_flags &= !STORY_FLAG_BIT;
                Flow::CloseOut
            }

            // ---- program 3: states 31..=37 ----
            0x1F => {
                p.flags |= PLAYER_ENGAGED;
                fx.push(ProgramEffect::SetFlag(FLAG_PLAYER_BUSY));
                a.saved_speed = p.speed;
                p.speed = 0;
                p.flags |= PLAYER_MOTION_HELD;
                a.accum = 0;
                a.state = a.state.wrapping_add(1);
                Flow::FallThrough
            }
            0x20 => {
                if accumulate_below!(0x28) {
                    Flow::Done
                } else {
                    fx.push(ProgramEffect::Sfx(SFX_BEAT));
                    a.accum = 0;
                    a.state = a.state.wrapping_add(1);
                    Flow::FallThrough
                }
            }
            0x21 => {
                stage_per_vsync!(RECORD_TICK);
                if accumulate_below!(0x14) {
                    Flow::Done
                } else {
                    stage_once!(RECORD_PAIR_B.0);
                    stage_once!(RECORD_PAIR_B.1);
                    Flow::Advance
                }
            }
            0x22 => {
                stage_per_vsync!(RECORD_TICK);
                if accumulate_below!(0x32) {
                    Flow::Done
                } else {
                    stage_once!(RECORD_PAIR_C.0);
                    stage_once!(RECORD_PAIR_C.1);
                    Flow::Advance
                }
            }
            0x23 => {
                stage_per_vsync!(RECORD_TICK);
                if accumulate_below!(0x14) {
                    Flow::Done
                } else {
                    fx.push(ProgramEffect::ClearFlag(FLAG_PLAYER_BUSY));
                    p.speed = a.saved_speed;
                    p.flags &= !PLAYER_MOTION_HELD;
                    Flow::Advance
                }
            }
            0x24 => {
                stage_once!(RECORD_TICK);
                if accumulate_below!(0x40) {
                    Flow::Done
                } else {
                    Flow::Advance
                }
            }
            0x25 => {
                if accumulate_below!(0x20) {
                    Flow::Done
                } else {
                    fx.push(ProgramEffect::ClearFlag(FLAG_PROGRAM_1));
                    Flow::CloseOut
                }
            }

            // The fifteen table slots that point straight at the epilogue -
            // the unused tails of the four ten-wide program blocks.
            _ => Flow::Done,
        };

        match flow {
            Flow::Done => break,
            Flow::FallThrough => continue,
            Flow::Advance => {
                a.accum = 0;
                a.state = a.state.wrapping_add(1);
                break;
            }
            Flow::CloseOut => {
                if !env.release_guard_set {
                    p.flags &= !PLAYER_ENGAGED;
                }
                a.flags |= RETIRE_BIT;
                retired = true;
                break;
            }
            Flow::Retire => {
                a.flags |= RETIRE_BIT;
                retired = true;
                break;
            }
        }
    }

    ProgramStep {
        actor: a,
        player: p,
        story_flags,
        effects: fx,
        retired,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> ProgramEnv {
        ProgramEnv {
            frame_delta: 2,
            bgm_request: 0,
            bgm_current: 0,
            dev_flags: 0,
            xa_busy: 0,
            story_flags: 0,
            release_guard_set: false,
        }
    }

    fn player() -> ProgramPlayer {
        ProgramPlayer {
            pos: (0x100, 0x200, 0x300),
            rot: (1, 2, 3),
            flags: 0,
            speed: 0x1000,
            lift: 0,
        }
    }

    fn actor(program: u16) -> ProgramActor {
        ProgramActor {
            program,
            ..Default::default()
        }
    }

    #[test]
    fn the_entry_state_lands_each_program_on_its_own_block() {
        // The `x10` is the whole reason the table looks sparse. Every live
        // block in the jump table must be an entry_successor of some program.
        for p in 0..PROGRAM_COUNT {
            assert_eq!(entry_successor(0, p), 1 + 10 * p);
        }
        assert_eq!(
            (0..PROGRAM_COUNT)
                .map(|p| entry_successor(0, p))
                .collect::<Vec<_>>(),
            vec![1, 11, 21, 31]
        );
        // And every block start is inside the table's bound.
        assert!(entry_successor(0, PROGRAM_COUNT - 1) < STATE_COUNT);
    }

    #[test]
    fn the_entry_state_stops_for_the_frame_rather_than_running_the_block() {
        // Retail's entry arm ends `j 0x801d5618` - it does NOT fall into the
        // program it just selected. Getting this wrong runs a program's first
        // two states on the frame it is chosen.
        let s = step_scene_program(actor(2), player(), env());
        assert_eq!(s.actor.state, 21);
        assert!(s.effects.is_empty(), "the entry arm emits nothing");
    }

    #[test]
    fn out_of_range_states_do_nothing_at_all() {
        for state in [STATE_COUNT, STATE_COUNT + 1, 0x1000] {
            let mut a = actor(0);
            a.state = state;
            let s = step_scene_program(a, player(), env());
            assert_eq!(s.actor, a, "state {state:#x} must be inert");
            assert!(s.effects.is_empty());
        }
    }

    #[test]
    fn the_unused_table_slots_are_inert_but_are_not_out_of_range() {
        // Fifteen in-range states point at the epilogue. They must behave
        // like the out-of-range case, and they must be exactly the tails of
        // the four blocks - if a future edit makes one of them do something,
        // this is where it shows up.
        let dead: Vec<u16> = (0..STATE_COUNT)
            .filter(|s| {
                let mut a = actor(0);
                a.state = *s;
                let out = step_scene_program(a, player(), env());
                out.actor == a && out.effects.is_empty() && !out.retired
            })
            .collect();
        assert_eq!(
            dead,
            vec![6, 7, 8, 9, 10, 16, 17, 18, 19, 20, 27, 28, 29, 30]
        );
    }

    #[test]
    fn program_0_falls_through_three_states_in_one_frame() {
        // States 1 -> 2 -> 3 -> 4 are one frame's work in retail, because
        // each ends by bumping `+0x54` with no jump. A per-state-per-frame
        // reading would take four frames to reach the first part stage.
        let mut a = actor(0);
        a.state = 1;
        let mut e = env();
        // Make the BGM gate pass immediately.
        e.bgm_request = BGM_TRACK;
        e.bgm_current = BGM_TRACK;
        let s = step_scene_program(a, player(), e);
        assert_eq!(s.actor.state, 4, "1,2,3 all ran; 4 is where it parked");
        assert!(
            s.effects
                .contains(&ProgramEffect::SetFlag(FLAG_SCENE_ACTIVE)),
            "state 1 ran"
        );
        assert!(
            s.effects.contains(&ProgramEffect::Sfx(SFX_OPENING)),
            "state 3 ran"
        );
        assert!(
            s.effects.iter().any(|f| matches!(
                f,
                ProgramEffect::StagePart {
                    record: RECORD_TICK,
                    ..
                }
            )),
            "state 4 staged the ambient record in the same frame"
        );
    }

    #[test]
    fn the_bgm_gate_requests_once_and_then_waits() {
        let mut a = actor(0);
        a.state = 2;
        let mut e = env();
        // Nothing requested yet and nothing playing: request the track.
        e.bgm_request = 0;
        e.bgm_current = 0;
        let s = step_scene_program(a, player(), e);
        assert_eq!(s.effects, vec![ProgramEffect::RequestBgm(BGM_TRACK)]);
        assert_eq!(s.actor.state, 2, "still waiting");
        // Requested but not acknowledged: no re-request, no advance.
        e.bgm_request = BGM_TRACK;
        e.bgm_current = 0;
        let s = step_scene_program(a, player(), e);
        assert!(s.effects.is_empty());
        assert_eq!(s.actor.state, 2);
        // Acknowledged: fall through into state 3's SFX.
        e.bgm_current = BGM_TRACK;
        let s = step_scene_program(a, player(), e);
        assert!(s.effects.contains(&ProgramEffect::Sfx(SFX_OPENING)));
    }

    #[test]
    fn a_program_that_inherits_a_sibling_track_does_not_re_request_it() {
        // The `0x7F3..=0x7F5` window: a request already in the family is
        // waited on, never overwritten. A naive `== 0x7F3` test would keep
        // stamping 0x7F3 over 0x7F4 and stall the handshake forever.
        for track in BGM_ACCEPTED {
            let mut a = actor(0);
            a.state = 2;
            let mut e = env();
            e.bgm_request = track;
            e.bgm_current = 0;
            let s = step_scene_program(a, player(), e);
            assert!(
                !s.effects
                    .iter()
                    .any(|f| matches!(f, ProgramEffect::RequestBgm(_))),
                "track {track:#x} was re-requested"
            );
        }
    }

    #[test]
    fn the_stage_rate_follows_the_cadence_byte() {
        // Retail stages once per vsync the tick spans, so the emission rate
        // is the same wall-clock rate at any cadence. A tick of zero stages
        // nothing at all.
        for dt in [0u8, 1, 2, 3] {
            let mut a = actor(0);
            a.state = 4;
            let mut e = env();
            e.frame_delta = dt;
            let s = step_scene_program(a, player(), e);
            let staged = s
                .effects
                .iter()
                .filter(|f| matches!(f, ProgramEffect::StagePart { record, .. } if *record == RECORD_TICK))
                .count();
            assert_eq!(staged, usize::from(dt), "dt {dt}");
        }
    }

    #[test]
    fn the_accumulator_threshold_is_reached_at_the_same_time_at_any_cadence() {
        // The property the whole `accum += DAT_1F800393` idiom exists for:
        // state 4's 0x28 threshold must arrive after 0x28 vsyncs whether the
        // game ticks every vsync or every third one.
        for dt in [1u8, 2, 3, 4] {
            let mut a = actor(0);
            a.state = 4;
            let mut e = env();
            e.frame_delta = dt;
            let mut vsyncs = 0u32;
            let mut guard = 0;
            loop {
                let s = step_scene_program(a, player(), e);
                vsyncs += u32::from(dt);
                a = s.actor;
                guard += 1;
                assert!(guard < 1000, "state 4 never advanced at dt {dt}");
                if a.state != 4 {
                    break;
                }
            }
            // The first tick that carries the accumulator to >= 0x28 fires,
            // so the span is 0x28 vsyncs rounded up to a whole tick.
            let expect = 0x28u32.div_ceil(u32::from(dt)) * u32::from(dt);
            assert_eq!(vsyncs, expect, "dt {dt}");
        }
    }

    #[test]
    fn program_2_engages_the_player_and_releases_it_again() {
        // The property that makes this a cutscene actor rather than a
        // decoration: whatever it does in between, the player must come out
        // unlocked and with its speed back. A program that never releases is
        // a softlock.
        let mut a = actor(2);
        let mut p = player();
        let original_speed = p.speed;
        let mut e = env();
        e.frame_delta = 4;
        let mut engaged_at_some_point = false;
        let mut guard = 0;
        loop {
            let s = step_scene_program(a, p, e);
            a = s.actor;
            p = s.player;
            e.story_flags = s.story_flags;
            if p.flags & PLAYER_ENGAGED != 0 {
                engaged_at_some_point = true;
            }
            guard += 1;
            assert!(guard < 5000, "program 2 never closed out");
            if s.retired {
                break;
            }
        }
        assert!(engaged_at_some_point, "the program must lock the player");
        assert_eq!(p.flags & PLAYER_ENGAGED, 0, "and unlock it on close-out");
        assert_eq!(p.flags & PLAYER_MOTION_HELD, 0);
        assert_eq!(p.flags & PLAYER_LIFTING, 0);
        assert_eq!(p.speed, original_speed, "the parked speed comes back");
        assert_eq!(e.story_flags & STORY_FLAG_BIT, 0, "and its story bit");
    }

    #[test]
    fn the_close_out_keeps_the_player_engaged_while_the_guard_flag_is_set() {
        // `FUN_8003CE64(0x18)` gates only the release, not the retire. A
        // close-out under a set guard still removes the actor - so a caller
        // that reads "retired" as "player is free" is wrong.
        let mut a = actor(2);
        a.state = 0x1A;
        let mut p = player();
        p.flags |= PLAYER_ENGAGED;
        let mut e = env();
        e.release_guard_set = true;
        let s = step_scene_program(a, p, e);
        assert!(s.retired);
        assert_ne!(s.player.flags & PLAYER_ENGAGED, 0, "guard held the release");
        e.release_guard_set = false;
        let s = step_scene_program(a, p, e);
        assert!(s.retired);
        assert_eq!(s.player.flags & PLAYER_ENGAGED, 0);
    }

    #[test]
    fn the_lift_step_is_clamped_and_always_makes_progress() {
        // A lift that never arrives is not a lift: from any positive height
        // the step must be non-zero, and it must never exceed the clamp.
        for lift in [1i16, 0x20, 0x100, 0x618, 0x2000] {
            let s = lift_step(lift, 0);
            assert!(s > 0, "lift {lift:#x} produced a zero step");
            assert!(s <= LIFT_STEP_MAX, "lift {lift:#x} step {s} over the clamp");
        }
        // The clamp bites exactly at a computed 0x11, not at 0x10.
        assert_eq!(lift_step(0x10 * 32 - LIFT_STEP_BIAS, 0), LIFT_STEP_MAX);
        assert_eq!(lift_step(0x11 * 32 - LIFT_STEP_BIAS, 0), LIFT_STEP_MAX);
        // And below the clamp it is the plain rounded shift.
        assert_eq!(lift_step(0x40, 0), (0x40 + LIFT_STEP_BIAS) >> 5);
    }

    #[test]
    fn the_lift_leg_ends_when_the_player_angle_returns_to_the_latched_one() {
        // State 0x18's exit test is `actor.angle == player.pos.1`, and the
        // player's angle is `-lift` - so the leg ends when the lift reaches
        // `-actor.angle`. With the latched angle 0 that is lift 0.
        let mut a = actor(2);
        a.state = 0x18;
        a.angle = 0;
        let mut p = player();
        p.pos.1 = 0;
        p.lift = 0x200;
        let mut e = env();
        e.frame_delta = 3;
        let mut guard = 0;
        loop {
            let s = step_scene_program(a, p, e);
            a = s.actor;
            p = s.player;
            guard += 1;
            assert!(guard < 500, "the lift leg never converged");
            if a.state != 0x18 {
                break;
            }
        }
        assert_eq!(p.lift, 0);
        assert_eq!(a.state, 0x19);
    }

    #[test]
    fn the_voice_stream_and_the_one_shot_cue_are_the_same_clip() {
        // Program 2 asks for XA17 twice by two different mechanisms - the
        // whole-clip stream in state 0x16 and the chunked cue in 0x17. If a
        // later edit splits them, the scene plays two different voice lines.
        let mut a = actor(2);
        a.state = 0x16;
        let mut e = env();
        e.dev_flags = 1;
        let s = step_scene_program(a, player(), e);
        assert!(
            s.effects
                .contains(&ProgramEffect::XaStream { clip: VOICE_CLIP })
        );

        let mut a = actor(2);
        a.state = 0x17;
        a.accum = 0x28;
        let s = step_scene_program(a, player(), env());
        assert!(s.effects.contains(&ProgramEffect::XaCue {
            clip: VOICE_CLIP,
            chan: VOICE_CHANNEL,
            dur: VOICE_DURATION,
        }));
    }

    #[test]
    fn the_voice_wait_holds_while_a_stream_is_in_flight() {
        // State 0x19 gates on `_DAT_8007BC20` BEFORE the threshold, so a long
        // clip holds the program open past 0x30 vsyncs rather than cutting
        // the line off.
        let mut a = actor(2);
        a.state = 0x19;
        a.accum = 0x100;
        let mut e = env();
        e.xa_busy = 1;
        assert_eq!(step_scene_program(a, player(), e).actor.state, 0x19);
        e.xa_busy = 0;
        assert_eq!(step_scene_program(a, player(), e).actor.state, 0x1A);
    }

    #[test]
    fn programs_1_and_3_share_their_two_part_pairs() {
        // States 0x0C/0x21 and 0x0D/0x22 are the same beats in two programs.
        // Asserting the record pairs match keeps a copy-paste divergence from
        // showing up as one program's effects going missing.
        let run = |state: u16| {
            let mut a = actor(0);
            a.state = state;
            a.accum = 0x100;
            step_scene_program(a, player(), env())
                .effects
                .into_iter()
                .filter_map(|f| match f {
                    ProgramEffect::StagePart { record, .. } if record != RECORD_TICK => {
                        Some(record)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(run(0x0C), vec![RECORD_PAIR_B.0, RECORD_PAIR_B.1]);
        assert_eq!(run(0x0C), run(0x21));
        assert_eq!(run(0x0D), vec![RECORD_PAIR_C.0, RECORD_PAIR_C.1]);
        assert_eq!(run(0x0D), run(0x22));
    }

    #[test]
    fn program_1_retires_itself_without_touching_the_player_lock() {
        // Program 1's closer is state 0x0F, which takes the bare retire path
        // (`0x801D5608`) - no guard test, no release. It is the one program
        // that never engages the player in the first place.
        let mut a = actor(1);
        a.state = 0x0F;
        a.accum = 0x64;
        let mut p = player();
        p.flags |= PLAYER_ENGAGED;
        let s = step_scene_program(a, p, env());
        assert!(s.retired);
        assert_ne!(
            s.player.flags & PLAYER_ENGAGED,
            0,
            "the bare retire path leaves the player flags alone"
        );
    }

    #[test]
    fn the_snapshot_biases_only_the_position_y() {
        let p = ProgramPlayer {
            pos: (10, 20, 30),
            rot: (40, 50, 60),
            ..Default::default()
        };
        let (pos, rot) = snapshot(&p);
        assert_eq!(pos, (10, 20 + SNAPSHOT_Y_BIAS, 30));
        assert_eq!(rot, (40, 50, 60));
    }

    #[test]
    fn the_lift_leg_restages_against_the_moved_player() {
        // Retail re-snapshots inside state 0x18 rather than reusing the
        // prologue's copy. The parts must therefore be staged at the player's
        // POST-step height, not its pre-step one.
        let mut a = actor(2);
        a.state = 0x18;
        a.angle = -0x100;
        let mut p = player();
        p.pos.1 = 0;
        p.lift = 0x200;
        let before = p.pos.1;
        let s = step_scene_program(a, p, env());
        assert_ne!(s.player.pos.1, before, "the leg moved the player");
        let staged_y = s
            .effects
            .iter()
            .find_map(|f| match f {
                ProgramEffect::StagePart { pos, .. } => Some(pos.1),
                _ => None,
            })
            .expect("the leg staged the ambient record");
        assert_eq!(staged_y, s.player.pos.1.wrapping_add(SNAPSHOT_Y_BIAS));
    }
}
