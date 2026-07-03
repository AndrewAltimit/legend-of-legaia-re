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
    /// `bones_per_frame` (the header byte at `+0`) is zero - the
    /// renderer's loop bound (`bones * 6` ushorts) would degenerate
    /// to nothing useful.
    ZeroBonesPerFrame,
    /// `frame_count` (the header byte at `+1`) is zero - the consumer
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
/// Per-bone 9-byte block decodes as a [`BoneFrame`] - six packed
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
