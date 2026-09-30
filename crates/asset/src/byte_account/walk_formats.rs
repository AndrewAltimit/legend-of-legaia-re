//! Small fixed-format and leaf sub-asset walkers.
//! Split out of `byte_account.rs`.

use super::*;

// --- small fixed formats ---------------------------------------------------

pub(super) fn walk_bse_bank(buf: &[u8], sink: &mut Sink) {
    let Some(b) = crate::bse_bank::detect(buf) else {
        sink.note("bse_bank::detect returned None");
        return;
    };
    sink.claim(0, b.body_offset, OWNER_HEADER, "tag + body offset");
    sink.claim(
        b.body_offset,
        b.body_offset + b.records * crate::bse_bank::RECORD_BYTES,
        OWNER_RECORD,
        format!("{} 8-byte records", b.records),
    );
}

pub(super) fn walk_init_pak(buf: &[u8], sink: &mut Sink, opts: &AccountOptions, depth: u8) {
    match crate::init_pak::parse(buf) {
        Ok(p) => {
            for (i, l) in p.logos.iter().enumerate() {
                sink.claim(
                    l.file_offset,
                    l.file_offset + l.byte_len,
                    OWNER_TIM,
                    format!("publisher logo {i}"),
                );
            }
        }
        Err(e) => sink.note(format!("init_pak::parse: {e}")),
    }
    // `init.pak` is BOTH: a boot overlay with a static-overlay row and a
    // five-TIM logo pack. The dump corpus is the parser for the code half, so
    // run it here rather than letting the overlay-row override in
    // `pick_walker` replace this walker - that override used to drop every
    // logo claim whenever `--funcs` was given, which is exactly when the
    // sweep runs, so the pack's own bytes read as unwalked format.
    if opts.funcs_dir.is_some() {
        walk_overlay_code(buf, sink, opts);
    } else {
        sink.note("non-TIM region is the boot overlay's code; pass --funcs to credit it");
    }
    let _ = depth;
}

pub(super) fn walk_field_map(buf: &[u8], sink: &mut Sink) {
    use crate::field_map as fm;
    if fm::detect(buf).is_none() {
        sink.note("field_map::detect returned None");
        return;
    }
    sink.claim(
        fm::OBJECT_RECORDS_OFFSET,
        fm::OBJECT_RECORDS_OFFSET + fm::OBJECT_RECORDS_BYTES,
        OWNER_RECORD,
        format!("{} object descriptors", fm::OBJECT_RECORD_COUNT),
    );
    sink.claim(
        fm::COLLISION_GRID_OFFSET,
        fm::COLLISION_GRID_OFFSET + fm::COLLISION_GRID_BYTES,
        OWNER_GRID,
        "collision + floor grid",
    );
    sink.claim(
        fm::OBJECT_GRID_OFFSET,
        fm::OBJECT_GRID_OFFSET + fm::OBJECT_GRID_BYTES,
        OWNER_GRID,
        "per-tile object index",
    );
    sink.claim(
        fm::TRIGGER_BLOCK_OFFSET,
        fm::TRIGGER_BLOCK_OFFSET + fm::TRIGGER_BLOCK_BYTES,
        OWNER_RECORD,
        "trigger block",
    );
    sink.note(
        "a fixed-layout region file accounts to 100% by construction - the figure \
         says the regions are written down, not that their fields are pinned",
    );
}

pub(super) fn walk_scene_v12(buf: &[u8], sink: &mut Sink) {
    let Some(t) = crate::scene_v12_table::detect(buf) else {
        sink.note("scene_v12_table::detect returned None");
        return;
    };
    sink.claim(
        0,
        crate::scene_v12_table::RECORDS_OFFSET,
        OWNER_HEADER,
        "8-word v12 header",
    );
    let n = t.records.len();
    sink.claim(
        crate::scene_v12_table::RECORDS_OFFSET,
        crate::scene_v12_table::RECORDS_OFFSET + n * 8,
        OWNER_RECORD,
        format!("{n} inline trigger records"),
    );
}

pub(super) fn walk_scene_event_scripts(buf: &[u8], sink: &mut Sink) {
    // The walker is selected by the entry's class, so the entry has already
    // been placed as a prescript; the standalone record-count floor exists for
    // context-free buffers only. `edteien` (PROT 0780) holds two records and
    // is the one carrier the floor rejects - the positional read is the one
    // `scene_v12_table` and the engine's scene loader use for it.
    let ranges = crate::scene_event_scripts::record_ranges(buf).or_else(|| {
        let r = crate::scene_event_scripts::record_ranges_positional(buf)?;
        sink.note(format!(
            "{} record(s): below the standalone count floor, read positionally",
            r.len()
        ));
        Some(r)
    });
    let Some(ranges) = ranges else {
        sink.note("scene_event_scripts::record_ranges returned None");
        return;
    };
    let n = ranges.len();
    sink.claim(0, 2 + n * 2, OWNER_TOC, format!("{n} record offsets"));
    for (i, (a, b)) in ranges.iter().enumerate() {
        sink.claim(*a, *b, OWNER_SCRIPT, format!("stager record {i}"));
    }
}

/// The runtime `efect.dat` 2-pack (PROT `0873`).
///
/// Not the magic-prefixed [effect bundle](crate::effect_bundle) - a headerless
/// file whose first two words are its two packs' offsets, with the sprite
/// atlas inline between the header and pack 0
/// ([`crate::efect_pack`]). The class had a walker slot and no walker behind
/// it, so the whole 8 KB read as unwalked format.
///
/// Both packs address their members by **absolute file offset**, so a member
/// runs to the next offset in its own table and the last to its pack's extent -
/// pack 0's being pack 1's start, not the file's end.
pub(super) fn walk_efect_dat(buf: &[u8], sink: &mut Sink) {
    let Some(p) = crate::efect_pack::detect(buf) else {
        sink.note("efect_pack::detect returned None");
        return;
    };
    sink.claim(0, 8, OWNER_HEADER, "pack0 + pack1 offsets");
    if p.atlas_entries > 0 {
        sink.claim(
            8,
            p.pack0_offset,
            OWNER_RECORD,
            format!("{} sprite-atlas entries", p.atlas_entries),
        );
    }
    for (pack, at, count, limit, owner, what) in [
        (
            0usize,
            p.pack0_offset,
            p.pack0_count,
            p.pack1_offset,
            OWNER_ANM,
            "frame batch",
        ),
        (
            1,
            p.pack1_offset,
            p.pack1_count,
            buf.len(),
            OWNER_SCRIPT,
            "spawn script",
        ),
    ] {
        sink.claim(
            at,
            at + 4 + 4 * count,
            OWNER_TOC,
            format!("pack {pack}: {count} absolute offsets"),
        );
        for i in 0..count {
            let Some(start) = legaia_bytes::u32_le(buf, at + 4 + 4 * i).map(|v| v as usize) else {
                break;
            };
            let end = legaia_bytes::u32_le(buf, at + 8 + 4 * i)
                .map(|v| v as usize)
                .filter(|_| i + 1 < count)
                .unwrap_or(limit)
                .min(buf.len());
            if end > start {
                sink.claim(start, end, owner, format!("pack {pack} {what} {i}"));
            }
        }
    }
}

pub(super) fn walk_effect_bundle(buf: &[u8], sink: &mut Sink) {
    let Some(e) = crate::effect_bundle::detect(buf) else {
        sink.note("effect_bundle::detect returned None");
        return;
    };
    sink.claim(
        e.magic_offset,
        e.table_offset,
        OWNER_HEADER,
        "magic + header",
    );
    sink.claim(
        e.table_offset,
        e.table_offset + crate::effect_bundle::TABLE_SIZE,
        OWNER_TOC,
        format!("{}-slot schema", crate::effect_bundle::RECORD_COUNT),
    );
    for s in &e.slots {
        let start = e.magic_offset + s.offset as usize;
        if let Some(size) = s.size {
            sink.claim(start, start + size as usize, OWNER_RECORD, "effect slot");
        }
    }
}

/// `prot::timpack`: `[2 header bytes][u32 tim_num][i32 word_offsets]`, each
/// member starting at `word_index * 4 + 4` and running to the next member.
/// Re-derived from `docs/formats/tim-pack.md` - `legaia_prot::timpack` exposes
/// the member *bytes* but not the offsets they came from.
pub(super) fn walk_tim_pack(buf: &[u8], sink: &mut Sink) {
    if !legaia_prot::timpack::is_tim_pack(buf) {
        sink.note("timpack::is_tim_pack rejected the buffer");
        return;
    }
    let n = i32::from_le_bytes(buf[4..8].try_into().unwrap()) as usize;
    sink.claim(0, 8 + n * 4, OWNER_TOC, format!("{n} word offsets"));
    let mut offsets: Vec<usize> = (0..n)
        .filter_map(|x| {
            let e = i32::from_le_bytes(buf[8 + 4 * x..12 + 4 * x].try_into().unwrap());
            let off = (e as i64) * 4 + 4;
            (off >= 0 && off as usize <= buf.len()).then_some(off as usize)
        })
        .collect();
    offsets.sort_unstable();
    offsets.dedup();
    offsets.push(buf.len());
    for (i, w) in offsets.windows(2).enumerate() {
        let owner = if buf.get(w[0]) == Some(&0x10) {
            OWNER_TIM
        } else {
            OWNER_RECORD
        };
        sink.claim(w[0], w[1], owner, format!("member {i}"));
    }
}

/// `asset::pack` whose members are whole TIMs, claimed at their own extent
/// rather than out to the next member.
///
/// [`walk_pack`] ends the last member at the buffer end, which would swallow
/// any tail the pack does not reference. Entry 0892 has 948 such bytes past
/// its second TIM, and they are the interesting part of the accounting.
pub(super) fn walk_card_font_pack(buf: &[u8], sink: &mut Sink) {
    let Ok(entries) = crate::pack::parse_pack(buf) else {
        sink.note("pack::parse_pack failed");
        return;
    };
    let n = entries.len();
    sink.claim(0, 4 + n * 4, OWNER_TOC, format!("{n} word offsets"));
    for e in &entries {
        let member = &buf[e.byte_offset..e.byte_offset + e.size];
        let mut claimed = false;
        for h in crate::tim_scan::scan_buffer(member) {
            if h.offset != 0 {
                continue;
            }
            sink.claim(
                e.byte_offset,
                e.byte_offset + h.byte_len,
                OWNER_TIM,
                format!("member {} - {}x{} {}bpp", e.index, h.width, h.height, h.bpp),
            );
            claimed = true;
            break;
        }
        if !claimed {
            sink.claim(
                e.byte_offset,
                e.byte_offset + e.size,
                OWNER_RECORD,
                format!("member {}", e.index),
            );
        }
    }
}

pub(super) fn walk_pack(buf: &[u8], sink: &mut Sink) {
    let Ok(entries) = crate::pack::parse_pack(buf) else {
        sink.note("pack::parse_pack failed");
        return;
    };
    let n = entries.len();
    sink.claim(0, 4 + n * 4, OWNER_TOC, format!("{n} word offsets"));
    for e in &entries {
        sink.claim(
            e.byte_offset,
            e.byte_offset + e.size,
            member_owner(buf, e.byte_offset),
            format!("member {}", e.index),
        );
    }
}

/// Owner for a pack member, from its own leading magic.
pub(super) fn member_owner(buf: &[u8], start: usize) -> &'static str {
    match legaia_bytes::u32_le(buf, start) {
        Some(0x0000_0010) => OWNER_TIM,
        Some(0x8000_0002) => OWNER_TMD,
        _ => OWNER_RECORD,
    }
}

/// `[u32 count][u32 byte_offset[count]][members]` with **absolute** byte
/// offsets - the container the bundle's VDF (type `0x07`) and clip-bank
/// (type `0x05`) sections share, and the fallback when a clip bank's records
/// do not carry the ANM header. Distinct from [`crate::pack`], whose offsets
/// are word indices; the anchor `offsets[0] == 4 + 4*count` tells the two
/// apart because a word-offset table would put member 0 four times further on.
///
/// Returns the member ranges it claimed, so a caller can walk inside them.
pub(super) fn walk_offset_pack(
    buf: &[u8],
    sink: &mut Sink,
    owner: &'static str,
    what: &str,
) -> Vec<std::ops::Range<usize>> {
    let Some(count) = legaia_bytes::u32_le(buf, 0) else {
        sink.note("buffer too small for an offset-pack header");
        return Vec::new();
    };
    let count = count as usize;
    let table_end = 4 + count * 4;
    if count == 0 {
        // Several bundles ship an empty section - the count word and nothing
        // else. That is the whole container, not a parse failure.
        sink.claim(
            0,
            4.min(buf.len()),
            OWNER_HEADER,
            "empty container (count 0)",
        );
        return Vec::new();
    }
    if table_end > buf.len() {
        sink.note(format!("implausible offset-pack count {count}"));
        return Vec::new();
    }
    let mut offsets = Vec::with_capacity(count);
    for i in 0..count {
        let Some(off) = legaia_bytes::u32_le(buf, 4 + i * 4) else {
            sink.note("offset table truncated");
            return Vec::new();
        };
        let off = off as usize;
        if off < table_end || off > buf.len() || offsets.last().is_some_and(|&p| off < p) {
            sink.note(format!("offset[{i}] = 0x{off:X} is not a member start"));
            return Vec::new();
        }
        offsets.push(off);
    }
    sink.claim(0, table_end, OWNER_TOC, format!("{count} byte offsets"));
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let end = offsets.get(i + 1).copied().unwrap_or(buf.len());
        sink.claim(offsets[i], end, owner, format!("{what} {i}"));
        out.push(offsets[i]..end);
    }
    out
}

/// A bundle's type-`0x05` section: the ANM **clip bank**, not a Tactical-Arts
/// move table. Each clip is an 8-byte [`crate::player_anm`] header followed by
/// `bone_count * frame_count` 8-byte per-(bone, frame) transforms and an
/// 8-byte record-boundary trailer, so a clip's claimed extent is
/// `8 + bones*frames*8 + 8` and any shortfall against the offset table shows
/// up as residue rather than being absorbed.
pub(super) fn walk_clip_bank(buf: &[u8], sink: &mut Sink) {
    let Ok(bank) = crate::player_anm::parse(buf) else {
        // Not the `0x080C` record family - still an offset pack, and its
        // members are still real extents.
        walk_offset_pack(buf, sink, OWNER_ANM, "clip");
        return;
    };
    let n = bank.record_offsets.len();
    sink.claim(0, 4 + n * 4, OWNER_TOC, format!("{n} clip offsets"));
    for i in 0..n {
        let start = bank.record_offsets[i] as usize;
        let end = start + bank.record_sizes[i] as usize;
        let rec = match bank.record(i) {
            Ok(r) => r,
            Err(e) => {
                sink.note(format!("clip {i}: {e}"));
                continue;
            }
        };
        let body = crate::player_anm::RECORD_HEADER_SIZE
            + rec.bone_count as usize
                * rec.frame_count as usize
                * crate::player_anm::BONE_FRAME_BYTES;
        sink.claim(
            start,
            start + crate::player_anm::RECORD_HEADER_SIZE,
            OWNER_HEADER,
            format!("clip {i} header"),
        );
        let body_end = (start + body).min(end);
        sink.claim(
            start + crate::player_anm::RECORD_HEADER_SIZE,
            body_end,
            OWNER_ANM,
            format!(
                "clip {i}: {} bones x {} frames",
                rec.bone_count, rec.frame_count
            ),
        );
        if body_end < end {
            sink.claim(body_end, end, OWNER_PAD, format!("clip {i} trailer"));
        }
    }
}

/// A bundle's type-`0x04` section - a MES dialog container
/// (`docs/formats/mes.md`, crate `legaia-mes`).
///
/// The `Compact` form leads with the `0x00000404` magic and a fixed header
/// region; the `Records` form is variable-stride records delimited by the
/// `0x44 0x78` marker. Several retail bundles carry a 40-byte *empty* compact
/// MES - the magic and nothing else - which is the shape the header-region
/// bound below exists for.
pub(super) fn walk_mes(buf: &[u8], sink: &mut Sink) {
    match legaia_mes::detect_format(buf) {
        Some(legaia_mes::Format::Compact) => {
            let head = legaia_mes::compact::OFFSET_TABLE_END.min(buf.len());
            sink.claim(0, 4.min(buf.len()), OWNER_HEADER, "compact magic");
            sink.claim(4.min(buf.len()), head, OWNER_TOC, "compact header region");
            if head < buf.len() {
                sink.claim(head, buf.len(), OWNER_SCRIPT, "dialog bytecode");
            }
        }
        Some(legaia_mes::Format::Records) => {
            let Ok(blob) = legaia_mes::parse(buf) else {
                sink.note("mes::parse failed on a records blob");
                return;
            };
            let marks: Vec<usize> = blob
                .records
                .unwrap_or_default()
                .iter()
                .map(|r| r.offset)
                .collect();
            if let Some(&first) = marks.first() {
                sink.claim(0, first, OWNER_HEADER, "pre-record head");
            }
            for (i, &m) in marks.iter().enumerate() {
                let end = marks.get(i + 1).copied().unwrap_or(buf.len());
                sink.claim(m, end, OWNER_RECORD, format!("record {i}"));
            }
        }
        None => sink.note("mes::detect_format matched neither layout"),
    }
}

// --- leaf sub-assets -------------------------------------------------------

pub(super) fn walk_tim(buf: &[u8], sink: &mut Sink) {
    for h in crate::tim_scan::scan_buffer(buf) {
        sink.claim(
            h.offset,
            h.offset + h.byte_len,
            OWNER_TIM,
            format!("{}x{} {}bpp", h.width, h.height, h.bpp),
        );
    }
}

pub(super) fn walk_tmd(buf: &[u8], sink: &mut Sink) {
    for h in crate::tmd_scan::scan_buffer(buf) {
        sink.claim(
            h.offset,
            h.offset + h.byte_len,
            OWNER_TMD,
            format!("{} objects, {} verts", h.n_obj, h.total_verts),
        );
    }
}

pub(super) fn walk_vab(buf: &[u8], sink: &mut Sink) {
    match legaia_vab::parse(buf, 0) {
        Ok(r) => {
            let body = legaia_vab::VAB_HEADER_SIZE
                + legaia_vab::PROGRAMS_TABLE_SIZE
                + r.programs.len() * legaia_vab::TONES_PER_PROGRAM * legaia_vab::TONE_SIZE
                + legaia_vab::VAG_TABLE_ENTRIES * 2;
            sink.claim(0, body.min(buf.len()), OWNER_TOC, "VAB header + tables");
            for s in &r.vag_samples {
                sink.claim(
                    s.byte_offset,
                    s.byte_offset + s.size,
                    OWNER_VAB,
                    format!("VAG {}", s.index),
                );
            }
            sink.note(format!("VAB declares fsize {} B", r.header.fsize));
        }
        Err(e) => sink.note(format!("vab::parse: {e}")),
    }
}

/// Walk a Legaia SEQ from `off` and claim `[off, end_of_track)`.
///
/// Re-derived from `docs/formats/seq.md` rather than taken from `legaia-seq`,
/// which is a dev-dependency of this crate. Two Legaia-specific divergences
/// from PsyQ matter: the version field is a `u32` BE (header is 15 bytes, not
/// 13), and a meta event carries **no** MIDI variable-length `length` byte -
/// `FF 51` is followed by exactly 3 tempo bytes and `FF 2F` ends the track.
pub fn seq_extent(buf: &[u8], off: usize) -> Option<usize> {
    const HEADER: usize = 0x0F;
    if buf.get(off..off + 4)? != b"pQES" {
        return None;
    }
    let mut p = off + HEADER;
    let mut running: u8 = 0;
    loop {
        // Delta time (MIDI VLQ).
        let mut guard = 0;
        loop {
            let b = *buf.get(p)?;
            p += 1;
            guard += 1;
            if b & 0x80 == 0 || guard > 4 {
                break;
            }
        }
        let b = *buf.get(p)?;
        let status = if b & 0x80 != 0 {
            p += 1;
            running = b;
            b
        } else {
            running
        };
        match status {
            0xFF => {
                let kind = *buf.get(p)?;
                p += 1;
                match kind {
                    0x2F => return Some(p - off),
                    0x51 => p += 3,
                    0x58 => p += 4,
                    _ => return None,
                }
            }
            0x80..=0xBF | 0xE0..=0xEF => p += 2,
            0xC0..=0xDF => p += 1,
            _ => return None,
        }
        if p > buf.len() {
            return None;
        }
    }
}

/// Walk the multi-bank VAB archive (`monster.snd`, extraction 891).
///
/// Every claim comes out of a length the container states: the bank count and
/// the `count + 1` start sectors `FUN_8003E104` indexes, then each bank's two
/// DATA_FIELD chunk headers. Nothing here rests on a magic sweep - the `pBAV`
/// magic is only a gate on the class, never a claim boundary.
/// See [`crate::vab_multi_bank`].
pub(super) fn walk_vab_multi_bank(buf: &[u8], sink: &mut Sink) {
    use crate::vab_multi_bank::{self, SECTOR};
    let Some(r) = vab_multi_bank::detect(buf) else {
        sink.note("vab_multi_bank::detect declined");
        return;
    };
    sink.claim(0, 8, OWNER_HEADER, "reserved word + bank count");
    sink.claim(
        8,
        r.table_end().min(buf.len()),
        OWNER_TOC,
        format!(
            "bank start-sector table, {} words (one per bank plus the end sentinel)",
            r.count + 1
        ),
    );
    if r.banks.len() != r.count {
        sink.note(format!(
            "bank walk resolved {} of the {} banks the head declares",
            r.banks.len(),
            r.count
        ));
    }
    // The archive's first sector holds the index table; the reader stages
    // 0x400 bytes of it, so the rest of that sector is slack by construction.
    if r.banks.first().is_some_and(|b| b.offset() >= SECTOR) {
        sink.claim(r.table_end(), SECTOR, OWNER_PAD, "index-sector slack");
    }
    let mut vag_total = 0usize;
    for b in &r.banks {
        let i = b.index;
        sink.claim(
            b.offset(),
            b.offset() + 4,
            OWNER_HEADER,
            format!("bank {i} chunk 0 header"),
        );
        sink.claim(
            b.vab_offset(),
            b.body_chunk_offset(),
            OWNER_TOC,
            format!(
                "bank {i} VAB header + {} program slot(s), tone rows, VAG size table",
                b.programs
            ),
        );
        sink.claim(
            b.body_chunk_offset(),
            b.body_offset(),
            OWNER_HEADER,
            format!("bank {i} chunk 1 header"),
        );
        sink.claim(
            b.body_offset(),
            b.content_end(),
            OWNER_VAB,
            format!("bank {i} VAG bodies, {} samples", b.vags),
        );
        vag_total += b.vags as usize;
        // The stream terminator, then the sector slack the index table's next
        // entry declares. Not zero fill: the builder left its sector buffer's
        // previous contents behind it, which nothing reads - both chunk
        // lengths and `fsize` end before it.
        let end = b.offset() + b.span();
        if legaia_bytes::u32_le(buf, b.content_end()) == Some(0) {
            sink.claim(
                b.content_end(),
                b.content_end() + 4,
                OWNER_HEADER,
                format!("bank {i} stream terminator"),
            );
            sink.claim(
                b.content_end() + 4,
                end.min(buf.len()),
                OWNER_PAD,
                format!("bank {i} sector slack past the declared stream"),
            );
        } else {
            sink.claim(
                b.content_end(),
                end.min(buf.len()),
                OWNER_PAD,
                format!("bank {i} sector slack past the declared stream"),
            );
        }
    }
    sink.note(format!("{} banks, {vag_total} VAG bodies", r.banks.len()));
}

pub(super) fn walk_seq(buf: &[u8], sink: &mut Sink) {
    match seq_extent(buf, 0) {
        Some(n) => {
            sink.claim(0, 0x0F, OWNER_HEADER, "pQES header");
            sink.claim(0x0F, n, OWNER_SEQ, "event stream to end-of-track");
        }
        None => sink.note("SEQ event walk did not reach an end-of-track meta"),
    }
}

pub(super) fn walk_anm(buf: &[u8], sink: &mut Sink) {
    let payload = legaia_anm::peel_preamble(buf).unwrap_or(buf);
    let skew = buf.len() - payload.len();
    match legaia_anm::parse(payload) {
        Ok(pack) => {
            sink.claim(
                skew,
                skew + 4 + pack.records.len() * 4,
                OWNER_TOC,
                format!("{} record offsets", pack.records.len()),
            );
            for (i, r) in pack.records.iter().enumerate() {
                sink.claim(
                    skew + r.offset,
                    skew + r.offset + r.size,
                    OWNER_ANM,
                    format!("record {i}"),
                );
            }
        }
        Err(e) => {
            // The kingdom bundles' type-`0x06` sections are the same
            // `[u32 count][u32 byte_offset[count]]` container without the
            // `0x080C` record header `legaia_anm::parse` gates on, so the
            // member extents are still readable.
            sink.note(format!("anm::parse: {e}"));
            walk_offset_pack(buf, sink, OWNER_ANM, "record");
        }
    }
}

/// A decompressed MAN: `[0x2B header][u24 record offsets][record bodies]
/// [6 chained tail sections]`.
///
/// The record bodies are the bulk, and `man_section` exposes them only as the
/// partition offset tables - so the per-record extents are re-derived here from
/// those tables, the way `docs/formats/man-relocation.md` describes the layout:
/// an offset is relative to the data region, records tile it in table order,
/// and the region ends where section 0 starts (`data_region + u24_at_28`).
pub(super) fn walk_man(buf: &[u8], sink: &mut Sink) {
    use crate::man_section::RECORDS_BEGIN_OFFSET;
    match crate::man_section::parse(buf) {
        Ok(m) => {
            sink.claim(0, RECORDS_BEGIN_OFFSET, OWNER_HEADER, "MAN header");
            sink.claim(
                RECORDS_BEGIN_OFFSET,
                m.data_region_offset,
                OWNER_TOC,
                "u24 record-offset partitions",
            );
            let base = m.data_region_offset;
            let region_end = base.saturating_add(m.header.u24_at_28 as usize);
            let mut offs: Vec<usize> = m
                .partitions
                .iter()
                .flatten()
                .map(|&o| o as usize)
                .filter(|&o| base + o < region_end)
                .collect();
            offs.sort_unstable();
            offs.dedup();
            for (i, o) in offs.iter().enumerate() {
                let end = offs.get(i + 1).map_or(region_end, |n| base + n);
                sink.claim(base + o, end, OWNER_SCRIPT, format!("record {i}"));
            }
            for (i, s) in m.sections.iter().enumerate() {
                sink.claim(
                    s.offset,
                    s.offset + 3 + s.length as usize,
                    OWNER_SCRIPT,
                    format!("tail section {i}"),
                );
            }
        }
        Err(e) => sink.note(format!("man_section::parse: {e:?}")),
    }
}
