//! Move-buffer cursor advance + per-bone ramp envelope.
//!
//! PORT: FUN_800204F8, FUN_80020740
//!
//! These two SCUS helpers are the move-VM's per-frame pre-tick on a
//! single actor. They are invoked when the [`TickEvent::MoveVmKick`]
//! signal from [`actor_tick`] fires. Together they:
//!
//! 1. [`envelope_tick`] (FUN_80020740) ramps each per-bone weight in
//!    `+0xA0[bone]` toward `0x1000`, then back down toward `0`, in a
//!    triangular envelope. The `done_mask` bitfield at `+0x7C` tracks
//!    which bones have peaked. When every bone has finished, the
//!    envelope clears state and either snaps to "advance" (sets bit
//!    `0x800` in `+0x62`) or freezes the actor.
//! 2. [`cursor_advance`] (FUN_800204F8) advances the per-actor move
//!    cursor `+0x68`. When the requested-id field `+0x5C` changes, it
//!    re-resolves the current record pointer at `+0x4C`, resets the
//!    cursor, and arms the move VM by writing `1` to `+0x56`. The
//!    cursor then steps by `+0x6A * frame_delta` each frame, looping
//!    or clamping at the record boundary depending on `+0x62` bit
//!    `0x8`.
//!
//! The retail layout aliases `+0xA0` / `+0xB8` / `+0xC8` with fields
//! that the actor-physics tick reads as different types (`range_z_low`
//! / `path_active` / `spline_step_x`). To keep the alias from
//! cross-contaminating the physics tick this port lives on its own
//! struct ([`MoveBufferState`]) rather than extending
//! [`actor_tick::ActorPhysics`].
//!
//! ## Port boundary
//!
//! No bytes from `SCUS_942.54` live in this crate. The two reference
//! dumps (`ghidra/scripts/funcs/800204f8.txt`,
//! `ghidra/scripts/funcs/80020740.txt`) are the *spec*. Move-buffer
//! record bytes are supplied by the engine via [`MoveBufferHost`];
//! the trait abstracts the three retail buffer roots (`_DAT_8007B75C`,
//! `_DAT_8007B888`, `_DAT_8007B840`).
//!
//! [`TickEvent::MoveVmKick`]: crate::actor_tick::TickEvent::MoveVmKick
//! [`actor_tick`]: crate::actor_tick
//! [`actor_tick::ActorPhysics`]: crate::actor_tick::ActorPhysics

/// Upper bound on per-actor bones the envelope can track. The retail
/// envelope masks the lane index with `0x1F` (`uVar3 & 0x1F` in the
/// dump), so 32 lanes is the hard cap. Field-actor move buffers
/// observed in retail use ≤16 bones; the cap leaves headroom.
pub const MAX_BONES: usize = 32;

/// `actor[+0x10]` status-flag bit that gates the envelope pre-tick.
/// When set, [`cursor_advance`] calls [`envelope_tick`] before doing
/// any cursor work. Cleared when the actor stops streaming.
pub const STATUS_FLAG_ENVELOPE_ACTIVE: u32 = 0x1000;

/// `actor[+0x10]` status-flag bit that selects the alternate
/// move-buffer pool (`_DAT_8007B75C` in retail). The engine's
/// [`MoveBufferHost`] implementation reads this bit to choose which
/// buffer pool to resolve out of.
pub const STATUS_FLAG_ALT_POOL: u32 = 0x1000000;

/// Threshold on `cursor_requested` above which the retail dispatcher
/// switches from the primary `_DAT_8007B888` buffer to the secondary
/// `_DAT_8007B840` buffer (`MOVE2`). Only relevant when the alt-pool
/// bit is clear.
pub const MOVE2_THRESHOLD: i16 = 0x400;

/// `+0x62` flag bits read by the envelope and cursor.
pub mod env_flag {
    /// `0x2` - pause. When set, the cursor does not step.
    pub const PAUSE: u16 = 0x0002;
    /// `0x8` - clamp at the ends. When set, the cursor holds at `0`
    /// (below) and at `count * 16 - 1` (above); when clear it wraps -
    /// forward by `count * 16` below, back to `0` above
    /// (`0x800206AC`, `0x80020708`). An earlier port named this `CLAMP_ENDS` and
    /// had the low bound's two arms swapped.
    pub const CLAMP_ENDS: u16 = 0x0008;
    /// `0x80` - reverse. When set, cursor steps backward.
    pub const REVERSE: u16 = 0x0080;
    /// `0x100` - "clamped this frame". OR'd in by the cursor whenever
    /// it had to snap or wrap at the record boundary.
    pub const CLAMPED: u16 = 0x0100;
    /// `0x200` - start request. Cleared by the cursor on entry; the
    /// cursor (re)initialises `+0x68` to either `0` or `(count-1)*16`
    /// depending on REVERSE.
    pub const START_REQUEST: u16 = 0x0200;
    /// `0x400` - hold (envelope only). When set, the envelope skips
    /// the down-ramp pass, leaving every bone at peak.
    pub const HOLD: u16 = 0x0400;
    /// `0x800` - "envelope advance". OR'd in by the envelope when
    /// every bone has finished its ramp, signalling the caller to
    /// advance the move-VM PC.
    pub const ENVELOPE_ADVANCE: u16 = 0x0800;
    /// `0x1000` - "snap-down at lane 0". When the down-ramp wraps
    /// lane 0 below zero, this bit decides between clearing the
    /// finishing sign-bit in `done_mask` (set) and OR-ing
    /// ENVELOPE_ADVANCE (clear).
    pub const LANE0_SNAP_DOWN: u16 = 0x1000;
    /// `0x2000` - init request. When set, the envelope primes every
    /// lane to `0x1000`, sets every bit in `done_mask`, sets the
    /// finishing sign-bit, and clears this flag.
    pub const INIT: u16 = 0x2000;
    /// `0x4000` - "clear-and-reset on completion". When set together
    /// with the up-ramp reaching the last lane, the envelope zeroes
    /// every lane and clears `done_mask`.
    pub const RESET_ON_COMPLETE: u16 = 0x4000;
    /// `0x8000` - frozen. When set, [`super::envelope_tick`] returns
    /// immediately.
    pub const FROZEN: u16 = 0x8000;
}

use env_flag::*;

/// `done_mask` (`+0x7C`) sign bit. Set when every lane has reached
/// peak; the down-ramp pass runs only while this bit is set.
pub const DONE_MASK_FINISHING: u32 = 0x80000000;

/// Per-actor move-buffer state read & written by [`envelope_tick`]
/// and [`cursor_advance`]. Field offsets in the docstrings match the
/// retail actor record (`actor[+0xXX]`); the names are renamed to
/// match the move-VM semantics rather than the dispatch-byte-aliased
/// physics view in [`ActorPhysics`].
///
/// [`ActorPhysics`]: crate::actor_tick::ActorPhysics
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveBufferState {
    /// `actor[+0x10]` status-flag word. Bits read:
    /// [`STATUS_FLAG_ENVELOPE_ACTIVE`] gates [`envelope_tick`];
    /// [`STATUS_FLAG_ALT_POOL`] is forwarded to the host's record
    /// resolver.
    pub status_flags: u32,
    /// `actor[+0x4C]` "current move record" pointer. Stored as a
    /// boolean since this port doesn't dereference the pointer
    /// directly - the host's resolver supplies the bytes each frame.
    pub record_bound: bool,
    /// `actor[+0x56]` move-VM kick flag. Set to `1` whenever the
    /// cursor latches a new record; cleared by the move-VM
    /// dispatcher after it consumes the kick.
    pub move_vm_kick: i16,
    /// `actor[+0x5C]` requested move id (set by callers; the cursor
    /// reads it).
    pub cursor_requested: i16,
    /// `actor[+0x5E]` active move id (latched copy of
    /// `cursor_requested` from the last new-record event).
    pub cursor_active: i16,
    /// `actor[+0x62]` envelope + cursor state flags. See
    /// [`env_flag`].
    pub env_flags: u16,
    /// `actor[+0x68]` cursor phase in 1/16 frame units.
    pub phase: i16,
    /// `actor[+0x6A]` per-frame phase advance (re-derived from the
    /// record's `+0x06` divisor when the record's `+0x01` flag bit 0
    /// is set).
    pub phase_rate: i16,
    /// `actor[+0x6C]` per-actor bone count.
    pub bone_count: u8,
    /// `actor[+0x7C]` per-bone completion bitfield. Bits `0..bone_count`
    /// flag "this lane has peaked"; bit `0x80000000` flags "every
    /// lane is peaked, run the down-ramp".
    pub done_mask: u32,
    /// `actor[+0xA0 + lane*2]` per-bone ramp values in `0..=0x1000`.
    /// Only `lanes[..bone_count]` are read; lanes past that are
    /// reserved.
    pub lanes: [u16; MAX_BONES],
    /// `actor[+0xB0 + lane]` per-lane VDF sub-entry index - the morph
    /// record the lane's weight drives. Written a **byte** per lane by
    /// move-VM op `0x0A`, read back by the morph stager `FUN_8001C604`
    /// alongside [`Self::lanes`]; see [`crate::vdf_morph`].
    pub vdf_slot: [u8; MAX_BONES],
    /// `actor[+0xB8 + lane*2]` per-lane up-ramp velocity (scaled by
    /// `frame_delta`).
    ///
    /// This is **per lane**, not one global rate: `FUN_80020740` reads
    /// it at `0xb8(a1)` with `a1 = actor + lane*2`, and move-VM op
    /// `0x0A` writes one value per morph slot from its operand stream.
    pub up_velocity: [i16; MAX_BONES],
    /// `actor[+0xC8 + lane*2]` per-lane down-ramp velocity (scaled by
    /// `frame_delta`). Per lane for the same reason as
    /// [`Self::up_velocity`] (`0xc8(a1)`).
    pub down_velocity: [i16; MAX_BONES],
}

impl Default for MoveBufferState {
    fn default() -> Self {
        Self {
            status_flags: 0,
            record_bound: false,
            move_vm_kick: 0,
            cursor_requested: 0,
            cursor_active: 0,
            env_flags: 0,
            phase: 0,
            phase_rate: 0,
            bone_count: 0,
            done_mask: 0,
            lanes: [0; MAX_BONES],
            vdf_slot: [0; MAX_BONES],
            up_velocity: [0; MAX_BONES],
            down_velocity: [0; MAX_BONES],
        }
    }
}

impl MoveBufferState {
    /// Convenience: read the `0x1000`-saturated peak value.
    pub const PEAK: u16 = 0x1000;

    /// Set the same up-ramp velocity on every lane. Retail authors one
    /// value per morph slot (move-VM op `0x0A`); this is only a
    /// convenience for callers and tests that drive a uniform ramp.
    pub fn set_uniform_up_velocity(&mut self, v: i16) {
        self.up_velocity = [v; MAX_BONES];
    }

    /// Set the same down-ramp velocity on every lane. See
    /// [`Self::set_uniform_up_velocity`].
    pub fn set_uniform_down_velocity(&mut self, v: i16) {
        self.down_velocity = [v; MAX_BONES];
    }
}

/// Resolves a move-buffer record by id. Implementations live in the
/// engine layer; the retail counterpart is the per-arm pointer-table
/// lookup in [`cursor_advance`]:
///
/// ```text
///   if (actor[+0x10] & 0x01000000) {
///       table = _DAT_8007B75C;
///   } else if (actor[+0x5C] >= 0x400) {
///       table = _DAT_8007B840;          // MOVE2
///   } else {
///       table = _DAT_8007B888;          // MOVE
///   }
///   record_offset = table[(actor[+0x5C] & 0x3FF) * 4];
///   record = table + record_offset;
/// ```
///
/// `actor_status_flags` is the actor's `+0x10` word; the
/// implementation reads [`STATUS_FLAG_ALT_POOL`] and may inspect
/// `requested_id` (the `+0x5C` value) to pick the right buffer.
pub trait MoveBufferHost {
    /// Return the record bytes for `requested_id` against the buffer
    /// pool selected by `actor_status_flags`. Returns `None` when the
    /// id is out of range; the cursor leaves state untouched in that
    /// case (matching the retail behaviour - the index calculation
    /// would have read past the end of the table, but in practice the
    /// engine clips before reaching here).
    ///
    /// The returned slice must be long enough to read offsets `+0x01`
    /// (flag byte), `+0x02..=+0x03` (frame-count u16), and `+0x06`
    /// (divisor byte). Implementations typically return the full
    /// record body so the move-VM dispatcher can read further offsets.
    fn resolve_record(&self, actor_status_flags: u32, requested_id: i16) -> Option<&[u8]>;
}

/// Per-bone ramp envelope tick - port of `FUN_80020740`
/// (`ghidra/scripts/funcs/80020740.txt`).
///
/// `frame_delta` is the per-frame ramp step in 1/16 units (mirrors
/// `DAT_1F800393` in the retail dispatcher; one byte; `1` is the idle
/// rate).
///
/// The envelope has four phases, gated by `env_flags`:
/// 1. **Frozen** ([`FROZEN`]): bail immediately.
/// 2. **Init** ([`INIT`]): prime every lane to `0x1000`, set every
///    bit in `done_mask` plus the finishing sign-bit, clear
///    [`INIT`]. (Init touches the lane weights and `done_mask` only -
///    the per-lane velocities are authored by move-VM op `0x0A` and
///    are not reset here.)
/// 3. **Up-ramp**: each frame, every lane that hasn't peaked steps
///    up by `up_velocity[lane] * frame_delta`. When a lane peaks the
///    corresponding bit in `done_mask` is set; when the last lane
///    peaks the finishing sign-bit is set (or [`ENVELOPE_ADVANCE`] is
///    OR'd in if [`HOLD`] was set).
/// 4. **Down-ramp** (skipped when [`HOLD`] is set): each frame, every
///    peaked lane steps down by `down_velocity[lane] * frame_delta`. When
///    lane 0 wraps below zero, the envelope either clears the
///    finishing bit ([`LANE0_SNAP_DOWN`] set) or OR's in
///    [`ENVELOPE_ADVANCE`] (clear).
pub fn envelope_tick(state: &mut MoveBufferState, frame_delta: u8) {
    if state.env_flags & FROZEN != 0 {
        return;
    }

    if state.env_flags & INIT != 0 {
        // Prime every lane to peak; arm the finishing sign-bit.
        for lane in 0..state.bone_count as usize {
            state.done_mask |= 1u32 << (lane & 0x1F);
            state.lanes[lane] = MoveBufferState::PEAK;
        }
        state.done_mask |= DONE_MASK_FINISHING;
        state.env_flags &= !INIT;
    }

    // The retail body also has a debug-mime print branch gated on
    // `_DAT_8007B6D0 & 1` and `env_flags & 0x800`; engines drop that.

    // Early-exit ENVELOPE_ADVANCE gate. Retail flow:
    //   if (done_mask & 0x80000000) {
    //       // FINISHING: down-ramp done iff lanes[0] == 0.
    //       if (lanes[0] == 0) env_flags |= 0x800;
    //   } else {
    //       // UP-RAMP: ramp done iff last lane reached peak.
    //       if (lanes[bone_count - 1] == 0x1000) env_flags |= 0x800;
    //   }
    // Either way, the per-lane loop below still runs - the flag is
    // just the move-VM-facing signal.
    let bc = state.bone_count as usize;
    if bc > 0 {
        let advance = if state.done_mask & DONE_MASK_FINISHING != 0 {
            state.lanes[0] == 0
        } else {
            state.lanes[bc - 1] == MoveBufferState::PEAK
        };
        if advance {
            state.env_flags |= ENVELOPE_ADVANCE;
        }
    }

    // Up-ramp + down-ramp pass.
    for lane in 0..bc {
        let bit = 1u32 << (lane & 0x1F);
        let dmask = state.done_mask;
        let peaked = dmask & bit != 0;
        let finishing = dmask & DONE_MASK_FINISHING != 0;
        let prev_peaked = lane > 0 && dmask & (1u32 << ((lane - 1) & 0x1F)) != 0;

        // Up-ramp.
        if !peaked && !finishing {
            // Lane 0 always ramps when not yet peaked; later lanes
            // ramp only after the previous lane peaked (cascade).
            if lane == 0 || prev_peaked {
                let step = i32::from(state.up_velocity[lane]) * i32::from(frame_delta);
                let new = i32::from(state.lanes[lane]) + step;
                state.lanes[lane] = new as u16;
            }
            if state.lanes[lane] as i32 > i32::from(MoveBufferState::PEAK as i16) {
                // Overshoot: clamp + mark this lane done.
                state.lanes[lane] = MoveBufferState::PEAK;
                state.done_mask |= bit;
                if lane == bc - 1 {
                    if state.env_flags & HOLD == 0 {
                        state.done_mask |= DONE_MASK_FINISHING;
                    } else {
                        state.env_flags |= ENVELOPE_ADVANCE;
                    }
                    if state.env_flags & RESET_ON_COMPLETE != 0 {
                        for v in state.lanes[..bc].iter_mut() {
                            *v = 0;
                        }
                        state.done_mask = 0;
                    }
                }
            }
        }

        // Down-ramp. Skipped when HOLD is set.
        if state.env_flags & HOLD == 0 {
            let dmask = state.done_mask;
            let finishing = dmask & DONE_MASK_FINISHING != 0;
            let peaked = dmask & bit != 0;
            if finishing && peaked {
                // Drain this lane only when the next lane has
                // already drained (cascade goes top -> bottom).
                let next_drained =
                    lane == bc - 1 || (state.done_mask & (1u32 << ((lane + 1) & 0x1F))) == 0;
                if next_drained {
                    let step = i32::from(state.down_velocity[lane]) * i32::from(frame_delta);
                    let new = i32::from(state.lanes[lane] as i16) - step;
                    state.lanes[lane] = new as u16;
                }
                // The retail `0x3e80` overshoot test is the signed
                // wrap detector: when a `u16` lane underflows past
                // zero, the value lands above `0x3e80` (16000) as an
                // unsigned read. Detect drained.
                if state.lanes[lane] >= 0x3e81 {
                    state.lanes[lane] = 0;
                    state.done_mask &= !bit;
                    if lane == 0 {
                        if state.env_flags & LANE0_SNAP_DOWN == 0 {
                            state.env_flags |= ENVELOPE_ADVANCE;
                        } else {
                            state.done_mask &= !DONE_MASK_FINISHING;
                        }
                    }
                }
            }
        }
    }
}

/// Per-actor move-buffer cursor advance - port of
/// `FUN_800204F8` (`ghidra/scripts/funcs/800204f8.txt`).
///
/// `frame_delta` is the per-frame cursor step in 1/16 frame units
/// (mirrors `DAT_1F800393`; idle = `1`).
///
/// The cursor reads the current record bytes from `host`; the
/// resolver returns `Some(record_bytes)` for in-range ids and `None`
/// otherwise. When the cursor latches a new id it:
/// - copies `cursor_requested` into `cursor_active`,
/// - resets `phase` to `0`,
/// - sets `move_vm_kick = 1` (the move VM picks this up next frame),
/// - marks `record_bound = true` (the engine resolves the pointer
///   itself).
///
/// Each frame the cursor steps `phase` by `rate * frame_delta` (sign
/// flipped when [`REVERSE`] is set), where `rate` is `phase_rate` - or,
/// when the record's flag bit 0 is set, `(phase_rate * 2 + divisor - 1) /
/// divisor`, computed into a register and **not** stored back
/// (`0x800205C8..0x800205E0`; an earlier port wrote it into `phase_rate`,
/// so the rate shrank again every frame). At record boundaries:
/// - `phase < 0`: hold at 0 with [`CLAMP_ENDS`], else wrap forward by
///   `frame_count * 16`; OR in [`CLAMPED`].
/// - `phase >= frame_count * 16 - 1`: hold at `frame_count * 16 - 1` with
///   [`CLAMP_ENDS`], else back to 0; OR in [`CLAMPED`].
pub fn cursor_advance<H: MoveBufferHost>(state: &mut MoveBufferState, host: &H, frame_delta: u8) {
    if state.status_flags & STATUS_FLAG_ENVELOPE_ACTIVE != 0 {
        envelope_tick(state, frame_delta);
    }
    if state.cursor_requested <= 0 {
        return;
    }
    let Some(record) = host.resolve_record(state.status_flags, state.cursor_requested) else {
        return;
    };
    if record.len() < 7 {
        // Not enough bytes for the offsets the cursor reads. Bail
        // rather than panic; engines should supply at least 7 bytes.
        return;
    }
    if state.cursor_requested != state.cursor_active {
        state.cursor_active = state.cursor_requested;
        state.phase = 0;
        state.move_vm_kick = 1;
        state.record_bound = true;
    }

    let record_flag = record[1];
    let frame_count = u16::from_le_bytes([record[2], record[3]]);
    let divisor = record[6];

    let rate = if record_flag & 0x1 != 0 && divisor != 0 {
        // (phase_rate * 2 + divisor - 1) / divisor, signed; a local only.
        let numer = i32::from(state.phase_rate) * 2 + i32::from(divisor) - 1;
        numer / i32::from(divisor)
    } else {
        i32::from(state.phase_rate)
    };

    let mut e = state.env_flags;
    if e & START_REQUEST != 0 {
        e &= !START_REQUEST;
        if e & REVERSE != 0 {
            // Begin at end of record (count - 1) * 16.
            let initial = i32::from(frame_count.saturating_sub(1)) * 16;
            state.phase = initial as i16;
        } else {
            state.phase = 0;
        }
        state.env_flags = e;
    }

    state.env_flags &= !CLAMPED;
    // Every cursor store below is retail's `sh` - a 16-bit truncation, so an
    // out-of-range sum wraps rather than saturating.
    let e_step = state.env_flags;
    if e_step & PAUSE == 0 {
        let step = rate * i32::from(frame_delta);
        let new = if e_step & REVERSE != 0 {
            i32::from(state.phase) - step
        } else {
            i32::from(state.phase) + step
        };
        state.phase = new as i16;
    }

    let max = i32::from(frame_count) * 16 - 1;
    if state.phase < 0 {
        if state.env_flags & CLAMP_ENDS != 0 {
            state.phase = 0;
        } else {
            let wrap = i32::from(state.phase) + i32::from(frame_count) * 16;
            state.phase = wrap as i16;
        }
        state.env_flags |= CLAMPED;
    }
    if i32::from(state.phase) >= max {
        if state.env_flags & CLAMP_ENDS == 0 {
            state.phase = 0;
        } else {
            state.phase = max as i16;
        }
        state.env_flags |= CLAMPED;
    }
}

#[cfg(test)]
mod tests;
