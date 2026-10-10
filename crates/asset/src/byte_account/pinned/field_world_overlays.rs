//! Field overlay and world-map render overlay tables: the field probe-offset
//! tables, the follow-camera ease list, the `other3` roster and the world
//! map's per-prim dispatch row.

use super::*;

/// The field overlay's three probe-offset tables at the head of its data
/// segment, each bound to the `lui` pairs that form its base
/// ([`crate::field_probe_tables`]). Claimed only when
/// [`crate::field_probe_tables::check`] re-derives every base from this
/// image's own instructions.
pub(in crate::byte_account) fn claim_field_probe_tables(buf: &[u8], sink: &mut Sink) {
    use crate::field_probe_tables as fpt;
    let errs = fpt::check(buf);
    if !errs.is_empty() {
        sink.note(format!(
            "field probe tables not claimed: {} row(s) disagree with this image ({})",
            errs.len(),
            errs[0]
        ));
        return;
    }
    for (va, rows, sites) in fpt::TABLES {
        let off = (va - fpt::OVERLAY_BASE_VA) as usize;
        sink.claim(
            off,
            off + rows * fpt::ROW_BYTES,
            OWNER_RECORD,
            format!(
                "field probe table {va:#010x}, {rows} x (dx, dz) rows, formed at {} site(s)",
                sites.len()
            ),
        );
    }
}

/// The follow camera's ease list in the field overlay (`0x801F2798`).
///
/// `FUN_801DB510` (from `0x801DB5EC`) and `FUN_801DB8EC` (from `0x801DB914`)
/// form the base, then walk 12-byte records `[u32 live global][u32 target]
/// [u16][u16 width]` - easing each live word toward its target at the width
/// `+0x0A` names (`2` or `4`, `0x801DB60C` / `0x801DB690`) - advancing by
/// `0xC` until the record's first word is zero (`0x801DB710..0x801DB71C`).
/// The terminator is the extent, so the walk here stops where the loop does
/// and claims the zero word with it.
const FOLLOW_EASE_LIST_VA: u32 = 0x801F_2798;
const FOLLOW_EASE_RECORD_BYTES: usize = 12;
const FOLLOW_EASE_MAX_RECORDS: usize = 32;

pub(in crate::byte_account) fn claim_follow_ease_list(buf: &[u8], sink: &mut Sink) {
    use crate::field_probe_tables as fpt;
    let start = (FOLLOW_EASE_LIST_VA - fpt::OVERLAY_BASE_VA) as usize;
    let word = |at: usize| {
        buf.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let mut at = start;
    for _ in 0..FOLLOW_EASE_MAX_RECORDS {
        let Some(live) = word(at) else { return };
        if live == 0 {
            if at == start {
                sink.note("follow-camera ease list is empty at the pinned offset");
                return;
            }
            sink.claim(
                start,
                at + 4,
                OWNER_RECORD,
                format!(
                    "follow-camera ease list, {} x 12-byte records + zero terminator (FUN_801DB510)",
                    (at - start) / FOLLOW_EASE_RECORD_BYTES
                ),
            );
            return;
        }
        let target = word(at + 4).unwrap_or(0);
        let width = buf
            .get(at + 10..at + 12)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .unwrap_or(0);
        let ram = |p: u32| (0x8000_0000..0x8020_0000).contains(&p);
        if !ram(live) || !ram(target) || !matches!(width, 2 | 4) {
            sink.note("follow-camera ease list record out of shape; not claimed");
            return;
        }
        at += FOLLOW_EASE_RECORD_BYTES;
    }
    sink.note("follow-camera ease list has no terminator within the cap");
}

/// The `OTHER3` dev module's 81-record selection roster (PROT `0974`).
///
/// Three quarters of that entry is one fixed-stride table of NUL-padded
/// labels, and a shape test can only call the whole thing `ascii_text` - the
/// stride is in the drawing loop's index arithmetic, not in the bytes
/// ([`crate::other3_roster`]). Claiming each record at the stride covers its
/// padding too, because the stride is what the loop advances by.
pub(in crate::byte_account) fn claim_other3_roster(buf: &[u8], sink: &mut Sink) {
    use crate::other3_roster as roster;
    if roster::records(buf).is_none() {
        sink.note("no OTHER3 roster at the pinned offset in PROT 0974");
        return;
    }
    for i in 0..roster::RECORD_COUNT {
        let Some((off, len)) = roster::record_extent(i) else {
            break;
        };
        sink.claim(
            off,
            (off + len).min(buf.len()),
            OWNER_STRING,
            format!("OTHER3 roster label {i} (other3_roster)"),
        );
    }
    sink.note(format!(
        "{} roster labels on a {:#x} stride at {:#010x} (other3_roster)",
        roster::RECORD_COUNT,
        roster::RECORD_STRIDE,
        roster::ROSTER_VA
    ));
}

/// PROT entry of the world-map render overlay (slot B, base `0x801F69D8`).
pub(in crate::byte_account) const WORLD_MAP_RENDER_PROT_INDEX: u32 = 901;

/// Slot-B link base the world-map render overlay loads at.
const SLOT_B_BASE: u32 = 0x801F_69D8;

/// The per-prim dispatch row SCUS's `FUN_80043390` switches to while the
/// world-map overlay is resident: it forms `0x801F8968` itself (`lui
/// s4,0x8020` / `addiu s4,s4,-0x7698` at `0x800435F4..F8`), indexes it by the
/// prim group's `flags >> 1`, and adds no alpha offset on this branch, so one
/// twenty-slot row is the whole table (`docs/subsystems/world-map.md`).
const WORLD_MAP_PRIM_DISPATCH_VA: u32 = 0x801F_8968;

/// Slots in one dispatch row.
const PRIM_DISPATCH_SLOTS: usize = 20;

/// The four lit low-mode dispatchers slots `8..12` share with the SCUS
/// table at `0x8007657C`.
const PRIM_DISPATCH_LOW_MODE: [u32; 4] = [0x8004_409C, 0x8004_423C, 0x8004_4434, 0x8004_45B0];

/// Claim the world-map render overlay's per-prim dispatch row
/// ([`WORLD_MAP_PRIM_DISPATCH_VA`]), after a shape check: slots `0..8` are
/// zero (the index space below the lit rows, not padding), slots `8..12` are
/// the SCUS low-mode quartet, and slots `12..20` - the eight untextured and
/// textured high-mode emit leaves - each point into this image's own code.
/// The consumer is the executable's, so the row cannot be read off a `lui`
/// pair in this image the way [`CONSUMER_PINNED_TABLES`] rows are.
pub(in crate::byte_account) fn claim_world_map_prim_dispatch(buf: &[u8], sink: &mut Sink) {
    let start = (WORLD_MAP_PRIM_DISPATCH_VA - SLOT_B_BASE) as usize;
    let end = start + 4 * PRIM_DISPATCH_SLOTS;
    let words: Option<Vec<u32>> = (start..end)
        .step_by(4)
        .map(|o| legaia_bytes::u32_le(buf, o))
        .collect();
    let Some(words) = words else {
        return;
    };
    let own_top = SLOT_B_BASE + start as u32;
    let ok = words[..8].iter().all(|&w| w == 0)
        && words[8..12] == PRIM_DISPATCH_LOW_MODE
        && words[12..]
            .iter()
            .all(|&w| (SLOT_B_BASE..own_top).contains(&w) && w % 4 == 0);
    if !ok {
        sink.note("no world-map prim dispatch row at 0x801F8968: shape check failed");
        return;
    }
    sink.claim(
        start,
        end,
        OWNER_TOC,
        "per-prim dispatch row read by FUN_80043390 (slots 8..11 SCUS, 12..19 this image)",
    );
}
