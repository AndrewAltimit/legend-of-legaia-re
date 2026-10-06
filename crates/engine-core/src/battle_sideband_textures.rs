//! The **monster side-band texture pages**: the `readef.DAT` slots a
//! monster's turn streams into VRAM, which the module casts sample.
//!
//! Retail's initiative scheduler `FUN_801DABA4` seeds the side-band
//! applier's base byte `ctx[+0x277]` with `3 * monster_record[+0x1C]` on
//! every monster turn and arms the applier (`ctx[+0x276] = 1`); the applier
//! `FUN_801F12D0` then streams `readef.DAT` slot `base` to CLUT `(0, 488)` /
//! page `(512, 0)` and, for a base inside `0x0C..=0x36`, slot `base + 1` to
//! CLUT `(0, 490)` / page `(640, 0)` ([`docs/formats/summon-readef.md`]).
//! A capture-class cast has no stream of its own (the band runs no case
//! `0x32`), so what its records sample in those pages is what the caster's
//! own turn streamed.
//!
//! PROT 0954's Fatal Decision is the visible case: its sixteen wheel icons
//! are records whose sprite fields name tpage `0x8A` (8bpp at `(640, 0)`)
//! and CLUT `0x7A80` (row 490), 64x64 cells at `u = 0x40 * (id % 4)` - and
//! its three casters (Evil Shadow, Shade, Nightmare, monster ids `119..=121`)
//! all carry group `7`, so the turn streams `readef.DAT` slots 21 and 22.
//! Slot 22 is the 4x4 icon sheet.
//!
//! The engine streams nothing per turn, so the battle-entry VRAM build takes
//! these writes instead: every distinct monster group of the formation, in
//! seat order (a later seat's group overwrites an earlier one where they
//! differ, which is what the turn order would leave on the last monster's
//! turn). Party groups (`3 * (char - 1)`, below `0x0C`) are not written:
//! their first slot is a dev placeholder page and their second is an "ME"
//! archive, never a texture.
//!
//! REF: FUN_801DABA4 (the per-turn base-byte seed), FUN_801F12D0 (the
//! applier's two texture uploads)

use crate::battle_party_form::VramSink;
use crate::scene::ProtIndex;
use legaia_asset::summon_readef::{
    READEF_PROT_INDEX, SLOT_BYTES, SlotKind, TEXTURE_SLOT_VRAM_TARGETS, aux_slot_is_texture_upload,
};

/// The monster archive's PROT entry, which carries each record's group byte.
const MONSTER_ARCHIVE_PROT: u32 = 867;

/// The `readef.DAT` texture writes one group's turn makes, into `sink`.
/// Returns the number of pages written (`0`, `1` or `2`).
pub fn write_readef_group_textures(readef: &[u8], group: u8, sink: &mut impl VramSink) -> usize {
    let base = group.wrapping_mul(3);
    if base & 0x80 != 0 {
        return 0;
    }
    let mut slots = vec![(usize::from(base), 0usize)];
    if aux_slot_is_texture_upload(base) {
        slots.push((usize::from(base) + 1, 1));
    }
    let mut written = 0;
    for (slot, target) in slots {
        let Some(bytes) = readef.get(slot * SLOT_BYTES..(slot + 1) * SLOT_BYTES) else {
            continue;
        };
        let Some(t) = texture_slot_of(bytes) else {
            continue;
        };
        let ((clut_x, clut_y), (tex_x, tex_y)) = TEXTURE_SLOT_VRAM_TARGETS[target];
        let clut = &bytes[4..4 + t.clut_bytes()];
        // `FUN_801F12D0` loads the CLUT block as `256 x rows` halfwords.
        sink.write_block(clut_x, clut_y, 256, t.clut_rows as u16, clut);
        let page = &bytes[t.texture_offset..t.texture_offset + t.texture_bytes()];
        sink.write_block(tex_x, tex_y, t.texture_width_halfwords as u16, 256, page);
        written += 1;
    }
    written
}

fn texture_slot_of(slot: &[u8]) -> Option<legaia_asset::summon_readef::TextureSlot> {
    let parsed = legaia_asset::summon_readef::parse(slot).ok()?;
    match parsed.slots.first()?.kind {
        SlotKind::Texture(t) => Some(t),
        _ => None,
    }
}

/// The readef groups of the battle's monsters, in seat order, de-duplicated.
pub fn formation_readef_groups(entry867: &[u8], monster_ids: &[u16]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for &id in monster_ids {
        if let Ok(Some(rec)) = legaia_asset::monster_archive::record(entry867, id)
            && !out.contains(&rec.readef_group)
        {
            out.push(rec.readef_group);
        }
    }
    out
}

/// Record the formation's monster side-band pages into `sink` - the
/// battle-entry stand-in for the per-turn stream. Returns the pages written.
pub fn record_monster_sideband_textures(
    index: &ProtIndex,
    world: &crate::world::World,
    sink: &mut impl VramSink,
) -> usize {
    let ids: Vec<u16> = world
        .battle_monster_slots()
        .into_iter()
        .map(|(_, id, _)| id)
        .collect();
    if ids.is_empty() {
        return 0;
    }
    let (Ok(archive), Ok(readef)) = (
        index.entry_bytes(MONSTER_ARCHIVE_PROT),
        index.entry_bytes(u32::from(READEF_PROT_INDEX)),
    ) else {
        return 0;
    };
    formation_readef_groups(&archive, &ids)
        .into_iter()
        .map(|g| write_readef_group_textures(&readef, g, sink))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sink that keeps the block writes.
    #[derive(Default)]
    struct Blocks(Vec<(u16, u16, u16, u16, usize)>);
    impl VramSink for Blocks {
        fn upload_tim(&mut self, _: &legaia_tim::Tim) {}
        fn write_block(&mut self, x: u16, y: u16, w: u16, h: u16, b: &[u8]) {
            self.0.push((x, y, w, h, b.len()));
        }
        fn write_clut_row(&mut self, _: u16, _: u16, _: &[u8]) {}
    }

    fn texture_slot(mode: u32) -> Vec<u8> {
        let mut s = vec![0u8; SLOT_BYTES];
        s[..4].copy_from_slice(&mode.to_le_bytes());
        s
    }

    #[test]
    fn a_group_inside_the_two_page_band_writes_both_pages() {
        // Group 7 -> base 21: slots 21 (first target) and 22 (second).
        let mut readef = vec![0u8; SLOT_BYTES * 24];
        readef[21 * SLOT_BYTES..22 * SLOT_BYTES].copy_from_slice(&texture_slot(2));
        readef[22 * SLOT_BYTES..23 * SLOT_BYTES].copy_from_slice(&texture_slot(2));
        let mut sink = Blocks::default();
        assert_eq!(write_readef_group_textures(&readef, 7, &mut sink), 2);
        assert_eq!(
            sink.0,
            vec![
                (0, 488, 256, 1, 0x200),
                (512, 0, 128, 256, 0x10000),
                (0, 490, 256, 1, 0x200),
                (640, 0, 128, 256, 0x10000),
            ]
        );
    }

    #[test]
    fn a_group_below_the_band_writes_its_first_page_only() {
        // Group 2 -> base 6: the aux slot is not a texture upload.
        let mut readef = vec![0u8; SLOT_BYTES * 8];
        readef[6 * SLOT_BYTES..7 * SLOT_BYTES].copy_from_slice(&texture_slot(1));
        readef[7 * SLOT_BYTES..8 * SLOT_BYTES].copy_from_slice(&texture_slot(1));
        let mut sink = Blocks::default();
        assert_eq!(write_readef_group_textures(&readef, 2, &mut sink), 1);
        assert_eq!(sink.0[0], (0, 488, 256, 2, 0x400));
        assert_eq!(sink.0[1], (512, 0, 128, 256, 0x10000));
    }
}
