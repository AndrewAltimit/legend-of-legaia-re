//! "TMD-prefixed scene-stream" detector - a streaming-format variant
//! that opens with a bare Legaia TMD instead of a typed chunk header.
//!
//! ### Layout (empirically verified across 148 PROT entries, 2026-05)
//!
//! ```text
//! +0x00          u32 chunk0_header   ; (type=0x00 << 24) | size
//! +0x04          Legaia TMD          ; magic 0x80000002, fills `size` bytes
//! +0x04 + size   streaming chunks    ; specialised FUN_8001fe70-style
//!                                    ; chunks until terminator OR EOF
//! ```
//!
//! The chunk0 header looks like a standard streaming `(type << 24) | size`
//! with `type = 0x00`, but the payload is a Legaia TMD (magic
//! `0x80000002`). The retail loader for this shape is the battle scene
//! loader (`FUN_800520F0`) which calls `FUN_8001FE70` to walk the entry -
//! see [`battle_tim_chunks`] for the walker contract. `FUN_8001FE70` reads
//! chunk0 as `[TMD body size][TMD body]`, then enters a per-chunk loop in
//! the streaming tail where:
//!
//! - `type byte = 0x01` -> upload TIM payload via `LoadImage`
//! - `type byte = 0x02` -> stop the loop
//! - any other type -> skip silently (advance and continue)
//!
//! This is a different type-byte semantic than `FUN_8001F05C` uses (where
//! `type = 0x01` means `TIM_LIST` and would attempt to parse the payload
//! as a `(count, offsets[count], TIMs)` pack). Calling the standard
//! `FUN_8002541C` streaming walker on a scene_tmd_stream entry would
//! crash or upload garbage, which is why the runtime uses the specialised
//! `FUN_8001FE70` walker for these. See [`docs/formats/scene-bundles.md`]
//! for the format index and [`docs/subsystems/asset-loader.md`] for the
//! caller chain.
//!
//! ### Concatenated sub-streams (the "two-list" shape)
//!
//! Some scene_tmd_stream entries hold **more than one complete sub-stream**
//! concatenated: each is a full `[chunk0 TMD][type-0x01 TIM chunks][terminator]`
//! block, and each starts on a **`0x800` (sector) boundary** with zero padding
//! filling the gap. `0006_town01.BIN` is the canonical example — sub-stream 0
//! at `0x0` (TMD `0x383C` + TIMs at `0x3840` / `0xBA64`) and sub-stream 1 at
//! `0x14000` (its **own** leading TMD `0x2C20` + TIMs at `0x16C24` / `0x1EE48`).
//! So the bytes earlier docs called a "continuation TIM list" are really the
//! second sub-stream's TIM chunks; sub-stream 1 is a self-contained
//! scene_tmd_stream with its own TMD, not a bare tail of sub-stream 0. Use
//! [`sub_streams`] to enumerate them properly.
//!
//! `FUN_8001FE70` walks exactly one sub-stream and **returns a pointer just
//! past its terminator** (`return param_1 + 1`), i.e. the start of the next
//! sub-stream's region — so a sector/slot-indexed caller can walk the rest by
//! re-invoking the walker on that boundary. The one static caller
//! (`FUN_800513F0`, battle init) calls it **once** and consumes only
//! sub-stream 0 (its `s3 < 4` loop above the call is the 4-party-member setup,
//! not a sub-stream loop), so in battle the later sub-streams are not uploaded.
//! The multi-sub-stream caller is the per-scene field/town dispatch (overlay-
//! resident, descriptor-driven `FUN_8001F7C0` → `FUN_80020224` → `FUN_8001F05C`),
//! still capture-blocked. [`battle_tim_chunks`] reports both `WalkSource::Tail`
//! (sub-stream 0, inside `FUN_8001FE70`'s reach) and `WalkSource::Continuation`
//! (the later sub-streams' TIM chunks) so engine ports can choose whether to
//! upload one or all.
//!
//! This shape is dominant in scene-asset PROT entries (most `town*`, `dolk*`,
//! `rugi*`, and similar named blocks). Pre-TOC-fix the bare-TMD prefix made
//! many of these look like "low-entropy unknowns" because the inner streaming
//! header was 8824+ bytes deep - the standard streaming detector starts at
//! offset 0 and saw a non-streaming first chunk (`type_byte = 0x00` with
//! TMD-magic content).

use serde::Serialize;

use crate::AssetType;

/// Minimum sane object count in a Legaia TMD header. Defensive bound - real
/// scene TMDs have 1-8 objects (terrain mesh + a few props).
const MAX_TMD_OBJECTS: u32 = 64;

/// Maximum total streaming chunk count we'll walk before giving up. Real hits
/// have 1-6 chunks; anything past 16 is almost certainly a mis-detection.
const MAX_CHUNKS: usize = 64;

/// Maximum bytes consumed by the optional streaming-tail walk. We don't need
/// to walk forever - a few hundred KB is enough to confirm shape.
const MAX_TAIL_WALK: usize = 4 * 1024 * 1024;

/// Per-chunk record in the streaming tail.
#[derive(Debug, Clone, Serialize)]
pub struct TailChunk {
    /// Byte offset of the chunk header within the file.
    pub offset: usize,
    /// Asset type from the chunk's high-byte.
    pub asset_type: AssetType,
    /// Low-24-bit size from the chunk header.
    pub size: u32,
}

/// Detection result.
#[derive(Debug, Clone, Serialize)]
pub struct SceneTmdStream {
    /// Byte size of the leading TMD body (= `(chunk0_header & 0xFFFFFF)`).
    /// The TMD occupies `[4 .. 4 + tmd_size]`.
    pub tmd_size: usize,
    /// Object count from the leading TMD's header.
    pub tmd_nobj: u32,
    /// Streaming chunks walked from `4 + tmd_size` until terminator or break.
    pub tail_chunks: Vec<TailChunk>,
    /// Whether the streaming tail terminated cleanly (header low-24 == 0).
    pub tail_terminated: bool,
    /// Byte offset where the streaming tail walk stopped.
    pub tail_end: usize,
}

impl SceneTmdStream {
    /// Byte range of the leading TMD body inside the on-disc buffer.
    /// Hand `&buf[range]` to `legaia_tmd::parse`.
    pub fn tmd_range(&self) -> std::ops::Range<usize> {
        4..4 + self.tmd_size
    }
}

/// Try to detect a TMD-prefixed scene stream. Returns `None` when the buffer
/// doesn't match the schema; structural errors fail soft.
pub fn detect(buf: &[u8]) -> Option<SceneTmdStream> {
    if buf.len() < 32 {
        return None;
    }

    // (1) Bare TMD magic at offset 4.
    let tmd_magic = read_u32_le(buf, 4)?;
    if tmd_magic != 0x80000002 {
        return None;
    }

    // (2) TMD on-disc flags must be zero (post-fixup is 1, on-disc is 0).
    let tmd_flags = read_u32_le(buf, 8)?;
    if tmd_flags != 0 {
        return None;
    }

    // (3) Object count must be a small positive number.
    let nobj = read_u32_le(buf, 12)?;
    if nobj == 0 || nobj > MAX_TMD_OBJECTS {
        return None;
    }

    // (4) The chunk0 header packs `(type<<24) | size`, where the high byte
    //     is 0 (TIM dispatcher) and the low 24 bits give the TMD body size.
    //     Reject if the type byte isn't 0 - that would mean a different
    //     dispatcher fires on the leading chunk and this isn't the variant
    //     we're trying to detect.
    let chunk0_header = read_u32_le(buf, 0)?;
    if (chunk0_header >> 24) & 0xFF != 0 {
        return None;
    }
    let tmd_size = (chunk0_header & 0x00FF_FFFF) as usize;
    let min_tmd_size = 12 + (nobj as usize) * 28;
    if tmd_size < min_tmd_size {
        return None;
    }
    let tmd_end = 4usize.checked_add(tmd_size)?;
    if tmd_end > buf.len() {
        return None;
    }
    // Streaming chunks are 4-byte aligned; the TMD body must land on one.
    if !tmd_size.is_multiple_of(4) {
        return None;
    }

    // (5) Walk the streaming tail starting at `4 + tmd_size`. We accept the
    //     file even if the tail doesn't terminate cleanly - many entries
    //     are stored padded out to the next 0x800 sector boundary, which
    //     our walker may detect as garbage rather than a clean terminator.
    let mut tail_chunks = Vec::new();
    let mut cur = tmd_end;
    let mut terminated = false;
    let walk_cap = (cur + MAX_TAIL_WALK).min(buf.len());
    while cur + 4 <= walk_cap && tail_chunks.len() < MAX_CHUNKS {
        let header = match read_u32_le(buf, cur) {
            Some(v) => v,
            None => break,
        };
        if header & 0x00FF_FFFF == 0 {
            terminated = true;
            cur += 4;
            break;
        }
        let type_byte = ((header >> 24) & 0xFF) as u8;
        let asset_type = AssetType::from_byte(type_byte);
        if matches!(asset_type, AssetType::Unknown(_)) {
            // Tail is malformed (or truncated). Stop without recording the
            // bogus header - caller can still see how many good chunks parsed.
            break;
        }
        let size = header & 0x00FF_FFFF;
        tail_chunks.push(TailChunk {
            offset: cur,
            asset_type,
            size,
        });
        // Streaming chunks are 4-byte aligned by spec.
        cur = cur
            .checked_add(4 + ((size as usize + 3) & !3))
            .unwrap_or(buf.len());
    }

    // (6) Require at least one good streaming-tail chunk OR a clean terminator
    //     immediately at `tmd_end`. Otherwise we're matching arbitrary
    //     [u32 size][TMD] data with random bytes following it.
    if tail_chunks.is_empty() && !terminated {
        return None;
    }

    Some(SceneTmdStream {
        tmd_size,
        tmd_nobj: nobj,
        tail_chunks,
        tail_terminated: terminated,
        tail_end: cur,
    })
}

/// Cheap presence check - used by [`crate::categorize`] before doing the
/// full streaming-tail walk in callers that just need a yes/no.
pub fn is_scene_tmd_stream(buf: &[u8]) -> bool {
    detect(buf).is_some()
}

/// One concatenated sub-stream inside a scene_tmd_stream PROT entry.
#[derive(Debug, Clone, Serialize)]
pub struct SubStream {
    /// Byte offset of this sub-stream's `chunk0` header within the entry.
    /// Always `0x800`-aligned in retail (sub-stream 0 is at `0`).
    pub base: usize,
    /// The parsed sub-stream. All of its offsets (`tmd_range`, `tail_end`,
    /// …) are **relative to [`Self::base`]** — add `base` for an absolute
    /// file offset.
    pub stream: SceneTmdStream,
}

/// Maximum sub-streams to enumerate before giving up — a runaway guard far
/// above the observed max (2).
const MAX_SUB_STREAMS: usize = 16;

/// Enumerate the concatenated `[chunk0 TMD][TIM chunks][terminator]`
/// sub-streams in a scene_tmd_stream entry. Returns one [`SubStream`] per
/// block; the first is the battle-init walk's reach (`FUN_8001FE70`), any
/// further ones are the "continuation" sub-streams (each with its **own**
/// leading TMD) that sit on the next `0x800` sector boundary after the
/// previous terminator.
///
/// Returns an empty `Vec` if the buffer isn't a scene_tmd_stream. The walk
/// stops at the first region that doesn't [`detect`] as a sub-stream (e.g.
/// trailing sector padding or unrelated data), so it never over-reads into
/// garbage.
pub fn sub_streams(buf: &[u8]) -> Vec<SubStream> {
    let mut out = Vec::new();
    let mut base = 0usize;
    while base + 8 <= buf.len() && out.len() < MAX_SUB_STREAMS {
        let Some(stream) = detect(&buf[base..]) else {
            break;
        };
        let end = base + stream.tail_end;
        out.push(SubStream { base, stream });
        // Next sub-stream begins after the terminator, past the zero padding
        // that aligns it to the next sector. Skip word-aligned zeros.
        let mut next = round_up_4(end);
        while next + 4 <= buf.len() && read_u32_le(buf, next) == Some(0) {
            next += 4;
        }
        if next <= base {
            break; // no forward progress — defensive
        }
        base = next;
    }
    out
}

/// Where a battle TIM chunk was found relative to the
/// `FUN_8001FE70`-walked tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum WalkSource {
    /// Inside `FUN_8001FE70`'s reach - this chunk is uploaded by the
    /// battle-init dispatch when the entry is loaded.
    Tail,
    /// After the first terminator. `FUN_8001FE70` exits before reaching
    /// these chunks; their consumer is not yet pinned, but the bytes are
    /// reachable as a continuation list (matching size + alignment to the
    /// in-tail chunks).
    Continuation,
}

/// One type-0x01 TIM upload chunk identified inside a scene_tmd_stream
/// entry. The walker emulates `FUN_8001FE70` (the battle-init scene
/// dispatch) on the streaming tail, then continues past the first
/// terminator to surface any continuation lists.
#[derive(Debug, Clone, Serialize)]
pub struct BattleTimChunk {
    /// Byte offset of the 4-byte chunk header (the `(type<<24)|size` word)
    /// within the buffer.
    pub header_offset: usize,
    /// Byte offset of the chunk payload (= `header_offset + 4`). Equals
    /// the file offset of the inner PSX TIM magic.
    pub payload_offset: usize,
    /// Payload byte length (from the chunk header's low-24-bit size field).
    pub payload_len: usize,
    /// Whether `FUN_8001FE70` would dispatch this chunk during battle init.
    pub source: WalkSource,
}

/// Walk a scene_tmd_stream buffer in the same shape as `FUN_8001FE70`
/// (the battle scene loader's per-PROT walker) and report every type-0x01
/// TIM upload chunk. Continuation chunks past the first terminator are
/// also reported with `source = WalkSource::Continuation` so engine
/// callers can opt into / out of uploading them.
///
/// Returns an empty `Vec` if the buffer doesn't match the scene_tmd_stream
/// shape. Use [`detect`] first if you want a cheaper structural gate.
pub fn battle_tim_chunks(buf: &[u8]) -> Vec<BattleTimChunk> {
    let Some(stream) = detect(buf) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut hit_first_terminator = false;

    // FUN_8001FE70 walks: advance by `(size & ~3) + 4` from the previous
    // chunk header, read new header, dispatch on type byte. The first
    // chunk it sees is the chunk0 header (= the leading TMD), so the
    // advance lands directly on `4 + tmd_size` (= `stream.tail_end` of the
    // in-tail walk we already ran via `detect`).
    walk_chunks(buf, 4 + stream.tmd_size, |off, header| {
        let size = (header & 0x00FF_FFFF) as usize;
        let type_byte = ((header >> 24) & 0xFF) as u8;
        if size == 0 {
            // Zero-size header = terminator. FUN_8001FE70 exits its
            // `while (uVar2 != 0)` test here.
            hit_first_terminator = true;
            return ChunkWalk::Stop;
        }
        if type_byte == 0x02 {
            // Type-0x02 = explicit terminator. FUN_8001FE70 sets uVar2=0
            // after reading this header.
            hit_first_terminator = true;
            return ChunkWalk::Stop;
        }
        if type_byte == 0x01 {
            let payload_offset = off + 4;
            if payload_offset + size <= buf.len() {
                out.push(BattleTimChunk {
                    header_offset: off,
                    payload_offset,
                    payload_len: size,
                    source: WalkSource::Tail,
                });
            }
        }
        ChunkWalk::Advance(size)
    });

    // Continuation: past the first terminator, scan word-aligned for the
    // same `(0x01<<24)|size` shape with the standard `[TIM magic at +4]`
    // gate. We don't walk chunks formally because the bytes between
    // terminator and the next list are zero padding (no chunk headers
    // anywhere in that gap). Matching by magic-of-payload is the cheapest
    // and most robust gate.
    if hit_first_terminator {
        let mut off = round_up_4(stream.tail_end);
        while off + 8 <= buf.len() {
            let header = match read_u32_le(buf, off) {
                Some(v) => v,
                None => break,
            };
            let type_byte = ((header >> 24) & 0xFF) as u8;
            let size = (header & 0x00FF_FFFF) as usize;
            if type_byte == 0x01 && size >= 32 && off + 4 + size <= buf.len() {
                let payload_magic = read_u32_le(buf, off + 4).unwrap_or(0);
                if payload_magic == 0x0000_0010 {
                    out.push(BattleTimChunk {
                        header_offset: off,
                        payload_offset: off + 4,
                        payload_len: size,
                        source: WalkSource::Continuation,
                    });
                    off += 4 + ((size + 3) & !3);
                    continue;
                }
            }
            off += 4;
        }
    }

    out
}

enum ChunkWalk {
    Advance(usize),
    Stop,
}

fn walk_chunks<F>(buf: &[u8], mut cur: usize, mut visit: F)
where
    F: FnMut(usize, u32) -> ChunkWalk,
{
    let mut step = 0usize;
    while step < MAX_CHUNKS && cur + 4 <= buf.len() {
        let Some(header) = read_u32_le(buf, cur) else {
            break;
        };
        match visit(cur, header) {
            ChunkWalk::Stop => break,
            ChunkWalk::Advance(size) => {
                cur = match cur.checked_add(4 + ((size + 3) & !3)) {
                    Some(v) => v,
                    None => break,
                };
            }
        }
        step += 1;
    }
}

fn round_up_4(v: usize) -> usize {
    (v + 3) & !3
}

fn read_u32_le(buf: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(buf.get(off..off + 4)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthesize a TMD-prefixed scene stream:
    /// [u32 chunk0_header = (0<<24)|tmd_body_size]
    /// [TMD magic / flags / nobj]
    /// [object table]
    /// [body padding zeros]
    /// [streaming chunks]
    /// [terminator]
    fn synth(nobj: u32, body_bytes: usize, chunks: &[(u8, &[u8])]) -> Vec<u8> {
        // body_bytes must be 4-aligned for the chunk0 header to land cleanly.
        assert!(
            body_bytes.is_multiple_of(4),
            "test-only synth requires 4-aligned body"
        );
        let tmd_body_size = 12 + 28 * nobj as usize + body_bytes;
        let mut buf = Vec::with_capacity(4 + tmd_body_size + 1024);
        // chunk0 header: type=0, size = TMD body bytes
        let chunk0_header = tmd_body_size as u32 & 0x00FFFFFF;
        buf.extend_from_slice(&chunk0_header.to_le_bytes());
        buf.extend_from_slice(&0x80000002u32.to_le_bytes()); // magic
        buf.extend_from_slice(&0u32.to_le_bytes()); // flags
        buf.extend_from_slice(&nobj.to_le_bytes());
        for _ in 0..nobj {
            buf.extend_from_slice(&[0u8; 28]);
        }
        buf.extend(std::iter::repeat_n(0u8, body_bytes));
        // Streaming chunks
        for (type_byte, payload) in chunks {
            let header = ((*type_byte as u32) << 24) | (payload.len() as u32 & 0x00FFFFFF);
            buf.extend_from_slice(&header.to_le_bytes());
            buf.extend_from_slice(payload);
            // Pad to 4-byte boundary.
            while !buf.len().is_multiple_of(4) {
                buf.push(0);
            }
        }
        // Terminator (low 24 bits zero).
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf
    }

    #[test]
    fn detects_minimal_synthetic() {
        let buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
        let r = detect(&buf).expect("should detect");
        assert_eq!(r.tmd_nobj, 2);
        // TMD body = 12 (header) + 2*28 (object table) + 64 (padding)
        assert_eq!(r.tmd_size, 12 + 28 * 2 + 64);
        assert_eq!(r.tmd_range(), 4..4 + r.tmd_size);
        assert_eq!(r.tail_chunks.len(), 1);
        assert!(matches!(r.tail_chunks[0].asset_type, AssetType::Tim));
        assert!(r.tail_terminated);
    }

    #[test]
    fn rejects_wrong_magic() {
        let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
        buf[4..8].copy_from_slice(&0x80000041u32.to_le_bytes()); // PSX-standard TMD
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_zero_nobj() {
        let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
        buf[12..16].copy_from_slice(&0u32.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_silly_nobj() {
        let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
        buf[12..16].copy_from_slice(&0x10000u32.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_first_u32_oob() {
        let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
        // Set a TMD body size that exceeds the file.
        let oob = (buf.len() as u32) + 0x1000;
        buf[0..4].copy_from_slice(&oob.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_nonzero_chunk0_type() {
        // chunk0 header type byte must be 0 (TIM dispatcher) - anything else
        // is a different streaming variant we're not detecting here.
        let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
        let mut hdr = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        hdr |= 0x02_000000; // type = 2 (TMD)
        buf[0..4].copy_from_slice(&hdr.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_first_u32_too_small() {
        let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
        buf[0..4].copy_from_slice(&8u32.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_unaligned_first_u32() {
        let mut buf = synth(2, 64, &[(0x00, &[0x10; 0x100])]);
        // Pick an unaligned TMD body size (low 24 bits).
        let unaligned: u32 = 129;
        buf[0..4].copy_from_slice(&unaligned.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_no_streaming_tail() {
        // [u32 size][bare TMD] then random bytes that don't form a streaming chunk.
        let buf = synth(2, 64, &[]);
        // Replace terminator with garbage so neither chunk-walk nor terminator catches.
        let len = buf.len();
        let mut buf = buf;
        buf[len - 4..].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
        // Type byte 0xDE is unknown and chunk size huge → no good chunks, no terminator.
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn accepts_terminator_only_tail() {
        // Streaming tail consisting solely of a terminator.
        let buf = synth(1, 0, &[]);
        let r = detect(&buf).expect("should detect terminator-only tail");
        assert!(r.tail_terminated);
        assert!(r.tail_chunks.is_empty());
    }

    #[test]
    fn accepts_truncated_tail() {
        // Build a stream then truncate before the terminator.
        let buf = synth(2, 16, &[(0x01, &[0u8; 0x40]), (0x02, &[0u8; 0x40])]);
        // Drop the trailing terminator bytes.
        let truncated = &buf[..buf.len() - 4];
        let r = detect(truncated).expect("truncated tail should still parse");
        assert_eq!(r.tail_chunks.len(), 2);
        assert!(!r.tail_terminated);
    }

    /// Build a "town01-shaped" two-list scene_tmd_stream: leading TMD,
    /// then `tail_count` type-0x01 TIM chunks, zero-size terminator,
    /// `gap` bytes of zero padding, then `cont_count` more type-0x01
    /// chunks after the terminator. Each TIM payload is a 16-byte stub
    /// starting with the PSX TIM magic so the continuation-list gate
    /// (which checks payload magic) recognises it.
    fn synth_two_list(tail_count: usize, gap: usize, cont_count: usize) -> Vec<u8> {
        // TIM payload: `[u32 0x10][12 bytes filler]` = 16 bytes.
        let tim_payload: Vec<u8> = {
            let mut v = Vec::with_capacity(16);
            v.extend_from_slice(&0x0000_0010u32.to_le_bytes());
            v.extend_from_slice(&[0u8; 12]);
            v
        };
        // The payload size we declare must clear the continuation gate's
        // `size >= 32` floor, even though the payload itself is only 16
        // bytes long. Pad the chunk body to 32 bytes.
        let chunk_body: Vec<u8> = {
            let mut v = Vec::with_capacity(32);
            v.extend_from_slice(&tim_payload);
            v.extend_from_slice(&[0u8; 16]);
            v
        };
        let tail_chunks: Vec<(u8, &[u8])> = (0..tail_count)
            .map(|_| (0x01, chunk_body.as_slice()))
            .collect();
        let mut buf = synth(2, 64, &tail_chunks);
        // Pad to gap.
        buf.extend(std::iter::repeat_n(0u8, gap));
        // Continuation chunks.
        for _ in 0..cont_count {
            let header = (0x01u32 << 24) | (chunk_body.len() as u32 & 0x00FF_FFFF);
            buf.extend_from_slice(&header.to_le_bytes());
            buf.extend_from_slice(&chunk_body);
        }
        buf
    }

    #[test]
    fn battle_tim_chunks_finds_in_tail() {
        let buf = synth_two_list(2, 0, 0);
        let chunks = battle_tim_chunks(&buf);
        assert_eq!(chunks.len(), 2);
        assert!(chunks.iter().all(|c| c.source == WalkSource::Tail));
        for c in &chunks {
            assert_eq!(c.payload_len, 32);
            assert_eq!(c.payload_offset, c.header_offset + 4);
        }
    }

    #[test]
    fn battle_tim_chunks_finds_continuation() {
        let buf = synth_two_list(2, 0x100, 2);
        let chunks = battle_tim_chunks(&buf);
        assert_eq!(chunks.len(), 4);
        let tail = chunks
            .iter()
            .filter(|c| c.source == WalkSource::Tail)
            .count();
        let cont = chunks
            .iter()
            .filter(|c| c.source == WalkSource::Continuation)
            .count();
        assert_eq!(tail, 2);
        assert_eq!(cont, 2);
    }

    #[test]
    fn battle_tim_chunks_empty_for_non_scene_stream() {
        let buf = vec![0u8; 1024];
        assert!(battle_tim_chunks(&buf).is_empty());
    }

    /// Concatenate two full `[TMD][TIM][TIM][terminator]` sub-streams with a
    /// zero-padded gap — the real "two-list" shape (each sub-stream carries
    /// its OWN leading TMD), distinct from the bare post-terminator TIM list
    /// `synth_two_list` models.
    fn synth_two_sub_streams(gap: usize) -> Vec<u8> {
        let tim: &[u8] = &{
            let mut v = vec![0u8; 64];
            v[0..4].copy_from_slice(&0x0000_0010u32.to_le_bytes());
            v
        };
        let mut buf = synth(2, 64, &[(0x01, tim), (0x01, tim)]);
        buf.extend(std::iter::repeat_n(0u8, gap));
        let second = synth(3, 32, &[(0x01, tim), (0x01, tim)]);
        buf.extend_from_slice(&second);
        buf
    }

    #[test]
    fn sub_streams_enumerates_concatenated_blocks() {
        let buf = synth_two_sub_streams(0x80);
        let subs = sub_streams(&buf);
        assert_eq!(subs.len(), 2, "should find both concatenated sub-streams");
        // Each sub-stream carries its OWN leading TMD with the expected nobj.
        assert_eq!(subs[0].base, 0);
        assert_eq!(subs[0].stream.tmd_nobj, 2);
        assert!(
            subs[1].base >= subs[0].stream.tail_end,
            "second block follows the first"
        );
        assert_eq!(subs[1].stream.tmd_nobj, 3);
        // Offsets are sub-stream-relative: the TMD always sits at +4.
        assert_eq!(subs[1].stream.tmd_range().start, 4);
    }

    #[test]
    fn sub_streams_single_block_when_no_continuation() {
        let buf = synth(2, 64, &[(0x01, &[0x10u8; 64])]);
        let subs = sub_streams(&buf);
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].base, 0);
    }

    #[test]
    fn sub_streams_empty_for_non_scene_stream() {
        assert!(sub_streams(&vec![0u8; 1024]).is_empty());
    }

    #[test]
    fn battle_tim_chunks_stops_at_type_02_terminator() {
        // A type-0x02 chunk also terminates FUN_8001FE70's tail walk;
        // chunks after it should NOT be reported as Tail.
        let mut buf = synth(2, 64, &[(0x01, &[0u8; 32]), (0x02, &[0u8; 0])]);
        // Append a stray type-0x01 chunk past the type-0x02 terminator.
        // No padding gap here, so it should be reachable by the
        // continuation pass.
        let header = (0x01u32 << 24) | 32u32;
        buf.extend_from_slice(&header.to_le_bytes());
        // payload: TIM magic + filler.
        buf.extend_from_slice(&0x0000_0010u32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 28]);
        let chunks = battle_tim_chunks(&buf);
        let tail: Vec<_> = chunks
            .iter()
            .filter(|c| c.source == WalkSource::Tail)
            .collect();
        let cont: Vec<_> = chunks
            .iter()
            .filter(|c| c.source == WalkSource::Continuation)
            .collect();
        assert_eq!(tail.len(), 1, "only the pre-type-0x02 chunk is in-tail");
        assert_eq!(
            cont.len(),
            1,
            "post-terminator chunk surfaces in continuation"
        );
    }
}
