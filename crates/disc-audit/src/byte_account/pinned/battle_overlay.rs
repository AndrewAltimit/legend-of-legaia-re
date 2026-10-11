//! Battle overlay tables: jump tables, side-effect banners, UI strings,
//! effect prototype and sub-draw records, and the unread affinity block.

use super::*;

/// The battle overlay's head: twenty-two `switch` jump tables and the C
/// strings in front of them, each bound to the instruction pair that forms its
/// address ([`crate::battle_jump_tables`]).
///
/// A table's extent is its consumer's `sltiu` bound times four - read off the
/// dispatch, not scanned out of the bytes - so the claims are structural. They
/// are made only when [`crate::battle_jump_tables::check`] re-derives every row
/// from this image's own instructions; an image that disagrees gets a note and
/// no claim.
pub(in crate::byte_account) fn claim_battle_jump_tables(buf: &[u8], sink: &mut Sink) {
    use crate::battle_jump_tables as bjt;
    let errs = bjt::check(buf);
    if !errs.is_empty() {
        sink.note(format!(
            "battle jump tables not claimed: {} row(s) disagree with this image ({})",
            errs.len(),
            errs[0]
        ));
        return;
    }
    for t in &bjt::JUMP_TABLES {
        sink.claim(
            t.offset(),
            t.offset() + t.byte_len(),
            OWNER_TOC,
            format!(
                "jump table, {} arms on {} (jr {:#010x})",
                t.arms, t.index, t.jr
            ),
        );
    }
    for s in &bjt::HEAD_STRINGS {
        let off = s.offset();
        if let Some(len) = buf.get(off..).and_then(|t| t.iter().position(|&b| b == 0)) {
            sink.claim(
                off,
                off + len + 1,
                OWNER_STRING,
                format!("head string, address formed at {:#010x}", s.site),
            );
        }
    }
}

/// The banner strings the Seru-magic side-effect table names.
///
/// Each 8-byte `[element][band]` record's `+4` word is the banner-string
/// pointer the stager `FUN_801F3D3C` copies to `0x800775B4`
/// ([`legaia_game_tables::seru_side_effect`]), so the table - already claimed at its
/// pinned offset - is the consumer that pins each string's start, and the
/// string's own NUL pins its end. A pointer outside the image claims nothing.
pub(in crate::byte_account) fn claim_side_effect_banners(buf: &[u8], sink: &mut Sink) {
    use legaia_game_tables::seru_side_effect as seru;
    let base = seru::OVERLAY_LINK_BASE;
    let mut n = 0usize;
    let mut seen = std::collections::BTreeSet::new();
    for r in 0..seru::SIDE_EFFECT_ELEMENTS * seru::SIDE_EFFECT_BANDS {
        let at = seru::SIDE_EFFECT_TABLE_FILE_OFFSET + r * seru::SIDE_EFFECT_RECORD_STRIDE + 4;
        let Some(w) = buf
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        else {
            continue;
        };
        let Some(off) = w.checked_sub(base).map(|o| o as usize) else {
            continue;
        };
        if off >= buf.len() || !seen.insert(off) {
            continue;
        }
        let Some(len) = buf[off..].iter().position(|&b| b == 0) else {
            continue;
        };
        sink.claim(
            off,
            off + len + 1,
            OWNER_STRING,
            format!("Seru side-effect banner, record {r} +4"),
        );
        n += 1;
    }
    if n > 0 {
        sink.note(format!(
            "{n} Seru side-effect banner string(s) named by the side-effect table's +4 words"
        ));
    }
}

/// The battle overlay's NUL-terminated UI strings, whose extents no fixed
/// `count * stride` row can express.
///
/// Every address here is a `pub const` (or a pointer read from one), so this is
/// the same kind of binding as [`pinned_overlay_tables`] - what differs is only
/// that a C string's length is in its own bytes rather than in a table, so the
/// extent is NUL-scanned from the pinned start instead of computed. Without
/// that these read as `ascii_text` residue while the parser that consumes them
/// names the exact byte they start at.
pub(in crate::byte_account) fn claim_battle_overlay_strings(buf: &[u8], sink: &mut Sink) {
    use legaia_minigame_formats::muscle_dome as dome;
    use legaia_ui_formats::battle_ui_strings as bui;
    let base = bui::OVERLAY_BASE_VA;
    let claim_cstr = |off: usize, what: String, sink: &mut Sink| {
        let Some(tail) = buf.get(off..) else { return };
        // A string with no terminator in the image is not a string - claim
        // nothing rather than run to the end of the entry.
        let Some(len) = tail.iter().position(|&b| b == 0) else {
            sink.note(format!("{what}: no NUL terminator at {off:#x}"));
            return;
        };
        sink.claim(off, off + len + 1, OWNER_STRING, what);
    };
    for (va, label) in bui::OVERLAY_LABELS {
        let Some(off) = va.checked_sub(base).map(|o| o as usize) else {
            continue;
        };
        claim_cstr(
            off,
            format!("battle UI label {label:?} (battle_ui_strings)"),
            sink,
        );
    }
    for (i, off) in dome::victory_message_offsets(buf).into_iter().enumerate() {
        claim_cstr(
            off,
            format!("muscle-dome victory message {i} (muscle_dome)"),
            sink,
        );
    }
}

/// The battle overlay's **effect-prototype record pool** - the bytes the
/// `0x801F6324` pointer table points INTO, as distinct from the table itself.
///
/// The table has a pinned constant and a row in [`pinned_overlay_tables`], so
/// the 61 pointers were credited while the 54 unique records they name were
/// not: the pool read as one unbroken `low_entropy` residue run, which is how a
/// fully decoded structure looks when only its index is claimed.
/// [`legaia_game_tables::move_power::parse_effect_proto_records`] already decodes it to
/// `[i16 model_sel][u16 reserved][move-VM bytecode]` part records
/// ([`move-power.md`](../../../docs/formats/move-power.md)); this walks the same
/// offsets and claims each record's extent.
///
/// A record ends where the next one begins - the pool is packed, with the last
/// record bounded by the table itself rather than by the end of the entry, which
/// is the one place `parse_records_at`'s generic bound is too generous for a
/// byte claim.
pub(in crate::byte_account) fn claim_effect_proto_records(buf: &[u8], sink: &mut Sink) {
    use legaia_game_tables::move_power as mp;
    let Some(aux) = mp::EffectAuxTables::parse(buf) else {
        sink.note("no effect-prototype table: PROT 0898 structural guard failed");
        return;
    };
    let table = mp::EFFECT_PROTO_TABLE_FILE_OFFSET;
    let mut offs: Vec<usize> = (0..mp::EFFECT_AUX_TABLE_LEN as u8)
        .filter_map(|i| aux.proto_record_offset(i))
        .filter(|&f| f + 4 <= table)
        .collect();
    offs.sort_unstable();
    offs.dedup();
    for (i, &f) in offs.iter().enumerate() {
        let end = offs.get(i + 1).copied().unwrap_or(table);
        let model_sel = i16::from_le_bytes([buf[f], buf[f + 1]]);
        sink.claim(
            f,
            end,
            OWNER_RECORD,
            format!("move-FX part record, model_sel {model_sel} (move_power)"),
        );
    }
    sink.note(format!(
        "{} unique effect-prototype record(s) behind the {}-entry \
         0x801F6324 table (move_power::parse_effect_proto_records)",
        offs.len(),
        mp::EFFECT_AUX_TABLE_LEN
    ));
}

/// The battle overlay's **sub-draw record pool** - the step records the
/// `0x801F4D34` pointer table names, as distinct from the table itself (the
/// same shape as [`claim_effect_proto_records`]). Each record's extent is the
/// consumer's own `3 + 2*count` read
/// ([`legaia_minigame_formats::muscle_dome::subdraw_record_extents`]); the word padding between
/// records stays residue.
pub(in crate::byte_account) fn claim_subdraw_records(buf: &[u8], sink: &mut Sink) {
    let recs = legaia_minigame_formats::muscle_dome::subdraw_record_extents(buf);
    for &(off, len) in &recs {
        sink.claim(
            off,
            off + len,
            OWNER_RECORD,
            format!(
                "battle HUD sub-draw step, {} element pair(s) (muscle_dome)",
                (len - 3) / 2
            ),
        );
    }
    sink.note(format!(
        "{} unique sub-draw record(s) behind the {}-entry 0x801F4D34 table \
         (muscle_dome::subdraw_record_extents)",
        recs.len(),
        legaia_minigame_formats::muscle_dome::SUBDRAW_PTR_TABLE_LEN
    ));
}

/// The battle overlay's unread second affinity block
/// ([`legaia_game_tables::element_affinity::UNREAD_AFFINITY_BLOCK_VA`]) - claimed as dead
/// data under that name, the way [`claim_str_dead_vlc_table`] claims the STR
/// overlay's unreferenced table, and only after a shape check: it must end
/// exactly where the summon power-percent table begins and every byte must be
/// a plausible percentage (`1..=200`).
pub(in crate::byte_account) fn claim_unread_affinity_block(buf: &[u8], sink: &mut Sink) {
    use legaia_game_tables::element_affinity as elem;
    let start = elem::UNREAD_AFFINITY_BLOCK_FILE_OFFSET;
    let len = elem::ELEMENT_COUNT * elem::ELEMENT_COUNT;
    if start + len != elem::SUMMON_POWER_PCT_FILE_OFFSET
        || start != elem::AFFINITY_MATRIX_FILE_OFFSET + len
    {
        sink.note("the unread affinity block's constants no longer tile the gap");
        return;
    }
    let Some(block) = buf.get(start..start + len) else {
        return;
    };
    if !block.iter().all(|b| (1..=200).contains(b)) {
        sink.note("no percentage block between the affinity matrix and the summon table");
        return;
    }
    sink.claim(
        start,
        start + len,
        OWNER_RECORD,
        "unread second 8x8 affinity block, dead data (element_affinity)",
    );
}
