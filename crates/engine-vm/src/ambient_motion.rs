//! Ambient / idle **facing** channel of the second per-actor motion VM
//! (`FUN_80038158`, SCUS_942.54) - the runtime interpreter for its two
//! rotate ops, plus the generic ramp scheduler one of them delegates to.
//!
//! PORT: FUN_80038158, FUN_80036d80, FUN_8003c5f0
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
}

impl AmbientOp {
    pub fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0x01 => Self::Restart,
            0x04 => Self::FacingRamp,
            0x05 => Self::Wait,
            0x0D => Self::FacingTween,
            _ => return None,
        })
    }
}

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
        }
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
        let result = self.step_ops(code, speed);
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
