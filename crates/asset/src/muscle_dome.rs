//! Muscle Dome minigame - **resident in the battle-action overlay (PROT 0898)**.
//!
//! The Muscle Dome arena is *not* a separate overlay. Its match
//! state machine [`FUN_801d0748`] and all its data (the deck/hand tables at
//! `0x801f4b8c`/`0x801f4b94`, the per-step sub-draw script-record table
//! `PTR_DAT_801f4d34`, the victory-message string table `0x801f4dfc`) are
//! resident in the **battle-action overlay** (PROT entry 0898, base
//! `0x801CE818` - the same overlay [`crate::move_power`] reads). The
//! Duckstation "`overlay_muscle_dome.bin`" capture was that overlay's slot.
//!
//! This resolves the long-open "muscle-dome overlay identity" thread: the
//! arena runs on the battle engine (its fighters are battle actors in
//! `&DAT_801c9370`, card plays resolve through the battle-action path), so it
//! ships *inside* the battle overlay rather than aliasing it. The `0977`
//! "Ronginus" entry is only the mode-24 sub-id-5 *door/init* slot (arena
//! roster + `other6` paths), not the match SM.
//!
//! ## What is pinned here
//!
//! `FUN_801d0748`'s prologue reads the Muscle Dome context base
//! `_DAT_8007bd24` (`lui v0,0x8008; lw v0,-0x42dc(v0)`), a signature unique to
//! the arena controller; it lands at battle-overlay file offset
//! [`MATCH_SM_FILE_OFFSET`]. The deck / script / victory tables sit in the
//! `0x801f4xxx` data band of the same overlay. [`verify_resident`] confirms the
//! overlay image hosts them (the disc-reproducible identity check); the deck
//! byte semantics live in `docs/subsystems/minigame-muscle-dome.md`.

/// PROT index of the host overlay (the battle-action overlay).
pub const MUSCLE_OVERLAY_PROT_INDEX: usize = 898;

/// Load base of the battle-action overlay.
pub const MUSCLE_OVERLAY_BASE_VA: u32 = 0x801C_E818;

/// VA of the Muscle Dome context base pointer `_DAT_8007bd24` (read by the
/// match SM prologue).
pub const MUSCLE_CTX_PTR_VA: u32 = 0x8007_BD24;

/// VA of the match-controller `FUN_801d0748`.
pub const MATCH_SM_VA: u32 = 0x801D_0748;

/// File offset of the match controller within the overlay image.
pub const MATCH_SM_FILE_OFFSET: usize = (MATCH_SM_VA - MUSCLE_OVERLAY_BASE_VA) as usize;

/// VA of the per-slot deck/hand move-index table (`&DAT_801f4b8c`).
pub const DECK_TABLE_VA: u32 = 0x801F_4B8C;

/// VA of the per-slot card sprite-id table (`&DAT_801f4b94`).
pub const HAND_SPRITE_TABLE_VA: u32 = 0x801F_4B94;

/// Hand size - the deal loop builds exactly four card slots.
pub const HAND_SLOTS: usize = 4;

/// First / last valid hand command id: the deck entries are the four
/// direction-command ids `0xC..=0xF` (the weapon-swing runtime slots; a
/// card's cost is the same per-(char,cmd) record `+0x74` byte the Arts
/// gauge reads, `DAT_801c9360[char][cmd]+0x74`).
pub const HAND_COMMAND_MIN: u8 = 0x0C;
/// See [`HAND_COMMAND_MIN`].
pub const HAND_COMMAND_MAX: u8 = 0x0F;

/// VA of the per-step sub-draw script-record pointer table (`PTR_DAT_801f4d34`).
pub const SUBDRAW_PTR_TABLE_VA: u32 = 0x801F_4D34;

/// Entries in the sub-draw pointer table.
///
/// Two bounds agree on 50. The table ends where [`VICTORY_MSG_TABLE_VA`]
/// begins, which is a pinned constant rather than a count word; and every one
/// of those 50 words is an address inside the overlay image, while the run of
/// in-image words past the boundary is the victory table's own three entries.
pub const SUBDRAW_PTR_TABLE_LEN: usize = 50;

/// VA of the victory-message string-pointer table.
pub const VICTORY_MSG_TABLE_VA: u32 = 0x801F_4DFC;

/// Entries in the victory-message table on the retail image - one per Ra-Seru.
/// [`victory_message_count`] re-derives it from the bytes; this is the value it
/// returns, kept as a constant so the table has a declared extent without a
/// buffer in hand.
pub const VICTORY_MSG_TABLE_LEN: usize = 3;

/// The match-controller prologue signature: `lui v0,0x8008; lw v0,-0x42dc(v0);
/// addiu sp,sp,-0x48` (little-endian machine code). The `lui`/`lw` pair loads
/// `_DAT_8007bd24`, unique to the Muscle Dome controller.
pub const MATCH_SM_SIGNATURE: [u8; 12] = [
    0x08, 0x80, 0x02, 0x3c, // lui   v0, 0x8008
    0x24, 0xbd, 0x42, 0x8c, // lw    v0, -0x42dc(v0)
    0xb8, 0xff, 0xbd, 0x27, // addiu sp, sp, -0x48
];

/// Whether a `u32` value is a VA inside the given overlay image.
fn in_overlay(va: u32, len: usize) -> bool {
    va >= MUSCLE_OVERLAY_BASE_VA && ((va - MUSCLE_OVERLAY_BASE_VA) as usize) < len
}

/// Read a little-endian `u32` at an overlay VA.
fn read_va(overlay: &[u8], va: u32) -> Option<u32> {
    let off = (va.checked_sub(MUSCLE_OVERLAY_BASE_VA)?) as usize;
    let b = overlay.get(off..off + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Confirm the Muscle Dome match SM + its pointer tables are resident in the
/// supplied battle-action overlay image (PROT 0898 as-loaded bytes). Returns
/// `true` when the match-controller signature is at [`MATCH_SM_FILE_OFFSET`] and
/// the sub-draw / victory tables hold in-overlay pointers - i.e. the arena lives
/// in this overlay.
pub fn verify_resident(overlay: &[u8]) -> bool {
    // Match-SM signature at the expected offset.
    let sig_ok = overlay
        .get(MATCH_SM_FILE_OFFSET..MATCH_SM_FILE_OFFSET + MATCH_SM_SIGNATURE.len())
        .map(|s| s == MATCH_SM_SIGNATURE)
        .unwrap_or(false);
    if !sig_ok {
        return false;
    }
    // First sub-draw script-record pointer resolves in-overlay.
    let subdraw_ok = read_va(overlay, SUBDRAW_PTR_TABLE_VA)
        .map(|p| in_overlay(p, overlay.len()))
        .unwrap_or(false);
    // First victory-message pointer resolves in-overlay.
    let victory_ok = read_va(overlay, VICTORY_MSG_TABLE_VA)
        .map(|p| in_overlay(p, overlay.len()))
        .unwrap_or(false);
    subdraw_ok && victory_ok
}

/// File offset and byte length of every unique **sub-draw record** the
/// [`SUBDRAW_PTR_TABLE_VA`] table points at, in offset order.
///
/// A record is `[u8 count][u8 anim][u8 panel]` then `count` `(record, mode)`
/// byte pairs - the consumer `FUN_801D388C` loads the pointer
/// (`0x801D4BA4..0x801D4BB0`), reads `+1` and `+2`, and walks
/// `+3 + 2*i` / `+4 + 2*i` for `i < +0` into `FUN_801D8DE8`
/// (`0x801D4CAC..0x801D4D8C`). So the length is `3 + 2*count`; the pool pads
/// each record to the next word, and the padding is not the record's.
/// Pointers outside the image are skipped.
pub fn subdraw_record_extents(overlay: &[u8]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = (0..SUBDRAW_PTR_TABLE_LEN as u32)
        .filter_map(|i| read_va(overlay, SUBDRAW_PTR_TABLE_VA + i * 4))
        .filter_map(|ptr| {
            let off = ptr.checked_sub(MUSCLE_OVERLAY_BASE_VA)? as usize;
            let count = usize::from(*overlay.get(off)?);
            let len = 3 + 2 * count;
            (off + len <= overlay.len()).then_some((off, len))
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Decode the four **hand command ids** (`DAT_801f4b8c[0..4]`): per hand
/// slot, the direction-command id the deal loop assigns to that card and
/// the commit path (`FUN_801d388c` case `0xb`) appends into the fighter's
/// `+0x1df` action queue. `None` unless all four are distinct ids in
/// `HAND_COMMAND_MIN..=HAND_COMMAND_MAX` (the structural validity check).
pub fn hand_command_ids(overlay: &[u8]) -> Option<[u8; HAND_SLOTS]> {
    let off = (DECK_TABLE_VA - MUSCLE_OVERLAY_BASE_VA) as usize;
    let b = overlay.get(off..off + HAND_SLOTS)?;
    let ids = [b[0], b[1], b[2], b[3]];
    let valid = ids
        .iter()
        .all(|&id| (HAND_COMMAND_MIN..=HAND_COMMAND_MAX).contains(&id));
    let distinct = (0..HAND_SLOTS).all(|i| (i + 1..HAND_SLOTS).all(|j| ids[i] != ids[j]));
    (valid && distinct).then_some(ids)
}

/// Decode the four per-slot card **sprite ids** (`DAT_801f4b94[0..4]`) - the
/// deal loop's card-face selector (with a `+2` "unlearned" variant gated on
/// the character record's per-move flag).
pub fn hand_sprite_ids(overlay: &[u8]) -> Option<[u8; HAND_SLOTS]> {
    let off = (HAND_SPRITE_TABLE_VA - MUSCLE_OVERLAY_BASE_VA) as usize;
    let b = overlay.get(off..off + HAND_SLOTS)?;
    Some([b[0], b[1], b[2], b[3]])
}

/// Count the victory-message string pointers at [`VICTORY_MSG_TABLE_VA`]
/// (consecutive in-overlay pointers, stopping at the first that isn't).
pub fn victory_message_count(overlay: &[u8]) -> usize {
    let mut n = 0;
    while let Some(p) = read_va(overlay, VICTORY_MSG_TABLE_VA + (n as u32) * 4) {
        if !in_overlay(p, overlay.len()) {
            break;
        }
        n += 1;
    }
    n
}

/// File offsets of the victory-message strings [`VICTORY_MSG_TABLE_VA`] points
/// at, in table order - the pointers [`victory_message_count`] counts, resolved
/// to offsets inside the supplied overlay image.
pub fn victory_message_offsets(overlay: &[u8]) -> Vec<usize> {
    (0..victory_message_count(overlay))
        .filter_map(|n| read_va(overlay, VICTORY_MSG_TABLE_VA + (n as u32) * 4))
        .filter_map(|p| p.checked_sub(MUSCLE_OVERLAY_BASE_VA).map(|o| o as usize))
        .collect()
}

/// PROT entry (extraction space) of the dome data container (`other6.lzs`
/// slot 0): LZS section 0 is `[12-byte header][TIM][TIM]`, the two 4bpp hub
/// pages at VRAM `(320, 0)` / `(320, 256)` with CLUT rows 502 / 503.
pub const HUB_CONTAINER_PROT_INDEX: u32 = 1220;

/// Set STP (bit 15) on every **non-zero** CLUT entry of `tim` - the
/// `FUN_800198E0` upload with the STP flag `_DAT_8007B998` raised.
///
/// The arena's entry routine raises that flag immediately before it loads
/// the dome data file: `FUN_801CEA6C` stores `s2 = 1` to `0x8007B998`
/// (`sw s2,-0x4668(v0)` at `0x801CEB00`, the delay slot of the `jal
/// 0x80020DE0` that reads the file). So the hub CLUTs reach VRAM bit-15-set
/// even though the file stores them clear - a live dome VRAM snapshot shows
/// every non-zero entry of rows 502 / 503 as `entry | 0x8000`, and the zero
/// entries as `0`. Which is what decides how the hub's variant passes
/// blend: the all-white "knockout" palettes the variant-2 emitter bump
/// selects (`clut + 1`, e.g. 7 under 6, 9 under 8) are STP-clear on the disc,
/// and drawn from the file they would be opaque white plates instead of the
/// subtractive (`B - F`) under-layer retail draws.
pub fn apply_upload_stp(tim: &mut legaia_tim::Tim) {
    if let Some(clut) = tim.clut.as_mut() {
        for e in clut.entries.iter_mut() {
            if *e != 0 {
                *e |= 0x8000;
            }
        }
    }
}

/// The two hub page TIMs out of the dome data container (raw extraction
/// [`HUB_CONTAINER_PROT_INDEX`] bytes), with the arena's upload STP applied
/// ([`apply_upload_stp`]) - the CLUT words as they sit in VRAM, which is
/// what every host must classify and decode the hub quads against.
pub fn hub_page_tims(container: &[u8]) -> Option<(legaia_tim::Tim, legaia_tim::Tim)> {
    let sections = legaia_lzs::decompress_container(container).ok()?;
    let blob = sections.first()?;
    let mut t0 = legaia_tim::parse(blob.get(0xC..)?).ok()?;
    let mut t1 = legaia_tim::parse(blob.get(0xC + t0.byte_extent()..)?).ok()?;
    apply_upload_stp(&mut t0);
    apply_upload_stp(&mut t1);
    Some((t0, t1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_stp_sets_bit_15_on_non_zero_entries_only() {
        let mut tim = legaia_tim::Tim {
            flags: 0x8,
            mode: legaia_tim::PixelMode::Bpp4,
            clut: Some(legaia_tim::Clut {
                fb_x: 0,
                fb_y: 502,
                w: 4,
                h: 1,
                entries: vec![0x0000, 0x7FFF, 0x9C84, 0x1086],
            }),
            image: legaia_tim::Image {
                fb_x: 320,
                fb_y: 0,
                fb_w: 1,
                h: 1,
                data: vec![0; 2],
            },
        };
        apply_upload_stp(&mut tim);
        assert_eq!(
            tim.clut.unwrap().entries,
            vec![0x0000, 0xFFFF, 0x9C84, 0x9086]
        );
    }

    #[test]
    fn hand_tables_decode() {
        let mut buf = vec![0u8; 0x27000];
        let deck = (DECK_TABLE_VA - MUSCLE_OVERLAY_BASE_VA) as usize;
        buf[deck..deck + 4].copy_from_slice(&[0x0C, 0x0F, 0x0E, 0x0D]);
        let spr = (HAND_SPRITE_TABLE_VA - MUSCLE_OVERLAY_BASE_VA) as usize;
        buf[spr..spr + 4].copy_from_slice(&[13, 16, 17, 12]);
        assert_eq!(hand_command_ids(&buf), Some([0x0C, 0x0F, 0x0E, 0x0D]));
        assert_eq!(hand_sprite_ids(&buf), Some([13, 16, 17, 12]));
        // Duplicate / out-of-range ids are rejected.
        buf[deck] = 0x0F;
        assert_eq!(hand_command_ids(&buf), None);
        buf[deck] = 0x10;
        assert_eq!(hand_command_ids(&buf), None);
    }

    #[test]
    fn offsets_and_signature() {
        assert_eq!(MATCH_SM_FILE_OFFSET, 0x1F30);
        // The lui/lw pair loads _DAT_8007bd24.
        assert_eq!(0x8008u32 << 16, 0x8008_0000);
        assert_eq!(0x8008_0000u32.wrapping_sub(0x42dc), MUSCLE_CTX_PTR_VA);
        assert_eq!(MATCH_SM_SIGNATURE.len(), 12);
    }

    #[test]
    fn verify_resident_rejects_empty() {
        assert!(!verify_resident(&[]));
        assert!(!verify_resident(&[0u8; 0x30000]));
    }
}
