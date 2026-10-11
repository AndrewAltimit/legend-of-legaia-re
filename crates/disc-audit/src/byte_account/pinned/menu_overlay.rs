//! Menu-side overlay tables: the debug menu's value-monitor name tables and
//! the save screen's text slots.

use super::*;

/// PROT entry of the DEBUG MODE overlay whose value-monitor rows
/// [`CONSUMER_PINNED_TABLES`] carries.
pub(super) const DEBUG_MENU_PROT_INDEX: u32 = 971;

/// Bytes per value-monitor name label: `FUN_8001C93C`'s kind-1 arm reads the
/// row's `+0x24` table at `value << 4` (`sll a0,a0,0x4` at `0x8001C9F8`).
const VALUE_MONITOR_LABEL_BYTES: usize = 0x10;

/// The name tables the DEBUG MODE value-monitor rows point at. A kind-1 row
/// draws `*(row + 0x24) + (value << 4)` as a label, so the table is reached
/// through a data word rather than a formed address. Each distinct in-image
/// table runs to the next one, or to the rows themselves for the last, and is
/// claimed only when that span is whole sixteen-byte labels, each a printable
/// NUL-terminated string inside its slot.
pub(in crate::byte_account) fn claim_value_monitor_name_tables(buf: &[u8], sink: &mut Sink) {
    const BASE: u32 = 0x801C_E818;
    let Some(rows) = CONSUMER_PINNED_TABLES
        .iter()
        .find(|t| t.prot == DEBUG_MENU_PROT_INDEX)
    else {
        return;
    };
    let rows_off = (rows.base_va - BASE) as usize;
    let mut tables: Vec<usize> = (0..rows.count)
        .filter_map(|k| {
            let row = rows_off + k * rows.stride;
            let kind = legaia_bytes::u16_le(buf, row)?;
            let table = legaia_bytes::u32_le(buf, row + 0x24)?;
            (kind == 1 && table > BASE && table < rows.base_va).then(|| (table - BASE) as usize)
        })
        .collect();
    tables.sort_unstable();
    tables.dedup();
    let mut n = 0usize;
    for (i, &start) in tables.iter().enumerate() {
        let end = tables.get(i + 1).copied().unwrap_or(rows_off);
        if (end - start) % VALUE_MONITOR_LABEL_BYTES != 0 {
            continue;
        }
        let labels_ok = buf[start..end].chunks(VALUE_MONITOR_LABEL_BYTES).all(|l| {
            let len = l.iter().position(|&b| b == 0).unwrap_or(l.len());
            len > 0 && len < l.len() && l[..len].iter().all(|&b| (0x20..0x7F).contains(&b))
        });
        if !labels_ok {
            continue;
        }
        sink.claim(
            start,
            end,
            OWNER_STRING,
            format!(
                "value-monitor name table, {} x 0x10 labels (row +0x24, FUN_8001C93C kind 1)",
                (end - start) / VALUE_MONITOR_LABEL_BYTES
            ),
        );
        n += 1;
    }
    if n > 0 {
        sink.note(format!(
            "{n} value-monitor name table(s) named by the rows' +0x24 words"
        ));
    }
}

/// First of the menu overlay's six `0x80`-byte save-screen text slots.
const SAVE_TEXT_SLOTS_VA: u32 = 0x801E_ED24;

/// Bytes per save-screen text slot.
const SAVE_TEXT_SLOT_BYTES: usize = 0x80;

/// Save-screen text slots: six, closed by the region filename prefix the
/// image forms at `0x801EF024` (`0x801DE9A0`).
const SAVE_TEXT_SLOTS: usize = 6;

/// `(lui site, address formed)` pairs that reach into the slots: the
/// play-time digits of slot 0 (`0x801EED29`, `FUN_801DD35C`) and the page
/// digits of slot 3 (`0x801EEEA4`, `FUN_801DE234`), each poked in place.
const SAVE_TEXT_SLOT_FORMS: [(u32, u32); 2] =
    [(0x801D_D628, 0x801E_ED29), (0x801D_E8C4, 0x801E_EEA4)];

/// The menu overlay's save-screen text slots: six `0x80`-byte buffers from
/// [`SAVE_TEXT_SLOTS_VA`], each a NUL-terminated string at the slot start and
/// zero fill to the slot end, the sixth closed exactly by the next string the
/// image forms. Two slots are written in place by the code that forms them
/// ([`SAVE_TEXT_SLOT_FORMS`]); the other four (`0x801EEDA4`, `0x801EEE24`,
/// `0x801EEF24`, `0x801EEFA4`) are named by no instruction, word, `gp` or
/// base-plus-displacement access in any image
/// (`find-address-word-refs.py --prot`, `find-gp-relative-refs.py --prot`),
/// so those are claimed as dead text and the slot stride is read off the
/// layout, which the shape check pins slot by slot.
pub(in crate::byte_account) fn claim_save_text_slots(buf: &[u8], sink: &mut Sink) {
    const MENU_BASE: u32 = 0x801C_E818;
    let at = |va: u32| (va - MENU_BASE) as usize;
    let start = at(SAVE_TEXT_SLOTS_VA);
    let end = start + SAVE_TEXT_SLOTS * SAVE_TEXT_SLOT_BYTES;
    let Some(slots) = buf.get(start..end) else {
        return;
    };
    let forms_ok = SAVE_TEXT_SLOT_FORMS
        .iter()
        .all(|&(site, va)| lui_pair_address(buf, at(site)) == Some(va));
    let shape_ok = slots.chunks(SAVE_TEXT_SLOT_BYTES).all(|slot| {
        let n = slot.iter().position(|&b| b == 0).unwrap_or(slot.len());
        n > 0
            && n < slot.len()
            && slot[..n].iter().all(|&b| (0x20..0x7F).contains(&b))
            && slot[n..].iter().all(|&b| b == 0)
    }) && buf.get(end).is_some_and(|&b| b != 0);
    if !forms_ok || !shape_ok {
        sink.note("save-screen text slots at 0x801EED24 not claimed: shape or form check failed");
        return;
    }
    sink.claim(
        start,
        end,
        OWNER_STRING,
        "save-screen text slots, 6 x 0x80 (slots 0 and 3 written in place, 1/2/4/5 referenced by nothing)",
    );
}
