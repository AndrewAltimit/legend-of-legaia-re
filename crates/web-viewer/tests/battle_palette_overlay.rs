//! Disc-gated check that the Battle-form palette overlay targets the VRAM rows
//! Vahn's mesh samples and lands his true (byte-exact) battle CLUT there.
//!
//! Replicates `LegaiaViewer::battle_char_vram_bytes_battle`'s logic via the
//! public crate APIs (the viewer struct itself needs a wasm canvas). Skips when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::disc::{extract_prot_dat, parse_prot_toc};
use std::env;

fn prot_entry(prot: &[u8], index: u32) -> Option<&[u8]> {
    let meta = parse_prot_toc(prot)?
        .into_iter()
        .find(|e| e.index == index)?;
    let off = meta.byte_offset as usize;
    let end = off.saturating_add(meta.size_bytes as usize);
    prot.get(off..end)
}

#[test]
fn vahn_battle_palette_lands_on_mesh_rows() {
    let Some(path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping");
        return;
    };
    let disc = std::fs::read(&path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT");

    // Vahn's battle mesh = PROT 1204 slot 0 -> distinct CLUT rows it samples.
    let pack_raw = prot_entry(&prot, legaia_asset::battle_char_pack::PROT_ENTRY_INDEX)
        .expect("PROT 1204 present");
    let pack = legaia_asset::battle_char_pack::parse(pack_raw).expect("parse 1204");
    let tmd_bytes = pack.slot(0).expect("slot 0").tmd_bytes.clone();
    let tmd = legaia_tmd::parse(&tmd_bytes).expect("Vahn battle TMD");
    let mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &tmd_bytes);
    let mut rows: Vec<u16> = mesh.cba_tsb.iter().map(|ct| (ct[0] >> 6) & 0x1FF).collect();
    rows.sort_unstable();
    rows.dedup();
    // Same rows the doc/runtime pin for Vahn's nominal CBA.
    assert_eq!(rows, vec![490, 491], "Vahn battle mesh CLUT rows");

    // Vahn's true palette from edstati3 PROT 0861.
    let edstati3 = prot_entry(&prot, 861).expect("PROT 0861 present");
    let rec0 = legaia_asset::battle_char_palette::find_record0(edstati3).expect("record0");
    let pal = legaia_asset::battle_char_palette::parse_record(edstati3, rec0).expect("palette");
    assert_eq!(pal.bands.len(), 3, "Vahn has 3 effective bands");

    // Overlaying onto a fresh VRAM, each band's STP colours land at (row, base+i)
    // for every sampled row, and differ from a zeroed cell (i.e. it wrote).
    const W: usize = 1024;
    let mut vram = vec![0u8; W * 512 * 2];
    for &row in &rows {
        for band in &pal.bands {
            for (i, w) in band.vram_words().iter().enumerate() {
                let off = (row as usize * W + band.base as usize + i) * 2;
                vram[off] = (*w & 0xFF) as u8;
                vram[off + 1] = (*w >> 8) as u8;
            }
        }
    }
    // Spot-check: band@0x00 colour 0 on row 490 is the STP-set form of the disc
    // colour (bit 15 set on the non-zero word).
    let band0 = pal
        .bands
        .iter()
        .find(|b| b.base == 0x00)
        .expect("band@0x00");
    let c0 = band0.colors[0];
    let want = if c0 != 0 { c0 | 0x8000 } else { 0 };
    let off = (490 * W) * 2;
    let got = u16::from_le_bytes([vram[off], vram[off + 1]]);
    assert_eq!(got, want, "band@0x00[0] STP-set at row 490 col 0");
}

/// Noa (PROT 0864) and Gala (PROT 0865) equipment-robust palettes cover every
/// column their battle mesh samples - the condition for a colour-complete render.
/// Gala in particular validates the rec0-relative `0x2000` `sec_base` alignment
/// (a `0x1000`-aligned `sec_base` lands his sub region on a zero-padded block).
#[test]
fn noa_gala_collected_palettes_cover_mesh_columns() {
    let Some(path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping");
        return;
    };
    let disc = std::fs::read(&path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT");
    let pack_raw =
        prot_entry(&prot, legaia_asset::battle_char_pack::PROT_ENTRY_INDEX).expect("1204");
    let pack = legaia_asset::battle_char_pack::parse(pack_raw).expect("parse 1204");

    for &(slot, prot_index, name) in &[(1usize, 864u32, "Noa"), (2, 865, "Gala")] {
        let tmd_bytes = pack.slot(slot).expect("slot").tmd_bytes.clone();
        let tmd = legaia_tmd::parse(&tmd_bytes).expect("battle TMD");
        let mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &tmd_bytes);
        let mut cols: Vec<u16> = mesh.cba_tsb.iter().map(|ct| (ct[0] & 0x3F) * 16).collect();
        cols.sort_unstable();
        cols.dedup();

        let entry = prot_entry(&prot, prot_index).expect("PROT entry present");
        let pal = legaia_asset::battle_char_palette::collect_palette(entry, 0, &cols)
            .unwrap_or_else(|e| panic!("collect {name} palette: {e}"));

        use std::collections::BTreeSet;
        let mut covered: BTreeSet<u16> = BTreeSet::new();
        for band in &pal.bands {
            assert!(
                cols.contains(&band.base),
                "{name} band@{:X} not a mesh column",
                band.base
            );
            for i in 0..band.colors.len() as u16 {
                covered.insert(band.base + i);
            }
        }
        let uncovered: Vec<u16> = cols
            .iter()
            .copied()
            .filter(|c| !covered.contains(c))
            .collect();
        assert!(
            uncovered.is_empty(),
            "{name} palette leaves mesh columns uncovered: {uncovered:X?} (bands {:X?})",
            pal.bands.iter().map(|b| b.base).collect::<Vec<_>>()
        );
    }
}
