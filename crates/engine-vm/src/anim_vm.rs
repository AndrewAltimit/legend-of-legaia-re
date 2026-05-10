//! Per-actor animation runtime - wraps the actor-tick anim dispatch.
//!
//! ## Background
//!
//! `FUN_80024CFC` in `SCUS_942.54` is the only static-binary entry point
//! that touches an animation record. It stows the per-record byte pointer
//! in `actor[+0x4C]`, sets `actor[+0x56] = 0xB` and `actor[+0x68] = 100`,
//! and returns. The thing that actually consumes those fields is the
//! per-actor tick at `FUN_80021DF4` (also in `SCUS_942.54`, 4732 bytes,
//! 1183 instructions). The tick reads `actor[+0x5A]` as the dispatch
//! byte and ladders through opcodes `0x01..=0x07` (see
//! [`DispatchByte`]).
//!
//! The dispatch byte selects a layered set of side-effects:
//!
//! - The **keyframe pose decoder** for opcode `0x06` is ported in
//!   [`legaia_anm::AnimPlayer`] - that's the per-bone interpolation that
//!   writes the renderer-consumed pose buffer at `actor[+0x4C]`.
//! - The **per-actor physics tick** that wraps the keyframe decoder is
//!   ported in [`crate::actor_tick`]. It models the position / velocity /
//!   acceleration math for every dispatch byte (`0x01..=0x07`), the
//!   positional SFX emitter (`0x05`), and the per-arm render submissions
//!   (line draws for `0x04`, scene-graph triangle for `0x07`). Audio
//!   cues and render submissions surface via
//!   [`actor_tick::TickEvent`](crate::actor_tick::TickEvent).
//!
//! For the bulk of retail ANM data (records the runtime calls "opcode 6")
//! the per-record body is a per-bone keyframe table, and the interpolation
//! math is statically reachable in `FUN_80021DF4`. That algorithm is
//! already ported in [`legaia_anm::AnimPlayer`].
//!
//! For everything else - records with header `a` field other than `0x06`
//! or `0x0A` - the per-record body shape is opaque. The scaffold below
//! lets engines wire the runtime they need *now*: the dispatcher's `Host`
//! trait exposes a single hook (`on_opaque_record`) for record-level
//! side-effects (sprite swaps, voice cues), and the keyframe-driven case
//! is fully handled by delegating to `AnimPlayer`. Per-actor physics -
//! the part that's the same for every record kind - is in
//! [`crate::actor_tick`].
//!
//! ## What this scaffold provides
//!
//! - A typed `RecordKind` derived from the header `a` field, populated
//!   from real-data observations in `crates/anm`.
//! - `AnimSlot`: per-actor playback state (record bytes, bone count,
//!   factor, finished flag).
//! - `AnimRuntime`: a fixed-size pool of `AnimSlot`s indexed by actor id,
//!   with `play(actor, record, bone_count, kind)` / `tick(actor)` /
//!   `stop(actor)` / `reset(actor)` operations.
//! - `Host`: callbacks the runtime makes when it sees a record kind it
//!   can't handle on its own (the eventual overlay-resident dispatcher).
//! - `AnimEvent`: stream surface so engines can react without polling
//!   per-actor state.
//!
//! When the overlay capture lands, the only change required is to fill
//! the `Host::on_opaque_record` body with the real per-kind dispatch -
//! every other piece of plumbing (per-actor pool, frame stepping,
//! lifecycle hooks, event stream) stays as-is.

use legaia_anm::{AnimPlayer, PoseFrame, RecordHeader};

/// Maximum actor pool size - matches the retail per-scene actor count
/// observed in the field overlay (`actor[+0x*]` table at
/// `0x801E473C`, 16-byte stride, ≤ 32 entries used).
pub const MAX_ACTOR_SLOTS: usize = 32;

/// Field offset of the per-record byte pointer on a retail actor record
/// (`actor[+0x4C]`). `FUN_80024CFC` in `SCUS_942.54` writes this when a
/// new animation is registered; the per-frame tick reads it.
pub const ACTOR_RECORD_PTR_OFFSET: usize = 0x4C;

/// Field offset of the dispatch byte on a retail actor record
/// (`actor[+0x5A]`, read as a `u16` by `FUN_80021DF4`).
pub const ACTOR_DISPATCH_BYTE_OFFSET: usize = 0x5A;

/// Field offset of the per-actor frame counter (`actor[+0x68]`,
/// initialised to 100 by `FUN_80024CFC`).
pub const ACTOR_FRAME_COUNTER_OFFSET: usize = 0x68;

/// Per-actor anim-driver dispatch byte (`actor[+0x5A]`). The actor tick
/// at `FUN_80021DF4` ladders through these. Only [`Keyframe`] is fully
/// understood and implemented in this crate; the rest are opaque (the
/// per-handler bodies in `FUN_80021DF4` are ~1100 instructions each
/// and have not yet been ported).
///
/// Values are observed from a static read of the SCUS dispatcher (see
/// the comparison ladder at `0x80021E78..0x80022F04` in
/// `ghidra/scripts/funcs/80021df4.txt`).
///
/// [`Keyframe`]: DispatchByte::Keyframe
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchByte {
    /// `0x01` - pose-snap branch. Specific behaviour TBD.
    Snap,
    /// `0x02` - shares the keyframe interpolation block with `0x06`
    /// in `FUN_80021DF4`.
    KeyframeAlt,
    /// `0x03` - small handler at `0x800226DC`. Shares state-write logic
    /// with `0x05`.
    Path,
    /// `0x04` - handler at `0x80022CBC..0x80022EE4`.
    Damp,
    /// `0x05` - handler at `0x800228B0..0x80022B80`. Reads geometry
    /// from `actor[+0x80]` and writes pose state.
    PathAlt,
    /// `0x06` - keyframe interpolation. Per-bone math at
    /// `0x80021EA0..0x80021FA4` and the long block continuing through
    /// `0x80022FXX`. Fully ported in [`legaia_anm::AnimPlayer`].
    Keyframe,
    /// `0x07` - handler at `0x80022C24..0x80022CC0`.
    Spline,
}

impl DispatchByte {
    /// Map an `actor[+0x5A]` value to a typed dispatch byte. Returns
    /// `None` for values outside the observed `0x01..=0x07` range.
    pub fn from_byte(b: u16) -> Option<Self> {
        Some(match b {
            0x01 => DispatchByte::Snap,
            0x02 => DispatchByte::KeyframeAlt,
            0x03 => DispatchByte::Path,
            0x04 => DispatchByte::Damp,
            0x05 => DispatchByte::PathAlt,
            0x06 => DispatchByte::Keyframe,
            0x07 => DispatchByte::Spline,
            _ => return None,
        })
    }

    /// Round-trip back to the raw byte the tick compares against.
    pub fn as_byte(self) -> u16 {
        match self {
            DispatchByte::Snap => 0x01,
            DispatchByte::KeyframeAlt => 0x02,
            DispatchByte::Path => 0x03,
            DispatchByte::Damp => 0x04,
            DispatchByte::PathAlt => 0x05,
            DispatchByte::Keyframe => 0x06,
            DispatchByte::Spline => 0x07,
        }
    }

    /// `true` when this runtime can drive the dispatch natively
    /// (currently only [`Keyframe`]).
    ///
    /// The other six dispatch bytes drive per-actor physics state - see
    /// [`crate::actor_tick`] for the dispatch-byte-aware physics tick.
    /// `handled_natively` only reports whether [`AnimPlayer`] (the keyframe
    /// pose decoder) can drive this dispatch byte; it does **not** mean the
    /// physics arms aren't ported.
    ///
    /// [`Keyframe`]: DispatchByte::Keyframe
    /// [`AnimPlayer`]: legaia_anm::AnimPlayer
    pub fn handled_natively(self) -> bool {
        matches!(self, DispatchByte::Keyframe)
    }

    /// Human-readable name suitable for logs and disassembly traces.
    pub fn name(self) -> &'static str {
        match self {
            DispatchByte::Snap => "Snap",
            DispatchByte::KeyframeAlt => "KeyframeAlt",
            DispatchByte::Path => "Path",
            DispatchByte::Damp => "Damp",
            DispatchByte::PathAlt => "PathAlt",
            DispatchByte::Keyframe => "Keyframe",
            DispatchByte::Spline => "Spline",
        }
    }
}

/// Coarse classification of the per-record body, derived from the
/// header `a` field (first u16 of the 8-byte common header).
///
/// Values are observed across the title-screen + town-overlay corpus.
/// The `Keyframe` variant is the only one whose body layout is fully
/// understood (see `KeyframeReader` in `legaia_anm`); all other variants
/// are opaque until the overlay dispatcher is captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    /// `header.a == 0x06` - per-bone keyframe table. Handled by
    /// `AnimPlayer`. Dominant in retail field/town animations.
    Keyframe,
    /// `header.a == 0x0A` - observed on a small subset of records. The
    /// body shape is unknown; treated as opaque for now.
    KindA,
    /// `header.a == 0x02` - observed on title-screen records.
    Kind2,
    /// `header.a == 0x03` - observed on title-screen records.
    Kind3,
    /// Any other `header.a` value. Carries the raw byte so the host can
    /// route by it.
    Other(u16),
}

impl RecordKind {
    /// Classify a record by reading its 8-byte header.
    ///
    /// Returns `None` if the buffer is too small for a header.
    pub fn from_record(record: &[u8]) -> Option<Self> {
        let h = RecordHeader::from_bytes(record).ok()?;
        Some(match h.a {
            0x06 => RecordKind::Keyframe,
            0x0A => RecordKind::KindA,
            0x02 => RecordKind::Kind2,
            0x03 => RecordKind::Kind3,
            other => RecordKind::Other(other),
        })
    }

    /// `true` for the only record kind the engine can drive without
    /// the overlay-resident dispatcher (i.e. the keyframe path).
    pub fn handled_natively(self) -> bool {
        matches!(self, RecordKind::Keyframe)
    }
}

/// Per-actor animation state.
///
/// `Idle` slots hold no record. `Playing` slots own the record bytes,
/// the bone count, and either an `AnimPlayer` (for `RecordKind::Keyframe`)
/// or a frame counter for the opaque kinds. The runtime advances the
/// counter every `tick`; the host fills in the per-kind side-effects.
#[derive(Debug, Clone, Default)]
pub enum AnimSlot {
    /// No animation is registered on this actor.
    #[default]
    Idle,
    /// A keyframe-style animation is playing - handled natively.
    Keyframe {
        player: AnimPlayer,
        kind: RecordKind,
    },
    /// An opaque-kind record is registered; the runtime advances a
    /// frame counter and surfaces it to the host on each tick. The host
    /// owns interpretation.
    Opaque {
        record: Vec<u8>,
        bone_count: usize,
        kind: RecordKind,
        /// Per-frame counter (mirrors retail `actor[+0x68]`, which
        /// `FUN_80024CFC` initialises to 100).
        frame_counter: u32,
    },
}

impl AnimSlot {
    /// Helper: explicit constructor for the idle variant.
    pub fn idle() -> Self {
        AnimSlot::Idle
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, AnimSlot::Idle)
    }

    /// Header `a` byte for the active record, or `None` if idle. Useful
    /// when routing by record kind without re-parsing.
    pub fn header_a(&self) -> Option<u16> {
        match self {
            AnimSlot::Idle => None,
            AnimSlot::Keyframe { .. } => Some(0x06),
            AnimSlot::Opaque { kind, .. } => match kind {
                RecordKind::Kind2 => Some(0x02),
                RecordKind::Kind3 => Some(0x03),
                RecordKind::KindA => Some(0x0A),
                RecordKind::Keyframe => Some(0x06),
                RecordKind::Other(v) => Some(*v),
            },
        }
    }
}

/// Frame events surfaced by the runtime. Engines drain these to
/// drive renderer / SFX side-effects without inspecting actor state.
#[derive(Debug, Clone, PartialEq)]
pub enum AnimEvent {
    /// A keyframe pose was produced for this actor on this frame.
    /// `pose.finished` mirrors `PoseFrame::finished` (true after the
    /// non-looping cycle has clamped at 0xFF).
    PoseUpdated { actor: u8, pose: PoseFrame },
    /// An opaque-kind record advanced one frame; the host's
    /// `on_opaque_record` hook saw it. The runtime only reports that
    /// the tick happened - the host is responsible for any side
    /// effect (sprite frame swap, voice cue, etc.).
    OpaqueTick {
        actor: u8,
        kind: RecordKind,
        frame: u32,
    },
    /// A non-looping animation has finished. Engines typically clear
    /// the slot or transition to an idle pose.
    Finished { actor: u8, kind: RecordKind },
    /// `play` was called on a slot that was already busy. Host can
    /// treat this as either a forced replace or a no-op.
    Replaced { actor: u8 },
}

/// Engine-side callbacks the runtime makes for opaque-kind records.
///
/// The eventual overlay-resident dispatcher will replace
/// `on_opaque_record` with the real per-kind interpretation. Until
/// then, engines override this trait to wire whatever fallback they
/// need (e.g. swap to a static pose, defer to a static animation
/// asset, log for analysis).
pub trait Host {
    /// Called once per `AnimRuntime::tick` for every Opaque-kind slot.
    ///
    /// `frame_counter` mirrors `actor[+0x68]` and increments each tick
    /// while the slot is alive. The default body just records the
    /// event; engines that have a fallback override this.
    ///
    /// Returning `true` keeps the slot alive; `false` ends it
    /// (transitions to `Idle` on the next tick boundary).
    fn on_opaque_record(
        &mut self,
        actor: u8,
        kind: RecordKind,
        record: &[u8],
        frame_counter: u32,
    ) -> bool {
        let _ = (actor, kind, record, frame_counter);
        true
    }
}

/// Default host that does nothing on opaque records (slots stay alive
/// indefinitely until `stop` is called). Useful for tests and engines
/// that don't yet wire the overlay dispatcher.
#[derive(Debug, Default)]
pub struct NullHost;

impl Host for NullHost {}

/// Per-record consumer-struct kind byte at offset `+0x00` of the
/// runtime ANM record pointed at by `actor[+0x234]`.
///
/// Per the SCUS battle-anim consumer ladder in `FUN_8004AD80`
/// (`ghidra/scripts/funcs/8004ad80.txt` lines ~1680..1706), the kind
/// byte drives a per-frame side-effect ladder. Five kinds have
/// explicit branches; everything else is "raw playback" (the surrounding
/// per-field reads still happen, but no kind-specific sub-state mutates).
///
/// Cross-references to `docs/formats/anm.md`'s kind table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpaqueRecordKind {
    /// `0x02` - handshake state. When the global character action byte
    /// is `-0x4D`/`-0x4B` and the actor's anim flag at `+0x14C` is zero,
    /// the consumer flips this kind to `Kind4` and ticks the per-record
    /// counter at `+0x56`.
    Kind2,
    /// `0x04` - action engaged. If actor's anim-flag is zero, set
    /// `actor[+0x1DA] = 7`; else copy `actor[+0x1F2]` into `+0x1DA`.
    Kind4,
    /// `0x05` - OR `actor[+0x1DC] |= 4` (set bit 2 of per-actor flag byte).
    Kind5,
    /// `0x07` - set `actor[+0x1DA] = 8`.
    Kind7,
    /// `0x08` - OR `actor[+0x1DC] |= 8`.
    Kind8,
    /// Any other byte. Carries the raw value so callers can route on it.
    Other(u8),
}

impl OpaqueRecordKind {
    /// Classify the kind byte (offset `+0x00` of the consumer struct).
    pub fn from_byte(b: u8) -> Self {
        match b {
            0x02 => OpaqueRecordKind::Kind2,
            0x04 => OpaqueRecordKind::Kind4,
            0x05 => OpaqueRecordKind::Kind5,
            0x07 => OpaqueRecordKind::Kind7,
            0x08 => OpaqueRecordKind::Kind8,
            other => OpaqueRecordKind::Other(other),
        }
    }

    /// Round-trip back to the raw byte the consumer compares against.
    pub fn as_byte(self) -> u8 {
        match self {
            OpaqueRecordKind::Kind2 => 0x02,
            OpaqueRecordKind::Kind4 => 0x04,
            OpaqueRecordKind::Kind5 => 0x05,
            OpaqueRecordKind::Kind7 => 0x07,
            OpaqueRecordKind::Kind8 => 0x08,
            OpaqueRecordKind::Other(v) => v,
        }
    }

    /// `true` for kinds with explicit per-frame side-effect logic in
    /// `FUN_8004AD80`. `Other` returns `false` (raw playback).
    pub fn has_side_effect(self) -> bool {
        !matches!(self, OpaqueRecordKind::Other(_))
    }
}

/// Typed view onto the runtime per-record consumer struct that
/// `actor[+0x234]` points at. Wraps a borrowed buffer and exposes the
/// fields the SCUS battle-anim consumers (`FUN_80047430`,
/// `FUN_80049348`, `FUN_8004AD80`, etc.) read.
///
/// All offsets are relative to the start of the buffer. Out-of-range
/// reads return `None` so that engines walking partially-loaded records
/// behave predictably.
#[derive(Debug, Clone, Copy)]
pub struct OpaqueAnimRecord<'a> {
    bytes: &'a [u8],
}

impl<'a> OpaqueAnimRecord<'a> {
    /// Wrap a byte slice. The slice must point at the runtime struct's
    /// first byte (the kind byte). The minimum useful length is
    /// `+0x88 + 4` (covers all the listed offsets); shorter slices are
    /// permitted but field accessors will return `None` for out-of-range
    /// reads.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// `+0x00` kind byte.
    pub fn kind(&self) -> Option<OpaqueRecordKind> {
        Some(OpaqueRecordKind::from_byte(*self.bytes.first()?))
    }

    fn read_u8(&self, off: usize) -> Option<u8> {
        self.bytes.get(off).copied()
    }
    fn read_u16_le(&self, off: usize) -> Option<u16> {
        let bytes = self.bytes.get(off..off + 2)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }
    fn read_i16_le(&self, off: usize) -> Option<i16> {
        Some(self.read_u16_le(off)? as i16)
    }

    /// `+0x0E` movement-scaling factor.
    pub fn movement_scale(&self) -> Option<i16> {
        self.read_i16_le(0x0E)
    }

    /// `+0x21D` per-frame stride byte. The consumer uses
    /// `8 / stride` to pick the items-per-block factor (caps at 8;
    /// `stride == 0` is treated as `1`).
    pub fn frame_stride(&self) -> Option<u8> {
        self.read_u8(0x21D)
    }

    /// `+0x56` sub-state counter; ticks during the `Kind2 -> Kind4`
    /// transition.
    pub fn substate_counter(&self) -> Option<u16> {
        self.read_u16_le(0x56)
    }

    /// `+0x76` flag byte.
    pub fn flag_76(&self) -> Option<u8> {
        self.read_u8(0x76)
    }

    /// `+0x77` adjustment byte.
    pub fn adjust_77(&self) -> Option<u8> {
        self.read_u8(0x77)
    }

    /// `+0x78` per-frame multiplier.
    pub fn multiplier_78(&self) -> Option<u8> {
        self.read_u8(0x78)
    }

    /// `+0x84` depth / elevation byte. Stored to `actor[+0x21B]` and
    /// shifted left 4 into `actor[+0x176]` by the consumer.
    pub fn depth_84(&self) -> Option<u8> {
        self.read_u8(0x84)
    }

    /// `+0x85` count A.
    pub fn count_85(&self) -> Option<u8> {
        self.read_u8(0x85)
    }

    /// `+0x86` count B.
    pub fn count_86(&self) -> Option<u8> {
        self.read_u8(0x86)
    }

    /// `+0x87` special-effect ID. Non-zero values are passed to
    /// `FUN_8004E13C` by the consumer.
    pub fn effect_id(&self) -> Option<u8> {
        self.read_u8(0x87)
    }

    /// `+0x88` u32 pointer-shaped value to nested per-frame data. The
    /// consumer reads the byte at `*(ptr + 1)` as the per-frame item
    /// count. Engines that re-host the struct should resolve this
    /// pointer in their own address space before consuming the
    /// per-frame data.
    pub fn nested_data_ptr_raw(&self) -> Option<u32> {
        let bytes = self.bytes.get(0x88..0x88 + 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// `+0x172` counter slot.
    pub fn counter_172(&self) -> Option<u16> {
        self.read_u16_le(0x172)
    }

    /// `+0x176` animation-frame counter.
    pub fn anim_frame_176(&self) -> Option<u16> {
        self.read_u16_le(0x176)
    }
}

/// Per-actor animation runtime - fixed-size pool of `AnimSlot`s.
#[derive(Debug)]
pub struct AnimRuntime {
    slots: Vec<AnimSlot>,
    /// Latest pose per actor (mirrors `World::Actor::pose_frame`).
    /// Engines that don't want the event stream can read this directly.
    poses: Vec<Option<PoseFrame>>,
    /// Pending events surfaced from the most recent `tick`. Engines
    /// drain via `take_events`.
    events: Vec<AnimEvent>,
    /// Frame counter - useful for trace logs / fingerprinting.
    pub frame: u64,
}

impl Default for AnimRuntime {
    fn default() -> Self {
        Self::with_slots(MAX_ACTOR_SLOTS)
    }
}

impl AnimRuntime {
    /// Build a runtime with `n` actor slots. The retail per-scene actor
    /// table tops out at `MAX_ACTOR_SLOTS`; engines can pick a smaller
    /// pool to bound memory.
    pub fn with_slots(n: usize) -> Self {
        Self {
            slots: vec![AnimSlot::Idle; n],
            poses: vec![None; n],
            events: Vec::new(),
            frame: 0,
        }
    }

    /// Register `record` on actor `id`. Errors if `id` is out of range.
    /// If the slot was busy, emits `AnimEvent::Replaced` and overwrites
    /// it (matching the retail convention - `FUN_80024CFC` always
    /// overwrites).
    pub fn play(
        &mut self,
        id: u8,
        record: Vec<u8>,
        bone_count: usize,
    ) -> Result<RecordKind, AnimError> {
        let i = id as usize;
        if i >= self.slots.len() {
            return Err(AnimError::ActorOutOfRange { actor: id });
        }
        let kind = RecordKind::from_record(&record).ok_or(AnimError::HeaderTooSmall)?;
        let was_busy = !self.slots[i].is_idle();
        let new_slot = match kind {
            RecordKind::Keyframe => {
                let player = AnimPlayer::new(record, bone_count.max(1))
                    .map_err(|e| AnimError::PlayerInit(e.to_string()))?;
                AnimSlot::Keyframe { player, kind }
            }
            _ => AnimSlot::Opaque {
                record,
                bone_count,
                kind,
                frame_counter: 100, // mirrors actor[+0x68] init
            },
        };
        self.slots[i] = new_slot;
        if was_busy {
            self.events.push(AnimEvent::Replaced { actor: id });
        }
        Ok(kind)
    }

    /// Stop animation on actor `id`. Out-of-range actor is a no-op.
    pub fn stop(&mut self, id: u8) {
        if let Some(s) = self.slots.get_mut(id as usize) {
            *s = AnimSlot::Idle;
        }
        if let Some(p) = self.poses.get_mut(id as usize) {
            *p = None;
        }
    }

    /// Snapshot the current pose for actor `id`, if any.
    pub fn pose(&self, id: u8) -> Option<&PoseFrame> {
        self.poses.get(id as usize).and_then(|p| p.as_ref())
    }

    /// Read the current slot for actor `id`.
    pub fn slot(&self, id: u8) -> Option<&AnimSlot> {
        self.slots.get(id as usize)
    }

    /// `true` if any slot is non-idle.
    pub fn any_active(&self) -> bool {
        self.slots.iter().any(|s| !s.is_idle())
    }

    /// Number of actor slots in this runtime.
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Drain any pending events from the most recent `tick`.
    pub fn take_events(&mut self) -> Vec<AnimEvent> {
        std::mem::take(&mut self.events)
    }

    /// Advance one frame. Walks every slot and dispatches per-kind:
    ///
    /// - `Keyframe` slots: tick the embedded `AnimPlayer`, write the
    ///   resulting pose into `poses[i]`, emit `PoseUpdated`. If the
    ///   player reports `finished`, also emit `Finished` and clear
    ///   the slot.
    /// - `Opaque` slots: bump the frame counter, ask the host whether
    ///   to keep going via `on_opaque_record`, emit `OpaqueTick`.
    ///   If the host returns `false`, emit `Finished` and clear.
    /// - `Idle` slots: skipped.
    pub fn tick<H: Host>(&mut self, host: &mut H) {
        self.frame = self.frame.saturating_add(1);
        for i in 0..self.slots.len() {
            let actor = i as u8;
            // Take ownership briefly so we can mutate the slot while
            // also borrowing the host.
            let mut slot = std::mem::replace(&mut self.slots[i], AnimSlot::Idle);
            match &mut slot {
                AnimSlot::Idle => {
                    // Already idle - restore (still idle).
                    self.slots[i] = AnimSlot::Idle;
                    continue;
                }
                AnimSlot::Keyframe { player, kind } => {
                    let pose = player.tick();
                    let finished = pose.finished;
                    self.poses[i] = Some(pose.clone());
                    self.events.push(AnimEvent::PoseUpdated {
                        actor,
                        pose: pose.clone(),
                    });
                    if finished {
                        self.events.push(AnimEvent::Finished { actor, kind: *kind });
                        self.slots[i] = AnimSlot::Idle;
                    } else {
                        self.slots[i] = slot;
                    }
                }
                AnimSlot::Opaque {
                    record,
                    bone_count: _,
                    kind,
                    frame_counter,
                } => {
                    *frame_counter = frame_counter.saturating_add(1);
                    let alive = host.on_opaque_record(actor, *kind, record, *frame_counter);
                    self.events.push(AnimEvent::OpaqueTick {
                        actor,
                        kind: *kind,
                        frame: *frame_counter,
                    });
                    if !alive {
                        self.events.push(AnimEvent::Finished { actor, kind: *kind });
                        self.slots[i] = AnimSlot::Idle;
                    } else {
                        self.slots[i] = slot;
                    }
                }
            }
        }
    }
}

/// Errors the runtime can return from `play`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnimError {
    ActorOutOfRange { actor: u8 },
    HeaderTooSmall,
    PlayerInit(String),
}

impl std::fmt::Display for AnimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnimError::ActorOutOfRange { actor } => {
                write!(f, "actor {actor} is out of range for this AnimRuntime")
            }
            AnimError::HeaderTooSmall => write!(f, "record buffer too small for an 8-byte header"),
            AnimError::PlayerInit(msg) => write!(f, "AnimPlayer init failed: {msg}"),
        }
    }
}

impl std::error::Error for AnimError {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic keyframe-style record: 8-byte header (a=0x06),
    /// `bone_count` 8-byte output slots, `bone_count` 24-byte keyframe
    /// entries.
    fn synth_keyframe_record(bone_count: usize) -> Vec<u8> {
        let mut buf = vec![0u8; legaia_anm::RECORD_HEADER_SIZE + 8 * bone_count + 24 * bone_count];
        // header.a = 0x06 (Keyframe)
        buf[0..2].copy_from_slice(&0x0006u16.to_le_bytes());
        // header.b = 0x14 (frame count)
        buf[2..4].copy_from_slice(&0x0014u16.to_le_bytes());
        // marker_1 = 0x080C (canonical)
        buf[4..6].copy_from_slice(&0x080Cu16.to_le_bytes());
        // flag = 0x0002
        buf[6..8].copy_from_slice(&0x0002u16.to_le_bytes());
        // Set first bone keyframe so interpolation has something non-trivial
        let kf_off = legaia_anm::RECORD_HEADER_SIZE + 8 * bone_count;
        buf[kf_off..kf_off + 2].copy_from_slice(&10i16.to_le_bytes());
        buf[kf_off + 6..kf_off + 8].copy_from_slice(&100i16.to_le_bytes());
        buf
    }

    /// Build a synthetic opaque record with header `a` set to `kind_byte`.
    fn synth_opaque_record(kind_byte: u16, body_len: usize) -> Vec<u8> {
        let mut buf = vec![0u8; legaia_anm::RECORD_HEADER_SIZE + body_len];
        buf[0..2].copy_from_slice(&kind_byte.to_le_bytes());
        buf[4..6].copy_from_slice(&0x080Cu16.to_le_bytes());
        buf[6..8].copy_from_slice(&0x0002u16.to_le_bytes());
        buf
    }

    #[test]
    fn dispatch_byte_round_trips_full_observed_range() {
        for b in 1u16..=7 {
            let d = DispatchByte::from_byte(b).expect("0x01..=0x07 must round-trip");
            assert_eq!(d.as_byte(), b);
        }
    }

    #[test]
    fn dispatch_byte_rejects_out_of_range() {
        assert!(DispatchByte::from_byte(0x00).is_none());
        assert!(DispatchByte::from_byte(0x08).is_none());
        assert!(DispatchByte::from_byte(0xFF).is_none());
    }

    #[test]
    fn dispatch_byte_handled_natively_only_for_keyframe() {
        assert!(DispatchByte::Keyframe.handled_natively());
        assert!(!DispatchByte::Snap.handled_natively());
        assert!(!DispatchByte::KeyframeAlt.handled_natively());
        assert!(!DispatchByte::Path.handled_natively());
        assert!(!DispatchByte::Damp.handled_natively());
        assert!(!DispatchByte::PathAlt.handled_natively());
        assert!(!DispatchByte::Spline.handled_natively());
    }

    #[test]
    fn actor_field_offsets_match_documented_layout() {
        // Sanity: these are read by both this crate and the docs;
        // bumping them here without bumping the docs would silently
        // diverge.
        assert_eq!(ACTOR_RECORD_PTR_OFFSET, 0x4C);
        assert_eq!(ACTOR_DISPATCH_BYTE_OFFSET, 0x5A);
        assert_eq!(ACTOR_FRAME_COUNTER_OFFSET, 0x68);
    }

    #[test]
    fn record_kind_from_header_a() {
        let r6 = synth_keyframe_record(2);
        assert_eq!(RecordKind::from_record(&r6), Some(RecordKind::Keyframe));
        let r2 = synth_opaque_record(0x02, 16);
        assert_eq!(RecordKind::from_record(&r2), Some(RecordKind::Kind2));
        let r3 = synth_opaque_record(0x03, 16);
        assert_eq!(RecordKind::from_record(&r3), Some(RecordKind::Kind3));
        let ra = synth_opaque_record(0x0A, 16);
        assert_eq!(RecordKind::from_record(&ra), Some(RecordKind::KindA));
        let other = synth_opaque_record(0x42, 16);
        assert_eq!(
            RecordKind::from_record(&other),
            Some(RecordKind::Other(0x42))
        );
        // Too-small buffer → None.
        assert!(RecordKind::from_record(&[0u8; 4]).is_none());
    }

    #[test]
    fn opaque_record_kind_byte_round_trips_known_kinds() {
        for b in [0x02u8, 0x04, 0x05, 0x07, 0x08] {
            let k = OpaqueRecordKind::from_byte(b);
            assert_eq!(k.as_byte(), b);
            assert!(k.has_side_effect());
        }
        let other = OpaqueRecordKind::from_byte(0x18);
        assert_eq!(other.as_byte(), 0x18);
        assert!(!other.has_side_effect());
    }

    #[test]
    fn opaque_anim_record_reads_documented_offsets() {
        let mut buf = vec![0u8; 0x300];
        buf[0x00] = 0x18; // kind = Other(0x18) (somersault-class)
        buf[0x0E..0x10].copy_from_slice(&((-128i16).to_le_bytes()));
        buf[0x21D] = 4;
        buf[0x56..0x58].copy_from_slice(&7u16.to_le_bytes());
        buf[0x84] = 0x10;
        buf[0x85] = 12;
        buf[0x86] = 24;
        buf[0x87] = 0x42;
        buf[0x88..0x8C].copy_from_slice(&0x80AB_CDEFu32.to_le_bytes());
        buf[0x176..0x178].copy_from_slice(&255u16.to_le_bytes());

        let r = OpaqueAnimRecord::new(&buf);
        assert_eq!(r.kind(), Some(OpaqueRecordKind::Other(0x18)));
        assert_eq!(r.movement_scale(), Some(-128));
        assert_eq!(r.frame_stride(), Some(4));
        assert_eq!(r.substate_counter(), Some(7));
        assert_eq!(r.depth_84(), Some(0x10));
        assert_eq!(r.count_85(), Some(12));
        assert_eq!(r.count_86(), Some(24));
        assert_eq!(r.effect_id(), Some(0x42));
        assert_eq!(r.nested_data_ptr_raw(), Some(0x80AB_CDEF));
        assert_eq!(r.anim_frame_176(), Some(255));
    }

    #[test]
    fn opaque_anim_record_short_buffer_returns_none_for_out_of_range() {
        let buf = vec![0x04u8, 0, 0, 0]; // only 4 bytes
        let r = OpaqueAnimRecord::new(&buf);
        assert_eq!(r.kind(), Some(OpaqueRecordKind::Kind4));
        assert_eq!(r.depth_84(), None);
        assert_eq!(r.nested_data_ptr_raw(), None);
    }

    #[test]
    fn record_kind_handled_natively_only_for_keyframe() {
        assert!(RecordKind::Keyframe.handled_natively());
        assert!(!RecordKind::Kind2.handled_natively());
        assert!(!RecordKind::Kind3.handled_natively());
        assert!(!RecordKind::KindA.handled_natively());
        assert!(!RecordKind::Other(0x42).handled_natively());
    }

    #[test]
    fn play_keyframe_creates_keyframe_slot() {
        let mut rt = AnimRuntime::with_slots(4);
        let kind = rt.play(2, synth_keyframe_record(3), 3).unwrap();
        assert_eq!(kind, RecordKind::Keyframe);
        assert!(matches!(rt.slot(2), Some(AnimSlot::Keyframe { .. })));
        assert!(rt.slot(0).map(|s| s.is_idle()).unwrap_or(false));
    }

    #[test]
    fn play_opaque_creates_opaque_slot_with_init_counter_100() {
        let mut rt = AnimRuntime::with_slots(4);
        let kind = rt.play(1, synth_opaque_record(0x02, 32), 0).unwrap();
        assert_eq!(kind, RecordKind::Kind2);
        match rt.slot(1).unwrap() {
            AnimSlot::Opaque {
                kind,
                frame_counter,
                ..
            } => {
                assert_eq!(*kind, RecordKind::Kind2);
                assert_eq!(*frame_counter, 100);
            }
            other => panic!("expected Opaque slot, got {other:?}"),
        }
    }

    #[test]
    fn play_replacing_busy_slot_emits_replaced_event() {
        let mut rt = AnimRuntime::with_slots(4);
        rt.play(0, synth_keyframe_record(2), 2).unwrap();
        rt.play(0, synth_keyframe_record(2), 2).unwrap();
        let evs = rt.take_events();
        assert!(evs.contains(&AnimEvent::Replaced { actor: 0 }));
    }

    #[test]
    fn play_actor_out_of_range_errors() {
        let mut rt = AnimRuntime::with_slots(4);
        let err = rt
            .play(7, synth_keyframe_record(1), 1)
            .expect_err("should be an error");
        assert_eq!(err, AnimError::ActorOutOfRange { actor: 7 });
    }

    #[test]
    fn play_too_small_buffer_errors() {
        let mut rt = AnimRuntime::with_slots(4);
        let err = rt.play(0, vec![0u8; 4], 1).expect_err("should error");
        assert_eq!(err, AnimError::HeaderTooSmall);
    }

    #[test]
    fn tick_keyframe_emits_pose_updated_and_writes_pose() {
        let mut rt = AnimRuntime::with_slots(4);
        rt.play(0, synth_keyframe_record(1), 1).unwrap();
        let mut host = NullHost;
        rt.tick(&mut host);
        let evs = rt.take_events();
        assert!(matches!(evs[0], AnimEvent::PoseUpdated { actor: 0, .. }));
        assert!(rt.pose(0).is_some());
    }

    #[test]
    fn tick_opaque_calls_host_and_emits_opaque_tick() {
        struct CountingHost {
            calls: Vec<(u8, RecordKind, u32)>,
        }
        impl Host for CountingHost {
            fn on_opaque_record(
                &mut self,
                actor: u8,
                kind: RecordKind,
                _record: &[u8],
                frame_counter: u32,
            ) -> bool {
                self.calls.push((actor, kind, frame_counter));
                true
            }
        }
        let mut rt = AnimRuntime::with_slots(4);
        rt.play(2, synth_opaque_record(0x02, 16), 0).unwrap();
        let mut host = CountingHost { calls: vec![] };
        rt.tick(&mut host);
        assert_eq!(host.calls.len(), 1);
        let (actor, kind, frame) = host.calls[0];
        assert_eq!(actor, 2);
        assert_eq!(kind, RecordKind::Kind2);
        // Initial counter 100 → after one tick it's 101.
        assert_eq!(frame, 101);
        let evs = rt.take_events();
        assert!(matches!(
            evs[0],
            AnimEvent::OpaqueTick {
                actor: 2,
                kind: RecordKind::Kind2,
                frame: 101
            }
        ));
    }

    #[test]
    fn tick_opaque_host_returning_false_clears_slot_and_emits_finished() {
        struct EarlyExitHost;
        impl Host for EarlyExitHost {
            fn on_opaque_record(
                &mut self,
                _actor: u8,
                _kind: RecordKind,
                _record: &[u8],
                _frame: u32,
            ) -> bool {
                false
            }
        }
        let mut rt = AnimRuntime::with_slots(4);
        rt.play(0, synth_opaque_record(0x03, 16), 0).unwrap();
        let mut host = EarlyExitHost;
        rt.tick(&mut host);
        let evs = rt.take_events();
        assert!(evs.iter().any(|e| matches!(
            e,
            AnimEvent::Finished {
                actor: 0,
                kind: RecordKind::Kind3
            }
        )));
        assert!(rt.slot(0).unwrap().is_idle());
    }

    #[test]
    fn tick_keyframe_finished_clears_slot_when_non_looping() {
        let mut rt = AnimRuntime::with_slots(4);
        // Build a keyframe record that finishes quickly.
        rt.play(1, synth_keyframe_record(1), 1).unwrap();
        // Force the embedded player into non-looping mode + max delta.
        if let Some(AnimSlot::Keyframe { player, .. }) = rt.slots.get_mut(1) {
            player.looping = false;
            player.frame_delta = 0xFF;
        } else {
            panic!("expected keyframe slot");
        }
        let mut host = NullHost;
        // 0xFF + 0xFF wraps past 0xFF on the second tick → finished=true.
        rt.tick(&mut host);
        rt.tick(&mut host);
        let evs = rt.take_events();
        assert!(evs.iter().any(|e| matches!(
            e,
            AnimEvent::Finished {
                actor: 1,
                kind: RecordKind::Keyframe
            }
        )));
        assert!(rt.slot(1).unwrap().is_idle());
    }

    #[test]
    fn idle_slots_are_skipped_during_tick() {
        let mut rt = AnimRuntime::with_slots(4);
        // Don't register anything; tick should produce no events.
        let mut host = NullHost;
        rt.tick(&mut host);
        assert!(rt.take_events().is_empty());
        assert!(!rt.any_active());
    }

    #[test]
    fn stop_clears_slot_and_pose() {
        let mut rt = AnimRuntime::with_slots(4);
        rt.play(0, synth_keyframe_record(1), 1).unwrap();
        let mut host = NullHost;
        rt.tick(&mut host);
        assert!(rt.pose(0).is_some());
        rt.stop(0);
        assert!(rt.slot(0).unwrap().is_idle());
        assert!(rt.pose(0).is_none());
    }

    #[test]
    fn frame_counter_advances_each_tick() {
        let mut rt = AnimRuntime::with_slots(2);
        let mut host = NullHost;
        rt.tick(&mut host);
        rt.tick(&mut host);
        rt.tick(&mut host);
        assert_eq!(rt.frame, 3);
    }

    #[test]
    fn header_a_round_trips_through_slot() {
        let mut rt = AnimRuntime::with_slots(2);
        rt.play(0, synth_keyframe_record(1), 1).unwrap();
        rt.play(1, synth_opaque_record(0x0A, 16), 0).unwrap();
        assert_eq!(rt.slot(0).unwrap().header_a(), Some(0x06));
        assert_eq!(rt.slot(1).unwrap().header_a(), Some(0x0A));
    }
}
