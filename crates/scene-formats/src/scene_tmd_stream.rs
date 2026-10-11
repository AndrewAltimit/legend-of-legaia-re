//! "TMD-prefixed scene-stream" detector - a streaming-format variant
//! that opens with a bare Legaia TMD instead of a typed chunk header.
//!
//! ### Layout (empirically verified across all 182 matching PROT entries)
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
//! ### One entry, one sub-stream (the falsified "two-list" shape)
//!
//! A scene_tmd_stream PROT entry holds **exactly one** complete
//! `[chunk0 TMD][type-0x01 TIM chunks][terminator]` stream, followed by zero
//! padding out to the entry's sector-aligned end. `FUN_8001FE70` walks that
//! one stream and **returns a pointer just past its terminator**
//! (`return param_1 + 1`); the one static caller (`FUN_800513F0`, battle init)
//! calls it once, which is all an entry contains.
//!
//! Earlier revisions of this module described a "concatenated sub-streams" /
//! "two-list" shape, with `0006_town01.BIN` carrying a second sub-stream at
//! `0x14000` (own TMD `0x2C20`, TIMs at `0x16C24` / `0x1EE48`). **That reading
//! is falsified.** It was an artifact of the superseded PROT entry-size
//! expression, which over-read every entry into its successor's head bytes.
//! Entry size is the sector gap to the next entry (`toc[p+3] - toc[p+2]`,
//! retail's own `FUN_8003E68C`) - see `docs/formats/prot.md`. Entry 0006 is
//! exactly `0x14000` bytes, and the "second sub-stream" is PROT entry **0007**,
//! whose TOC start is exactly `0x14000` past entry 0006's: its own leading TMD
//! is `0x2C20` and its own tail TIMs sit at `0x2C24` / `0xAE48` - the stale
//! offsets minus `0x14000`. The town0b / town0c clusters that appeared to
//! "confirm" the shape are four-entry clusters of the same layout, so each
//! over-read simply reproduced the artifact.
//!
//! Across the corrected corpus **no** entry enumerates more than one
//! sub-stream and **no** entry yields a `WalkSource::Continuation` chunk.
//! [`sub_streams`] and the `Continuation` arm are retained as regression
//! detectors: if extraction ever regresses to an over-reading size expression,
//! they light up again.
//!
//! This shape is dominant in scene-asset PROT entries (most `town*`, `dolk*`,
//! `rugi*`, and similar named blocks). Pre-TOC-fix the bare-TMD prefix made
//! many of these look like "low-entropy unknowns" because the inner streaming
//! header was 8824+ bytes deep - the standard streaming detector starts at
//! offset 0 and saw a non-streaming first chunk (`type_byte = 0x00` with
//! TMD-magic content).

use serde::Serialize;

use legaia_container_formats::AssetType;

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
    let tmd_magic = legaia_bytes::u32_le(buf, 4)?;
    if tmd_magic != 0x80000002 {
        return None;
    }

    // (2) TMD on-disc flags must be zero (post-fixup is 1, on-disc is 0).
    let tmd_flags = legaia_bytes::u32_le(buf, 8)?;
    if tmd_flags != 0 {
        return None;
    }

    // (3) Object count must be a small positive number.
    let nobj = legaia_bytes::u32_le(buf, 12)?;
    if nobj == 0 || nobj > MAX_TMD_OBJECTS {
        return None;
    }

    // (4) The chunk0 header packs `(type<<24) | size`, where the high byte
    //     is 0 (TIM dispatcher) and the low 24 bits give the TMD body size.
    //     Reject if the type byte isn't 0 - that would mean a different
    //     dispatcher fires on the leading chunk and this isn't the variant
    //     we're trying to detect.
    let chunk0_header = legaia_bytes::u32_le(buf, 0)?;
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
        let header = match legaia_bytes::u32_le(buf, cur) {
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

/// Cheap presence check - used by [`legaia_disc_audit::categorize`] before doing the
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
    /// …) are **relative to [`Self::base`]** - add `base` for an absolute
    /// file offset.
    pub stream: SceneTmdStream,
}

/// Maximum sub-streams to enumerate before giving up - a runaway guard far
/// above the observed max (2).
const MAX_SUB_STREAMS: usize = 16;

/// Enumerate the `[chunk0 TMD][TIM chunks][terminator]` sub-streams in a
/// scene_tmd_stream entry. On a correctly-sized entry this always returns
/// **exactly one** [`SubStream`], based at 0 - the battle-init walk's reach
/// (`FUN_8001FE70`).
///
/// The function keeps looking past the terminator (skipping the zero padding
/// that runs to the entry's sector-aligned end) because a *second* hit means
/// the buffer spans more than one PROT entry - i.e. it was produced by the
/// superseded over-reading entry-size expression. See the module docs; this
/// is a regression detector, not a retail shape.
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
        while next + 4 <= buf.len() && legaia_bytes::u32_le(buf, next) == Some(0) {
            next += 4;
        }
        if next <= base {
            break; // no forward progress - defensive
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
    /// After the first terminator, where a correctly-sized entry has only
    /// zero padding. **No retail PROT entry produces this** - every chunk
    /// past the terminator that the old corpus reported belonged to the
    /// *next* entry, reached through the superseded over-reading entry-size
    /// expression (see the module docs). Retained as a regression detector:
    /// a `Continuation` hit means the buffer spans more than one entry.
    Continuation,
}

/// One type-0x01 TIM upload chunk identified inside a scene_tmd_stream
/// entry. The walker emulates `FUN_8001FE70` (the battle-init scene
/// dispatch) on the streaming tail, then continues past the first
/// terminator so an over-read buffer's spill-over is visible rather than
/// silently attributed to this entry.
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
/// TIM upload chunk. On a correctly-sized entry every hit is
/// `WalkSource::Tail`; anything reported past the first terminator carries
/// `WalkSource::Continuation` and means the buffer spans more than one PROT
/// entry (see [`WalkSource::Continuation`]).
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
            let header = match legaia_bytes::u32_le(buf, off) {
                Some(v) => v,
                None => break,
            };
            let type_byte = ((header >> 24) & 0xFF) as u8;
            let size = (header & 0x00FF_FFFF) as usize;
            if type_byte == 0x01 && size >= 32 && off + 4 + size <= buf.len() {
                let payload_magic = legaia_bytes::u32_le(buf, off + 4).unwrap_or(0);
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
        let Some(header) = legaia_bytes::u32_le(buf, cur) else {
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

// ---------------------------------------------------------------------------
// Backdrop shell shape
// ---------------------------------------------------------------------------

/// Which way a battle-stage backdrop shell faces open.
///
/// A `scene_tmd_stream` entry's object 0 is the stage shell - sky dome +
/// distant mountain ring + far ground ring - and retail authors it as a
/// **half** shell hugging one side of the `X = 0` or `Z = 0` plane. The
/// half is complete as authored; nothing is truncated on the way in. What
/// closes the circle is a **second draw of the same mesh** under a
/// per-stage transform, which is why this measurement names the authored
/// half rather than a missing one. See
/// [`battle_backdrop`](crate::battle_backdrop) for the placement, and
/// `docs/subsystems/battle.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum OpenSide {
    NegX,
    PosX,
    NegZ,
    PosZ,
}

impl OpenSide {
    /// Axis-and-sign label, e.g. `"-X"`.
    pub fn label(self) -> &'static str {
        match self {
            OpenSide::NegX => "-X",
            OpenSide::PosX => "+X",
            OpenSide::NegZ => "-Z",
            OpenSide::PosZ => "+Z",
        }
    }
}

/// The measured shape of a backdrop shell's vertex pool.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ShellShape {
    /// Inclusive min corner of the measured vertex pool, `[x, y, z]`.
    pub min: [i32; 3],
    /// Inclusive max corner of the measured vertex pool, `[x, y, z]`.
    pub max: [i32; 3],
    /// The thin side: the direction the shell is open toward.
    pub open: OpenSide,
    /// Share of that axis's extent lying on the open side of the plane.
    /// `0.0` = the pool never crosses the plane at all; `0.5` = perfectly
    /// symmetric (i.e. not a half shell).
    pub open_fraction: f32,
}

/// Upper bound on [`ShellShape::open_fraction`] for a pool to read as
/// half-authored rather than as a full surround.
///
/// Retail leaves this test unambiguous: measured over object 0 of all 182
/// `scene_tmd_stream` entries the largest observed value is `0.079`, and the
/// nearest thing to a counterexample is still under a twelfth of the extent.
/// The threshold sits well above the observed maximum and well below the
/// `0.5` a symmetric pool would score.
pub const HALF_SHELL_MAX_OPEN_FRACTION: f32 = 0.10;

impl ShellShape {
    /// Whether the pool is authored against one side of `X = 0` / `Z = 0`.
    pub fn is_half_shell(&self) -> bool {
        self.open_fraction <= HALF_SHELL_MAX_OPEN_FRACTION
    }

    /// One-line human description for a viewer status line.
    ///
    /// Names the **completion**, not a missing half: retail draws this mesh
    /// twice, and the second copy fills the open side. A viewer that showed
    /// only the authored half and labelled it "half shell" has repeatedly
    /// been read as a broken parse - and, once the second copy was known,
    /// as a viewer bug. Callers that can resolve the per-stage transform
    /// from `SCUS_942.54` should prefer
    /// [`battle_backdrop::describe_placement`], which names which of the
    /// two it is.
    ///
    /// [`battle_backdrop::describe_placement`]: crate::battle_backdrop::describe_placement
    pub fn describe(&self) -> String {
        if self.is_half_shell() {
            format!(
                "battle-stage backdrop (drawn twice; authored half open toward {})",
                self.open.label()
            )
        } else {
            "battle-stage backdrop (full surround)".to_string()
        }
    }
}

/// Share of an axis extent lying on the thinner side of the `0` plane.
/// `0.0` when the span never crosses it, `0.5` when it straddles evenly.
fn open_fraction(lo: i32, hi: i32) -> f32 {
    let extent = (hi - lo) as f32;
    if extent <= 0.0 {
        return 0.5;
    }
    let below = (-lo).max(0) as f32;
    let above = hi.max(0) as f32;
    below.min(above) / extent
}

/// Classify a backdrop TMD's shell shape from its **object 0** vertex pool -
/// the shell retail actually draws (`FUN_800513F0` registers the TMD and
/// links one background actor; the trailing objects are near props / ground
/// ribbons that stay off-camera).
///
/// Returns `None` when the TMD has no object 0 or object 0 has no vertices.
pub fn shell_shape(tmd: &legaia_tmd::Tmd) -> Option<ShellShape> {
    shell_shape_of(tmd.objects.first()?.vertices.iter())
}

/// [`shell_shape`] over the TMD's **whole** vertex pool (every object), for
/// callers that want the entry-wide extent rather than the drawn shell's.
pub fn shell_shape_all_objects(tmd: &legaia_tmd::Tmd) -> Option<ShellShape> {
    shell_shape_of(tmd.objects.iter().flat_map(|o| o.vertices.iter()))
}

fn shell_shape_of<'a>(verts: impl Iterator<Item = &'a legaia_tmd::Vector>) -> Option<ShellShape> {
    let (mut min, mut max) = ([i32::MAX; 3], [i32::MIN; 3]);
    let mut any = false;
    for v in verts {
        any = true;
        for (i, c) in [v.x as i32, v.y as i32, v.z as i32].into_iter().enumerate() {
            min[i] = min[i].min(c);
            max[i] = max[i].max(c);
        }
    }
    if !any {
        return None;
    }
    let fx = open_fraction(min[0], max[0]);
    let fz = open_fraction(min[2], max[2]);
    // Pick the thinner axis; on that axis the open side is whichever end of
    // the span sits nearer the plane.
    let open = if fx <= fz {
        if (-min[0]).max(0) <= max[0].max(0) {
            OpenSide::NegX
        } else {
            OpenSide::PosX
        }
    } else if (-min[2]).max(0) <= max[2].max(0) {
        OpenSide::NegZ
    } else {
        OpenSide::PosZ
    };
    Some(ShellShape {
        min,
        max,
        open,
        open_fraction: fx.min(fz),
    })
}

#[cfg(test)]
mod tests;
