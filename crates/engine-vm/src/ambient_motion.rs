//! Ambient / idle **facing** channel of the second per-actor motion VM
//! (`FUN_80038158`, SCUS_942.54) - the runtime interpreter for its two
//! rotate ops, plus the generic ramp scheduler one of them delegates to.
//!
//! PORT: FUN_80038158, FUN_80036d80, FUN_8003c5f0
//! REF: FUN_801cf8ac, FUN_801d5a68, FUN_801cfe4c, FUN_80056798
//!
//! [`super::motion_vm`] ports the *other* motion VM (`FUN_8003774C`, the
//! pursue / patrol / face-target one). This module is the ambient sibling:
//! the VM whose bytecode arrives as MAN tail-section 1
//! (`legaia_asset::man_motion`) and which gives a standing town NPC its idle
//! turn-in-place behaviour. Without it engine NPCs hold one heading forever
//! where retail NPCs slowly look around.
//!
//! ## The two rotate ops
//!
//! Dispatch is the 32-entry jump table at `0x80010FE8` indexed by `op - 1`
//! (ops `0x01..=0x20`); `0x04` lands at `0x8003859C` and `0x0D` at
//! `0x800386A4`. Both aim at the same eight-point compass LUT
//! (`0x80073F04`) the walk ops snap to, so **every ambient turn ends on a
//! compass point** - the endpoint is never an arbitrary bearing.
//!
//! ### `0x04` `[04, b1, b2]` - the in-VM ramp
//!
//! Retail body `0x800385D0..0x800386A0`. Per tick:
//!
//! ```text
//! frames    = b2 & 0x7F                  ; frame budget
//! remaining = frames - cursor            ; cursor = actor +0x8B, u8
//! target    = LUT[b1 & 7]
//! cursor   += 1                          ; unit-per-tick, NOT the frame scalar
//! if remaining == 0:                     ; terminal
//!     heading = target                   ; exact snap
//!     cursor  = 0 ; pc += 3 ; fall through to the next op THIS tick
//! else:
//!     arc      = (target - heading) mod 0x1000   ; or (heading - target)
//!     heading += arc / remaining                 ; or -=, raw u16 wrapping
//!     yield
//! ```
//!
//! Two things a port gets wrong by default:
//!
//! - **The cursor is unit-per-tick**, not `_DAT_1F800393`-scaled the way the
//!   `FUN_8003774C` ramps are. A `b2 & 0x7F` of 24 is 24 stepping ticks
//!   regardless of the frame scalar.
//! - **There is no shortest-arc choice.** `b1 & 0x80` alone picks the
//!   direction (set = decreasing), so a `0x04` turn can deliberately take the
//!   long way round. The `FUN_8003774C` `0x38` sibling *does* have a
//!   shortest-path opt-in; this one does not.
//!
//! The terminal tick does **not** consume the frame (retail never increments
//! the `s8` did-work counter on that arm), so the snap and the following op
//! execute in the same tick.
//!
//! ### `0x0D` `[0D, b1, b2, b3]` - the pre-unwrap + tween
//!
//! Retail body `0x800386A4..0x80038828`. This op does not move the heading
//! itself. On its first tick it **pre-unwraps** the live heading past the
//! `0x1000` boundary so a plain linear interpolation travels the intended
//! way, then hands `&actor+0x26` to the generic 16-bit ramp scheduler
//! ([`RampScheduler`], retail `0x801C66A0` / installer `FUN_8003C5F0`):
//!
//! ```text
//! cursor16 = actor+0x8B | actor+0xB7 << 8
//! if cursor16 == 0:                              ; install tick, once
//!     target = LUT[b1 & 7]
//!     if b1 & 0x80:  if heading <  target: heading += 0x1000   ; go decreasing
//!     else:          if target  <  heading: heading -= 0x1000  ; go increasing
//!     install ramp { dest: &heading, start: heading, end: target,
//!                    total: b2 | b3 << 8, kind: 2 }
//! if cursor16 >= (b2 | b3 << 8):                 ; terminal
//!     cursor16 = 0 ; pc += 4 ; fall through this tick
//! else:
//!     cursor16 += _DAT_1F800393 ; yield
//! ```
//!
//! So `0x0D`'s wait cursor **is** frame-scalar-driven (the opposite of
//! `0x04`'s), which is exactly what keeps it in lockstep with the scheduler:
//! the scheduler decrements its own `remaining` by the same scalar, so op and
//! ramp retire together.
//!
//! **Masking.** Neither op normalises the heading per tick. `0x04`'s
//! write-back is raw `u16` wrapping - a decreasing ramp through zero holds
//! `0xFFxx` for its whole run. `0x0D` is worse: the pre-unwrap deliberately
//! parks the heading *outside* `0..0xFFF` (up to `0x1FFF`, or negative stored
//! as `0xFxxx`) and the scheduler interpolates on that raw value, so raw
//! headings above `0x1000` are observable live mid-turn. Only the endpoint
//! lands back in range - it is written as the LUT entry verbatim. Renderers
//! consume the heading mod `0x1000`, so none of this is visible on screen,
//! but a port that masks per tick diverges from the traced `+0x26`. The arc
//! measurement inside `0x04` *is* taken mod `0x1000`, which is why a raw
//! out-of-range heading still feeds back correctly.
//!
//! ## The generic ramp scheduler
//!
//! `FUN_80036D80` walks the 64-slot pool at `0x801C66A0` (stride `0x20`, slot
//! 0 is the list header, so 63 usable) once per frame:
//!
//! ```text
//! remaining -= _DAT_1F800393
//! if remaining <= 0:  value = end ; free the slot
//! else:               value = end + (start - end) * remaining / total
//! store value to *dest, width per `kind` (1=u8, 2=u16, 3=packed RGB, 4=u32)
//! ```
//!
//! The division truncates toward zero (MIPS `div`), and `total` is never
//! rewritten, so the interpolation is a straight lerp off the install-time
//! endpoints - not an incremental accumulation. The heading channel is
//! `kind == 2`.
//!
//! ## Deliberate departures
//!
//! - The pool is ticked in slot order rather than through retail's
//!   intrusive linked list. Order is only observable when two live ramps
//!   share a destination, which a single actor's heading channel cannot do.
//! - Retail frees a slot whose owning actor has `+0x10 & 8` set (despawned).
//!   The port exposes [`RampScheduler::free_owner`] for the host to call
//!   instead of modelling the actor flag word here.
//! - Ops other than the ones listed in [`AmbientOp`] are stepped over by
//!   width and do **not** consume the tick, so this interpreter drives the
//!   facing channel only. A per-tick op budget ([`MAX_OPS_PER_TICK`]) stops
//!   a stream whose real yield op is one of the stepped-over ones from
//!   spinning; the tick ends as a yield with no facing change, which is the
//!   correct facing-channel answer for that frame.

use legaia_asset::man_motion::op_width;

/// Retail's `0x801C66A0` pool is 64 slots of stride `0x20`; slot 0 is the
/// intrusive list header, leaving 63 allocatable.
pub const RAMP_SLOTS: usize = 63;

/// Per-tick op budget. Retail has none - it loops until an op sets the
/// did-work counter. Because this port steps over the ops it does not model
/// (which include retail's yielding walk ops), it needs a stop.
pub const MAX_OPS_PER_TICK: usize = 256;

/// The eight-point compass LUT at `0x80073F04`, in the **retail** heading
/// space (`0` = -Z, entry `i` = `i * 0x200`). Both rotate ops index it
/// `& 7`.
///
/// [`super::motion_vm::heading_lut_engine`] is the same table carried into
/// the engine's `render_26` space (`+0x800`); this module stays in retail
/// space because the raw-wrapping write-back law is only meaningful there.
pub fn heading_lut_retail(idx: u8) -> u16 {
    u16::from(idx & 7) * 0x200
}

/// The per-direction **axis bitmask** table at `0x80073F14` - the sixteen
/// bytes immediately after the eight-entry compass LUT, and the table every
/// walk op reduces its heading index through before touching a coordinate.
///
/// Bit `1` = `+Z`, `2` = `-Z`, `4` = `+X`, `8` = `-X`, applied in that order,
/// so a diagonal entry moves both axes by the same per-tick step. Entries
/// `0..=7` are exactly the compass of [`heading_lut_retail`] in bit form
/// (`0` = `-Z`, walking clockwise); `8..=15` are the tail of the table and
/// carry no usable direction - `8` sets all four bits (whose four writes
/// cancel to zero net motion) and the rest are inert. No disc-authored walk
/// op indexes past `7`.
pub const WALK_DIR_BITS: [u8; 16] = [
    0x02, 0x0A, 0x08, 0x09, 0x01, 0x05, 0x04, 0x06, 0xFF, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00,
];

/// Apply one walk step of `step` units along the axes `WALK_DIR_BITS[idx]`
/// selects, in retail's write order. Returns the new `(x, z)`.
///
/// Retail loads and stores the coordinates as `lhu`/`sh` (16-bit wrapping),
/// which is what the `wrapping_*` here reproduces.
fn walk_apply(x: i16, z: i16, idx: u8, step: i16) -> (i16, i16) {
    let mask = WALK_DIR_BITS[usize::from(idx & 0x0F)];
    let (mut x, mut z) = (x, z);
    if mask & 1 != 0 {
        z = z.wrapping_add(step);
    }
    if mask & 2 != 0 {
        z = z.wrapping_sub(step);
    }
    if mask & 4 != 0 {
        x = x.wrapping_add(step);
    }
    if mask & 8 != 0 {
        x = x.wrapping_sub(step);
    }
    (x, z)
}

/// The collision service the **walk** ops need from the host.
///
/// Retail's two probes both resolve to `FUN_801cf8ac`, a box test of one
/// point against the **player actor** - not against the wall grid and not
/// against other NPCs. That is the single most surprising fact about ambient
/// wandering: an NPC's containment is its op's authored AABB, and the only
/// thing that can stop a step is the player standing in it.
pub trait AmbientBlocking {
    /// Directional steps `0x03` / `0x19` / `0x20`: retail probes the single
    /// `DAT_801F2254` compass point for the op's heading-LUT index
    /// (`radius 64` ahead) through `FUN_801cf8ac`.
    fn step_blocked(&self, x: i16, z: i16, lut_index: u8) -> bool;

    /// Wander `0x18` walk phase: retail probes the three-point fan of
    /// `DAT_801F21B4` row `dir4` (`0` = `Z-`, `1` = `X-`, `2` = `Z+`,
    /// `3` = `X+` - the `FUN_801cfe4c` direction space) through
    /// `FUN_801d5a68`, OR-ing the three results.
    fn wander_blocked(&self, x: i16, z: i16, dir4: u8) -> bool;
}

/// Nothing ever blocks - the standalone / facing-only reading, and what
/// [`AmbientMotion::tick`] uses.
#[derive(Debug, Clone, Copy, Default)]
pub struct NeverBlocks;

impl AmbientBlocking for NeverBlocks {
    fn step_blocked(&self, _x: i16, _z: i16, _lut_index: u8) -> bool {
        false
    }
    fn wander_blocked(&self, _x: i16, _z: i16, _dir4: u8) -> bool {
        false
    }
}

/// Every direction blocks - the fully-boxed-in reading.
///
/// Note what this does *not* buy: a blocked directional step re-runs its own
/// op forever without advancing the PC, so a stream held under this never
/// reaches whatever follows that op (a story-flag write included). A host
/// that wants an actor to hold its seat should let the ops run normally and
/// re-pin the position instead.
#[derive(Debug, Clone, Copy, Default)]
pub struct AlwaysBlocks;

impl AmbientBlocking for AlwaysBlocks {
    fn step_blocked(&self, _x: i16, _z: i16, _lut_index: u8) -> bool {
        true
    }
    fn wander_blocked(&self, _x: i16, _z: i16, _dir4: u8) -> bool {
        true
    }
}

/// Destination width of a scheduler slot - retail's `slot+0x18`. Only
/// [`RampKind::U16`] is reachable from the motion VM's `0x0D`; the others
/// exist because the same pool carries sound and render-bank ramps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RampKind {
    /// `kind 1` - `sb`.
    U8,
    /// `kind 2` - `sh`. The heading channel.
    U16,
    /// `kind 3` - three packed 8-bit lanes lerped independently.
    Rgb,
    /// `kind 4` - `sw`.
    U32,
}

/// One live slot of the `0x801C66A0` pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ramp {
    /// Opaque host tag for the destination (retail stores a raw pointer at
    /// `slot+0x04`). The motion VM's heading channel uses [`RAMP_DEST_HEADING`].
    pub dest: u32,
    /// Host tag for the owning actor (retail `slot+0x00`), for
    /// [`RampScheduler::free_owner`].
    pub owner: u32,
    /// Install-time value, `slot+0x08`.
    pub start: i32,
    /// Endpoint, `slot+0x0C`. The terminal tick stores this verbatim.
    pub end: i32,
    /// Total duration, `slot+0x10` - never rewritten.
    pub total: i32,
    /// Countdown, `slot+0x14`.
    pub remaining: i32,
    pub kind: RampKind,
}

/// Destination tag the `0x0D` install uses for `&actor+0x26`.
pub const RAMP_DEST_HEADING: u32 = 0x26;

/// A value the scheduler produced this tick, and where it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RampWrite {
    pub dest: u32,
    pub owner: u32,
    /// The value retail stores. Truncate per [`RampWrite::kind`] at the
    /// destination; the heading channel takes the low 16 bits raw.
    pub value: i32,
    pub kind: RampKind,
    /// `true` on the tick the slot retired (the endpoint write).
    pub finished: bool,
}

/// The 63-slot ramp pool - retail `0x801C66A0`, installer `FUN_8003C5F0`,
/// ticker `FUN_80036D80`.
#[derive(Debug, Clone, Default)]
pub struct RampScheduler {
    slots: Vec<Option<Ramp>>,
    /// Retail's `DAT_80073ED0` pool-exhausted counter, incremented instead
    /// of installing when every slot is busy.
    pub overflow: u32,
}

impl RampScheduler {
    pub fn new() -> Self {
        Self {
            slots: vec![None; RAMP_SLOTS],
            overflow: 0,
        }
    }

    /// `FUN_8003C5F0`: first-free scan, or bump [`RampScheduler::overflow`]
    /// and drop the request. Returns the slot index on success.
    pub fn install(&mut self, ramp: Ramp) -> Option<usize> {
        if self.slots.is_empty() {
            self.slots = vec![None; RAMP_SLOTS];
        }
        match self.slots.iter().position(Option::is_none) {
            Some(i) => {
                self.slots[i] = Some(ramp);
                Some(i)
            }
            None => {
                self.overflow = self.overflow.wrapping_add(1);
                None
            }
        }
    }

    /// Free every slot owned by `owner` - the port's stand-in for retail's
    /// "owner actor has `+0x10 & 8`" despawn sweep.
    pub fn free_owner(&mut self, owner: u32) {
        for s in self.slots.iter_mut() {
            if s.is_some_and(|r| r.owner == owner) {
                *s = None;
            }
        }
    }

    pub fn active(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// `true` while a ramp is driving `dest`.
    pub fn is_driving(&self, dest: u32) -> bool {
        self.slots.iter().any(|s| s.is_some_and(|r| r.dest == dest))
    }

    /// One frame of `FUN_80036D80`. `speed` is `_DAT_1F800393`.
    pub fn tick(&mut self, speed: u8) -> Vec<RampWrite> {
        let mut out = Vec::new();
        for slot in self.slots.iter_mut() {
            let Some(mut r) = *slot else { continue };
            r.remaining -= i32::from(speed);
            let (value, finished) = if r.remaining <= 0 {
                (r.end, true)
            } else {
                (lerp_remaining(r.start, r.end, r.remaining, r.total), false)
            };
            let value = match r.kind {
                RampKind::Rgb => rgb_lerp(r.start, r.end, r.remaining, r.total, finished),
                _ => value,
            };
            out.push(RampWrite {
                dest: r.dest,
                owner: r.owner,
                value,
                kind: r.kind,
                finished,
            });
            *slot = if finished { None } else { Some(r) };
        }
        out
    }
}

/// Retail's scalar lerp: `end + (start - end) * remaining / total`, with a
/// truncating (toward-zero) divide. Reproduces `0x80036E00..0x80036E50`.
fn lerp_remaining(start: i32, end: i32, remaining: i32, total: i32) -> i32 {
    if total == 0 {
        return end;
    }
    end + (start - end).saturating_mul(remaining) / total
}

/// `kind 3`: the same lerp run independently on three packed 8-bit lanes
/// (`0x80036E54..`). Present for completeness - the motion VM never installs
/// this kind.
fn rgb_lerp(start: i32, end: i32, remaining: i32, total: i32, finished: bool) -> i32 {
    if finished {
        return end;
    }
    let mut out = 0i32;
    for shift in [0u32, 8, 16] {
        let s = (start >> shift) & 0xFF;
        let e = (end >> shift) & 0xFF;
        out |= (lerp_remaining(s, e, remaining, total) & 0xFF) << shift;
    }
    out
}

/// The ambient VM ops this module executes. Everything else in the
/// `0x01..=0x20` space is stepped over by [`op_width`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmbientOp {
    /// `0x01` - reset the cursor and jump back to the variant's first op.
    Restart,
    /// `0x04 b1 b2` - the in-VM facing ramp.
    FacingRamp,
    /// `0x05 frames` - wait. Always consumes the tick.
    Wait,
    /// `0x0D b1 b2 b3` - pre-unwrap + hand the heading to the scheduler.
    FacingTween,
    /// `0x03` / `0x19` / `0x20` `b1 b2` - a straight-line directional step.
    DirStep,
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
            0x03 | 0x19 | 0x20 => Self::DirStep,
            0x04 => Self::FacingRamp,
            0x05 => Self::Wait,
            0x0D => Self::FacingTween,
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
        }
    }

    /// Seat the channel at a world position (retail `actor+0x14`/`+0x18`).
    pub fn with_position(mut self, x: i16, z: i16) -> Self {
        self.x = x;
        self.z = z;
        self
    }

    /// Retail `FUN_80056798` - the PsyQ 15-bit `rand()`.
    fn rand(&mut self) -> u32 {
        u32::from(crate::battle_formulas::psyq_rand_step(&mut self.rng))
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
        for w in self.ramps.tick(speed) {
            if w.dest == RAMP_DEST_HEADING && w.owner == self.owner {
                // `kind 2` = `sh`: the low 16 bits, raw.
                self.heading = w.value as u16;
            }
        }
        result
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
                    // `0x8003882C`. The cursor is a u8 in retail (`sb`).
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
                // Not modelled here: stepped over without consuming the tick.
                None => self.pc = self.pc.wrapping_add(width as u16),
            }
        }
        AmbientTick::Yield
    }

    /// `0x04` body, retail `0x800385D0`. Returns `true` if the tick is
    /// consumed (the stepping arm), `false` on the terminal snap - which
    /// falls through to the next op in the same tick.
    fn step_facing_ramp(&mut self, body: &[u8]) -> bool {
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
            return false;
        }
        let decreasing = b1 & 0x80 != 0;
        // Same law as `motion_vm::rotate_step` with a unit speed: the arc is
        // re-measured from the live heading mod 0x1000 every tick, the
        // write-back wraps raw.
        self.heading =
            super::motion_vm::rotate_step(self.heading, target, decreasing, 1, remaining);
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
    fn reload_requested_move(&mut self) {
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
    /// asymmetry `0x04` has against the [`super::motion_vm`] ramps.
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

/// A `0x04` / `0x0D` site found by walking a variant's bytecode - the shape
/// the disc census and the oracle both consume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmbientFacingSite {
    /// Byte offset within the variant's bytecode.
    pub offset: usize,
    /// `0x04` or `0x0D`.
    pub op: u8,
    /// Compass LUT index, `b1 & 7`.
    pub lut_index: u8,
    /// `b1 & 0x80` - set forces the decreasing direction.
    pub decreasing: bool,
    /// `0x04`: `b2 & 0x7F` frames. `0x0D`: the 16-bit `b2 | b3 << 8`.
    pub duration: i32,
}

impl AmbientFacingSite {
    /// Endpoint heading in retail space.
    pub fn target(&self) -> u16 {
        heading_lut_retail(self.lut_index)
    }
}

/// Walk one variant's bytecode and collect every `0x04` / `0x0D` site,
/// stepping non-facing ops by [`op_width`]. Stops at the first op whose
/// width is unknown.
pub fn facing_sites(code: &[u8]) -> Vec<AmbientFacingSite> {
    let mut out = Vec::new();
    let mut pc = 0usize;
    while pc < code.len() {
        let op = code[pc];
        let Some(width) = op_width(op) else { break };
        if pc + width > code.len() {
            break;
        }
        let body = &code[pc..pc + width];
        match op {
            0x04 => out.push(AmbientFacingSite {
                offset: pc,
                op,
                lut_index: body[1] & 7,
                decreasing: body[1] & 0x80 != 0,
                duration: i32::from(body[2] & 0x7F),
            }),
            0x0D => out.push(AmbientFacingSite {
                offset: pc,
                op,
                lut_index: body[1] & 7,
                decreasing: body[1] & 0x80 != 0,
                duration: i32::from(i16::from_le_bytes([body[2], body[3]])),
            }),
            _ => {}
        }
        pc += width;
    }
    out
}

/// Replay one facing site standalone until it retires, returning the raw
/// per-tick heading trace (the value after each tick, including the
/// terminal one). `speed` is `_DAT_1F800393`.
///
/// The oracle uses this to assert a disc-sourced op lands on its compass
/// endpoint in its authored budget.
pub fn replay_site(site: &AmbientFacingSite, start_heading: u16, speed: u8) -> Vec<u16> {
    let code: Vec<u8> = match site.op {
        0x04 => vec![
            0x04,
            (site.lut_index & 7) | if site.decreasing { 0x80 } else { 0 },
            (site.duration as u8) & 0x7F,
        ],
        _ => {
            let d = (site.duration as i16).to_le_bytes();
            vec![
                0x0D,
                (site.lut_index & 7) | if site.decreasing { 0x80 } else { 0 },
                d[0],
                d[1],
            ]
        }
    };
    let mut vm = AmbientMotion::new(1, start_heading);
    let mut trace = Vec::new();
    // A budget of frames + 2 is enough for `0x04` (frames stepping ticks,
    // one terminal); `0x0D` retires in `ceil(duration / speed) + 1`.
    let cap = (site.duration.max(0) as usize / usize::from(speed.max(1)))
        + site.duration.max(0) as usize
        + 8;
    for _ in 0..cap {
        vm.tick(&code, speed);
        trace.push(vm.heading);
        if usize::from(vm.pc) >= code.len() {
            break;
        }
    }
    trace
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compass_lut_is_retail_space() {
        assert_eq!(heading_lut_retail(0), 0x000);
        assert_eq!(heading_lut_retail(2), 0x400);
        assert_eq!(heading_lut_retail(7), 0xE00);
        // Ops index it `& 7`.
        assert_eq!(heading_lut_retail(0x8F), heading_lut_retail(7));
    }

    #[test]
    fn op04_ramps_over_its_budget_and_snaps_exactly() {
        // `[04, lut=2 increasing, 8 frames]`: 0x000 -> 0x400 in 8 ticks.
        let code = [0x04u8, 0x02, 0x08];
        let mut vm = AmbientMotion::new(1, 0x000);
        let mut headings = Vec::new();
        for _ in 0..8 {
            assert_eq!(vm.tick(&code, 1), AmbientTick::Yield);
            headings.push(vm.heading);
        }
        // Eight equal steps of 0x400/8 = 0x80.
        assert_eq!(
            headings,
            vec![0x080, 0x100, 0x180, 0x200, 0x280, 0x300, 0x380, 0x400]
        );
        // The ninth tick is the terminal snap, which does NOT consume the
        // frame: PC has advanced past the op.
        vm.tick(&code, 1);
        assert_eq!(vm.heading, 0x400);
        assert_eq!(vm.pc, 3);
        assert_eq!(vm.cursor, 0);
    }

    #[test]
    fn op04_cursor_is_unit_per_tick_not_frame_scaled() {
        // A frame scalar of 4 must not shorten the leg: retail's `0x04`
        // cursor is `addiu a0, a0, 1`.
        let code = [0x04u8, 0x02, 0x08];
        let mut vm = AmbientMotion::new(1, 0x000);
        for _ in 0..8 {
            assert_eq!(vm.tick(&code, 4), AmbientTick::Yield);
        }
        assert_eq!(vm.pc, 0, "still mid-leg after 8 ticks at speed 4");
        vm.tick(&code, 4);
        assert_eq!(vm.pc, 3);
    }

    #[test]
    fn op04_decreasing_holds_raw_wrapped_headings() {
        // 0x100 -> 0x000 decreasing is a 0x100 arc; but force the LONG way
        // by aiming at 0x200 decreasing: arc = (0x100 - 0x200) mod 0x1000
        // = 0xF00, so the heading walks down through zero and holds 0xFxxx
        // raw until the snap.
        let code = [0x04u8, 0x81, 0x06]; // lut 1 = 0x200, decreasing, 6 frames
        let mut vm = AmbientMotion::new(1, 0x100);
        let mut raw = Vec::new();
        for _ in 0..6 {
            vm.tick(&code, 1);
            raw.push(vm.heading);
        }
        assert!(
            raw.iter().any(|h| *h > 0x0FFF),
            "a decreasing ramp through zero must hold raw >0xFFF headings: {raw:04X?}"
        );
        // The raw hold is invisible to a renderer: masking recovers the
        // heading the ramp is actually pointing at.
        assert!(
            raw.iter().any(|h| *h != *h & 0x0FFF),
            "at least one tick's raw value differs from its rendered form"
        );
        vm.tick(&code, 1);
        assert_eq!(vm.heading, 0x200, "terminal snap lands back in range");
    }

    #[test]
    fn op04_has_no_shortest_arc_choice() {
        // 0x000 -> 0x200 with the decreasing bit set takes the 0xE00 long
        // way, not the 0x200 short one.
        let code = [0x04u8, 0x81, 0x0E];
        let mut vm = AmbientMotion::new(1, 0x000);
        vm.tick(&code, 1);
        // First step is 0xE00 / 14 = 0x100 *downward*.
        assert_eq!(vm.heading, 0x000u16.wrapping_sub(0x100));
    }

    #[test]
    fn op0d_pre_unwraps_then_tweens_to_the_compass_point() {
        // Increasing from 0xE00 to 0x200: retail parks the start at
        // 0xE00 - 0x1000 = 0xFE00 (raw) so the lerp runs upward through the
        // wrap.
        let code = [0x0Du8, 0x01, 0x10, 0x00]; // lut 1 = 0x200, increasing, 16 frames
        let mut vm = AmbientMotion::new(1, 0x0E00);
        vm.tick(&code, 1);
        assert!(vm.ramps.is_driving(RAMP_DEST_HEADING));
        // Start value is the pre-unwrapped one.
        let mut seen_out_of_range = false;
        for _ in 0..40 {
            if vm.heading > 0x0FFF {
                seen_out_of_range = true;
            }
            vm.tick(&code, 1);
            if usize::from(vm.pc) >= code.len() {
                break;
            }
        }
        assert!(
            seen_out_of_range,
            "the tween runs on the pre-unwrap value, which is out of range"
        );
        assert_eq!(vm.heading, 0x200, "endpoint is the compass entry verbatim");
        assert_eq!(vm.ramps.active(), 0, "slot freed on retirement");
    }

    #[test]
    fn op0d_cursor_is_frame_scaled_and_retires_with_its_ramp() {
        // Duration 32 at speed 4 -> 8 scheduler ticks; the op retires one
        // tick later (its terminal arm re-reads the cursor).
        let code = [0x0Du8, 0x02, 0x20, 0x00];
        let mut vm = AmbientMotion::new(1, 0x000);
        let mut ticks = 0;
        for _ in 0..64 {
            vm.tick(&code, 4);
            ticks += 1;
            if usize::from(vm.pc) >= code.len() {
                break;
            }
        }
        assert_eq!(ticks, 9, "32/4 = 8 stepping ticks + the terminal one");
        assert_eq!(vm.heading, 0x400);
    }

    #[test]
    fn op0d_zero_duration_is_an_instant_compass_write() {
        let code = [0x0Du8, 0x03, 0x00, 0x00];
        let mut vm = AmbientMotion::new(1, 0x000);
        vm.tick(&code, 1);
        assert_eq!(vm.heading, heading_lut_retail(3));
        assert_eq!(vm.pc, 4);
    }

    #[test]
    fn op05_wait_consumes_frames_at_the_frame_scalar() {
        let code = [0x05u8, 0x08, 0x01]; // wait 8, then op 0x01 restart
        let mut vm = AmbientMotion::new(1, 0);
        for _ in 0..4 {
            assert_eq!(vm.tick(&code, 2), AmbientTick::Yield);
        }
        assert_eq!(vm.pc, 2, "8 frames at scalar 2 = 4 ticks");
    }

    #[test]
    fn op01_restarts_the_variant_without_consuming_the_tick() {
        // wait(2) then restart: the restart must loop straight back into
        // the wait in the same tick, so the stream never stalls.
        let code = [0x05u8, 0x02, 0x01];
        let mut vm = AmbientMotion::new(1, 0);
        for _ in 0..6 {
            assert_eq!(vm.tick(&code, 1), AmbientTick::Yield);
        }
        assert!(usize::from(vm.pc) < code.len());
    }

    #[test]
    fn unmodelled_ops_are_stepped_over_by_width() {
        // 0x07 SET flag (width 3) then a facing ramp: the facing channel
        // must reach the ramp.
        let code = [0x07u8, 0x10, 0x00, 0x04, 0x02, 0x04];
        let mut vm = AmbientMotion::new(1, 0x000);
        vm.tick(&code, 1);
        assert_eq!(vm.pc, 3, "stepped past the flag op into the ramp");
        assert_ne!(vm.heading, 0x000);
    }

    /// `b1` high nibble = heading-LUT index, low nibble = pace; `b2 & 0x3F`
    /// = tiles.
    fn dir_step(op: u8, lut: u8, bits: u8, tiles: u8) -> [u8; 3] {
        [op, (lut << 4) | (bits & 0x0F), tiles & 0x3F]
    }

    #[test]
    fn dir_step_walks_one_tile_per_operand_tile_at_any_pace() {
        // The distance a leg covers is `(b2 & 0x3F) * 0x80` whatever the
        // pace selector is - `budget * step` is pace-invariant.
        for bits in 0..4u8 {
            // LUT 4 = +Z.
            let code = dir_step(0x03, 4, bits, 3);
            let mut vm = AmbientMotion::new(1, 0).with_position(1000, 2000);
            let mut ticks = 0;
            while usize::from(vm.pc) < code.len() && ticks < 10_000 {
                vm.tick(&code, 1);
                ticks += 1;
            }
            assert_eq!(vm.x, 1000, "pure +Z leg leaves X alone");
            assert_eq!(vm.z, 2000 + 3 * 0x80, "bits={bits}: three tiles of +Z");
            assert_eq!(ticks, (3u32 << (u32::from(bits) + 2)) as usize);
            assert_eq!(vm.heading, 0x800, "0x03 snaps the facing to LUT[4]");
        }
    }

    #[test]
    fn dir_step_0x19_moves_without_touching_the_facing() {
        let code = dir_step(0x19, 6, 0, 1); // LUT 6 = +X
        let mut vm = AmbientMotion::new(1, 0x123).with_position(0, 0);
        while usize::from(vm.pc) < code.len() {
            vm.tick(&code, 1);
        }
        assert_eq!((vm.x, vm.z), (0x80, 0));
        assert_eq!(vm.heading, 0x123, "0x19 leaves the heading standing");
    }

    #[test]
    fn dir_step_0x20_halves_the_budget() {
        let code = dir_step(0x20, 4, 0, 2);
        let mut vm = AmbientMotion::new(1, 0).with_position(0, 0);
        while usize::from(vm.pc) < code.len() {
            vm.tick(&code, 1);
        }
        assert_eq!(vm.z, 0x80, "two tiles authored, one tile walked");
    }

    #[test]
    fn dir_step_cursor_is_unit_per_tick_not_frame_scaled() {
        // The `0x04` asymmetry applies to the walk ops too: the leg is the
        // same number of ticks at any frame scalar, so a cadence change
        // cannot change how far a leg travels.
        let code = dir_step(0x03, 4, 1, 2);
        for speed in [1u8, 2, 4] {
            let mut vm = AmbientMotion::new(1, 0).with_position(0, 0);
            let mut ticks = 0;
            while usize::from(vm.pc) < code.len() && ticks < 1000 {
                vm.tick(&code, speed);
                ticks += 1;
            }
            assert_eq!(ticks, 2 << 3, "speed={speed}");
            assert_eq!(vm.z, 2 * 0x80, "speed={speed}");
        }
    }

    #[test]
    fn dir_step_blocked_holds_position_and_pc() {
        let code = dir_step(0x03, 4, 0, 4);
        let mut vm = AmbientMotion::new(1, 0).with_position(500, 500);
        for _ in 0..50 {
            assert_eq!(vm.tick_with(&code, 1, &AlwaysBlocks), AmbientTick::Yield);
        }
        assert_eq!((vm.x, vm.z), (500, 500), "a blocked step never commits");
        assert_eq!(vm.pc, 0, "and never retires the op");
        assert_eq!(vm.cursor & 0xFF, 0, "nor burns budget");
    }

    /// `[18, min_x, min_z, max_x, max_z]` over a tile-space box, pace `bits`
    /// scattered over the four high bits.
    fn wander(min_x: u8, min_z: u8, max_x: u8, max_z: u8, bits: u8) -> [u8; 5] {
        [
            0x18,
            min_x | ((bits & 8) << 4),
            min_z | ((bits & 4) << 5),
            max_x | ((bits & 2) << 6),
            max_z | ((bits & 1) << 7),
        ]
    }

    #[test]
    fn wander_bits_round_trip_the_scatter() {
        for bits in 0..16u8 {
            let b = wander(0x10, 0x11, 0x20, 0x21, bits);
            let got =
                ((b[1] & 0x80) >> 4) | ((b[2] & 0x80) >> 5) | ((b[3] & 0x80) >> 6) | (b[4] >> 7);
            assert_eq!(got, bits);
        }
    }

    #[test]
    fn wander_moves_the_actor_and_stays_inside_its_box() {
        let code = wander(0x10, 0x10, 0x18, 0x18, 3);
        let (min, max) = (0x10 * 0x80 + 0x40, 0x18 * 0x80 + 0x40);
        let mut vm = AmbientMotion::new(1, 0).with_position(0x14 * 0x80 + 0x40, 0x14 * 0x80 + 0x40);
        let start = (vm.x, vm.z);
        let mut moved_ticks = 0;
        for _ in 0..4000 {
            // Retail loops the stream; the op retires often, so re-arm it.
            if usize::from(vm.pc) >= code.len() {
                vm.pc = 0;
            }
            vm.tick(&code, 1);
            if vm.moved {
                moved_ticks += 1;
            }
            assert!(
                i32::from(vm.x) >= min - 0x40 && i32::from(vm.x) <= max + 0x40,
                "X {} escaped the box",
                vm.x
            );
            assert!(
                i32::from(vm.z) >= min - 0x40 && i32::from(vm.z) <= max + 0x40,
                "Z {} escaped the box",
                vm.z
            );
        }
        assert!(moved_ticks > 100, "the villager actually wanders");
        assert_ne!((vm.x, vm.z), start);
    }

    #[test]
    fn wander_outside_its_box_retires_without_moving() {
        let code = wander(0x10, 0x10, 0x18, 0x18, 3);
        let mut vm = AmbientMotion::new(1, 0).with_position(0, 0);
        vm.tick(&code, 1);
        assert_eq!((vm.x, vm.z), (0, 0));
        assert_eq!(vm.pc, 5, "op skipped entirely");
    }

    #[test]
    fn wander_turn_takes_the_shortest_arc() {
        // The wander's turn phase differs from `0x04`/`0x0D`: it picks the
        // short way round rather than an authored direction. Facing +Z
        // (0x800), a turn to -Z (0x000) may go either way, so use a
        // three-eighths case: from LUT 6 (+X, 0xC00) to LUT 0 (-Z, 0x000)
        // the short arc is increasing through the 0x1000 wrap.
        let mut vm = AmbientMotion::new(1, 0xC00).with_position(0, 0);
        vm.wander_dir = 0;
        vm.wander_phase = 1;
        // A live turn phase always carries a non-zero cursor (retail's phase
        // 1 writes 1 into it); a zero cursor would re-arm the entry guard.
        vm.cursor = 1;
        let code = wander(0x00, 0x00, 0x7F, 0x7F, 3);
        let mut seen = Vec::new();
        for _ in 0..64 {
            vm.tick(&code, 1);
            seen.push(vm.heading);
            if vm.wander_phase != 1 {
                break;
            }
        }
        assert_eq!(*seen.last().unwrap(), 0x000, "lands on the compass point");
        // Increasing through the wrap: every sample is >= 0xC00 until it
        // wraps to a small value, never dipping through 0x800.
        assert!(
            seen.iter().all(|&h| h >= 0xC00 || h <= 0x100),
            "took the long way round: {seen:04X?}"
        );
    }

    #[test]
    fn wander_masks_the_heading_every_tick() {
        // Unlike `0x04` / `0x0D`, no raw out-of-range heading is ever held.
        let code = wander(0x00, 0x00, 0x7F, 0x7F, 0);
        let mut vm = AmbientMotion::new(7, 0xC00).with_position(0x40 * 0x80, 0x40 * 0x80);
        for _ in 0..2000 {
            if usize::from(vm.pc) >= code.len() {
                vm.pc = 0;
            }
            vm.tick(&code, 1);
            assert!(vm.heading <= 0x0FFF, "raw heading {:#X} held", vm.heading);
        }
    }

    #[test]
    fn wander_walk_segment_is_half_a_tile_at_any_pace() {
        for bits in 0..4u8 {
            let mut vm = AmbientMotion::new(1, 0x800).with_position(0x40 * 0x80, 0x40 * 0x80);
            vm.wander_dir = 4; // +Z, already faced
            vm.wander_phase = 1;
            vm.cursor = 1;
            let code = wander(0x00, 0x00, 0x7F, 0x7F, bits);
            let z0 = vm.z;
            // One tick to clear the (no-op) turn phase, then the segment.
            for _ in 0..(2u32 << bits) + 1 {
                vm.tick(&code, 1);
            }
            assert_eq!(i32::from(vm.z) - i32::from(z0), 0x40, "bits={bits}");
        }
    }

    #[test]
    fn op17_installs_the_default_move_without_consuming_the_tick() {
        // `0x17` then a `0x05` wait: both run in the same tick.
        let code = [0x17u8, 0x0A, 0x0B, 0x05, 0x04];
        let mut vm = AmbientMotion::new(1, 0);
        vm.tick(&code, 1);
        assert_eq!(vm.default_move, [0x0A, 0x0B]);
        assert_eq!(vm.pc, 3, "stepped into the wait");
    }

    #[test]
    fn walk_ops_restamp_the_requested_move() {
        let code = [0x17u8, 0x0A, 0x0B, 0x03, 0x40, 0x01];
        let mut vm = AmbientMotion::new(1, 0).with_position(0, 0);
        vm.tick(&code, 1);
        assert_eq!(
            vm.requested_move,
            Some(0x0B),
            "the step prologue stamps the record's anim byte"
        );
    }

    #[test]
    fn scheduler_lerp_matches_the_retail_form() {
        // value = end + (start - end) * remaining / total, truncating.
        assert_eq!(lerp_remaining(0, 100, 100, 100), 0);
        assert_eq!(lerp_remaining(0, 100, 50, 100), 50);
        assert_eq!(lerp_remaining(0, 100, 1, 100), 99);
        assert_eq!(lerp_remaining(0, 100, 0, 100), 100);
        // Truncation is toward zero, so a descending ramp rounds up.
        assert_eq!(lerp_remaining(100, 0, 33, 100), 33);
    }

    #[test]
    fn scheduler_overflows_instead_of_evicting() {
        let mut s = RampScheduler::new();
        let r = Ramp {
            dest: 1,
            owner: 1,
            start: 0,
            end: 1,
            total: 100,
            remaining: 100,
            kind: RampKind::U16,
        };
        for _ in 0..RAMP_SLOTS {
            assert!(s.install(r).is_some());
        }
        assert!(s.install(r).is_none());
        assert_eq!(s.overflow, 1);
        assert_eq!(s.active(), RAMP_SLOTS);
    }

    #[test]
    fn facing_sites_walks_a_mixed_stream() {
        // 0x17 default-move (3), 0x04 ramp (3), 0x05 wait (2), 0x0D (4).
        let code = [
            0x17u8, 0x01, 0x02, 0x04, 0x83, 0x18, 0x05, 0x10, 0x0D, 0x05, 0x40, 0x00,
        ];
        let sites = facing_sites(&code);
        assert_eq!(sites.len(), 2);
        assert_eq!(sites[0].op, 0x04);
        assert_eq!(sites[0].lut_index, 3);
        assert!(sites[0].decreasing);
        assert_eq!(sites[0].duration, 0x18);
        assert_eq!(sites[1].op, 0x0D);
        assert_eq!(sites[1].lut_index, 5);
        assert!(!sites[1].decreasing);
        assert_eq!(sites[1].duration, 0x40);
        assert_eq!(sites[1].target(), 0xA00);
    }

    #[test]
    fn replay_lands_every_synthetic_site_on_its_compass_point() {
        for lut in 0..8u8 {
            for dec in [false, true] {
                for dur in [1i32, 5, 24, 0x7F] {
                    let site = AmbientFacingSite {
                        offset: 0,
                        op: 0x04,
                        lut_index: lut,
                        decreasing: dec,
                        duration: dur,
                    };
                    let trace = replay_site(&site, 0x321, 1);
                    assert_eq!(
                        *trace.last().unwrap(),
                        site.target(),
                        "lut {lut} dec {dec} dur {dur}"
                    );
                }
            }
        }
    }
}
