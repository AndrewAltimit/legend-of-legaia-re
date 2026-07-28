//! Disc-gated regression for [`legaia_asset::battle_texture_catalog`] - the
//! headerless 4bpp party-character art inside the player battle files.
//!
//! This family is invisible to both TIM catalogs by construction (no magic,
//! no header, geometry supplied by the loader), so the thing worth pinning
//! is that it is now *reachable*: the right entries, the right block count,
//! and the specific block a texture rip could previously only be found by
//! hand. Assertions are on shape, counts and offsets only - no decoded
//! pixels, no fingerprints of art.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset (CI without disc data).

use std::path::PathBuf;

use legaia_asset::battle_texture_catalog::{self as catalog, BattleTextureSlot};

fn extracted_prot_dir() -> Option<PathBuf> {
    [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

fn disc_gate() -> bool {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return false;
    }
    true
}

/// Per-player-file pinned shape. Extraction filenames carry the CDNAME +2
/// label shift (`0863/0864_edstati3`) - see `docs/formats/cdname.md`.
struct Pin {
    entry: u32,
    file: &'static str,
    /// Blocks the two `record[0]` header words point at. Always 2: they
    /// chain, and the pair finishes the decoded record exactly.
    record0_blocks: usize,
    /// Flagged equipment-section pools.
    section_blocks: usize,
}

const PINS: &[Pin] = &[
    Pin {
        entry: 863,
        file: "0863_edstati3.BIN", // Vahn, PLAYER1
        record0_blocks: 2,
        section_blocks: 52,
    },
    Pin {
        entry: 864,
        file: "0864_edstati3.BIN", // Noa, PLAYER2
        record0_blocks: 2,
        section_blocks: 47,
    },
    Pin {
        entry: 865,
        file: "0865_battle_data.BIN", // Gala, PLAYER3
        record0_blocks: 2,
        section_blocks: 41,
    },
    Pin {
        entry: 866,
        file: "0866_battle_data.BIN", // PLAYER4, all-default table
        record0_blocks: 2,
        section_blocks: 5,
    },
];

#[test]
fn catalogs_every_player_file_block() {
    if !disc_gate() {
        return;
    }
    let Some(dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let mut total = 0usize;
    for pin in PINS {
        let path = dir.join(pin.file);
        if !path.exists() {
            eprintln!("[skip] {} missing", path.display());
            return;
        }
        let file = std::fs::read(&path).expect("read player file");
        let mut id = 0u32;
        let rows = catalog::build_from_file(pin.entry, &file, &mut id);

        let record0 = rows.iter().filter(|b| b.is_record0()).count();
        let sections = rows.len() - record0;
        assert_eq!(
            (record0, sections),
            (pin.record0_blocks, pin.section_blocks),
            "entry {} block census",
            pin.entry
        );

        for b in &rows {
            assert_eq!(b.entry_index, pin.entry);
            assert_eq!(b.bpp, 4, "the whole family is 4bpp");
            // Two rect widths tile the party band: 32 and 64 halfwords.
            assert!(
                b.width == 128 || b.width == 256,
                "entry {} block {} has width {}",
                pin.entry,
                b.id,
                b.width
            );
            assert_eq!(b.height, 128);
            assert_eq!(
                b.byte_len,
                4 + b.clut_entries * 2 + (b.width * b.height / 2) as usize,
                "declared extent must equal the block's parts"
            );
            assert_eq!(b.clut_entries, b.clut_count * 16);
            // Every block re-resolves through the selector it prints, and
            // the resolved upload agrees with the row.
            let r = catalog::resolve_block(&file, b.slot(), 0).expect("resolve block");
            assert_eq!(r.pool_offset as u64, b.pool_offset);
            assert_eq!(r.upload.pixel_width() as u32, b.width);
            assert_eq!(r.upload.pixel_height() as u32, b.height);
            assert_eq!(r.upload.clut.len(), b.clut_entries);
            assert_eq!(r.upload.clut_x, b.clut_x);
            // Retail's stream always fits its own slot allocation - the
            // budget a replacement has to hit.
            assert!(
                r.stream_consumed <= r.stream_capacity,
                "entry {} block {} stream {} > capacity {}",
                pin.entry,
                b.id,
                r.stream_consumed,
                r.stream_capacity
            );
        }
        total += rows.len();
    }
    assert_eq!(total, 153, "whole-family block count");
}

/// The block a DuckStation texture rip of Noa's Ra-Seru armband ("Terra
/// $8", equipment id `0x11`) comes from. It is the worked example for why
/// this tier exists: every TIM-keyed path misses it, and before this
/// catalog it could only be reached by hand.
#[test]
fn the_raseru_armband_block_is_reachable_by_its_coordinates() {
    if !disc_gate() {
        return;
    }
    let Some(dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let path = dir.join("0864_edstati3.BIN");
    if !path.exists() {
        eprintln!("[skip] {} missing", path.display());
        return;
    }
    let file = std::fs::read(&path).expect("read PLAYER2");
    let mut id = 0u32;
    let rows = catalog::build_from_file(864, &file, &mut id);

    let armband = rows
        .iter()
        .find(|b| b.record_index == 14)
        .expect("entry 864 record 14 is cataloged");
    assert_eq!(armband.section, 2, "the Ra-Seru equipment section");
    assert_eq!(armband.record_id, 0x11, "equipment id");
    assert_eq!(armband.pool_offset, 0x3784);
    assert_eq!((armband.width, armband.height, armband.bpp), (128, 128, 4));
    assert_eq!(
        armband.clut_entries, 32,
        "TWO 16-colour palettes - reading it as one 16-entry CLUT is the trap"
    );
    assert_eq!(armband.clut_count, 2);
    assert_eq!(armband.slot(), BattleTextureSlot::Section(14));
    assert_eq!(
        armband.label, "Noa - equip 0x11",
        "without an item table the label still carries the character + id"
    );

    // With the disc's own item-name table it becomes the string a person
    // would actually search for. The equipment ids in a player file are
    // item ids, which is what makes this join possible at all.
    let scus = ["extracted/SCUS_942.54", "../../extracted/SCUS_942.54"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| legaia_asset::item_names::ItemNameTable::from_scus(&b));
    if let Some(names) = scus {
        let mut id = 0u32;
        let named = catalog::build_from_file_with_names(864, &file, &mut id, Some(&names));
        let row = named
            .iter()
            .find(|b| b.record_index == 14)
            .expect("armband row");
        assert_eq!(row.label, "Noa - Ra-Seru Terra $8");
    }

    // It decodes to RGBA through either of its palettes, and the two
    // palettes really are different art (so the count is not cosmetic).
    let r = catalog::resolve_block(&file, armband.slot(), 1).expect("resolve");
    let p0 = r.upload.rgba(0).expect("palette 0");
    let p1 = r.upload.rgba(1).expect("palette 1");
    assert_eq!(p0.len(), 128 * 128 * 4);
    assert_ne!(p0, p1, "the second palette is a real recolour");
    assert!(r.upload.rgba(2).is_err(), "only two palettes exist");
    // The block is the record's tail - the property the catalog gates on.
    assert_eq!(r.pool_offset + r.upload.block_bytes(), r.decoded.len());
}

/// The tier is not a TIM tier, and the difference is measurable rather than
/// rhetorical: neither TIM catalog contributes a single row from any of the
/// four player entries.
#[test]
fn no_tim_catalog_reaches_the_player_files() {
    if !disc_gate() {
        return;
    }
    let prot_path = ["extracted/PROT.DAT", "../../extracted/PROT.DAT"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_file());
    let Some(prot_path) = prot_path else {
        eprintln!("[skip] extracted/PROT.DAT missing");
        return;
    };
    let archive = legaia_prot::archive::Archive::open(&prot_path).expect("open PROT.DAT");
    let prot = std::fs::read(&prot_path).expect("read PROT.DAT");
    let spans: Vec<(u64, u64, u32)> = archive
        .entries
        .iter()
        .map(|e| (e.byte_offset, e.size_bytes, e.index))
        .collect();

    let player = |e: u32| catalog::PLAYER_FILE_ENTRIES.contains(&e);

    let raw = legaia_asset::tim_catalog::build_from_spans(&prot, &spans);
    let raw_hits = raw
        .iter()
        .filter(|t| t.entry_index.is_some_and(player))
        .count();
    assert_eq!(raw_hits, 0, "the raw TIM tier finds nothing in 863..866");

    let deep = legaia_asset::tim_deep_catalog::build_from_spans(&prot, &spans);
    let deep_hits = deep.iter().filter(|t| player(t.entry_index)).count();
    assert_eq!(deep_hits, 0, "the LZS TIM tier finds nothing in 863..866");

    // ... while the battle tier finds the whole family, and only there.
    let battle = catalog::build_from_spans(&prot, &spans);
    assert_eq!(battle.len(), 153);
    let mut entries: Vec<u32> = battle.iter().map(|b| b.entry_index).collect();
    entries.dedup();
    assert_eq!(entries, catalog::PLAYER_FILE_ENTRIES.to_vec());
    // Ids are dense and scan-ordered, so a row's id addresses it stably
    // within one build.
    for (i, b) in battle.iter().enumerate() {
        assert_eq!(b.id as usize, i);
    }
}

/// Two retail blocks ship `clut_n = 0` - pixels with no palette of their
/// own. They are real blocks and must stay reachable; the CLUT row the file
/// assembles is what decodes them.
#[test]
fn paletteless_blocks_decode_through_the_assembled_row() {
    if !disc_gate() {
        return;
    }
    let Some(dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let path = dir.join("0863_edstati3.BIN");
    if !path.exists() {
        eprintln!("[skip] {} missing", path.display());
        return;
    }
    let file = std::fs::read(&path).expect("read PLAYER1");
    let mut id = 0u32;
    let rows = catalog::build_from_file(863, &file, &mut id);

    let bare: Vec<_> = rows.iter().filter(|b| b.clut_entries == 0).collect();
    assert_eq!(bare.len(), 2, "PLAYER1's two pixel-only blocks");
    assert!(
        bare.iter().any(|b| b.is_record0()),
        "one of them is a record[0] block"
    );

    let row = catalog::assemble_clut_row(&file).expect("assemble the file's CLUT row");
    assert_eq!(row.len(), catalog::CLUT_ROW_ENTRIES);
    assert!(
        row.iter().any(|&e| e != 0),
        "the row must carry real colours"
    );
    for b in bare {
        let r = catalog::resolve_block(&file, b.slot(), 0).expect("resolve");
        assert_eq!(r.upload.palette_count(), 0);
        assert!(
            r.upload.rgba(0).is_err(),
            "nothing block-local to decode with"
        );
        let rgba = r
            .upload
            .rgba_with_palette(&row[..16])
            .expect("decode through the assembled row");
        assert_eq!(rgba.len(), (b.width * b.height * 4) as usize);
    }
}
