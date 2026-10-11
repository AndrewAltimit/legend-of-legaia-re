//! The second per-actor motion VM (`FUN_80038158`, SCUS_942.54) - the
//! ambient / idle interpreter for MAN tail-section 1 - plus the generic ramp
//! scheduler its `0x0D` rotate delegates to. The section below documents the
//! two rotate ops in detail; [`AmbientMotion::step_ops_with`] runs the whole
//! opcode table.
//!
//! PORT: FUN_80038158, FUN_80036d80, FUN_8003c5f0
//! REF: FUN_801cf8ac, FUN_801d5a68, FUN_801cfe4c, FUN_80056798
//!
//! [`super::motion_vm`] ports the *other* motion VM (`FUN_8003774C`, the
//! pursue / patrol / face-target one). This module is the ambient sibling:
//! the VM whose bytecode arrives as MAN tail-section 1
//! (`legaia_scene_formats::man_motion`) and which gives a standing town NPC its idle
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
//! Retail body `0x8003859C..0x800386A0`. Per tick:
//!
//! ```text
//! requested move = record.anim            ; if record.move != 0x8C (prologue)
//! frames    = b2 & 0x7F                  ; frame budget
//! remaining = frames - cursor            ; cursor = actor +0x8B, u8
//! target    = LUT[b1 & 7]
//! cursor   += 1                          ; unit-per-tick, NOT the frame scalar
//! if remaining == 0:                     ; terminal
//!     heading = target                   ; exact snap
//!     cursor  = 0 ; pc += 3 ; fall through to the next op THIS tick
//!     restamp move/anim from the record's move byte  ; epilogue 0x800390A8
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
//!
//!   **Not executed here**: the stepped-over set, by op number, is `0x02`,
//!   `0x06`, `0x07`, `0x08`, `0x09`, `0x0A`, `0x0B`, `0x0C`, `0x0E`, `0x0F`,
//!   `0x10`, `0x11`, `0x12`, `0x13`, `0x14`, `0x15` and `0x16` - every
//!   remaining entry in [`legaia_scene_formats::man_motion::op_width`]'s space. Their
//!   widths are known (that is what lets the walk stay in sync) and `0x07` /
//!   `0x08` are separately decoded as the story-flag writers by the static
//!   census `legaia-engine man-scripts --motion-flag-census`, but no body here
//!   executes any of them.
//!
//!   This sentence used to open with the port-catalog's module-wide
//!   inert-disclosure marker. It is a statement about retail *opcodes*, not
//!   about this module's wiring - the module is live, driven per frame from
//!   `World`'s field actor tick - and as a blanket it declared every anchor
//!   in the file inert, which is what put four rows in `--live-audit`'s
//!   stale section. The marker is matched anywhere in a doc block, so it
//!   cannot appear in prose here. See
//!   `docs/tooling/stale-not-wired-triage.md`.
//!
//! ## The `0x05` wait is interruptible
//!
//! Op `0x05` is not a bare countdown. Its arm opens at `0x8003882C` with a
//! **touch-wake** head that reads the motion VM's one-slot mailbox
//! `DAT_80073F1C` - the word the collision probe posts through `FUN_8003D038`
//! ([`crate::motion_vm::post_touch`]) when the player's box overlaps an
//! actor. When the mailbox names this actor and its `0x801C6470` record
//! carries a default move, the head rewrites the wait cursor to
//! `duration - DAT_1F800393` so the countdown immediately below expires the
//! wait on that frame, then clears the mailbox. That is why walking into a
//! standing town NPC breaks it out of its idle pause instead of leaving it
//! parked for the rest of the authored countdown; see
//! [`AmbientMotion::pending_touch`].

use legaia_scene_formats::man_motion::op_width;

mod blocking;
pub use blocking::*;
mod ramps;
pub use ramps::*;
mod machine;
pub use machine::*;
mod facing_sites;
pub use facing_sites::*;

/// Retail's `0x801C66A0` pool is 64 slots of stride `0x20`; slot 0 is the
/// intrusive list header, leaving 63 allocatable.
pub const RAMP_SLOTS: usize = 63;

/// Per-tick op budget. Retail has none - it loops until an op sets the
/// did-work counter. Because this port steps over the ops it does not model
/// (which include retail's yielding walk ops), it needs a stop.
pub const MAX_OPS_PER_TICK: usize = 256;

#[cfg(test)]
mod tests;
