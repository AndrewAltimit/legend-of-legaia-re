//! The decoded op, the tick result and the `AmbientMotion` interpreter itself.

use super::*;

/// Every op of retail's 32-entry table at `0x80010FE8`, indexed `op - 1`.
///
/// Table slots `0x1A..=0x1F` all point at the epilogue `0x80039B44`, which
/// advances nothing: retail re-dispatches the same byte forever. They are
/// [`AmbientOp::from_byte`]'s `None`, and so is anything outside
/// `0x01..=0x20`, which the dispatch head rejects with `sltiu v1, 0x20`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmbientOp {
    /// `0x01` - reset the cursor and jump back to the variant's first op.
    Restart,
    /// `0x02 lo hi` - write the requested-move / anim pair, while the
    /// default-move record is still unset.
    AnimPair,
    /// `0x03` / `0x19` / `0x20` `b1 b2` - a straight-line directional step.
    DirStep,
    /// `0x04 b1 b2` - the in-VM facing ramp.
    FacingRamp,
    /// `0x05 frames` - wait. Always consumes the tick.
    Wait,
    /// `0x06 b1 b2 b3 b4` - one random full-tile step inside a box measured
    /// from the actor's anchor tile.
    HomeStep,
    /// `0x07 lo hi` - set a system story flag.
    FlagSet,
    /// `0x08 lo hi` - clear a system story flag.
    FlagClear,
    /// `0x09 lo hi` - queue an SFX cue id in the four-slot `DAT_8007B6D8`
    /// ring.
    SfxCue,
    /// `0x0A lo hi` - raise the translucent-draw bit, then the `0x02` tail.
    TranslucentOn,
    /// `0x0B lo hi` - drop the translucent-draw bit, then the `0x02` tail.
    TranslucentOff,
    /// `0x0C r g b m m d d` - fade the packed-RGB tint and the draw-mode
    /// word.
    TintFade,
    /// `0x0D b1 b2 b3` - pre-unwrap + hand the heading to the scheduler.
    FacingTween,
    /// `0x0E lo hi` - re-bind the actor's mesh from one of two model banks.
    ModelSwap,
    /// `0x0F b1 b2` - teleport to a tile centre and re-anchor.
    TileTeleport,
    /// `0x10 b1` - set a bit of one of five selectable halfwords.
    BitSet,
    /// `0x11 b1` - clear the same.
    BitClear,
    /// `0x12 b1` - wait for that bit to change. Always consumes the tick.
    BitWait,
    /// `0x13 x y w h dx dy` - a libgpu `MoveImage` VRAM blit.
    MoveImage,
    /// `0x14 v v d d` - tween the render scale `+0x72`.
    ScaleTween,
    /// `0x15 v v d d` - tween the X Euler angle `+0x24`.
    PitchTween,
    /// `0x16 v v d d` - tween the Z Euler angle `+0x28`.
    RollTween,
    /// `0x17 move anim` - write the actor's default-move record. Does not
    /// consume the tick.
    DefaultMove,
    /// `0x18 b1 b2 b3 b4` - bounded random wander.
    Wander,
}

impl AmbientOp {
    pub fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0x01 => Self::Restart,
            0x02 => Self::AnimPair,
            0x03 | 0x19 | 0x20 => Self::DirStep,
            0x04 => Self::FacingRamp,
            0x05 => Self::Wait,
            0x06 => Self::HomeStep,
            0x07 => Self::FlagSet,
            0x08 => Self::FlagClear,
            0x09 => Self::SfxCue,
            0x0A => Self::TranslucentOn,
            0x0B => Self::TranslucentOff,
            0x0C => Self::TintFade,
            0x0D => Self::FacingTween,
            0x0E => Self::ModelSwap,
            0x0F => Self::TileTeleport,
            0x10 => Self::BitSet,
            0x11 => Self::BitClear,
            0x12 => Self::BitWait,
            0x13 => Self::MoveImage,
            0x14 => Self::ScaleTween,
            0x15 => Self::PitchTween,
            0x16 => Self::RollTween,
            0x17 => Self::DefaultMove,
            0x18 => Self::Wander,
            _ => return None,
        })
    }
}

/// The "no default move installed" sentinel retail seeds a
/// `0x801C6470` record with (and the variant-swap preamble restores).
pub const DEFAULT_MOVE_UNSET: u8 = 0x8C;

/// Outcome of one [`AmbientMotion::tick`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmbientTick {
    /// The tick was consumed by a yielding op (or the op budget).
    Yield,
    /// The stream ran off its end, or hit an op whose width is unknown -
    /// retail would have kept interpreting garbage; the port stops.
    Done,
}

/// Live state of one actor's ambient facing channel.
#[derive(Debug, Clone)]
pub struct AmbientMotion {
    /// Retail `actor+0x84`: byte offset of the next op, relative to the
    /// slice handed to [`AmbientMotion::tick`].
    pub pc: u16,
    /// Retail `actor+0x8B | actor+0xB7 << 8`. `0x04` and `0x05` use the low
    /// byte only (and wrap it at 256, as retail's `sb` does); `0x0D` uses
    /// the full 16 bits.
    pub cursor: u16,
    /// Retail `actor+0x26`, **raw** - deliberately not masked into
    /// `0..0xFFF`. Read it `& 0xFFF` to render.
    pub heading: u16,
    /// Host tag for this actor, stamped onto the ramps it installs.
    pub owner: u32,
    pub ramps: RampScheduler,
    /// Retail `actor+0x14` - live world X.
    pub x: i16,
    /// Retail `actor+0x18` - live world Z.
    pub z: i16,
    /// Set by a tick whose walk op actually moved [`Self::x`] /
    /// [`Self::z`]. The position sibling of the heading's move gate.
    pub moved: bool,
    /// Set by a tick in which a **walk** op wrote the heading - the `0x03`
    /// facing snap, or the `0x18` wander's turn and walk phases.
    ///
    /// This separates the two facings the VM produces. The `0x04` / `0x0D`
    /// ramps are ambient turning in their own right; a walk op's heading
    /// write is *walk-direction-implied* facing and has no meaning apart
    /// from the step it accompanies. A host that suppresses the walking must
    /// suppress this facing with it, or an NPC pivots on the spot through a
    /// motion it never performs.
    pub walk_yaw: bool,
    /// Retail `0x801C6470[slot]` byte 2 - the `0x18` wander phase
    /// (`0` pick, `1` turn, `2` hand-off, `3` walk). It lives in the
    /// default-move arena rather than on the actor, which is why a `0x17`
    /// write and a wander share a record.
    pub wander_phase: u8,
    /// Retail `actor+0x86` bits 12-13 - the wander's chosen heading-LUT
    /// index, always one of the four cardinals `0`, `2`, `4`, `6`.
    pub wander_dir: u8,
    /// Retail `0x801C6470[slot]` bytes 0-1: `[move_id, anim_id]`, both
    /// [`DEFAULT_MOVE_UNSET`] until a `0x17` writes them.
    pub default_move: [u8; 2],
    /// Retail `actor+0x88` / `actor+0x5C` - the requested move / anim id the
    /// walk ops restamp from the default-move record. Hosts read it to drive
    /// the actor's animation stream.
    pub requested_move: Option<u8>,
    /// Seed for the `FUN_80056798` equivalent the `0x18` wander draws its
    /// direction and its continue-or-stop coin flip from.
    pub rng: u32,
    /// The motion VM's one-slot **touch mailbox**, retail `DAT_80073F1C`:
    /// the actor id the collision probe last posted through `FUN_8003D038`
    /// ([`crate::motion_vm::post_touch`]).
    ///
    /// Retail keeps one global word and every actor's `0x05` wait arm tests
    /// it against its own `+0x50`, so at most one actor can ever match; a
    /// per-channel `Option` carries the same information without a shared
    /// cell, with `None` standing for retail's `0xFF` empty sentinel. The
    /// host writes it on contact; [`AmbientMotion::step_ops_with`]'s wait arm
    /// consumes it, which is retail's `sw a0, 0x3f1c(a1)` at `0x8003887C`.
    pub pending_touch: Option<u32>,
    /// Retail `actor+0x10` - the actor's 32-bit flag word. Ops `0x10` /
    /// `0x11` / `0x12` address it as two halfwords (`+0x10` low, `+0x12`
    /// high); `0x0A` / `0x0B` / `0x0E` move
    /// [`crate::ambient_motion_ops::ACTOR_FLAG_TRANSLUCENT`] in it, and
    /// `0x0F` reads
    /// [`crate::ambient_motion_ops::ACTOR_FLAG_Y_OVERRIDE`] out of it.
    pub actor_flags: u32,
    /// Retail `actor+0x62` - the motion-clip control word, the `0x40`
    /// selector of the bit ops.
    pub clip_control: u16,
    /// Retail `_DAT_1F800394`, the scratchpad global flag word - the `0x80`
    /// selector of the bit ops, low half at `0x1F800394` and high half at
    /// `0x1F800396`. It is a global, so the host seeds it before the tick
    /// and reads it back after; retail's word is simply shared memory.
    pub globals: u32,
    /// Retail `actor+0x8C` / `+0x8D` - the anchor tile. Written by the
    /// `0x0F` teleport (and by the spawner before that); the `0x06` step
    /// measures its box relative to it.
    pub home_tile: (u8, u8),
    /// Retail `actor+0x74` - the packed RGB tint word (`0x00BBGGRR`), an
    /// argument of the mesh submit `FUN_80043390`. `FUN_80020E3C` seeds it
    /// `0x00808080`, PSX-neutral.
    pub tint: u32,
    /// Retail `actor+0x78` - the draw-mode word that rides alongside
    /// [`Self::tint`] into the same submit. Seeded `0`.
    pub blend: u16,
    /// Retail `actor+0x72` - the uniform render scale (`0x1000` = 1.0),
    /// [`None`] until a `0x14` writes it so a host can distinguish "the
    /// stream set it" from "nobody has".
    pub scale: Option<u16>,
    /// Retail `actor+0x24` - the X Euler angle, `0x15`'s destination.
    pub pitch: i16,
    /// Retail `actor+0x28` - the Z Euler angle, `0x16`'s destination.
    pub roll: i16,
    /// The halfword `0x02` / `0x0A` / `0x0B` write to **both** `actor+0x88`
    /// and `actor+0x5C`. [`Self::requested_move`] carries its low byte,
    /// which is what the walk ops' record reload also produces; this field
    /// keeps the full sign-extended value retail stored.
    pub move_pair: Option<i16>,
    /// Retail's four-slot SFX-cue ring `DAT_8007B6D8`, written by `0x09`
    /// through `FUN_80035B50`.
    pub event_ring: [i16; 4],
    /// Retail's ring write cursor `gp+0x158`, wrapping at 4.
    pub event_write: usize,
    /// Side effects this tick produced, in retail's order. The host drains
    /// them after [`Self::tick_with`]; see
    /// [`crate::ambient_motion_ops::AmbientEffect`].
    pub effects: Vec<crate::ambient_motion_ops::AmbientEffect>,
}

impl AmbientMotion {
    /// `heading` is the actor's current raw `+0x26`.
    pub fn new(owner: u32, heading: u16) -> Self {
        Self {
            pc: 0,
            cursor: 0,
            heading,
            owner,
            ramps: RampScheduler::new(),
            x: 0,
            z: 0,
            moved: false,
            walk_yaw: false,
            wander_phase: 0,
            wander_dir: 0,
            default_move: [DEFAULT_MOVE_UNSET; 2],
            requested_move: None,
            // Any seed does; the host overrides it per actor so two NPCs
            // running the same authored stream do not wander in lockstep.
            rng: 0x1234_5678,
            pending_touch: None,
            actor_flags: 0,
            clip_control: 0,
            globals: 0,
            home_tile: (0, 0),
            tint: 0x0080_8080,
            blend: 0,
            scale: None,
            pitch: 0,
            roll: 0,
            move_pair: None,
            event_ring: [0; 4],
            event_write: 0,
            effects: Vec::new(),
        }
    }

    /// Seat the channel at a world position (retail `actor+0x14`/`+0x18`).
    pub fn with_position(mut self, x: i16, z: i16) -> Self {
        self.x = x;
        self.z = z;
        // Retail's spawner derives the anchor pair `+0x8C` / `+0x8D` from
        // the seated world position the same way the `0x0F` teleport writes
        // it back (`FUN_801D3F24`), so a channel seated without one would
        // measure a `0x06` box from tile `(0, 0)`.
        self.home_tile = (((x >> 7) & 0x7F) as u8, ((z >> 7) & 0x7F) as u8);
        self
    }

    /// Retail `FUN_80056798` - the PsyQ 15-bit `rand()`.
    pub(crate) fn rand(&mut self) -> u32 {
        u32::from(legaia_engine_system::rand::psyq_rand_step(&mut self.rng))
    }

    /// The renderable heading - retail's consumers take `+0x26 & 0xFFF`.
    pub fn render_heading(&self) -> u16 {
        self.heading & 0x0FFF
    }

    /// One frame: run ops until one yields, then advance the ramp
    /// scheduler and apply any heading write it produced.
    ///
    /// `code` is the variant's bytecode (retail: `variant_header + 4`
    /// onwards); `speed` is `_DAT_1F800393`.
    ///
    /// Ordering note: retail runs the per-actor tick and the pool tick from
    /// different points of the frame and their relative order is not pinned.
    /// It does not change where a turn lands - the scheduler's endpoint
    /// write is the LUT entry either way - only which frame carries an
    /// intermediate value.
    pub fn tick(&mut self, code: &[u8], speed: u8) -> AmbientTick {
        self.tick_with(code, speed, &NeverBlocks)
    }

    /// [`Self::tick`] with a host-supplied collision service for the walk
    /// ops.
    pub fn tick_with(
        &mut self,
        code: &[u8],
        speed: u8,
        blocking: &dyn AmbientBlocking,
    ) -> AmbientTick {
        let result = self.step_ops_with(code, speed, blocking);
        self.tick_ramps(speed);
        result
    }

    /// Advance the ramp scheduler alone and apply its writes. Retail's pool
    /// tick `FUN_80036D80` is its own actor template (the word at
    /// `0x800742F4`), not a callee of the per-actor op loop, so a frame on
    /// which the field-actor driver skips `FUN_80038158` still lands the
    /// ramps already in flight.
    pub fn tick_ramps(&mut self, speed: u8) {
        for w in self.ramps.tick(speed) {
            if w.owner != self.owner {
                continue;
            }
            match w.dest {
                // `kind 2` = `sh`: the low 16 bits, raw.
                RAMP_DEST_HEADING => self.heading = w.value as u16,
                RAMP_DEST_PITCH => self.pitch = w.value as i16,
                RAMP_DEST_ROLL => self.roll = w.value as i16,
                RAMP_DEST_SCALE => self.scale = Some(w.value as u16),
                RAMP_DEST_TINT => self.tint = w.value as u32,
                // Retail's slot is `kind 4`, a `sw` over a field it reads
                // back with `lhu`; the port keeps the 16-bit reading.
                RAMP_DEST_BLEND => self.blend = w.value as u16,
                _ => {}
            }
        }
    }

    /// The op loop alone, without the scheduler tick - for hosts that drive
    /// a shared [`RampScheduler`] themselves.
    pub fn step_ops(&mut self, code: &[u8], speed: u8) -> AmbientTick {
        self.step_ops_with(code, speed, &NeverBlocks)
    }

    /// [`Self::step_ops`] with a walk-collision service.
    pub fn step_ops_with(
        &mut self,
        code: &[u8],
        speed: u8,
        blocking: &dyn AmbientBlocking,
    ) -> AmbientTick {
        self.moved = false;
        self.walk_yaw = false;
        self.effects.clear();
        for _ in 0..MAX_OPS_PER_TICK {
            let pc = usize::from(self.pc);
            let Some(&op) = code.get(pc) else {
                return AmbientTick::Done;
            };
            let Some(width) = op_width(op) else {
                return AmbientTick::Done;
            };
            if pc + width > code.len() {
                return AmbientTick::Done;
            }
            let body = &code[pc..pc + width];
            match AmbientOp::from_byte(op) {
                Some(AmbientOp::Restart) => {
                    // `0x80038334`: cursor cleared, PC back to the variant's
                    // first op. No yield - retail keeps interpreting.
                    self.cursor = 0;
                    self.pc = 0;
                }
                Some(AmbientOp::Wait) => {
                    // `0x8003882C`. The arm opens with the **touch-wake**
                    // short-circuit (`0x8003882C..0x8003887C`), then falls
                    // into the plain countdown at `0x80038880`.
                    if self.take_touch_wake() {
                        // `0x80038874`/`0x80038878`: cursor = duration - speed,
                        // as a `sb`, so the countdown below lands exactly on
                        // the duration and expires the wait this frame.
                        let cut = body[1].wrapping_sub(speed);
                        self.cursor = (self.cursor & 0xFF00) | u16::from(cut);
                    }
                    // The cursor is a u8 in retail (`sb`).
                    let cur = ((self.cursor & 0xFF) as u8).wrapping_add(speed);
                    self.cursor = (self.cursor & 0xFF00) | u16::from(cur);
                    if u16::from(cur) >= u16::from(body[1]) {
                        self.cursor &= 0xFF00;
                        self.pc = self.pc.wrapping_add(2);
                    }
                    // This arm always consumes the frame.
                    return AmbientTick::Yield;
                }
                Some(AmbientOp::FacingRamp) => {
                    if self.step_facing_ramp(body) {
                        return AmbientTick::Yield;
                    }
                }
                Some(AmbientOp::FacingTween) => {
                    if self.step_facing_tween(body, speed) {
                        return AmbientTick::Yield;
                    }
                }
                Some(AmbientOp::DefaultMove) => {
                    // `0x80039AF8`. Guarded `+0x50 < 0x8C` in retail (the
                    // arena bound); the port's channel keys are placement
                    // slots, already inside it. No did-work increment, so
                    // the next op runs in the same tick.
                    self.default_move = [body[1], body[2]];
                    self.pc = self.pc.wrapping_add(3);
                }
                Some(AmbientOp::DirStep) => {
                    self.step_dir_step(op, body, blocking);
                    // Every arm of `0x800383F8` increments the did-work
                    // counter before it branches, so the op always ends the
                    // tick - whether it stepped, blocked or retired.
                    return AmbientTick::Yield;
                }
                Some(AmbientOp::Wander) => {
                    self.step_wander(body, blocking);
                    return AmbientTick::Yield;
                }
                Some(AmbientOp::AnimPair) => self.step_op_anim_pair(body),
                Some(AmbientOp::HomeStep) => {
                    self.step_op_home_step(body, blocking);
                    // `0x80038A1C` / `0x80038A3C`: every path through the
                    // arm increments the did-work counter.
                    return AmbientTick::Yield;
                }
                Some(AmbientOp::FlagSet) => self.step_op_system_flag(body, true),
                Some(AmbientOp::FlagClear) => self.step_op_system_flag(body, false),
                Some(AmbientOp::SfxCue) => self.step_op_sfx_cue(body),
                Some(AmbientOp::TranslucentOn) => self.step_op_translucency(body, true),
                Some(AmbientOp::TranslucentOff) => self.step_op_translucency(body, false),
                Some(AmbientOp::TintFade) => self.step_op_tint_fade(body),
                Some(AmbientOp::ModelSwap) => self.step_op_model_swap(body),
                Some(AmbientOp::TileTeleport) => self.step_op_tile_teleport(body),
                Some(AmbientOp::BitSet) => self.step_op_bit_write(body, true),
                Some(AmbientOp::BitClear) => self.step_op_bit_write(body, false),
                Some(AmbientOp::BitWait) => {
                    self.step_op_bit_wait(body);
                    // `0x80039634`: the increment is in the delay slot ahead
                    // of the selector decode, so the arm consumes the tick
                    // even on the frame the wait retires.
                    return AmbientTick::Yield;
                }
                Some(AmbientOp::MoveImage) => self.step_op_move_image(body),
                Some(AmbientOp::ScaleTween) => self.step_op_scalar_tween(body, RAMP_DEST_SCALE),
                Some(AmbientOp::PitchTween) => self.step_op_scalar_tween(body, RAMP_DEST_PITCH),
                Some(AmbientOp::RollTween) => self.step_op_scalar_tween(body, RAMP_DEST_ROLL),
                // Retail's table slots `0x1A..=0x1F` land on the epilogue
                // without advancing anything, so the same byte re-dispatches
                // forever; anything outside `0x01..=0x20` never reaches the
                // table at all. `op_width` has no entry for either, so the
                // loop has already returned `Done` above - this arm is
                // unreachable and kept only so the match stays total.
                None => return AmbientTick::Done,
            }
        }
        AmbientTick::Yield
    }

    /// The `0x05` wait arm's **touch-wake** head, retail
    /// `0x8003882C..0x8003887C`.
    ///
    /// PORT: FUN_80038158 (`0x8003882C..0x8003887C`)
    ///
    /// Three gates in retail's order, all skipping to the plain countdown:
    ///
    /// 1. `DAT_80073F1C == 0xFF` - the mailbox is empty ([`None`] here);
    /// 2. the posted id is not this actor's `+0x50` ([`Self::owner`]);
    /// 3. the actor's `0x801C6470` record byte 0 is [`DEFAULT_MOVE_UNSET`] -
    ///    an actor with no default move installed never wakes on a bump.
    ///
    /// Gate 3 is the same byte [`crate::motion_vm::post_touch`] tests on the
    /// producing side, so a suppressed post and a suppressed consume agree;
    /// retail reads it twice because the arena is live and op `0x17` can
    /// write it between the two.
    ///
    /// Returns `true` when the wait must be cut short, having consumed the
    /// mailbox (retail's `sw a0, 0x3f1c(a1)`, storing the `0xFF` sentinel).
    fn take_touch_wake(&mut self) -> bool {
        let Some(posted) = self.pending_touch else {
            return false;
        };
        if posted != self.owner || self.default_move[0] == DEFAULT_MOVE_UNSET {
            return false;
        }
        self.pending_touch = None;
        true
    }

    /// `0x04` body, retail `0x800385D0`. Returns `true` if the tick is
    /// consumed (the stepping arm), `false` on the terminal snap - which
    /// falls through to the next op in the same tick.
    fn step_facing_ramp(&mut self, body: &[u8]) -> bool {
        // Prologue `0x8003859C..0x800385C8`, run on every tick: the same
        // restamp the walk ops open with - the record's **anim** byte, guarded
        // on its move byte.
        if self.default_move[0] != DEFAULT_MOVE_UNSET {
            self.requested_move = Some(self.default_move[1]);
        }
        let (b1, b2) = (body[1], body[2]);
        let frames = u32::from(b2 & 0x7F);
        let cursor = u32::from((self.cursor & 0xFF) as u8);
        let target = heading_lut_retail(b1 & 7);
        // `subu`: retail would underflow here, but the cursor only ever
        // counts up from 0 to `frames`.
        let remaining = frames.saturating_sub(cursor);
        let next = ((self.cursor & 0xFF) as u8).wrapping_add(1);
        self.cursor = (self.cursor & 0xFF00) | u16::from(next);

        if remaining == 0 {
            self.heading = target;
            self.cursor &= 0xFF00;
            self.pc = self.pc.wrapping_add(3);
            // The terminal leaves through the shared epilogue `0x800390A8`
            // (`j` at `0x80038610`), which restamps the move byte.
            self.reload_requested_move();
            return false;
        }
        let decreasing = b1 & 0x80 != 0;
        // Same law as `motion_vm::rotate_step` with a unit speed: the arc is
        // re-measured from the live heading mod 0x1000 every tick, the
        // write-back wraps raw.
        self.heading =
            crate::motion_vm::rotate_step(self.heading, target, decreasing, 1, remaining);
        true
    }

    /// `0x0D` body, retail `0x800386A4`. Same return convention as
    /// [`AmbientMotion::step_facing_ramp`].
    fn step_facing_tween(&mut self, body: &[u8], speed: u8) -> bool {
        let b1 = body[1];
        let duration = i32::from(i16::from_le_bytes([body[2], body[3]]));

        if self.cursor == 0 {
            let target = heading_lut_retail(b1 & 7);
            // Retail compares the sign-extended `lh` values with `sltu`.
            // Both operands are non-negative for every reachable heading,
            // so this is the plain ordering.
            let cur = i32::from(self.heading as i16);
            let tgt = i32::from(target as i16);
            let unwrapped = if b1 & 0x80 != 0 {
                // Force decreasing: park the start above the target.
                if cur < tgt {
                    self.heading.wrapping_add(0x1000)
                } else {
                    self.heading
                }
            } else {
                // Force increasing: park the start below the target.
                if tgt < cur {
                    self.heading.wrapping_sub(0x1000)
                } else {
                    self.heading
                }
            };
            self.heading = unwrapped;
            let owner = self.owner;
            self.ramps.install(Ramp {
                dest: RAMP_DEST_HEADING,
                owner,
                start: i32::from(self.heading as i16),
                end: i32::from(target),
                total: duration,
                remaining: duration,
                kind: RampKind::U16,
            });
        }

        if i32::from(self.cursor) >= duration {
            self.cursor = 0;
            self.pc = self.pc.wrapping_add(4);
            return false;
        }
        self.cursor = self.cursor.wrapping_add(u16::from(speed));
        true
    }

    /// Retail epilogue `0x800390B4`: restamp the requested move from the
    /// default-move record's **move** byte, when one is installed.
    pub(crate) fn reload_requested_move(&mut self) {
        if self.default_move[0] != DEFAULT_MOVE_UNSET {
            self.requested_move = Some(self.default_move[0]);
        }
    }

    /// `0x03` / `0x19` / `0x20` `[op, b1, b2]`, retail body `0x800383F8` -
    /// one shared case for all three; they differ only in the two flags
    /// noted below.
    ///
    /// ```text
    /// lut    = b1 >> 4                     ; heading-LUT index
    /// bits   = b1 & 0x0F                   ; pace selector
    /// shift  = bits + 2
    /// budget = (b2 & 0x3F) << shift        ; ticks; 0x20 halves it
    /// step   = 0x80 >> shift               ; units per tick
    /// ```
    ///
    /// The product `budget * step` is `(b2 & 0x3F) * 0x80` whatever `bits`
    /// is, so `b2 & 0x3F` is the leg's length in **128-unit tiles** and
    /// `bits` only sets the pace.
    ///
    /// - `0x03` additionally snaps the heading to `LUT[lut]` on every tick.
    ///   `0x19` and `0x20` move without touching the facing.
    /// - `0x20` halves the tick budget (so it walks half the distance).
    ///
    /// The cursor is `+1` per tick, not `_DAT_1F800393`-scaled - the same
    /// asymmetry `0x04` has against the [`crate::motion_vm`] ramps.
    fn step_dir_step(&mut self, op: u8, body: &[u8], blocking: &dyn AmbientBlocking) {
        let (b1, b2) = (body[1], body[2]);
        // Prologue `0x800383F8`: restamp the requested move from the
        // record's **anim** byte, guarded on the move byte. (The epilogue
        // reload uses the move byte for both - retail is inconsistent here
        // and the port keeps it.)
        if self.default_move[0] != DEFAULT_MOVE_UNSET {
            self.requested_move = Some(self.default_move[1]);
        }
        let lut = b1 >> 4;
        let shift = u32::from(b1 & 0x0F) + 2;
        let mut budget = u32::from(b2 & 0x3F) << shift;

        if op == 0x03 && lut < 8 {
            // Departure: retail's index is unmasked and `8..=15` overread
            // into the adjacent bitmask table. No authored op does it.
            self.heading = heading_lut_retail(lut);
            self.walk_yaw = true;
        }
        if op == 0x20 {
            budget >>= 1;
        }

        if blocking.step_blocked(self.x, self.z, lut) {
            // `0x800384E0`: no cursor advance, no PC advance - the op
            // re-runs next tick against the moved-on player.
            self.reload_requested_move();
            return;
        }

        let step = (0x80u32 >> shift) as i16;
        let (nx, nz) = walk_apply(self.x, self.z, lut, step);
        self.moved = nx != self.x || nz != self.z;
        self.x = nx;
        self.z = nz;

        // Retail's cursor is a `u8` and the comparison masks it, so a budget
        // of 256 or more would never retire. No authored operand reaches it
        // (the widest on disc is 160).
        let cur = ((self.cursor & 0xFF) as u8).wrapping_add(1);
        self.cursor = (self.cursor & 0xFF00) | u16::from(cur);
        if u32::from(cur) < budget {
            return;
        }
        self.cursor &= 0xFF00;
        self.pc = self.pc.wrapping_add(3);
        self.reload_requested_move();
    }

    /// `0x18` `[18, b1, b2, b3, b4]`, retail body `0x80038B90` - the bounded
    /// random wander, and what a fresh Rim Elm villager actually runs.
    ///
    /// The four operand bytes carry a tile-space AABB in their low 7 bits
    /// (`min_x`, `min_z`, `max_x`, `max_z`, each `tile << 7 | 0x40` - a tile
    /// **centre**), and a 4-bit pace selector scattered over their high bits.
    ///
    /// A three-phase machine runs inside the op, its phase byte living in
    /// the actor's default-move record rather than on the actor:
    ///
    /// 1. **Pick** - draw a random cardinal (`rand() & 6`) and reject it if
    ///    a half-tile probe in that direction would leave the AABB. A
    ///    rejected draw **retires the op** rather than redrawing.
    /// 2. **Turn** - rotate to the picked compass point at
    ///    `0x1000 >> (bits + 2)` per tick, by the **shortest arc**. Skipped
    ///    when the actor already faces that way.
    /// 3. **Walk** - step `0x80 >> (bits + 2)` units for `2 << bits` ticks
    ///    (64 units - half a tile - whatever the pace), then flip a coin:
    ///    half the time the op retires, half the time it picks again.
    ///
    /// Two things separate this from the ambient facing ops. The turn takes
    /// the shortest arc where `0x04` / `0x0D` take the authored one, and it
    /// masks the heading into `0..0xFFF` on **every** tick where those two
    /// deliberately hold raw out-of-range values.
    fn step_wander(&mut self, body: &[u8], blocking: &dyn AmbientBlocking) {
        let (b1, b2, b3, b4) = (body[1], body[2], body[3], body[4]);
        let bound = |b: u8| (i32::from(b & 0x7F) << 7) + 0x40;
        let (min_x, min_z, max_x, max_z) = (bound(b1), bound(b2), bound(b3), bound(b4));
        let bits =
            u32::from(((b1 & 0x80) >> 4) | ((b2 & 0x80) >> 5) | ((b3 & 0x80) >> 6) | (b4 >> 7));
        let (x, z) = (i32::from(self.x), i32::from(self.z));

        // Entry guard, only on the op's first tick: an actor seated outside
        // its own wander box retires the op instead of walking home.
        if self.cursor & 0xFF == 0 {
            self.wander_phase = 0;
            if x < min_x || z < min_z || x > max_x || z > max_z {
                self.pc = self.pc.wrapping_add(5);
                return;
            }
        }

        if self.wander_phase == 0 {
            // Phase 1 - pick. `0x80038C60`.
            self.cursor &= 0xFF00;
            let dir = (self.rand() & 6) as u8;
            let mask = WALK_DIR_BITS[usize::from(dir)];
            let rejected = (mask & 1 != 0 && max_z < z + 0x40)
                || (mask & 2 != 0 && z - 0x40 < min_z)
                || (mask & 4 != 0 && max_x < x + 0x40)
                || (mask & 8 != 0 && x - 0x40 < min_x);
            if rejected {
                // `0x80038D0C` lands on the PC-advance epilogue: a draw that
                // would leave the box ends the op.
                self.pc = self.pc.wrapping_add(5);
                self.reload_requested_move();
                return;
            }
            self.wander_dir = dir;
            self.wander_phase = 1;
            if self.heading & 0x0FFF == heading_lut_retail(dir) & 0x0FFF {
                self.wander_phase = 2;
            }
        }

        if self.wander_phase == 1 {
            // Phase 2 - turn. `0x80038DBC`.
            self.cursor = (self.cursor & 0xFF00) | 1;
            if self.default_move[0] != DEFAULT_MOVE_UNSET {
                self.requested_move = Some(self.default_move[0]);
            }
            let turn_step = 0x1000i32 >> (bits + 2);
            let target = i32::from(heading_lut_retail(self.wander_dir));
            let live = i32::from(self.heading & 0x0FFF);
            // Unwrap the target above the live heading, then measure: an
            // increasing arc wider than a half turn means the short way
            // round is decreasing.
            let goal = if target < live {
                target + 0x1000
            } else {
                target
            };
            let (stepped, overshot) = if live + 0x800 < goal {
                let s = live + 0x1000 - turn_step;
                (s, s < goal)
            } else {
                let s = live + turn_step;
                (s, goal < s)
            };
            let landed = if overshot { goal } else { stepped };
            self.heading = (landed & 0x0FFF) as u16;
            self.walk_yaw = true;
            if landed != goal {
                return;
            }
            self.wander_phase = 2;
            return;
        }

        if self.wander_phase == 2 {
            // `0x80038EB0` - a pure hand-off; phase 3 runs in the same tick.
            self.cursor &= 0xFF00;
            self.wander_phase = 3;
        }

        if self.wander_phase != 3 {
            return;
        }

        // Phase 3 - walk. `0x80038EC8`.
        let dir = self.wander_dir;
        self.heading = heading_lut_retail(dir);
        self.walk_yaw = true;
        if blocking.wander_blocked(self.x, self.z, dir >> 1) {
            // `0x80038F50`: drop back to the pick phase, PC unchanged.
            self.wander_phase = 0;
            self.reload_requested_move();
            return;
        }
        if self.default_move[0] != DEFAULT_MOVE_UNSET {
            self.requested_move = Some(self.default_move[1]);
        }
        let step = (0x80u32 >> (bits + 2)) as i16;
        let (nx, nz) = walk_apply(self.x, self.z, dir, step);
        self.moved = nx != self.x || nz != self.z;
        self.x = nx;
        self.z = nz;

        let cur = ((self.cursor & 0xFF) as u8).wrapping_add(1);
        self.cursor = (self.cursor & 0xFF00) | u16::from(cur);
        if u32::from(cur) < (2u32 << bits) {
            return;
        }
        // Segment done. A 50/50 draw decides whether the op retires or the
        // actor picks a fresh direction and keeps wandering.
        self.wander_phase = 0;
        if self.rand() & 0x0F < 8 {
            self.cursor &= 0xFF00;
            self.pc = self.pc.wrapping_add(5);
            self.reload_requested_move();
        }
    }
}
