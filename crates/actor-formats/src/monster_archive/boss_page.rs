//! The **boss effect page**: the extra texture page the battle loader stages
//! for every monster id above `0xB2`.
//!
//! The monster loader `FUN_800542C8` reads each distinct formation id's
//! `0x14000`-byte archive slot and uploads its texture pool to the seat's own
//! column ([`super::MonsterMesh::battle_render_mesh`]). For an id `>= 0xB3`
//! (`sltiu v0,v0,0xb3` at `0x80054830`) it then reads one more slot: a seek
//! of `0x8C000` bytes **relative** to the end of the block it just read
//! (`FUN_80060920(handle, 0x8C000, 1)` on the host-file path,
//! `FUN_8003E964(0x118, 1)` - the same span in sectors - on the CD path),
//! so the slot of id `id + 8`, which holds a bare PSX TIM. Two uploads follow,
//! both at fixed coordinates rather than the TIM header's:
//!
//! - the pixels at `+0x220` to the rect `(0x180, 0x100, 0x40, 0x100)`
//!   (`0x80054924..0x8005494C`) - texture page `0x16`, the seat-1 column;
//! - the 256-entry CLUT at `+0x14`, every non-zero entry with the STP bit
//!   (`0x8000`) ORed in, to `(0, 0x1E5, 0x100, 1)` - CLUT row 485
//!   (`0x80054958..0x800549B8`).
//!
//! Cort's two forms (`0xB4`, `0xB5`) are the ids that use it: the Mystic
//! Shield's sphere samples page `0x16` / CLUT `0x7940`. The page for `0xB4`
//! is slot `0xBC`, byte-identical to slots `0xBF..=0xC2`.

use super::SLOT_STRIDE;

/// The first monster id whose load stages the boss effect page.
pub const BOSS_PAGE_MIN_ID: u16 = 0xB3;
/// The loader's relative seek past the block it just read.
pub const BOSS_PAGE_SEEK: usize = 0x8C000;
/// Where the pixels land: `(x, y, w, h)` in VRAM halfwords.
pub const BOSS_PAGE_RECT: (u16, u16, u16, u16) = (0x180, 0x100, 0x40, 0x100);
/// The CLUT row the page's palette lands on (`0x1E5`).
pub const BOSS_PAGE_CLUT_ROW: u16 = 0x1E5;
/// Byte offset of the CLUT data in the slot (the TIM's CLUT block body).
const CLUT_AT: usize = 0x14;
/// Entries the loader uploads.
const CLUT_ENTRIES: usize = 0x100;
/// Byte offset of the pixel data in the slot (past the image block header).
const PIXELS_AT: usize = 0x220;

/// The boss effect page the loader stages after monster `id`'s block, as
/// `(clut_bytes, pixel_bytes)` with the STP bit already applied to the
/// palette. `None` below [`BOSS_PAGE_MIN_ID`], or when the slot the seek
/// lands on is not a TIM (head word `0x10`) or is short.
pub fn boss_effect_page(archive: &[u8], id: u16) -> Option<(Vec<u8>, &[u8])> {
    if id < BOSS_PAGE_MIN_ID {
        return None;
    }
    let at = usize::from(id) * SLOT_STRIDE + BOSS_PAGE_SEEK;
    let slot = archive.get(at..)?;
    if legaia_bytes::u32_le(slot, 0)? != 0x10 {
        return None;
    }
    let mut clut = slot.get(CLUT_AT..CLUT_AT + CLUT_ENTRIES * 2)?.to_vec();
    for c in clut.as_chunks_mut::<2>().0 {
        let v = u16::from_le_bytes(*c);
        if v != 0 {
            *c = (v | 0x8000).to_le_bytes();
        }
    }
    let (_, _, w, h) = BOSS_PAGE_RECT;
    let n = usize::from(w) * usize::from(h) * 2;
    let pixels = slot.get(PIXELS_AT..PIXELS_AT + n)?;
    Some((clut, pixels))
}

/// Stage monster `id`'s boss effect page into `vram` the way
/// `FUN_800542C8` does after the block upload. Returns whether it wrote.
pub fn write_boss_effect_page(archive: &[u8], id: u16, vram: &mut legaia_tim::Vram) -> bool {
    let Some((clut, pixels)) = boss_effect_page(archive, id) else {
        return false;
    };
    let (x, y, w, h) = BOSS_PAGE_RECT;
    vram.write_block(x, y, w, h, pixels);
    vram.write_clut_row(0, BOSS_PAGE_CLUT_ROW, &clut);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive_with_page(id: u16) -> Vec<u8> {
        let at = usize::from(id) * SLOT_STRIDE + BOSS_PAGE_SEEK;
        let mut a = vec![0u8; at + SLOT_STRIDE];
        a[at..at + 4].copy_from_slice(&0x10u32.to_le_bytes());
        a[at + CLUT_AT..at + CLUT_AT + 2].copy_from_slice(&0u16.to_le_bytes());
        a[at + CLUT_AT + 2..at + CLUT_AT + 4].copy_from_slice(&0x1234u16.to_le_bytes());
        a[at + PIXELS_AT] = 0xAB;
        a
    }

    #[test]
    fn the_page_sits_eight_slots_past_the_monster_and_takes_stp() {
        let a = archive_with_page(0xB4);
        let (clut, px) = boss_effect_page(&a, 0xB4).expect("page");
        assert_eq!(
            &clut[0..2],
            &0u16.to_le_bytes(),
            "the zero entry stays clear"
        );
        assert_eq!(&clut[2..4], &0x9234u16.to_le_bytes());
        assert_eq!(px[0], 0xAB);
        assert_eq!(px.len(), 0x40 * 0x100 * 2);
    }

    #[test]
    fn ids_below_0xb3_stage_nothing() {
        let a = archive_with_page(0xB2);
        assert!(boss_effect_page(&a, 0xB2).is_none());
    }
}
