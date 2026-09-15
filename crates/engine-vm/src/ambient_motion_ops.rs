//! The scripted-motion VM's non-facing opcodes - the seventeen case bodies of
//! `FUN_80038158`'s jump table (`0x80010FE8`) that `ambient_motion` itself
//! does not carry.
//!
//! PORT: FUN_80038158 (the `0x02`, `0x06`..`0x0C`, `0x0E`..`0x16` arms)
//! REF: FUN_80035B50, FUN_80024E08, FUN_80058490, FUN_80019278, FUN_8001698C
//!
//! [`super::ambient_motion`] carries the dispatch loop, the variant cursor,
//! the two facing ramps, the walk ops and the ramp scheduler. This file is
//! the rest of the table, written as further `impl AmbientMotion` blocks so
//! the VM keeps **one** executing home: the loop in `step_ops_with` reaches
//! every one of retail's `0x01..=0x20` slots and nothing is stepped over by
//! width any more.
//!
//! ## What "consumes the tick" means here
//!
//! Retail's interpreter is a `while (!did_work)` loop over the bytecode: the
//! dispatch head at `0x800382F0` re-reads `actor+0x84`, jumps through the
//! table, and the epilogue at `0x80039B44` loops back unless the arm
//! incremented the did-work register `s8`. So an op "consumes the tick" iff
//! its arm does `addiu s8, s8, 1`; everything else advances the PC and the
//! **next op runs in the same frame**. Of the arms in this file only `0x06`
//! (the home-box step) and `0x12` (the wait-for-bit) ever do - and `0x12`
//! does it unconditionally, in the delay slot at `0x80039634`, even on the
//! frame it retires.
//!
//! ## The two flag words each addressed as two halves
//!
//! `0x10` / `0x11` / `0x12` all share one selector decode on their single
//! operand byte (`0x80039500`, `0x80039590`, `0x80039624` - three verbatim
//! copies). `b1 & 0xC0` picks the word and `b1 & 0x30` picks which half of
//! it, in the same sense both times: **non-zero selects the low halfword**.
//!
//! | `b1 & 0xC0` | `b1 & 0x30` | retail address | field |
//! |---|---|---|---|
//! | `0x00` | non-zero | `actor+0x10` | low half of the actor flag word |
//! | `0x00` | zero | `actor+0x12` | high half of the same word |
//! | `0x40` | any | `actor+0x62` | the motion-clip control word |
//! | `0x80` | non-zero | `0x1F800394` | low half of the scratchpad global word |
//! | `0x80` | zero | `0x1F800396` | high half of the same word |
//! | `0xC0` | any | *(none)* | stores the assert code `0x3039` at `0x8007B828` and leaves the pointer **null** |
//!
//! The bit index is `b1 & 0x0F` in every case, so only bits `0..=15` of the
//! selected halfword are reachable - which is how the high-half selectors
//! earn their place. The `0xC0` arm then reads and writes through a null
//! pointer; no authored operand selects it, and the port reports
//! [`AmbientEffect::BitTargetFault`] instead of reproducing the scribble.

use super::ambient_motion::{
    AmbientBlocking, AmbientMotion, DEFAULT_MOVE_UNSET, RAMP_DEST_BLEND, RAMP_DEST_PITCH,
    RAMP_DEST_ROLL, RAMP_DEST_SCALE, RAMP_DEST_TINT, Ramp, RampKind, WALK_DIR_BITS,
    heading_lut_retail, walk_apply,
};

/// Actor flag-word bit `0x0100_0000` - the translucent-draw bit. Ops `0x0A`
/// and `0x0B` set / clear it outright and `0x0E` derives it from which model
/// bank it swapped to. Same bit the dance minigame's spawn sets on its
/// kind-0 floor actors.
pub const ACTOR_FLAG_TRANSLUCENT: u32 = 0x0100_0000;

/// Actor flag-word bit `0x2000_0000` - the "Y is externally driven" override
/// the driver `FUN_8003BC08` reads. Op `0x0F` skips its ground-height
/// resample when it is set.
pub const ACTOR_FLAG_Y_OVERRIDE: u32 = 0x2000_0000;

/// Which model-id base op `0x0E` adds its operand to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelBank {
    /// `*(u16*)0x8007B6F8` - the scene's own model-bank base, the same word
    /// the placement spawner `FUN_8003A1E4` adds a placement's model byte to
    /// (`0x8003A2F0`). Selected by an operand below `0xF0`.
    Scene,
    /// `*(u16*)0x8007B824` - the second base, indexed by `operand - 0xF0`.
    /// Selecting it also raises [`ACTOR_FLAG_TRANSLUCENT`].
    Special,
}

/// A side effect one tick of the VM produced for its host to apply. Retail
/// writes each of these straight into a global or calls a library routine;
/// the port queues them on the channel so the host drains them after the
/// tick, in retail's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmbientEffect {
    /// Op `0x07` `[07, lo, hi]` - set bit `lo | hi << 8` of the system
    /// story-flag bank `DAT_80085758` (`0x800390D0`: byte `idx >> 3`, mask
    /// `0x80 >> (idx & 7)`).
    SystemFlagSet(u16),
    /// Op `0x08` - the same index, cleared (`0x80039114`).
    SystemFlagClear(u16),
    /// Op `0x09` `[09, lo, hi]` - `FUN_80035B50((s16)(lo | hi << 8))`, the
    /// **SFX-cue enqueue**: store the cue id into the four-slot ring
    /// `DAT_8007B6D8` at the cursor `gp+0x158`, latch that index at
    /// `gp+0x15A`, zero the slot's companion delay word
    /// `DAT_8007C338 + i*4`, and advance the cursor mod 4. The same kernel
    /// the field VM's op `0x36` sub-`0` runs.
    SfxCue(i16),
    /// Op `0x0E` - `FUN_80024E08(actor, base + offset)`: zero the actor's
    /// anim cursor `+0x5C`, write the model id to `+0x64` and reload the
    /// mesh.
    ModelSwap { bank: ModelBank, offset: i16 },
    /// Op `0x0F` - the actor was moved to a tile centre. `sample_ground` is
    /// retail's `FUN_80019278` re-sample of `+0x16`, which it skips when the
    /// actor carries [`ACTOR_FLAG_Y_OVERRIDE`].
    Teleport { x: i16, z: i16, sample_ground: bool },
    /// Op `0x13` - `FUN_80058490(&rect, dx, dy)`, the libgpu `MoveImage`
    /// VRAM-to-VRAM blit. `rect` is `[x, y, w, h]` as four little-endian
    /// halfwords from operand bytes 1..8.
    MoveImage { rect: [u16; 4], dx: i16, dy: i16 },
    /// A `0x10` / `0x11` / `0x12` operand whose `b1 & 0xC0` is `0xC0`.
    /// Retail stores the assert code `0x3039` at `0x8007B828` and then reads
    /// and writes through a null pointer; the port reports it and leaves
    /// every word alone.
    BitTargetFault,
}

/// Which word a `0x10` / `0x11` / `0x12` operand addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitTarget {
    /// `actor+0x10` - low half of the actor flag word.
    ActorFlagsLo,
    /// `actor+0x12` - high half of the same word.
    ActorFlagsHi,
    /// `actor+0x62` - the motion-clip control word.
    ClipControl,
    /// `0x1F800394` - low half of the scratchpad global flag word.
    GlobalsLo,
    /// `0x1F800396` - high half of the same word.
    GlobalsHi,
    /// `b1 & 0xC0 == 0xC0`: retail's null-pointer arm.
    Fault,
}

impl BitTarget {
    /// The shared selector decode of `0x80039500` / `0x80039590` /
    /// `0x80039624`.
    pub fn from_selector(b1: u8) -> Self {
        match b1 & 0xC0 {
            0x00 => {
                if b1 & 0x30 != 0 {
                    Self::ActorFlagsLo
                } else {
                    Self::ActorFlagsHi
                }
            }
            0x40 => Self::ClipControl,
            0x80 => {
                if b1 & 0x30 != 0 {
                    Self::GlobalsLo
                } else {
                    Self::GlobalsHi
                }
            }
            _ => Self::Fault,
        }
    }
}

impl AmbientMotion {
    // -----------------------------------------------------------------
    // 0x02 / 0x0A / 0x0B - the requested-move pair, gated on the arena
    // -----------------------------------------------------------------

    /// The shared tail at `0x800383D4`: write the operand halfword to
    /// **both** `actor+0x88` (requested move) and `actor+0x5C` (anim
    /// cursor). One `sh` each, of the same sign-extended `b1 | b2 << 8`.
    fn write_move_pair(&mut self, body: &[u8]) {
        let v = i16::from_le_bytes([body[1], body[2]]);
        self.move_pair = Some(v);
        self.requested_move = Some(v as u8);
    }

    /// PORT: FUN_80038158 (`0x80038350`, op `0x02`)
    ///
    /// `[02, lo, hi]`. Writes the requested-move / anim pair **only while
    /// the actor's `0x801C6470` record still holds the `0x8C` unset
    /// sentinel** (`beq v0, t6, 0x800383D4` - the branch is taken on
    /// *equal*). An actor that has run a `0x17` therefore ignores this op
    /// entirely: the default-move record outranks it. Advances three bytes
    /// and does not consume the tick.
    pub fn step_op_anim_pair(&mut self, body: &[u8]) {
        if self.default_move[0] == DEFAULT_MOVE_UNSET {
            self.write_move_pair(body);
        }
        self.pc = self.pc.wrapping_add(3);
    }

    /// PORT: FUN_80038158 (`0x80038378` op `0x0A`, `0x800383A4` op `0x0B`)
    ///
    /// `[0A|0B, lo, hi]`. Raise (`0x0A`) or drop (`0x0B`)
    /// [`ACTOR_FLAG_TRANSLUCENT`] in the actor flag word, then fall into the
    /// same `0x800383D4` tail as `0x02`. The guard is the mirror image of
    /// `0x02`'s - `bne v0, t6, 0x80039B34` - so these two ops are *also*
    /// gated on the record still being unset, and an actor with a default
    /// move installed neither changes its translucency nor its move pair.
    /// Three bytes, no tick consumed.
    pub fn step_op_translucency(&mut self, body: &[u8], set: bool) {
        if self.default_move[0] != DEFAULT_MOVE_UNSET {
            self.pc = self.pc.wrapping_add(3);
            return;
        }
        if set {
            self.actor_flags |= ACTOR_FLAG_TRANSLUCENT;
        } else {
            self.actor_flags &= !ACTOR_FLAG_TRANSLUCENT;
        }
        self.write_move_pair(body);
        self.pc = self.pc.wrapping_add(3);
    }

    // -----------------------------------------------------------------
    // 0x06 - the home-relative one-tile step
    // -----------------------------------------------------------------

    /// PORT: FUN_80038158 (`0x800388C4`, op `0x06`)
    ///
    /// `[06, b1, b2, b3, b4]` - **one** random cardinal step of a full tile,
    /// bounded by a box measured from the actor's own anchor tile.
    ///
    /// The four operand bytes carry the box in their low seven bits as
    /// **signed tile deltas from the anchor** `actor+0x8C` / `+0x8D` (the
    /// pair op `0x0F` writes), and the 4-bit pace selector scattered over
    /// their high bits exactly as the `0x18` wander scatters it:
    ///
    /// ```text
    /// bits  = (b1&0x80)>>4 | (b2&0x80)>>5 | (b3&0x80)>>6 | b4>>7
    /// x_lo  = ((b1 + home_x    ) & 0x7F) << 7
    /// z_lo  = ((b2 + home_z    ) & 0x7F) << 7
    /// x_hi  = ((b3 + home_x + 1) & 0x7F) << 7
    /// z_hi  = ((b4 + home_z + 1) & 0x7F) << 7
    /// ```
    ///
    /// On the op's first tick (cursor zero) it draws `rand() & 6` - a
    /// cardinal, never a diagonal - and rejects the draw if the whole-tile
    /// destination (`±0x80`, not the wander's half tile) would leave that
    /// box, in which case the op retires. Otherwise the direction is stashed
    /// in the same `actor+0x86` bits 12-13 the wander uses and the op walks
    /// `0x80 >> (bits + 2)` units for `4 << bits` ticks - **128 units, a
    /// full tile, at any pace** - and then retires. There is no coin flip
    /// and no separate turn phase: the heading is snapped to the compass
    /// point every tick.
    ///
    /// The collision probe is the wander's three-point fan
    /// (`FUN_801D5A68`), not the directional step's single point; a blocked
    /// tick advances neither the cursor nor the PC, so a block on the very
    /// first tick redraws the direction next frame while a block mid-leg
    /// retries the same one.
    ///
    /// This arm **always** consumes the tick: every path through it passes
    /// an `addiu s8, s8, 1` (`0x80038A1C` on the rejected draw, `0x80038A3C`
    /// on every other).
    ///
    /// The `pad-echo / bounded chase step` reading this op carried is
    /// falsified: no pad word is read anywhere in the arm and the direction
    /// comes from the BIOS `rand()` at `0x80056798`.
    pub fn step_op_home_step(&mut self, body: &[u8], blocking: &dyn AmbientBlocking) {
        let (b1, b2, b3, b4) = (body[1], body[2], body[3], body[4]);
        let bits =
            u32::from(((b1 & 0x80) >> 4) | ((b2 & 0x80) >> 5) | ((b3 & 0x80) >> 6) | (b4 >> 7));

        if self.cursor & 0xFF == 0 {
            let (hx, hz) = (i32::from(self.home_tile.0), i32::from(self.home_tile.1));
            let bound =
                |b: u8, home: i32, plus_one: i32| ((i32::from(b) + home + plus_one) & 0x7F) << 7;
            let (x, z) = (i32::from(self.x), i32::from(self.z));
            let dir = (self.rand() & 6) as u8;
            let mask = WALK_DIR_BITS[usize::from(dir)];
            let rejected = (mask & 1 != 0 && bound(b4, hz, 1) < z + 0x80)
                || (mask & 2 != 0 && z - 0x80 < bound(b2, hz, 0))
                || (mask & 4 != 0 && bound(b3, hx, 1) < x + 0x80)
                || (mask & 8 != 0 && x - 0x80 < bound(b1, hx, 0));
            if rejected {
                // `0x80038A18`: PC past the op, tick consumed by the caller.
                self.pc = self.pc.wrapping_add(5);
                return;
            }
            self.wander_dir = dir;
        }

        let dir = self.wander_dir;
        self.heading = heading_lut_retail(dir);
        self.walk_yaw = true;
        if blocking.wander_blocked(self.x, self.z, dir >> 1) {
            // `0x80038A98` jumps straight into the `0x800390B4` reload.
            self.reload_requested_move();
            return;
        }
        if self.default_move[0] != DEFAULT_MOVE_UNSET {
            // `0x80038AC0`: the record's **anim** byte, as in the walk ops'
            // prologue.
            self.requested_move = Some(self.default_move[1]);
        }
        let step = (0x80u32 >> (bits + 2)) as i16;
        let (nx, nz) = walk_apply(self.x, self.z, dir, step);
        self.moved = nx != self.x || nz != self.z;
        self.x = nx;
        self.z = nz;

        let cur = ((self.cursor & 0xFF) as u8).wrapping_add(1);
        self.cursor = (self.cursor & 0xFF00) | u16::from(cur);
        if u32::from(cur) < (4u32 << bits) {
            return;
        }
        // `0x80039098` - the shared retire epilogue the wander also lands on.
        self.cursor &= 0xFF00;
        self.pc = self.pc.wrapping_add(5);
        self.reload_requested_move();
    }

    // -----------------------------------------------------------------
    // 0x07 / 0x08 / 0x09 - the posting ops
    // -----------------------------------------------------------------

    /// PORT: FUN_80038158 (`0x800390D0` op `0x07`, `0x80039114` op `0x08`)
    ///
    /// `[07|08, lo, hi]` - set or clear bit `(s16)(lo | hi << 8)` of the
    /// system story-flag bank `DAT_80085758`, MSB-first within the byte
    /// (`0x80 >> (idx & 7)`), which is the same bit law as `FUN_8003CE08`.
    /// Retail sign-extends the index and then arithmetic-shifts it right by
    /// three, so a negative operand indexes *below* the bank; no authored
    /// stream does. Three bytes, no tick consumed - which is why a stream
    /// can raise a flag and keep running in the same frame.
    pub fn step_op_system_flag(&mut self, body: &[u8], set: bool) {
        let idx = u16::from_le_bytes([body[1], body[2]]);
        self.effects.push(if set {
            AmbientEffect::SystemFlagSet(idx)
        } else {
            AmbientEffect::SystemFlagClear(idx)
        });
        self.pc = self.pc.wrapping_add(3);
    }

    /// PORT: FUN_80038158 (`0x8003915C`, op `0x09`)
    /// REF: FUN_80035B50
    ///
    /// `[09, lo, hi]` - `FUN_80035B50((s16)(lo | hi << 8))`, the SFX-cue
    /// enqueue. The callee stores the cue id into the four-slot ring
    /// `DAT_8007B6D8` at the write cursor `gp+0x158`, latches that index at
    /// `gp+0x15A`, zeroes the slot's companion delay word
    /// `DAT_8007C338 + i*4`, and advances the cursor, wrapping at 4 - the
    /// same kernel the field VM's op `0x36` sub-`0` runs, so a following
    /// delay write lands on the slot this op parked. Three bytes, no tick
    /// consumed, so a scripted beat can fire a cue and keep choreographing
    /// in the same frame.
    pub fn step_op_sfx_cue(&mut self, body: &[u8]) {
        let v = i16::from_le_bytes([body[1], body[2]]);
        self.event_ring[self.event_write & 3] = v;
        self.event_write = (self.event_write + 1) & 3;
        self.effects.push(AmbientEffect::SfxCue(v));
        self.pc = self.pc.wrapping_add(3);
    }

    // -----------------------------------------------------------------
    // 0x0C - the tint / draw-mode fade
    // -----------------------------------------------------------------

    /// PORT: FUN_80038158 (`0x8003918C`, op `0x0C`)
    ///
    /// `[0C, r, g, b, m_lo, m_hi, d_lo, d_hi]` - fade the actor's **packed
    /// RGB tint** `actor+0x74` and its companion draw-mode word
    /// `actor+0x78` over a shared duration.
    ///
    /// Both fields are arguments of the mesh submit `FUN_80043390`, which
    /// takes the colour word in `a1` (bytes 0-2 are R/G/B, byte 3 ORs a bit
    /// into the mode) and the mode word in `a2`; the actor spawn init
    /// `FUN_80020E3C` seeds them `0x00808080` (PSX-neutral) and `0`. The
    /// `glide channel` reading this op carried is falsified - nothing here
    /// touches a coordinate.
    ///
    /// The four branch shapes are retail's, at `0x800391BC` / `0x800391F4` /
    /// `0x80039220`:
    ///
    /// | duration | `+0x78` | `mode` operand | behaviour |
    /// |---|---|---|---|
    /// | `0` | any | any | both written outright |
    /// | non-zero | `0` | any | tint written outright, mode ramped `0 -> operand` |
    /// | non-zero | non-zero | `0` | tint untouched, mode ramped `cur -> 0` |
    /// | non-zero | non-zero | non-zero | both ramped |
    ///
    /// which is a fade-**in** that snaps the colour and opens the mode, and
    /// a fade-**out** that closes the mode and leaves the colour standing.
    /// The tint ramp is the scheduler's `kind 3`, the only caller of its
    /// three-lane packed-RGB lerp; the mode ramp is `kind 4`, a `sw` over a
    /// field retail itself reads back with `lhu` - a width mismatch the port
    /// keeps on the 16-bit side. Eight bytes, no tick consumed.
    pub fn step_op_tint_fade(&mut self, body: &[u8]) {
        let rgb = u32::from(body[1]) | (u32::from(body[2]) << 8) | (u32::from(body[3]) << 16);
        let mode = i16::from_le_bytes([body[4], body[5]]);
        let duration = i32::from(i16::from_le_bytes([body[6], body[7]]));
        self.pc = self.pc.wrapping_add(8);

        if duration == 0 {
            self.blend = mode as u16;
            self.tint = rgb;
            return;
        }
        if self.blend == 0 {
            self.tint = rgb;
        } else if mode != 0 {
            let owner = self.owner;
            let start = self.tint as i32;
            self.ramps.install(Ramp {
                dest: RAMP_DEST_TINT,
                owner,
                start,
                end: rgb as i32,
                total: duration,
                remaining: duration,
                kind: RampKind::Rgb,
            });
        }
        let owner = self.owner;
        let start = i32::from(self.blend);
        self.ramps.install(Ramp {
            dest: RAMP_DEST_BLEND,
            owner,
            start,
            end: i32::from(mode),
            total: duration,
            remaining: duration,
            kind: RampKind::U32,
        });
    }

    // -----------------------------------------------------------------
    // 0x0E / 0x0F - model swap and tile teleport
    // -----------------------------------------------------------------

    /// PORT: FUN_80038158 (`0x800393A0`, op `0x0E`)
    /// REF: FUN_80024E08
    ///
    /// `[0E, lo, hi]` - re-bind the actor's mesh. The operand is compared
    /// **unsigned** against `0xF0` (`sltiu`), which splits the id space in
    /// two:
    ///
    /// - below `0xF0`: clear [`ACTOR_FLAG_TRANSLUCENT`] and resolve against
    ///   `*(u16*)0x8007B6F8`, the scene's own model-bank base - the same
    ///   word the placement spawner `FUN_8003A1E4` adds a placement's model
    ///   byte to at `0x8003A2F0`;
    /// - `0xF0` and above (a negative operand included): raise
    ///   [`ACTOR_FLAG_TRANSLUCENT`] and resolve `operand - 0xF0` against the
    ///   second base `*(u16*)0x8007B824`.
    ///
    /// Either way `FUN_80024E08` zeroes the anim cursor `+0x5C`, stores the
    /// resolved id at `+0x64` and reloads the mesh. Three bytes, no tick
    /// consumed.
    pub fn step_op_model_swap(&mut self, body: &[u8]) {
        let id = i16::from_le_bytes([body[1], body[2]]);
        let effect = if (id as u32) < 0xF0 {
            self.actor_flags &= !ACTOR_FLAG_TRANSLUCENT;
            AmbientEffect::ModelSwap {
                bank: ModelBank::Scene,
                offset: id,
            }
        } else {
            self.actor_flags |= ACTOR_FLAG_TRANSLUCENT;
            AmbientEffect::ModelSwap {
                bank: ModelBank::Special,
                offset: (i32::from(id) - 0xF0) as i16,
            }
        };
        self.effects.push(effect);
        self.pc = self.pc.wrapping_add(3);
    }

    /// PORT: FUN_80038158 (`0x8003944C`, op `0x0F`)
    /// REF: FUN_80019278
    ///
    /// `[0F, b1, b2]` - teleport to a tile centre, in exactly the grid
    /// decode the field VM's `MoveTo` uses: `(b & 0x7F) * 0x80 + 0x40`, plus
    /// a further `0x40` when the byte's high bit is set (the half-tile
    /// offset). The tile numbers are **also** written to the actor's anchor
    /// pair `+0x8C` / `+0x8D`, which is what makes a following `0x06` step
    /// measure its box from where the teleport landed.
    ///
    /// The footing is then re-sampled through the bilinear ground sampler
    /// `FUN_80019278` into `+0x16` - unless the actor carries
    /// [`ACTOR_FLAG_Y_OVERRIDE`], in which case retail leaves the height to
    /// whoever owns it. Three bytes, no tick consumed.
    pub fn step_op_tile_teleport(&mut self, body: &[u8]) {
        let (b1, b2) = (body[1], body[2]);
        let (tx, tz) = (b1 & 0x7F, b2 & 0x7F);
        self.home_tile = (tx, tz);
        let mut x = (i32::from(tx) << 7) + 0x40;
        let mut z = (i32::from(tz) << 7) + 0x40;
        if b1 & 0x80 != 0 {
            x += 0x40;
        }
        if b2 & 0x80 != 0 {
            z += 0x40;
        }
        self.x = x as i16;
        self.z = z as i16;
        self.moved = true;
        self.effects.push(AmbientEffect::Teleport {
            x: self.x,
            z: self.z,
            sample_ground: self.actor_flags & ACTOR_FLAG_Y_OVERRIDE == 0,
        });
        self.pc = self.pc.wrapping_add(3);
    }

    // -----------------------------------------------------------------
    // 0x10 / 0x11 / 0x12 - the bit ops
    // -----------------------------------------------------------------

    /// Read the halfword a `0x10` / `0x11` / `0x12` selector addresses.
    /// [`BitTarget::Fault`] reads as zero (retail dereferences null).
    pub fn read_bit_word(&self, target: BitTarget) -> u16 {
        match target {
            BitTarget::ActorFlagsLo => self.actor_flags as u16,
            BitTarget::ActorFlagsHi => (self.actor_flags >> 16) as u16,
            BitTarget::ClipControl => self.clip_control,
            BitTarget::GlobalsLo => self.globals as u16,
            BitTarget::GlobalsHi => (self.globals >> 16) as u16,
            BitTarget::Fault => 0,
        }
    }

    fn write_bit_word(&mut self, target: BitTarget, value: u16) {
        match target {
            BitTarget::ActorFlagsLo => {
                self.actor_flags = (self.actor_flags & 0xFFFF_0000) | u32::from(value);
            }
            BitTarget::ActorFlagsHi => {
                self.actor_flags = (self.actor_flags & 0x0000_FFFF) | (u32::from(value) << 16);
            }
            BitTarget::ClipControl => self.clip_control = value,
            BitTarget::GlobalsLo => {
                self.globals = (self.globals & 0xFFFF_0000) | u32::from(value);
            }
            BitTarget::GlobalsHi => {
                self.globals = (self.globals & 0x0000_FFFF) | (u32::from(value) << 16);
            }
            BitTarget::Fault => {}
        }
    }

    /// PORT: FUN_80038158 (`0x80039500` op `0x10`, `0x80039590` op `0x11`)
    ///
    /// `[10|11, b1]` - set or clear bit `b1 & 0x0F` of the halfword
    /// [`BitTarget::from_selector`] picks. Two bytes, no tick consumed.
    pub fn step_op_bit_write(&mut self, body: &[u8], set: bool) {
        let b1 = body[1];
        let target = BitTarget::from_selector(b1);
        self.pc = self.pc.wrapping_add(2);
        if target == BitTarget::Fault {
            self.effects.push(AmbientEffect::BitTargetFault);
            return;
        }
        let bit = 1u16 << (b1 & 0x0F);
        let w = self.read_bit_word(target);
        self.write_bit_word(target, if set { w | bit } else { w & !bit });
    }

    /// PORT: FUN_80038158 (`0x80039624`, op `0x12`)
    ///
    /// `[12, b1]` - **wait until the selected bit changes**, which is not
    /// the same thing as waiting for it to be set. On the op's first tick
    /// the cursor is seeded from the bit's *current* value - `1` when it is
    /// already set, `2` when it is clear (`0x800396B4`) - and the op then
    /// waits for the opposite state. So the same authored byte means "wait
    /// for release" or "wait for signal" depending on what the flag holds
    /// when the stream reaches it.
    ///
    /// The arm consumes the tick **unconditionally** - `addiu s8, s8, 1`
    /// sits in the branch delay slot at `0x80039634`, before any of the
    /// selector or cursor work - so even the frame the wait retires costs
    /// one tick and the following op does not run until the next.
    pub fn step_op_bit_wait(&mut self, body: &[u8]) {
        let b1 = body[1];
        let target = BitTarget::from_selector(b1);
        if target == BitTarget::Fault {
            self.effects.push(AmbientEffect::BitTargetFault);
        }
        let shift = u32::from(b1 & 0x0F);
        let live = |me: &Self| (me.read_bit_word(target) >> shift) & 1 != 0;

        if self.cursor & 0xFF == 0 {
            let seeded = if live(self) { 1u8 } else { 2 };
            self.cursor = (self.cursor & 0xFF00) | u16::from(seeded);
        }
        if self.cursor & 0xFF == 1 && !live(self) {
            self.cursor &= 0xFF00;
            self.pc = self.pc.wrapping_add(2);
        }
        if self.cursor & 0xFF == 2 && live(self) {
            self.cursor &= 0xFF00;
            self.pc = self.pc.wrapping_add(2);
        }
    }

    // -----------------------------------------------------------------
    // 0x13 - the VRAM blit
    // -----------------------------------------------------------------

    /// PORT: FUN_80038158 (`0x8003973C`, op `0x13`)
    /// REF: FUN_80058490
    ///
    /// Live like every other arm - the interpreter reaches it on both hosts -
    /// but its *effect* has no consumer: the only surface a `MoveImage` could
    /// land on is `engine-render`'s software PSX VRAM, and `World` has no path
    /// from a field-actor tick into it. The op runs, decodes its operands and
    /// queues [`AmbientEffect::MoveImage`]; nothing drains that variant. That
    /// is a consumer gap, not a wiring gap, so this carries no inert marker.
    ///
    /// `[13, x_lo, x_hi, y_lo, y_hi, w_lo, w_hi, h_lo, h_hi, dx_lo, dx_hi,
    /// dy_lo, dy_hi]` - thirteen bytes, assembled on the stack as a libgpu
    /// `RECT` plus a destination corner and handed to `FUN_80058490`
    /// (`MoveImage`: `LoadImage`-family DMA that copies one VRAM rectangle
    /// to another). The rectangle halfwords are read unsigned; the two
    /// destination words are sign-extended. Retail returns `-1` without
    /// blitting for a zero width or height. No tick consumed.
    pub fn step_op_move_image(&mut self, body: &[u8]) {
        let h = |i: usize| u16::from_le_bytes([body[i], body[i + 1]]);
        self.effects.push(AmbientEffect::MoveImage {
            rect: [h(1), h(3), h(5), h(7)],
            dx: h(9) as i16,
            dy: h(11) as i16,
        });
        self.pc = self.pc.wrapping_add(13);
    }

    // -----------------------------------------------------------------
    // 0x14 / 0x15 / 0x16 - the three scalar tweens
    // -----------------------------------------------------------------

    /// PORT: FUN_80038158 (`0x800397EC` op `0x14`, `0x800398F0` op `0x15`,
    /// `0x800399F4` op `0x16`)
    ///
    /// `[14|15|16, v_lo, v_hi, d_lo, d_hi]` - three verbatim copies of one
    /// arm that differ only in which actor halfword they drive:
    ///
    /// | op | field | what it is |
    /// |---|---|---|
    /// | `0x14` | `actor+0x72` | uniform render scale, `0x1000` = 1.0 |
    /// | `0x15` | `actor+0x24` | the X Euler angle (pitch) |
    /// | `0x16` | `actor+0x28` | the Z Euler angle (roll) |
    ///
    /// A zero duration stores the value outright; otherwise a `kind 2`
    /// scheduler slot lerps the field from its live value to the operand
    /// over that many frame-scalar units. Five bytes, no tick consumed, so
    /// a stream can stack all three tweens in one frame.
    ///
    /// `+0x72` is the field the animated renderer `FUN_8001B964` skips the
    /// draw on when it is zero (`0x8001B9A0`) and feeds to `ScaleMatrix`
    /// when it is anything but `0x1000` (`0x8001BA6C..0x8001BAA4`); the
    /// field locomotion controller reads the *same* halfword as its
    /// per-actor speed multiplier, which is why the two readings of `+0x72`
    /// in this repo's docs are both right about one field.
    pub fn step_op_scalar_tween(&mut self, body: &[u8], dest: u32) {
        let end = i16::from_le_bytes([body[1], body[2]]);
        let duration = i32::from(i16::from_le_bytes([body[3], body[4]]));
        self.pc = self.pc.wrapping_add(5);
        if duration == 0 {
            self.store_tween_dest(dest, i32::from(end));
            return;
        }
        let owner = self.owner;
        let start = self.load_tween_dest(dest);
        self.ramps.install(Ramp {
            dest,
            owner,
            start,
            end: i32::from(end),
            total: duration,
            remaining: duration,
            kind: RampKind::U16,
        });
    }

    /// Install-time read of a `0x14` / `0x15` / `0x16` destination. `0x14`
    /// reads its field with `lhu` and the other two with `lh`, which is the
    /// only difference between the three arms' prologues.
    fn load_tween_dest(&self, dest: u32) -> i32 {
        match dest {
            RAMP_DEST_SCALE => i32::from(self.scale.unwrap_or(DEFAULT_SCALE)),
            RAMP_DEST_PITCH => i32::from(self.pitch),
            RAMP_DEST_ROLL => i32::from(self.roll),
            _ => 0,
        }
    }

    fn store_tween_dest(&mut self, dest: u32, value: i32) {
        match dest {
            RAMP_DEST_SCALE => self.scale = Some(value as u16),
            RAMP_DEST_PITCH => self.pitch = value as i16,
            RAMP_DEST_ROLL => self.roll = value as i16,
            _ => {}
        }
    }
}

/// The render scale an actor spawns with - `FUN_80020E3C` writes
/// `+0x72 = 0x1000` (1.0 in the renderer's 12-bit fixed point). The port's
/// channel carries `None` until an op writes the field, so a host can tell
/// "the stream set it" from "nobody has".
pub const DEFAULT_SCALE: u16 = 0x1000;
