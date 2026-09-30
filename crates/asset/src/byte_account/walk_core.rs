//! Walker plumbing (the claim sink) and the scene-bundle, streaming and slot-fill walkers.
//! Split out of `byte_account.rs`.

use super::*;

// ---------------------------------------------------------------------------
// Walkers
// ---------------------------------------------------------------------------

/// Accumulator every walker writes into.
pub(super) struct Sink {
    pub(super) claims: Vec<Claim>,
    pub(super) nested: Vec<Nested>,
    pub(super) notes: Vec<String>,
    pub(super) ambiguous_dumps: usize,
    pub(super) refuted_dumps: usize,
}

impl Sink {
    pub(super) fn new() -> Self {
        Self {
            claims: Vec::new(),
            nested: Vec::new(),
            notes: Vec::new(),
            ambiguous_dumps: 0,
            refuted_dumps: 0,
        }
    }
    pub(super) fn claim(
        &mut self,
        start: usize,
        end: usize,
        owner: &'static str,
        detail: impl Into<String>,
    ) {
        if end > start {
            self.claims.push(Claim::new(start, end, owner, detail));
        }
    }
    pub(super) fn note(&mut self, s: impl Into<String>) {
        self.notes.push(s.into());
    }
    pub(super) fn nest(
        &mut self,
        opts: &AccountOptions,
        depth: u8,
        origin: impl Into<String>,
        bytes: &[u8],
        walker: Walker,
    ) {
        if depth == 0 || bytes.is_empty() {
            return;
        }
        let origin = origin.into();
        let acc = run(
            bytes,
            origin.clone(),
            "nested".into(),
            walker,
            opts,
            depth - 1,
        );
        if self.nested.len() < opts.max_nested {
            self.nested.push(Nested {
                origin,
                account: acc,
            });
        }
    }
}

/// LZS-decode at `off`, claiming the compressed span. Returns the payload.
pub(super) fn take_lzs(
    sink: &mut Sink,
    buf: &[u8],
    off: usize,
    dec_size: usize,
    detail: &str,
) -> Option<Vec<u8>> {
    let src = buf.get(off..)?;
    match legaia_lzs::decompress_tracked(src, dec_size) {
        Ok((out, consumed)) => {
            sink.claim(off, off + consumed, OWNER_LZS, detail.to_string());
            Some(out)
        }
        Err(_) => None,
    }
}

/// Pick the walker for a bundle section from its type byte **and** its bytes.
///
/// The type byte says what kind of asset the section holds, not whether it
/// holds one or a [pack](crate::pack) of them, and the retail bundles use both:
/// a kingdom bundle's `TIM_LIST` section is a pack of atlases, a town bundle's
/// `TMD` section is one mesh. A pack read as a single asset leaves its offset
/// table and its inter-member slack in the residue, so the pack test runs
/// first for the two types that carry one.
pub(super) fn walker_for_section(type_byte: u8, payload: &[u8]) -> Walker {
    if matches!(
        AssetType::from_byte(type_byte),
        AssetType::Tim | AssetType::TimList | AssetType::Tmd | AssetType::Tmd2
    ) && walker_for_payload(payload) == Walker::Pack
    {
        return Walker::Pack;
    }
    walker_for_type(type_byte)
}

/// Pick the walker for a decoded payload from its asset type byte.
pub(super) fn walker_for_type(type_byte: u8) -> Walker {
    match AssetType::from_byte(type_byte) {
        AssetType::Tim | AssetType::TimList => Walker::Tim,
        AssetType::Tmd | AssetType::Tmd2 => Walker::Tmd,
        AssetType::Man => Walker::Man,
        // Type `0x05` is labelled MOVE by the dispatcher table but carries an
        // ANM clip bank, not a Tactical-Arts move table
        // (`docs/formats/world-map-overlay.md`, `legaia_asset::player_anm`).
        AssetType::Move | AssetType::Move2 => Walker::ClipBank,
        AssetType::Anm => Walker::Anm,
        AssetType::Mes => Walker::Mes,
        // VDF sections are the same `[u32 count][u32 byte_offset[count]]`
        // container as the clip bank, without the `0x080C` record header.
        AssetType::Vdf => Walker::OffsetPack,
        _ => Walker::Generic,
    }
}

// --- scene bundles ---------------------------------------------------------

pub(super) fn walk_scene_asset_table(
    buf: &[u8],
    sink: &mut Sink,
    opts: &AccountOptions,
    depth: u8,
) {
    let Some(r) = crate::scene_asset_table::resolve(buf) else {
        sink.note("scene_asset_table::resolve returned None");
        return;
    };
    let base = r.table_base;
    if base > 0 {
        // The prescript that precedes the sector-aligned table.
        if let Some(ranges) = crate::scene_scripted_asset_table::record_ranges(buf) {
            sink.claim(0, 4, OWNER_HEADER, "prescript count + first offset");
            for (i, (a, b)) in ranges.iter().enumerate() {
                sink.claim(*a, *b, OWNER_SCRIPT, format!("prescript record {i}"));
            }
        }
    }
    sink.claim(base, base + 8, OWNER_HEADER, "count + meta1");
    let count = r.table.count;
    sink.claim(
        base + 8,
        base + 8 + count * 8,
        OWNER_TOC,
        format!("{count} descriptors"),
    );
    for (i, d) in r.table.used().iter().enumerate() {
        let start = base + d.data_offset as usize;
        let detail = format!(
            "slot {i} type {:#04x} ({}) decoded {} B",
            d.type_byte,
            AssetType::from_byte(d.type_byte).name(),
            d.size
        );
        if let Some(out) = take_lzs(sink, buf, start, d.size as usize, &detail) {
            sink.nest(
                opts,
                depth,
                format!("slot {i} {}", AssetType::from_byte(d.type_byte).name()),
                &out,
                walker_for_section(d.type_byte, &out),
            );
        } else {
            sink.note(format!("{detail}: LZS decode failed"));
        }
    }
}

/// The same bundle, walked the way `FUN_80020224` walks it.
///
/// [`walk_scene_asset_table`] goes through the *detector*, whose count
/// allow-list is `4..=7` plus a MAN requirement below 6 - a classifier
/// heuristic, not a runtime rule. Retail reads the count word and loops
/// (`lw s3,0x0(s4)` / `blez s3`), so the count-1 and count-3 bundles this disc
/// also ships walk identically at runtime and had no walker here at all. Their
/// class is [`Class::LzsContainer`], whose own descriptor count is *fitted*
/// from a fixed list `{1,2,3,4,8,16}` that cannot even express the two count-5
/// entries - so the class figure was never the header's own count.
///
/// The claims are the same three kinds the scene-bundle walker makes, and the
/// payload extents are **measured** rather than inferred: a descriptor states
/// only the decompressed size, so the compressed span's end comes from what
/// `legaia_lzs::decompress_tracked` consumed.
pub(super) fn walk_descriptor_bundle(
    buf: &[u8],
    sink: &mut Sink,
    opts: &AccountOptions,
    depth: u8,
) {
    let Some(descriptors) = crate::scene_asset_table::descriptor_bundle_walk(buf) else {
        walk_lzs_container_orphan(buf, sink, opts, depth);
        return;
    };
    let count = descriptors.len();
    sink.claim(0, 8, OWNER_HEADER, "count + decompressed-size total");
    sink.claim(8, 8 + count * 8, OWNER_TOC, format!("{count} descriptors"));
    for (i, d) in descriptors.iter().enumerate() {
        let start = d.data_offset as usize;
        let ty = AssetType::from_byte(d.type_byte);
        let detail = format!(
            "slot {i} type {:#04x} ({}) decoded {} B",
            d.type_byte,
            ty.name(),
            d.size
        );
        let Some(out) = take_lzs(sink, buf, start, d.size as usize, &detail) else {
            sink.note(format!("{detail}: LZS decode failed"));
            continue;
        };
        // A `FLAG` slot is one the dispatcher answers with `type << 8` without
        // reading a byte (`docs/formats/asset-type.md`), and on this disc every
        // one of them decodes to the pochi fill file - the authoring tool wrote
        // its filler into the reserved descriptor. Account it as the filler it
        // is rather than sending 1927 bytes of ASCII to the generic walker.
        let walker = if crate::categorize::is_pochi_filler(&out) {
            Walker::PochiFiller
        } else {
            walker_for_section(d.type_byte, &out)
        };
        sink.nest(opts, depth, format!("slot {i} {}", ty.name()), &out, walker);
    }
}

/// Does this buffer open with an [offset pack](walk_offset_pack)?
///
/// The discriminating word is the **first offset**, not the count: a table of
/// `count` byte offsets puts member 0 immediately after itself, at
/// `4 + 4 * count`. A word-offset [`crate::pack`] would put it four times
/// further on, and an arbitrary pair of small integers almost never lands on
/// the identity exactly. That equality is the whole test, and it is why the
/// predicate can be used as a fallback without guessing.
pub(super) fn has_offset_pack_anchor(buf: &[u8]) -> bool {
    let Some(count) = legaia_bytes::u32_le(buf, 0).map(|c| c as usize) else {
        return false;
    };
    if count == 0 || 4 + count * 4 > buf.len() {
        return false;
    }
    legaia_bytes::u32_le(buf, 4).is_some_and(|off| off as usize == 4 + count * 4)
}

/// Three `lzs_container` entries are not descriptor bundles at all, because
/// that class never reads the header's count word - it *fits* a descriptor
/// count out of a fixed list, so any buffer whose first words happen to pass
/// the per-descriptor checks joins the class. This is where they land.
///
/// Two of the three are offset packs, one bare and one behind a DATA_FIELD
/// chunk header, and both are recovered from the anchor rather than from the
/// class ([`has_offset_pack_anchor`]). The third is a code image with a
/// leading string pool, which has no structural walker here and stays residue.
pub(super) fn walk_lzs_container_orphan(
    buf: &[u8],
    sink: &mut Sink,
    opts: &AccountOptions,
    depth: u8,
) {
    if has_offset_pack_anchor(buf) {
        sink.note("not a descriptor bundle - a bare offset pack");
        let members = walk_offset_pack(buf, sink, OWNER_RECORD, "member");
        nest_pack_members(buf, sink, opts, depth, &members);
        return;
    }
    // `[u32 (type << 24) | payload_len]` then the pack, the same wrapper
    // `prot::timpack` reads past for a `TIM_LIST` chunk
    // (`docs/formats/tim-pack.md`). The header's own length word has to agree
    // with the entry for the offset to mean anything.
    let header = legaia_bytes::u32_le(buf, 0).unwrap_or(0);
    let payload_len = (header & 0x00FF_FFFF) as usize;
    if payload_len >= 8
        && 4 + payload_len <= buf.len()
        && buf.len() >= 4
        && has_offset_pack_anchor(&buf[4..])
    {
        let ty = (header >> 24) as u8;
        sink.claim(
            0,
            4,
            OWNER_HEADER,
            format!(
                "chunk header type {:#04x} ({}), payload {payload_len} B",
                ty,
                AssetType::from_byte(ty).name()
            ),
        );
        let mut inner = Sink::new();
        let members = walk_offset_pack(&buf[4..], &mut inner, OWNER_RECORD, "member");
        for c in inner.claims {
            sink.claim(c.start + 4, c.end + 4, c.owner, c.detail);
        }
        for n in inner.notes {
            sink.note(n);
        }
        let shifted: Vec<_> = members.iter().map(|r| r.start + 4..r.end + 4).collect();
        nest_pack_members(buf, sink, opts, depth, &shifted);
        return;
    }
    sink.note("not a descriptor bundle and not an offset pack");
}

/// Account each member of a pack in its own right, picking the walker from the
/// member's own bytes.
pub(super) fn nest_pack_members(
    buf: &[u8],
    sink: &mut Sink,
    opts: &AccountOptions,
    depth: u8,
    members: &[std::ops::Range<usize>],
) {
    for (i, r) in members.iter().enumerate() {
        let Some(bytes) = buf.get(r.clone()) else {
            continue;
        };
        let walker = walker_for_payload(bytes);
        if walker != Walker::Generic {
            sink.nest(opts, depth, format!("member {i}"), bytes, walker);
        }
    }
}

/// A pochi filler slot: the fill file, then the mastering buffer's leftovers.
///
/// Both claims are `pad` - a filler slot carries no content by construction -
/// but they are separate claims because they are two different things, and the
/// residue classifier would otherwise rank 266 sectors of dev fill as work: the
/// fill is text-shaped, and `repeated_fill` only tests periods 1/2/4/8/16 while
/// the pochi line is 52 bytes long.
///
/// See [`docs/formats/pochi.md`](../../../docs/formats/pochi.md) for what pins
/// the tail: it is byte-identical to some other entry's bytes at the same file
/// offset, in all 266 slots.
pub(super) fn walk_pochi_filler(buf: &[u8], sink: &mut Sink) {
    let fill_end = crate::categorize::POCHI_FILL_LEN.min(buf.len());
    sink.claim(
        0,
        fill_end,
        OWNER_PAD,
        "pochi fill file, through the EOF byte",
    );
    if buf.len() > fill_end {
        sink.claim(
            fill_end,
            buf.len(),
            OWNER_PAD,
            "sector tail - the mastering buffer's prior contents, not fill",
        );
    }
}

/// One of the two headerless 16bpp stills, claimed as the four bands the
/// consumer uploads ([`crate::ringside_still`]).
///
/// Claiming four bands rather than one buffer is the point: the band size is
/// what the seek stride and the `LoadImage` rectangle independently agree on,
/// so a still that is the wrong length leaves the shortfall in the residue
/// instead of being absorbed by a whole-file claim.
pub(super) fn walk_ringside_still(buf: &[u8], sink: &mut Sink) {
    use crate::ringside_still as still;
    if !still::has_still_shape(buf) {
        sink.note(format!(
            "not {} bytes - the four {}-byte band uploads do not tile this buffer",
            still::ENTRY_BYTES,
            still::BAND_BYTES
        ));
        return;
    }
    for i in 0..still::BAND_COUNT {
        let span = still::band_span(i).expect("band index inside BAND_COUNT");
        let (x, y, w, h) = still::band_rect(i).expect("band index inside BAND_COUNT");
        sink.claim(
            span.start,
            span.end,
            OWNER_TEXTURE,
            format!("band {i} -> LoadImage rect ({x},{y}) {w}x{h}"),
        );
    }
}

// --- DATA_FIELD / streaming variants --------------------------------------

pub(super) fn walk_stream(buf: &[u8], sink: &mut Sink, opts: &AccountOptions, depth: u8) {
    // `[u32 size][bare TMD][chunks]` variant first - its leading chunk has no
    // typed header the generic walker would recognise.
    if let Some(s) = crate::scene_tmd_stream::detect(buf) {
        sink.claim(0, 4, OWNER_HEADER, "chunk0 header (bare TMD size)");
        sink.claim(
            4,
            4 + s.tmd_size,
            OWNER_TMD,
            format!("leading TMD, {} objects", s.tmd_nobj),
        );
        for (i, c) in s.tail_chunks.iter().enumerate() {
            let end = c.offset + 4 + ((c.size as usize) & !3);
            sink.claim(c.offset, c.offset + 4, OWNER_HEADER, format!("chunk {i}"));
            sink.claim(
                c.offset + 4,
                end,
                payload_owner(c.asset_type),
                format!("chunk {i} {}", c.asset_type.name()),
            );
            if let Some(p) = buf.get(c.offset + 4..end) {
                sink.nest(
                    opts,
                    depth,
                    format!("chunk {i} {}", c.asset_type.name()),
                    p,
                    walker_for_payload(p),
                );
            }
        }
        if s.tail_terminated {
            sink.claim(s.tail_end - 4, s.tail_end, OWNER_HEADER, "terminator");
            claim_last_sector_slack(buf, sink, s.tail_end, "slack past the terminator");
        }
        return;
    }
    let Ok(rep) = parse_streaming(buf, 8192) else {
        sink.note("parse_streaming failed");
        return;
    };
    if rep.chunks.is_empty() {
        sink.note("no streaming chunks parsed");
        return;
    }
    for (i, c) in rep.chunks.iter().enumerate() {
        let start = c.header_offset;
        let end = start + 4 + ((c.size as usize) & !3);
        sink.claim(start, start + 4, OWNER_HEADER, format!("chunk {i} header"));
        let t = AssetType::from_byte(c.type_byte);
        let owner = buf
            .get(start + 4..end.min(buf.len()))
            .map_or_else(|| payload_owner(t), |p| payload_owner_of(t, p));
        sink.claim(
            start + 4,
            end,
            owner,
            format!("chunk {i} {} ({} B)", c.type_name, c.size),
        );
        if let Some(p) = buf.get(start + 4..end.min(buf.len())) {
            sink.nest(
                opts,
                depth,
                format!("chunk {i} {}", c.type_name),
                p,
                walker_for_payload(p),
            );
        }
    }
    if rep.terminated {
        sink.claim(
            rep.bytes_consumed - 4,
            rep.bytes_consumed,
            OWNER_HEADER,
            "terminator",
        );
        claim_last_sector_slack(buf, sink, rep.bytes_consumed, "slack past the terminator");
    } else {
        sink.note(format!(
            "stream unterminated after {} chunks ({} B consumed)",
            rep.chunks.len(),
            rep.bytes_consumed
        ));
    }
}

pub(super) fn payload_owner(t: AssetType) -> &'static str {
    match t {
        AssetType::Tim | AssetType::TimList => OWNER_TIM,
        AssetType::Tmd | AssetType::Tmd2 => OWNER_TMD,
        AssetType::Anm => OWNER_ANM,
        AssetType::Man => OWNER_SCRIPT,
        _ => OWNER_RECORD,
    }
}

/// The owner for a chunk payload, preferring the payload's **own** magic over
/// the chunk header's type byte.
///
/// The two disagree on this disc: the standalone BGM streams carry their SEQ
/// behind a type-`0x02` header, which [`payload_owner`] would read as a TMD
/// and label `tmd`. The type byte selects the runtime's handler; the magic
/// says what the bytes are, and an owner names what the bytes are.
pub(super) fn payload_owner_of(t: AssetType, payload: &[u8]) -> &'static str {
    match legaia_bytes::u32_le(payload, 0) {
        Some(0x0000_0010) => OWNER_TIM,
        Some(0x8000_0002) => OWNER_TMD,
        Some(0x5641_4270) => OWNER_VAB,
        _ if payload.starts_with(b"pQES") => OWNER_SEQ,
        _ => payload_owner(t),
    }
}

/// Pick a walker for a payload from its own magic.
pub(super) fn walker_for_payload(buf: &[u8]) -> Walker {
    match legaia_bytes::u32_le(buf, 0) {
        Some(0x0000_0010) => Walker::Tim,
        Some(0x8000_0002) => Walker::Tmd,
        Some(0x5641_4270) => Walker::Vab,
        _ if buf.starts_with(b"pQES") => Walker::Seq,
        // A `TIM_LIST` / `TMD` chunk's payload is often a pack rather than a
        // single asset, and a pack's head word is a count with no magic - so
        // without this the payload fell to `Generic` and its members were
        // found only by the magic sweep, i.e. `accounted` near 100 % with
        // `structural` at 0. The pack anchor is checked before claiming it.
        _ if crate::pack::parse_pack(buf)
            .is_ok_and(|e| e.first().is_some_and(|f| f.byte_offset == 4 + 4 * e.len())) =>
        {
            Walker::Pack
        }
        _ => Walker::Generic,
    }
}

// --- fixed-stride streaming slots -----------------------------------------

/// Claim the trailing fill of one fixed-stride streaming slot as [`OWNER_PAD`].
///
/// Three archives on this disc are a flat array of fixed-size slots that the
/// runtime transfers **whole**, content length or not:
///
/// - the monster archive (`0867`), `0x14000` per slot: the battle loader
///   `FUN_800542C8` seeks `(id-1) * 0x14000` bytes (`sll v0,v1,0x2; addu
///   v0,v0,v1; sll v0,v0,0xe` at `0x80054524`) and reads `0x28` sectors
///   (`li a1,0x28` at `0x80054608`, `jal 0x8003E800`), then hands the LZS
///   decoder `slot + 4` - so the decoder stops at its own terminator and the
///   rest of the transferred window is never interpreted.
/// - `summon.dat` / `readef.DAT` (`0893` / `0894`), `0x10800` per slot: the
///   streaming SM `FUN_801F17F8` seeks `slot * 33 * 0x800` (`sll a1,v0,0x5;
///   addu a1,a1,v0; sll a1,a1,0xb` at `0x801F1948`) and reads `0x10800` bytes
///   (`lui a2,0x1; ori a2,a2,0x800` at `0x801F1958`/`0x801F1970` ->
///   `FUN_800559EC`, which divides by `0x800` for the sector count).
///
/// Every one of the three file extents is an exact multiple of its stride, so
/// the slot boundary is a declared bound rather than an inferred one, and the
/// bytes between a slot's content and that bound are the [`OWNER_PAD`]
/// definition verbatim - the same reading the multi-bank VAB's sector slack
/// gets.
///
/// The claim starts where the fill starts, not where the walker stopped: only
/// the slot's maximal all-zero **suffix** is claimed. That is the guard rail.
/// Claiming the whole gap unconditionally would absorb a walker that stopped
/// early inside real content, and the instrument would gain percentage points
/// by redefining itself instead of by reading the disc.
pub(super) fn claim_slot_fill(
    buf: &[u8],
    sink: &mut Sink,
    start: usize,
    end: usize,
    detail: String,
) {
    let Some(slot) = buf.get(start..end.min(buf.len())) else {
        return;
    };
    let mut fill = slot.len();
    while fill > 0 && slot[fill - 1] == 0 {
        fill -= 1;
    }
    // An entirely-zero slot is not a tail; leave it visible as residue.
    if fill == 0 || fill == slot.len() {
        return;
    }
    sink.claim(start + fill, start + slot.len(), OWNER_PAD, detail);
}

/// Shortest internal all-zero run that gets its own residue entry.
pub(super) const FILL_SPLIT_BYTES: usize = 2048;

/// Cut a residue run wherever a sector or more of fill sits inside it.
///
/// A residue run's boundaries are drawn by the *claims* around it, so a region
/// that is one kilobyte of content followed by a hundred kilobytes of fill
/// arrives as a single run - and the shape vocabulary then has to name the
/// whole thing with one word. It picks `ascii_text`, because that test counts
/// NUL as printable and one non-zero byte disqualifies `zero_pad`, so the fill
/// lands in `work_bytes` as if it were an unwalked string pool. Entry `0970`'s
/// 131172-byte hole did exactly that.
///
/// Splitting is not reclassifying: each piece still gets whatever shape its own
/// bytes earn, and the total residue is unchanged. It only stops one run from
/// being two findings glued together. The bound is a sector, so inter-record
/// zeros stay attached to the run they belong to.
pub(super) fn split_off_fill(buf: &[u8], gaps: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(gaps.len());
    for (a, b) in gaps {
        let Some(s) = buf.get(a..b) else {
            out.push((a, b));
            continue;
        };
        let mut cut = a;
        let mut i = 0usize;
        while i < s.len() {
            if s[i] != 0 {
                i += 1;
                continue;
            }
            let mut j = i;
            while j < s.len() && s[j] == 0 {
                j += 1;
            }
            if j - i >= FILL_SPLIT_BYTES {
                if a + i > cut {
                    out.push((cut, a + i));
                }
                out.push((a + i, a + j));
                cut = a + j;
            }
            i = j;
        }
        if b > cut {
            out.push((cut, b));
        }
    }
    out
}

/// Claim what is left of a PROT entry's **last sector** past a declared end.
///
/// A PROT entry's extent is sector-granular ([`prot.md`](https://andrewaltimit.github.io/legend-of-legaia-re/formats/prot.html):
/// `toc[p+3] - toc[p+2]`) while the container inside it declares its own end -
/// a stream terminator, a length word. What lies between the two is the
/// builder's sector buffer, and nothing addresses it: the reader stops at the
/// declared end and the next entry starts at the next sector.
///
/// The bound is deliberately one sector. A remainder of a sector or more is a
/// second region, not slack, and stays residue so it keeps ranking as work.
pub(super) fn claim_last_sector_slack(
    buf: &[u8],
    sink: &mut Sink,
    end: usize,
    detail: &'static str,
) {
    const SECTOR: usize = 2048;
    if end < buf.len() && buf.len() - end < SECTOR {
        sink.claim(end, buf.len(), OWNER_PAD, detail);
    }
}
