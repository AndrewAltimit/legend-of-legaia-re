//! Per-actor animation runtime - wraps the actor-tick anim dispatch.
//!
//! PORT: FUN_80024CFC
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

    /// `+0x0E` movement-scaling factor. Used by `FUN_80047430` as the
    /// per-frame velocity numerator: per-frame translation step is
    /// `(angle_lookup * movement_scale * frame_index) / frame_count` (the
    /// `frame_count` divisor is the byte at `*(+0x88) + 1`).
    pub fn movement_scale(&self) -> Option<i16> {
        self.read_i16_le(0x0E)
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

    /// `+0x84` max-frame byte. The consumer stamps it into
    /// `actor[+0x21B]` and shifts it left by 4 into `actor[+0x176]`
    /// (the per-actor frame-counter cap; the high 12 bits of the actor's
    /// `+0x68` u16 frame counter index a frame in the nested per-frame
    /// data array).
    pub fn depth_84(&self) -> Option<u8> {
        self.read_u8(0x84)
    }

    /// `+0x85` loop-target frame index. When the playback head reaches
    /// `count_86 - 1` and the actor's previous-action sentinel
    /// `actor[+0x21B]` is non-zero, the per-bone interpolator pulls the
    /// "next" frame from this index instead of `frame_index + 1` (the
    /// `FUN_8004998C` line ~1077 path: `pbVar17 = pbVar20 + count_85 *
    /// bones * 9 + 2`).
    pub fn count_85(&self) -> Option<u8> {
        self.read_u8(0x85)
    }

    /// `+0x86` loop-trigger frame index (the frame at which the
    /// loop-target lookup kicks in; the comparison is `frame_index ==
    /// count_86 - 1`).
    pub fn count_86(&self) -> Option<u8> {
        self.read_u8(0x86)
    }

    /// `+0x87` special-effect ID. Non-zero values are passed to
    /// `FUN_8004E13C` by the consumer.
    pub fn effect_id(&self) -> Option<u8> {
        self.read_u8(0x87)
    }

    /// `+0x88` u32 pointer-shaped value to nested per-frame data. Use
    /// [`NestedFrameData::from_bytes`] on the buffer the pointer
    /// references to walk the layout. Engines that re-host the struct
    /// should resolve this pointer in their own address space before
    /// consuming the per-frame data.
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

/// Per-actor LOD step byte at `actor[+0x21D]`.
///
/// Set by the consumer at `0x8004B080..0x8004B090` to `4` (default) or
/// `8`, and to `0` / `2` by other paths. `FUN_80049348` reads it as a
/// child-step divisor: `lod_step = 8 / (actor[+0x21D] | 1)`. The render
/// pass then iterates the actor's child-actor chain at `+0x1FB..` with
/// stride `lod_step`, so a higher value ≡ rendering fewer sub-actors per
/// frame (LOD culling). The value is **on the actor record**, not on
/// the nested ANM consumer struct; an earlier read of the docs that
/// placed it on the consumer struct was a misread of the `s1` register
/// at `0x8004ad80`.
///
/// Use [`ActorAnimState::lod_step_factor`] to fold the `8 / x` math and
/// the floor-at-1 rule.
pub const ACTOR_LOD_STEP_OFFSET: usize = 0x21D;

/// Per-actor previous-action sentinel at `actor[+0x21B]`. The consumer
/// stamps `OpaqueAnimRecord::depth_84` into this byte at the end of an
/// anim transition (`FUN_8004AD80` line 1058: `*(actor + 0x21B) =
/// *(consumer + 0x84)`). The per-bone interpolator at `FUN_8004998C`
/// reads it back when deciding whether to pull the "next" frame from
/// the loop target (`OpaqueAnimRecord::count_85`) or from the linear
/// `frame_index + 1` slot.
pub const ACTOR_PREV_ACTION_OFFSET: usize = 0x21B;

/// Per-actor frame-counter cap at `actor[+0x176]`. The consumer stamps
/// `OpaqueAnimRecord::depth_84 << 4` into this u16 at the start of a
/// new anim. The actor's own frame-counter `actor[+0x68]` (a u16 with
/// the frame index in the high 12 bits and the sub-frame interpolation
/// factor in the low 4 bits) clamps against it.
pub const ACTOR_FRAME_CAP_OFFSET: usize = 0x176;

/// Typed view onto a borrowed actor record's anim-related fields. Wraps
/// the same `0x2D4`-byte buffer the battle-action SM owns and exposes
/// the LOD step + frame-counter cap accessors so engines that re-host
/// the actor record don't have to repeat the offset arithmetic.
#[derive(Debug, Clone, Copy)]
pub struct ActorAnimState<'a> {
    bytes: &'a [u8],
}

impl<'a> ActorAnimState<'a> {
    /// Wrap a byte slice. The slice should point at the actor record's
    /// first byte; offsets are relative to that. Out-of-range reads
    /// return `None`.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// `actor[+0x21B]` previous-action sentinel.
    pub fn prev_action(&self) -> Option<u8> {
        self.bytes.get(ACTOR_PREV_ACTION_OFFSET).copied()
    }

    /// `actor[+0x21D]` raw LOD step byte (values observed: 0, 2, 4, 8).
    pub fn lod_step_raw(&self) -> Option<u8> {
        self.bytes.get(ACTOR_LOD_STEP_OFFSET).copied()
    }

    /// Folded LOD step: `8 / max(stride, 1)`, clamped to `[1, 8]`. For
    /// the observed inputs `0 / 2 / 4 / 8` this returns `8 / 4 / 2 / 1`
    /// — i.e. how many child actors the renderer skips per outer step.
    pub fn lod_step_factor(&self) -> Option<u8> {
        let raw = self.lod_step_raw()?;
        let denom = if raw == 0 { 1 } else { raw };
        Some((8 / denom).clamp(1, 8))
    }

    /// `actor[+0x176]` frame-counter cap (u16 LE).
    pub fn frame_cap(&self) -> Option<u16> {
        let bytes = self
            .bytes
            .get(ACTOR_FRAME_CAP_OFFSET..ACTOR_FRAME_CAP_OFFSET + 2)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// `actor[+0x68]` frame counter (u16 LE). Layout:
    ///
    /// - bits `[4..15]` (high 12 bits): frame index into the per-frame
    ///   data array.
    /// - bits `[0..3]` (low 4 bits): sub-frame interpolation factor
    ///   (`0` = exact frame, `1..=15` = lerp factor / 16 between this
    ///   frame and the next).
    pub fn frame_counter(&self) -> Option<u16> {
        let bytes = self.bytes.get(0x68..0x68 + 2)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Frame index extracted from `frame_counter()` (high 12 bits).
    pub fn frame_index(&self) -> Option<u16> {
        Some(self.frame_counter()? >> 4)
    }

    /// Sub-frame interpolation factor (low 4 bits, `0..=15`).
    pub fn sub_frame_factor(&self) -> Option<u8> {
        Some((self.frame_counter()? & 0x000F) as u8)
    }
}

/// One bone's keyframe within a nested per-frame data block: 6
/// sign-extended 12-bit values laid out as two `[i16; 3]` vectors.
///
/// The pair shape mirrors what `FUN_8004998C` writes to the GPU OT
/// scratch buffer at `(unaff_gp + 0x6F4)` - `vec_a` is the first vec3
/// (x, y, z) and `vec_b` is the second. The interpretation (rotation
/// quat / euler / position delta) is renderer-side; on the data side
/// they're just two signed-12-bit triplets per bone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoneFrame {
    /// First 12-bit triplet (`puVar14[0..3]` after sign extension).
    pub vec_a: [i16; 3],
    /// Second 12-bit triplet (`puVar14[3..6]` after sign extension).
    pub vec_b: [i16; 3],
}

impl BoneFrame {
    /// Decode a 9-byte bone keyframe.
    ///
    /// The packing pairs adjacent low bytes with shared high-nibble
    /// bytes. For each output index `k` in `0..=5`:
    ///
    /// ```text
    /// k=0: byte[0] | (byte[2] & 0x0F) << 8
    /// k=1: byte[1] | (byte[2] & 0xF0) << 4
    /// k=2: byte[3] | (byte[5] & 0x0F) << 8
    /// k=3: byte[4] | (byte[5] & 0xF0) << 4
    /// k=4: byte[6] | (byte[8] & 0x0F) << 8
    /// k=5: byte[7] | (byte[8] & 0xF0) << 4
    /// ```
    ///
    /// Each result is a 12-bit value; if bit 11 (`0x800`) is set the
    /// consumer ORs `0xF000` to sign-extend - we do the same here so
    /// the returned `i16` is the runtime-equivalent value.
    pub fn from_9_bytes(b: &[u8; 9]) -> Self {
        let pack = |lo: u8, nib: u8| -> i16 {
            let v = u16::from(lo) | (u16::from(nib) << 8);
            if v & 0x0800 != 0 {
                (v | 0xF000) as i16
            } else {
                v as i16
            }
        };
        let a0 = pack(b[0], b[2] & 0x0F);
        let a1 = pack(b[1], (b[2] & 0xF0) >> 4);
        let a2 = pack(b[3], b[5] & 0x0F);
        let b0 = pack(b[4], (b[5] & 0xF0) >> 4);
        let b1 = pack(b[6], b[8] & 0x0F);
        let b2 = pack(b[7], (b[8] & 0xF0) >> 4);
        BoneFrame {
            vec_a: [a0, a1, a2],
            vec_b: [b0, b1, b2],
        }
    }

    /// Encode this bone keyframe back into 9 bytes. Round-trip with
    /// [`BoneFrame::from_9_bytes`] for any value whose components fit
    /// the signed 12-bit range `[-2048, 2047]`. Out-of-range components
    /// are masked to 12 bits (matching what the runtime would observe
    /// after sign-extension).
    pub fn to_9_bytes(&self) -> [u8; 9] {
        let unpack = |v: i16| -> (u8, u8) {
            let m = (v as u16) & 0x0FFF;
            ((m & 0xFF) as u8, ((m >> 8) & 0x0F) as u8)
        };
        let (al0, ah0) = unpack(self.vec_a[0]);
        let (al1, ah1) = unpack(self.vec_a[1]);
        let (al2, ah2) = unpack(self.vec_a[2]);
        let (bl0, bh0) = unpack(self.vec_b[0]);
        let (bl1, bh1) = unpack(self.vec_b[1]);
        let (bl2, bh2) = unpack(self.vec_b[2]);
        [
            al0,
            al1,
            ah0 | (ah1 << 4),
            al2,
            bl0,
            ah2 | (bh0 << 4),
            bl1,
            bl2,
            bh1 | (bh2 << 4),
        ]
    }

    /// Linear interpolate between this frame and `other` by `frac/16`.
    /// Mirrors `FUN_8004998C`'s `dst = a + (b - a) * frac >> 4`
    /// formula component-wise. `frac` is clamped to `0..=15`.
    pub fn lerp16(&self, other: &BoneFrame, frac: u8) -> BoneFrame {
        let f = frac.min(15) as i32;
        let comp = |a: i16, b: i16| -> i16 {
            let a = a as i32;
            let b = b as i32;
            (a + (((b - a) * f) >> 4)) as i16
        };
        BoneFrame {
            vec_a: [
                comp(self.vec_a[0], other.vec_a[0]),
                comp(self.vec_a[1], other.vec_a[1]),
                comp(self.vec_a[2], other.vec_a[2]),
            ],
            vec_b: [
                comp(self.vec_b[0], other.vec_b[0]),
                comp(self.vec_b[1], other.vec_b[1]),
                comp(self.vec_b[2], other.vec_b[2]),
            ],
        }
    }
}

/// Stride per bone within a frame, in bytes.
pub const NESTED_BONE_STRIDE: usize = 9;

/// Header size at the start of the nested per-frame data buffer
/// (`bones_per_frame` byte + `frame_count` byte).
pub const NESTED_HEADER_SIZE: usize = 2;

/// Errors returned by [`NestedFrameData::from_bytes`] when the slice
/// cannot back the declared frame / bone counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NestedFrameDataError {
    /// Slice shorter than 2 bytes (no header).
    HeaderTooSmall,
    /// `bones_per_frame` (the header byte at `+0`) is zero — the
    /// renderer's loop bound (`bones * 6` ushorts) would degenerate
    /// to nothing useful.
    ZeroBonesPerFrame,
    /// `frame_count` (the header byte at `+1`) is zero — the consumer
    /// reads `(frame_count - 1) * 16` to seed the actor's frame-counter
    /// cap, so a zero count produces a wrap-around bug at runtime.
    ZeroFrameCount,
    /// Slice is shorter than `2 + bones * frames * 9`.
    BodyTruncated { needed: usize, got: usize },
}

impl std::fmt::Display for NestedFrameDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NestedFrameDataError::HeaderTooSmall => f.write_str("header too small (< 2 bytes)"),
            NestedFrameDataError::ZeroBonesPerFrame => {
                f.write_str("bones_per_frame header byte is zero")
            }
            NestedFrameDataError::ZeroFrameCount => f.write_str("frame_count header byte is zero"),
            NestedFrameDataError::BodyTruncated { needed, got } => {
                write!(f, "body truncated: needed {needed} bytes, got {got}")
            }
        }
    }
}

impl std::error::Error for NestedFrameDataError {}

/// Borrowed view of the buffer pointed at by `OpaqueAnimRecord::nested_data_ptr_raw()`
/// (i.e. consumer-struct field `+0x88`).
///
/// ## Layout
///
/// ```text
/// +0x00  u8  bones_per_frame  (B)
/// +0x01  u8  frame_count      (N)
/// +0x02..+0x02 + N*B*9        N frames × B bones × 9-byte keyframe
/// ```
///
/// Per-bone 9-byte block decodes as a [`BoneFrame`] — six packed
/// sign-extended 12-bit values arranged as two `[i16; 3]` vectors.
///
/// ## Provenance
///
/// - Header byte `+0` is the per-frame loop count: see `FUN_80048A08`
///   (`ghidra/scripts/funcs/80048a08.txt` line 749, the rendering loop's
///   bound is `**buf`).
/// - Header byte `+1` is the frame count: see `FUN_8004AD80`
///   (`ghidra/scripts/funcs/8004ad80.txt` line 1367,
///   `(buf[1] - 1) * 16` is stamped into the actor's frame counter as
///   the wrap-around terminus).
/// - Frame stride is `B * 9` and the body starts at offset `+2`: see
///   `FUN_8004998C` (`ghidra/scripts/funcs/8004998c.txt` line 1040,
///   `pbVar15 = pbVar20 + frame_index * B * 9 + 2`).
/// - Per-bone packing (6 × 12-bit → 9 bytes) and sign extension: see
///   `FUN_8004998C` lines 1049..1054 (unpack) and 1055..1062
///   (`if (uVar & 0x0800 != 0) ... |= 0xF000`).
#[derive(Debug, Clone, Copy)]
pub struct NestedFrameData<'a> {
    bytes: &'a [u8],
}

impl<'a> NestedFrameData<'a> {
    /// Wrap a slice and validate its size against the declared bone /
    /// frame counts. Returns `Err` if the slice is too small or either
    /// count is zero.
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self, NestedFrameDataError> {
        if bytes.len() < NESTED_HEADER_SIZE {
            return Err(NestedFrameDataError::HeaderTooSmall);
        }
        let bones = bytes[0];
        let frames = bytes[1];
        if bones == 0 {
            return Err(NestedFrameDataError::ZeroBonesPerFrame);
        }
        if frames == 0 {
            return Err(NestedFrameDataError::ZeroFrameCount);
        }
        let needed =
            NESTED_HEADER_SIZE + usize::from(bones) * usize::from(frames) * NESTED_BONE_STRIDE;
        if bytes.len() < needed {
            return Err(NestedFrameDataError::BodyTruncated {
                needed,
                got: bytes.len(),
            });
        }
        Ok(Self { bytes })
    }

    /// Wrap a slice **without** validating the body size. Useful for
    /// inspection of malformed buffers; out-of-range frame / bone reads
    /// will return `None`.
    pub fn from_bytes_lenient(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < NESTED_HEADER_SIZE {
            return None;
        }
        Some(Self { bytes })
    }

    /// Header byte at `+0`: number of bones per frame.
    pub fn bones_per_frame(&self) -> u8 {
        self.bytes[0]
    }

    /// Header byte at `+1`: number of frames in the buffer.
    pub fn frame_count(&self) -> u8 {
        self.bytes[1]
    }

    /// Total expected size in bytes: `2 + bones * frames * 9`.
    pub fn expected_size(&self) -> usize {
        NESTED_HEADER_SIZE
            + usize::from(self.bones_per_frame())
                * usize::from(self.frame_count())
                * NESTED_BONE_STRIDE
    }

    /// Stride in bytes between adjacent frames (`bones * 9`).
    pub fn frame_stride(&self) -> usize {
        usize::from(self.bones_per_frame()) * NESTED_BONE_STRIDE
    }

    /// Read one bone's keyframe at `(frame_idx, bone_idx)`.
    pub fn bone(&self, frame_idx: usize, bone_idx: usize) -> Option<BoneFrame> {
        if frame_idx >= usize::from(self.frame_count()) {
            return None;
        }
        if bone_idx >= usize::from(self.bones_per_frame()) {
            return None;
        }
        let off =
            NESTED_HEADER_SIZE + frame_idx * self.frame_stride() + bone_idx * NESTED_BONE_STRIDE;
        let slice = self.bytes.get(off..off + NESTED_BONE_STRIDE)?;
        let arr: &[u8; 9] = slice.try_into().ok()?;
        Some(BoneFrame::from_9_bytes(arr))
    }

    /// Iterate every bone in `frame_idx` in storage order.
    ///
    /// Returns `None` if `frame_idx` is out of range; otherwise an
    /// iterator that yields `BoneFrame` for `bone_idx` in
    /// `0..bones_per_frame`.
    pub fn frame(&self, frame_idx: usize) -> Option<FrameView<'a>> {
        if frame_idx >= usize::from(self.frame_count()) {
            return None;
        }
        let stride = self.frame_stride();
        let start = NESTED_HEADER_SIZE + frame_idx * stride;
        let body = self.bytes.get(start..start + stride)?;
        Some(FrameView {
            bones: body,
            bones_per_frame: self.bones_per_frame(),
        })
    }

    /// Linear-interpolate every bone in `frame_idx` toward the bone in
    /// `next_idx` by `frac/16`. Returns a `Vec<BoneFrame>` with one
    /// entry per bone. Mirrors `FUN_8004998C` line ~1115's loop body.
    ///
    /// `frac` is clamped to `0..=15` per the runtime (the consumer
    /// extracts the low nibble of `actor[+0x68]` for it).
    pub fn interpolate(
        &self,
        frame_idx: usize,
        next_idx: usize,
        frac: u8,
    ) -> Option<Vec<BoneFrame>> {
        let bones = usize::from(self.bones_per_frame());
        let mut out = Vec::with_capacity(bones);
        for b in 0..bones {
            let cur = self.bone(frame_idx, b)?;
            let nxt = self.bone(next_idx, b)?;
            out.push(cur.lerp16(&nxt, frac));
        }
        Some(out)
    }
}

/// Borrowed view of a single frame's bones inside a [`NestedFrameData`].
///
/// Use [`NestedFrameData::frame`] to obtain one. Iterating yields one
/// [`BoneFrame`] per bone in storage order.
#[derive(Debug, Clone, Copy)]
pub struct FrameView<'a> {
    bones: &'a [u8],
    bones_per_frame: u8,
}

impl<'a> FrameView<'a> {
    /// Number of bones in this frame.
    pub fn bones_per_frame(&self) -> u8 {
        self.bones_per_frame
    }

    /// Read one bone at `bone_idx`.
    pub fn bone(&self, bone_idx: usize) -> Option<BoneFrame> {
        if bone_idx >= usize::from(self.bones_per_frame) {
            return None;
        }
        let off = bone_idx * NESTED_BONE_STRIDE;
        let arr: &[u8; 9] = self
            .bones
            .get(off..off + NESTED_BONE_STRIDE)?
            .try_into()
            .ok()?;
        Some(BoneFrame::from_9_bytes(arr))
    }

    /// Iterate every bone in storage order.
    pub fn iter_bones(&self) -> impl Iterator<Item = BoneFrame> + '_ {
        (0..usize::from(self.bones_per_frame)).filter_map(|i| self.bone(i))
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
        assert_eq!(r.substate_counter(), Some(7));
        assert_eq!(r.depth_84(), Some(0x10));
        assert_eq!(r.count_85(), Some(12));
        assert_eq!(r.count_86(), Some(24));
        assert_eq!(r.effect_id(), Some(0x42));
        assert_eq!(r.nested_data_ptr_raw(), Some(0x80AB_CDEF));
        assert_eq!(r.anim_frame_176(), Some(255));
    }

    #[test]
    fn actor_anim_state_lod_step_factor_folds_division() {
        // raw=0 → denom 1, factor 8/1 = 8 (clamped, 1..=8)
        // raw=2 → factor 4
        // raw=4 → factor 2
        // raw=8 → factor 1
        let cases = &[(0u8, 8u8), (2, 4), (4, 2), (8, 1)];
        for &(raw, expected) in cases {
            let mut buf = vec![0u8; 0x250];
            buf[ACTOR_LOD_STEP_OFFSET] = raw;
            let s = ActorAnimState::new(&buf);
            assert_eq!(
                s.lod_step_factor(),
                Some(expected),
                "raw 0x{raw:02x} should give factor {expected}",
            );
        }
    }

    #[test]
    fn actor_anim_state_frame_counter_extracts_index_and_subframe() {
        // bits[4..15] = frame index, bits[0..3] = sub-frame factor.
        // 0x1234 → index = 0x123, sub = 0x4.
        let mut buf = vec![0u8; 0x250];
        buf[0x68..0x6A].copy_from_slice(&0x1234u16.to_le_bytes());
        buf[ACTOR_PREV_ACTION_OFFSET] = 0x55;
        buf[ACTOR_FRAME_CAP_OFFSET..ACTOR_FRAME_CAP_OFFSET + 2]
            .copy_from_slice(&0x0123u16.to_le_bytes());
        let s = ActorAnimState::new(&buf);
        assert_eq!(s.frame_counter(), Some(0x1234));
        assert_eq!(s.frame_index(), Some(0x123));
        assert_eq!(s.sub_frame_factor(), Some(0x4));
        assert_eq!(s.prev_action(), Some(0x55));
        assert_eq!(s.frame_cap(), Some(0x0123));
    }

    #[test]
    fn bone_frame_round_trips_for_in_range_components() {
        // Components in [-2048, 2047] round-trip exactly through
        // the 9-byte packed form.
        let cases = &[
            BoneFrame {
                vec_a: [0, 0, 0],
                vec_b: [0, 0, 0],
            },
            BoneFrame {
                vec_a: [1, -1, 100],
                vec_b: [-100, 2047, -2048],
            },
            BoneFrame {
                vec_a: [0x07FF, -0x0800, 0x055],
                vec_b: [0x0123, -0x0456, 0x0789],
            },
        ];
        for original in cases {
            let bytes = original.to_9_bytes();
            let decoded = BoneFrame::from_9_bytes(&bytes);
            assert_eq!(*original, decoded, "round-trip failed for {original:?}");
        }
    }

    #[test]
    fn bone_frame_sign_extends_high_nibble_bit() {
        // byte[2] = 0x80: low nibble 0x0 → vec_a[0]=0, high nibble 0x8 →
        // vec_a[1] = 0 | (0x8 << 8) = 0x800 → bit 11 set → sign-extends
        // to 0xF800 = -2048.
        let bytes = [0x00u8, 0x00, 0x80, 0, 0, 0, 0, 0, 0];
        let bf = BoneFrame::from_9_bytes(&bytes);
        assert_eq!(bf.vec_a, [0, -2048, 0]);
        // byte[2] = 0x07: high nibble 0, low nibble 7 → vec_a[0] =
        // 0 | (0x7 << 8) = 0x700, no sign extension (bit 11 clear).
        let bytes2 = [0u8, 0, 0x07, 0, 0, 0, 0, 0, 0];
        let bf2 = BoneFrame::from_9_bytes(&bytes2);
        assert_eq!(bf2.vec_a, [0x700, 0, 0]);
    }

    #[test]
    fn bone_frame_pack_uses_correct_byte_pairings() {
        // Packing rules from FUN_8004998C lines 1049..1054:
        //
        //   k=0 ← byte[0] | (byte[2] & 0x0F) << 8
        //   k=1 ← byte[1] | (byte[2] & 0xF0) << 4
        //   k=2 ← byte[3] | (byte[5] & 0x0F) << 8
        //   k=3 ← byte[4] | (byte[5] & 0xF0) << 4
        //   k=4 ← byte[6] | (byte[8] & 0x0F) << 8
        //   k=5 ← byte[7] | (byte[8] & 0xF0) << 4
        //
        // Pick low nibbles only so the assertions stay sign-clean.
        let bytes = [0x11u8, 0x22, 0x21, 0x33, 0x44, 0x43, 0x55, 0x66, 0x65];
        let bf = BoneFrame::from_9_bytes(&bytes);
        assert_eq!(bf.vec_a[0], 0x0111); // byte[0]=0x11, low nibble of byte[2] (0x1)
        assert_eq!(bf.vec_a[1], 0x0222); // byte[1]=0x22, high nibble of byte[2] (0x2)
        assert_eq!(bf.vec_a[2], 0x0333); // byte[3]=0x33, low nibble of byte[5] (0x3)
        assert_eq!(bf.vec_b[0], 0x0444); // byte[4]=0x44, high nibble of byte[5] (0x4)
        assert_eq!(bf.vec_b[1], 0x0555); // byte[6]=0x55, low nibble of byte[8] (0x5)
        assert_eq!(bf.vec_b[2], 0x0666); // byte[7]=0x66, high nibble of byte[8] (0x6)
    }

    #[test]
    fn nested_frame_data_round_trips_two_frames_three_bones() {
        // 2 frames × 3 bones × 9 bytes = 54 body bytes + 2 header = 56 total.
        let bones = 3usize;
        let frames = 2usize;
        let mut buf = vec![0u8; NESTED_HEADER_SIZE + frames * bones * NESTED_BONE_STRIDE];
        buf[0] = bones as u8;
        buf[1] = frames as u8;
        // Stamp distinct bone keyframes per (frame, bone) so we can
        // verify the indexing math.
        let mut written = Vec::new();
        for f in 0..frames {
            for b in 0..bones {
                let bone = BoneFrame {
                    vec_a: [(f as i16) * 100 + b as i16, 0, 0],
                    vec_b: [0, 0, (b as i16) * -10],
                };
                let off =
                    NESTED_HEADER_SIZE + f * bones * NESTED_BONE_STRIDE + b * NESTED_BONE_STRIDE;
                buf[off..off + NESTED_BONE_STRIDE].copy_from_slice(&bone.to_9_bytes());
                written.push(bone);
            }
        }

        let nfd = NestedFrameData::from_bytes(&buf).expect("valid");
        assert_eq!(nfd.bones_per_frame(), 3);
        assert_eq!(nfd.frame_count(), 2);
        assert_eq!(nfd.expected_size(), buf.len());
        assert_eq!(nfd.frame_stride(), bones * NESTED_BONE_STRIDE);

        for f in 0..frames {
            for b in 0..bones {
                let bone = nfd.bone(f, b).unwrap();
                let expected = written[f * bones + b];
                assert_eq!(bone, expected, "(frame={f}, bone={b})");
            }
        }
    }

    #[test]
    fn nested_frame_data_frame_view_iterates_in_storage_order() {
        let bones = 2u8;
        let frames = 1u8;
        let mut buf = vec![
            0u8;
            NESTED_HEADER_SIZE
                + usize::from(bones) * usize::from(frames) * NESTED_BONE_STRIDE
        ];
        buf[0] = bones;
        buf[1] = frames;
        let bone_a = BoneFrame {
            vec_a: [10, 20, 30],
            vec_b: [-1, -2, -3],
        };
        let bone_b = BoneFrame {
            vec_a: [100, 200, 300],
            vec_b: [-100, -200, -300],
        };
        buf[2..11].copy_from_slice(&bone_a.to_9_bytes());
        buf[11..20].copy_from_slice(&bone_b.to_9_bytes());

        let nfd = NestedFrameData::from_bytes(&buf).unwrap();
        let frame = nfd.frame(0).unwrap();
        let bones_seen: Vec<_> = frame.iter_bones().collect();
        assert_eq!(bones_seen.len(), 2);
        assert_eq!(bones_seen[0], bone_a);
        assert_eq!(bones_seen[1], bone_b);
    }

    #[test]
    fn nested_frame_data_rejects_zero_counts_and_truncated_body() {
        // Zero bone count.
        let buf = vec![0u8, 5, 0, 0, 0];
        assert_eq!(
            NestedFrameData::from_bytes(&buf).err(),
            Some(NestedFrameDataError::ZeroBonesPerFrame)
        );
        // Zero frame count.
        let buf = vec![3u8, 0, 0, 0, 0];
        assert_eq!(
            NestedFrameData::from_bytes(&buf).err(),
            Some(NestedFrameDataError::ZeroFrameCount)
        );
        // Truncated body: header says 2 bones × 1 frame = 18 bytes
        // body, but only 10 are present.
        let mut buf = vec![0u8; NESTED_HEADER_SIZE + 10];
        buf[0] = 2;
        buf[1] = 1;
        match NestedFrameData::from_bytes(&buf) {
            Err(NestedFrameDataError::BodyTruncated { needed, got }) => {
                assert_eq!(needed, NESTED_HEADER_SIZE + 2 * NESTED_BONE_STRIDE);
                assert_eq!(got, NESTED_HEADER_SIZE + 10);
            }
            other => panic!("expected BodyTruncated, got {other:?}"),
        }
        // Header too small.
        let buf = vec![0u8];
        assert_eq!(
            NestedFrameData::from_bytes(&buf).err(),
            Some(NestedFrameDataError::HeaderTooSmall)
        );
    }

    #[test]
    fn nested_frame_data_interpolate_matches_runtime_lerp_formula() {
        let bones = 1u8;
        let frames = 2u8;
        let mut buf = vec![
            0u8;
            NESTED_HEADER_SIZE
                + usize::from(bones) * usize::from(frames) * NESTED_BONE_STRIDE
        ];
        buf[0] = bones;
        buf[1] = frames;
        let f0 = BoneFrame {
            vec_a: [0, 0, 0],
            vec_b: [0, 0, 0],
        };
        let f1 = BoneFrame {
            vec_a: [16, -16, 32],
            vec_b: [0, 0, 0],
        };
        buf[2..11].copy_from_slice(&f0.to_9_bytes());
        buf[11..20].copy_from_slice(&f1.to_9_bytes());

        let nfd = NestedFrameData::from_bytes(&buf).unwrap();
        // At frac=0, result equals frame 0.
        let r0 = nfd.interpolate(0, 1, 0).unwrap();
        assert_eq!(r0[0], f0);
        // At frac=8 (half), each component advances halfway:
        // 0 + (16 - 0) * 8 >> 4 = 8.
        let r8 = nfd.interpolate(0, 1, 8).unwrap();
        assert_eq!(r8[0].vec_a, [8, -8, 16]);
        // Frac is clamped to 15.
        let r15 = nfd.interpolate(0, 1, 15).unwrap();
        // 0 + (16 - 0) * 15 >> 4 = 240 / 16 = 15
        assert_eq!(r15[0].vec_a, [15, -15, 30]);
        // Out-of-range frac is clamped to 15 internally.
        let r99 = nfd.interpolate(0, 1, 99).unwrap();
        assert_eq!(r99[0].vec_a, [15, -15, 30]);
    }

    #[test]
    fn nested_frame_data_lenient_accepts_short_buffer() {
        // Lenient view accepts any 2+ byte slice; out-of-range reads
        // return None.
        let buf = vec![3u8, 5, 0, 0, 0];
        let nfd = NestedFrameData::from_bytes_lenient(&buf).unwrap();
        assert_eq!(nfd.bones_per_frame(), 3);
        assert_eq!(nfd.frame_count(), 5);
        assert!(nfd.bone(0, 0).is_none()); // body too small
        assert!(nfd.frame(0).is_none());
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
