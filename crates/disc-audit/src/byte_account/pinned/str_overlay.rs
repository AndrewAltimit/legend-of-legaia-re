//! The STR / MDEC cutscene overlay's tables, including its unreferenced VLC
//! table.

use super::*;

/// PROT index of the STR/MDEC cutscene overlay.
pub(in crate::byte_account) const STR_OVERLAY_PROT_INDEX: u32 = 970;

/// The STR/MDEC overlay's two data-segment structures: the per-`fmv_id`
/// dispatch table with the movie paths it points at, and the compressed blob
/// the VLC lookup table is unpacked from.
///
/// Both are consumed by modules in this workspace at pinned constants
/// ([`legaia_game_tables::fmv_dispatch`], `legaia_mdec::strv2_table`) and neither was
/// claimed: the dispatch table and its path strings read as `plausible_mips` /
/// `ascii_text` residue, and the blob - the second-largest overlay residue run
/// on the disc - read as `mixed`, which is what a compressed stream looks like
/// to a shape test.
///
/// The blob's extent is **measured**, not assumed: `unpack_lz_tracked` walks
/// the control bytes to the `0xFF 0xFF` terminator and reports what it
/// consumed, the same way [`take_lzs`] measures an LZS span. What is left of
/// the entry's last sector past that terminator is the builder's sector
/// buffer, and takes the same one-sector-wide slack rule the streaming classes
/// take.
pub(in crate::byte_account) fn claim_str_overlay_tables(buf: &[u8], sink: &mut Sink) {
    use legaia_game_tables::fmv_dispatch as fmv;
    use legaia_mdec::strv2_table as vlc;

    let base = fmv::STR_OVERLAY_BASE_VA;
    let table_off = (fmv::FMV_TABLE_VA - base) as usize;
    let table_len = fmv::FMV_SLOT_COUNT * fmv::SLOT_STRIDE;
    match fmv::FmvTable::from_str_overlay(buf) {
        Some(_) => {
            sink.claim(
                table_off,
                (table_off + table_len).min(buf.len()),
                OWNER_TOC,
                format!(
                    "FMV dispatch table, {} x {} bytes (fmv_dispatch)",
                    fmv::FMV_SLOT_COUNT,
                    fmv::SLOT_STRIDE
                ),
            );
            // Each slot's `+0x00` is a pointer to its ISO9660 movie path; the
            // strings sit in the image's own head pool, so the extents come
            // from the pointers plus a NUL scan rather than from a table.
            for i in 0..fmv::FMV_SLOT_COUNT {
                let at = table_off + i * fmv::SLOT_STRIDE;
                let Some(w) = buf.get(at..at + 4) else { break };
                let ptr = u32::from_le_bytes(w.try_into().unwrap());
                let Some(off) = ptr.checked_sub(base).map(|o| o as usize) else {
                    continue;
                };
                let Some(tail) = buf.get(off..) else { continue };
                let Some(len) = tail.iter().position(|&b| b == 0) else {
                    continue;
                };
                sink.claim(
                    off,
                    off + len + 1,
                    OWNER_STRING,
                    format!("movie path for fmv_id {i} (fmv_dispatch)"),
                );
            }
        }
        None => sink.note("no FMV dispatch table at the pinned offset in PROT 0970"),
    }

    // The two MDEC command packets the table upload sends, each checked by
    // its own header word before it is claimed.
    for (va, header, what) in [
        (
            fmv::MDEC_QUANT_PACKET_VA,
            fmv::MDEC_QUANT_PACKET_HEADER,
            "MDEC quant-table packet: header + luma + chroma matrices (fmv_dispatch)",
        ),
        (
            fmv::MDEC_IDCT_PACKET_VA,
            fmv::MDEC_IDCT_PACKET_HEADER,
            "MDEC IDCT-table packet: header + 64-halfword matrix (fmv_dispatch)",
        ),
    ] {
        let off = (va - base) as usize;
        if legaia_bytes::u32_le(buf, off) == Some(header)
            && off + fmv::MDEC_PACKET_BYTES <= buf.len()
        {
            sink.claim(off, off + fmv::MDEC_PACKET_BYTES, OWNER_RECORD, what);
        } else {
            sink.note(format!(
                "no MDEC packet header {header:#010x} at {va:#010x}"
            ));
        }
    }

    claim_str_dead_vlc_table(buf, sink, base);

    let src = (vlc::STRV2_PACKED_VA - base) as usize;
    match buf.get(src..).map(vlc::unpack_lz_tracked) {
        Some(Ok((table, consumed))) => {
            let end = (src + consumed).min(buf.len());
            sink.claim(
                src,
                end,
                OWNER_LZS,
                format!(
                    "STRv2 VLC table source, mode-switched LZ77 -> {} bytes at {:#010x} \
                     (legaia_mdec::strv2_table, FUN_801f1a00)",
                    table.len(),
                    vlc::STRV2_TABLE_VA
                ),
            );
            // Past the terminator, inside the entry's last sector: the
            // builder's buffer, the same slack the streaming classes declare.
            claim_last_sector_slack(buf, sink, end, "slack past the VLC blob terminator");
        }
        _ => sink.note("the VLC blob at the pinned offset does not terminate"),
    }
}

/// First word of the STR overlay's second, unreferenced AC VLC lookup table.
pub(in crate::byte_account) const STR_DEAD_VLC_VA: u32 = 0x801D_0E9C;
/// One past its last word: the start of the overlay's uninitialised data
/// region (the `cutscene_str` row of `static-overlays.toml`).
pub(in crate::byte_account) const STR_DEAD_VLC_END_VA: u32 = 0x801D_199C;

/// The STR overlay's 2816-byte AC VLC lookup table at [`STR_DEAD_VLC_VA`],
/// directly above the MDEC / DMA register-pointer block, which **nothing
/// reads**: no word, `jal`, `j`, branch or `lui` pair in any image names an
/// address in it (`find-address-word-refs.py --prot`), no `lui` + load or
/// `gp`-relative access reaches it (`find-gp-relative-refs.py --prot`), and
/// the register block's own accesses stop at its last pointer, `0x801D0E98`.
/// The overlay's decoder reads the separate `0x11000`-byte table
/// `FUN_801F1A00` unpacks at `0x801E0A00` instead
/// (`legaia_mdec::strv2_table`). So this is initialised data the link carried
/// and no code uses - claimed as dead data under that name, not as a
/// structure anything consumes.
///
/// The claim is shape-checked before it is made: every word is either zero or
/// an entry `(len << 26) | (run << 10) | level` whose bits `16..26` are clear
/// and whose length field is a plausible code length - the MPEG-1 AC run /
/// level form (`0x1000_0401` is a 4-bit code for run 1, level 1).
pub(in crate::byte_account) fn claim_str_dead_vlc_table(buf: &[u8], sink: &mut Sink, base: u32) {
    let (start, end) = (
        (STR_DEAD_VLC_VA - base) as usize,
        (STR_DEAD_VLC_END_VA - base) as usize,
    );
    let Some(words) = buf.get(start..end) else {
        sink.note("the STR overlay is too short for its dead VLC table");
        return;
    };
    let (mut zero, mut entries) = (0usize, 0usize);
    for w in words
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
    {
        if w == 0 {
            zero += 1;
        } else if w & 0x03FF_0000 == 0 && (2..=17).contains(&(w >> 26)) {
            entries += 1;
        } else {
            sink.note(format!(
                "no AC VLC table at {STR_DEAD_VLC_VA:#010x}: word {w:#010x} is not a run/level entry"
            ));
            return;
        }
    }
    sink.claim(
        start,
        end,
        OWNER_RECORD,
        format!(
            "unreferenced AC VLC lookup table, {entries} run/level entries + {zero} empty              slots: dead data, no image forms an address in it"
        ),
    );
}
