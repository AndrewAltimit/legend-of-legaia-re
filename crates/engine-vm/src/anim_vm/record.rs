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
    /// - i.e. how many child actors the renderer skips per outer step.
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
