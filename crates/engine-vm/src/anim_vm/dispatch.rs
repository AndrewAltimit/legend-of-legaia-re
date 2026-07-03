use legaia_anm::RecordHeader;

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
