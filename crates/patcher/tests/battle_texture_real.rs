//! Disc-gated oracle for [`legaia_patcher::battle_texture`] - export and
//! replacement of the headerless party-character art in the player battle
//! files (PROT 863..866).
//!
//! Proves the write half on a scratch copy of the user's own disc: an
//! unedited round-trip writes nothing, a real repaint lands surgically with
//! every touched sector still EDC/ECC-valid, the sibling palette of the
//! same block is untouched, a repaint that cannot recompress into the
//! record's slot allocation is refused rather than corrupting the next
//! record, and a fixed edit is byte-deterministic.
//!
//! No decoded pixels are asserted on or written anywhere - only shapes,
//! counts and equality against what was read back. Skips + passes without
//! `LEGAIA_DISC_BIN`.

use legaia_asset::battle_texture_catalog::BattleTextureSlot;
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_SIZE};
use legaia_patcher::battle_texture::{self, BattleTextureTarget, PaletteSource};
use legaia_patcher::disc::DiscPatcher;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Noa's Ra-Seru armband ("Terra $8", equipment id `0x11`) - the block a
/// texture rip could previously only reach by hand.
const ARMBAND: BattleTextureTarget = BattleTextureTarget {
    entry: 864,
    slot: BattleTextureSlot::Section(14),
};

/// Mirror an RGBA image left-to-right. A real edit that reuses exactly the
/// source palette, so it exercises the pixel path without dragging palette
/// overflow in.
fn mirror(rgba: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for row in 0..h {
        let line = &rgba[row * w * 4..(row + 1) * w * 4];
        for col in (0..w).rev() {
            out.extend_from_slice(&line[col * 4..col * 4 + 4]);
        }
    }
    out
}

/// The block's raw CLUT bytes as stored in its decoded record.
fn stored_clut(patcher: &DiscPatcher, target: &BattleTextureTarget) -> Vec<u8> {
    let b = battle_texture::read_block(patcher, target).expect("read block");
    let n = b.upload.clut.len() * 2;
    b.decoded[b.pool_offset + 4..b.pool_offset + 4 + n].to_vec()
}

#[test]
fn catalog_and_export_reach_the_armband() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open");
    let rows = battle_texture::catalog(&patcher).expect("catalog");
    assert_eq!(rows.len(), 153, "whole-family block count off the disc");
    assert!(
        rows.iter()
            .all(|b| battle_texture::PLAYER_FILE_ENTRIES.contains(&b.entry_index)),
        "the tier only ever reports player-file entries"
    );

    let ex = battle_texture::export_block(&patcher, &ARMBAND, 0).expect("export");
    assert_eq!((ex.width, ex.height), (128, 128));
    assert_eq!(ex.rgba.len(), 128 * 128 * 4);
    assert_eq!(ex.palette_count, 2, "two 16-colour palettes, not one");
    assert_eq!(ex.palette, PaletteSource::Block(0));
    // 4bpp: at most 16 distinct colours can appear through one palette.
    let distinct: std::collections::HashSet<[u8; 4]> = ex
        .rgba
        .chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect();
    assert!(distinct.len() <= 16, "{} distinct colours", distinct.len());
    assert!(distinct.len() > 1, "the block is not a flat fill");

    // Asking for a palette the block does not have names the count.
    let err = battle_texture::export_block(&patcher, &ARMBAND, 2)
        .unwrap_err()
        .to_string();
    assert!(err.contains("out of range"), "{err}");
}

#[test]
fn unedited_round_trip_writes_nothing() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let original = disc.clone();
    let mut patcher = DiscPatcher::open(disc).expect("open");
    let ex = battle_texture::export_block(&patcher, &ARMBAND, 0).expect("export");

    let outcome = battle_texture::replace_block(
        &mut patcher,
        &ARMBAND,
        &ex.rgba,
        ex.width,
        ex.height,
        0,
        false,
        false,
    )
    .expect("replace");
    assert!(outcome.unchanged, "re-importing the export changes nothing");
    assert_eq!(outcome.palette_entries_changed, 0);
    assert_eq!(outcome.quantized_pixels, 0);
    // Not merely "no visible change" - no disc byte moved. Our LZS encoder
    // is not the mastering's, so this only holds because the write is
    // skipped when the record is unchanged.
    assert_eq!(patcher.image(), &original[..], "zero-run patch");
}

#[test]
fn a_repaint_is_surgical_edc_valid_and_deterministic() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let original = disc.clone();
    let mut patcher = DiscPatcher::open(disc).expect("open");

    let before_rows = battle_texture::catalog(&patcher).expect("catalog");
    let before_clut = stored_clut(&patcher, &ARMBAND);
    let ex = battle_texture::export_block(&patcher, &ARMBAND, 0).expect("export");
    let edited = mirror(&ex.rgba, ex.width, ex.height);
    assert_ne!(edited, ex.rgba, "the mirror really is a different image");

    let outcome = battle_texture::replace_block(
        &mut patcher,
        &ARMBAND,
        &edited,
        ex.width,
        ex.height,
        0,
        false,
        false,
    )
    .expect("replace");
    assert!(!outcome.unchanged);
    assert_eq!(
        outcome.palette_entries_changed, 0,
        "a mirror reuses the palette exactly"
    );
    assert_eq!(outcome.quantized_pixels, 0);
    assert!(
        outcome.fit.recompressed <= outcome.fit.capacity,
        "must fit the slot allocation"
    );
    assert_ne!(patcher.image(), &original[..], "something was written");
    assert_eq!(
        patcher.image().len(),
        original.len(),
        "the image never changes size"
    );

    // It reads back as the requested art.
    let after = battle_texture::export_block(&patcher, &ARMBAND, 0).expect("re-export");
    assert_eq!(after.rgba, edited, "the block decodes to the edited image");

    // Surgical: exactly this block's fingerprint moved, in the same
    // catalog position, and the sibling palette's stored bytes are intact.
    let after_rows = battle_texture::catalog(&patcher).expect("catalog after");
    assert_eq!(before_rows.len(), after_rows.len());
    let mut moved = Vec::new();
    for (b, a) in before_rows.iter().zip(&after_rows) {
        assert_eq!(
            (b.entry_index, b.record_index),
            (a.entry_index, a.record_index)
        );
        assert_eq!((b.pool_offset, b.byte_len), (a.pool_offset, a.byte_len));
        if b.fnv1a != a.fnv1a {
            moved.push((a.entry_index, a.record_index));
        }
    }
    assert_eq!(moved, vec![(864, 14)], "only the target block changed");
    let after_clut = stored_clut(&patcher, &ARMBAND);
    assert_eq!(
        after_clut, before_clut,
        "a mirror needs no new colours, so the CLUT bytes must be untouched"
    );

    // Every sector of the touched player file stays EDC/ECC-valid.
    let lba = patcher.entry_disc_lba(864).expect("entry lba") as usize;
    let footprint = patcher.entry_footprint(864).expect("footprint") as usize;
    let img = patcher.image();
    for s in 0..footprint.div_ceil(USER_DATA_SIZE) {
        let sb = (lba + s) * SECTOR_SIZE;
        assert!(
            legaia_iso::write::mode2_form1_sector_is_valid(&img[sb..sb + SECTOR_SIZE]),
            "player-file sector {s} must stay EDC/ECC-valid"
        );
    }

    // Deterministic: the same edit onto a fresh copy yields the same image.
    let mut again = DiscPatcher::open(original.clone()).expect("open");
    battle_texture::replace_block(
        &mut again, &ARMBAND, &edited, ex.width, ex.height, 0, false, false,
    )
    .expect("replace again");
    assert_eq!(again.image(), patcher.image(), "byte-deterministic");
}

#[test]
fn an_over_budget_repaint_is_refused_and_writes_nothing() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let original = disc.clone();
    let mut patcher = DiscPatcher::open(disc).expect("open");
    let ex = battle_texture::export_block(&patcher, &ARMBAND, 0).expect("export");

    // Maximum-entropy indices drawn from the block's own 16 colours: no new
    // palette entries at all, but the index stream stops compressing, so the
    // record cannot recompress into its slot.
    let palette: Vec<[u8; 4]> = {
        let mut seen: Vec<[u8; 4]> = Vec::new();
        for c in ex.rgba.chunks_exact(4) {
            let p = [c[0], c[1], c[2], c[3]];
            if !seen.contains(&p) {
                seen.push(p);
            }
        }
        seen
    };
    assert!(palette.len() > 1, "need real colours to shuffle");
    let mut x: u32 = 0x1234_5678;
    let noise: Vec<u8> = (0..ex.width * ex.height)
        .flat_map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            palette[(x >> 16) as usize % palette.len()]
        })
        .collect();

    let err = battle_texture::replace_block(
        &mut patcher,
        &ARMBAND,
        &noise,
        ex.width,
        ex.height,
        0,
        false,
        false,
    )
    .expect_err("incompressible art cannot fit the slot");
    let msg = err.to_string();
    assert!(msg.contains("slot allocates only"), "{msg}");
    assert!(msg.contains("over"), "{msg}");
    assert_eq!(
        patcher.image(),
        &original[..],
        "a refused replacement writes nothing"
    );

    // The dimension check fires before any of that.
    let err = battle_texture::replace_block(
        &mut patcher,
        &ARMBAND,
        &ex.rgba[..64],
        4,
        4,
        0,
        false,
        false,
    )
    .expect_err("wrong dimensions");
    assert!(err.to_string().contains("128x128"), "{err}");
}
