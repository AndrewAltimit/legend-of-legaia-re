//! Disc-gated regression test for the [`battle_data_pack`] detector
//! against the four retail player battle files (`data\battle\PLAYER1..4`,
//! retail `battle_data` CDNAME block = extraction 0863..0866;
//! Vahn / Noa / Gala / Terra).
//! Skips silently when `LEGAIA_DISC_BIN` is unset (CI without disc data).

use std::path::PathBuf;

use legaia_asset::battle_data_pack;

fn extracted_prot_dir() -> Option<PathBuf> {
    let cands = [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ];
    cands.into_iter().find(|p| p.is_dir())
}

/// Per-file pinned shape. The retail layout is stable across the corpus
/// (the disc data is read-only) so these are safe invariants.
struct Pin {
    /// Extraction filename (the 0863/0864 "edstati3" labels are the
    /// CDNAME +2 label shift - see `docs/formats/cdname.md`).
    file: &'static str,
    /// Header `desc_off` word = descriptor-table offset.
    table_offset: usize,
    /// Real (non-terminator) descriptor entries.
    records: usize,
    /// Descriptor entry 0's slot id (0 = default-variant slot).
    first_id: u32,
    /// TOC footprint in bytes. The slot region tiles it exactly:
    /// `data_base + last_offset + last_size == footprint`.
    footprint: usize,
}

const PINS: &[Pin] = &[
    Pin {
        file: "0863_edstati3.BIN", // Vahn, PLAYER1
        table_offset: 0x55F4,
        records: 54,
        first_id: 0x4B,
        footprint: 0xA9000,
    },
    Pin {
        file: "0864_edstati3.BIN", // Noa, PLAYER2
        table_offset: 0x75C4,
        records: 50,
        first_id: 0x51,
        footprint: 0x97800,
    },
    Pin {
        file: "0865_battle_data.BIN", // Gala, PLAYER3
        table_offset: 0x6C68,
        records: 43,
        first_id: 0x57,
        footprint: 0x6F000,
    },
    Pin {
        file: "0866_battle_data.BIN", // Terra, PLAYER4 (all-default table)
        table_offset: 0x6CAC,
        records: 5,
        first_id: 0,
        footprint: 0x17800,
    },
];

#[test]
fn detects_all_four_retail_player_files() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot_dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    for pin in PINS {
        let path = prot_dir.join(pin.file);
        if !path.exists() {
            eprintln!("[skip] {} missing", path.display());
            return;
        }
        let raw = std::fs::read(&path).expect("read player file");
        let pack = battle_data_pack::parse(&raw)
            .unwrap_or_else(|e| panic!("{}: parse pack: {e}", pin.file));

        assert_eq!(
            pack.table_offset, pin.table_offset,
            "{} table_offset",
            pin.file
        );
        assert_eq!(pack.records.len(), pin.records, "{} record count", pin.file);
        assert_eq!(pack.records[0].id, pin.first_id, "{} entry-0 id", pin.file);
        assert_eq!(pack.data_base, 0x8000, "{} data_base", pin.file);

        // Chain invariant + footprint tiling: entry 0 at offset 0, each
        // entry starting where the previous ends, last entry ending at
        // the TOC footprint. (0863's extracted file and 0865/0866's
        // extended TOC windows run past the footprint - the table stops
        // at the real file end regardless.)
        let mut expected_offset = 0u32;
        for r in &pack.records {
            assert_eq!(
                r.data_offset, expected_offset,
                "{} rec{} chain",
                pin.file, r.index
            );
            assert!(r.size > 0 && (r.size as usize).is_multiple_of(0x800));
            expected_offset += r.size;
        }
        assert_eq!(
            pack.data_base + expected_offset as usize,
            pin.footprint,
            "{} slot region tiles the footprint",
            pin.file
        );

        // Every slot must LZS-decode and carry a recognizable Legaia TMD
        // (at the canonical 0x20 offset or somewhere word-aligned past it).
        let mut tmds_found = 0usize;
        for record in &pack.records {
            let entry = battle_data_pack::decode_record(&raw, &pack, record.index)
                .unwrap_or_else(|e| panic!("{}: decode rec{}: {e}", pin.file, record.index));
            assert!(!entry.bytes.is_empty());
            if entry.tmd_range.is_some() {
                tmds_found += 1;
            }
        }
        assert_eq!(
            tmds_found, pin.records,
            "{}: every slot has an embedded TMD",
            pin.file
        );
    }
}

/// The four **per-direction swing costs** the Arts command gauge charges
/// per press (`swing_command_costs`, runtime slots `0xC..=0xF` = Left /
/// Right / Down / Up) resolve from every weapon-carrying player file's
/// default equipped set, and every one lands on a documented tier
/// (`docs/subsystems/arts-command-gauge.md`: favored `0x1E`, off-class
/// `0x2A`, far `0x36`).
///
/// This is the byte the Muscle Dome charges for the same command, read
/// through the same function - so the pin covers both input screens.
#[test]
fn swing_command_costs_read_the_documented_tiers() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot_dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let mut checked = 0usize;
    for file in [
        "0863_edstati3.BIN",
        "0864_edstati3.BIN",
        "0865_battle_data.BIN",
    ] {
        let path = prot_dir.join(file);
        if !path.exists() {
            eprintln!("[skip] {} missing", path.display());
            return;
        }
        let raw = std::fs::read(&path).expect("read player file");
        let pack = battle_data_pack::parse(&raw).expect("parse pack");
        // The section-default set (id 0 per slot) - what a fresh
        // character walks into a fight with.
        let costs = legaia_asset::battle_char_assembly::swing_command_costs(&raw, &pack, &[0u8; 5])
            .unwrap_or_else(|e| panic!("{file}: swing costs: {e}"));
        for (i, c) in costs.iter().enumerate() {
            let c = c.unwrap_or_else(|| panic!("{file}: slot 0x{:X} has no cost", 0xC + i));
            assert!(
                matches!(c, 0x1E | 0x2A | 0x36),
                "{file} slot 0x{:X}: unexpected cost 0x{c:02X}",
                0xC + i
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 12, "three files x four direction slots");
}

/// The same `+0x74` byte read through an explicitly **equipped** set: the
/// arm price moves with the weapon. That property is what makes the cost a
/// randomizer target, and why the port re-reads it per equipped set rather
/// than caching a per-character constant.
#[test]
fn swing_costs_track_the_equipped_set() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot_dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let path = prot_dir.join("0865_battle_data.BIN"); // Gala
    if !path.exists() {
        eprintln!("[skip] {} missing", path.display());
        return;
    }
    let raw = std::fs::read(&path).expect("read player file");
    let pack = battle_data_pack::parse(&raw).expect("parse pack");
    let base = legaia_asset::battle_char_assembly::swing_command_costs(&raw, &pack, &[0u8; 5])
        .expect("default costs")[0]
        .expect("default arm cost");
    // `select_sections` matches an equipped id positionally *inside* its
    // own section, and the arm swing (slot `0xC`) is section 2's record -
    // so equipment index 2 is the one that re-prices the arm. Sweep every
    // id the table carries through it.
    let mut seen = std::collections::BTreeSet::new();
    for rec in &pack.records {
        if rec.id == 0 || rec.id > 0xFF {
            continue;
        }
        let eq = [0, 0, rec.id as u8, 0, 0];
        if let Ok(c) = legaia_asset::battle_char_assembly::swing_command_costs(&raw, &pack, &eq)
            && let Some(arm) = c[0]
        {
            seen.insert(arm);
        }
    }
    assert!(seen.contains(&base), "the default arm cost is in the set");
    assert!(
        seen.len() > 1,
        "swapping the equipped weapon changes the arm cost (saw {seen:?})"
    );
}

/// `clut_uploads` is currently the documented no-op (the descriptor at
/// `u32[3..0x20]` has not been pinned by the byte-match corpus - see
/// `docs/formats/battle-data-pack.md`). Guard the contract so any
/// future descriptor work has to explicitly update this expectation
/// rather than silently change [`SceneResources::build_targeted`]'s
/// CLUT pass.
#[test]
fn clut_uploads_is_empty_for_every_retail_0865_record() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(prot_dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let path = prot_dir.join("0865_battle_data.BIN");
    if !path.exists() {
        eprintln!("[skip] {} missing", path.display());
        return;
    }
    let raw = std::fs::read(&path).expect("read 0865");
    let pack = battle_data_pack::parse(&raw).expect("parse pack");
    let mut total_uploads = 0usize;
    for record in &pack.records {
        let entry = battle_data_pack::decode_record(&raw, &pack, record.index)
            .unwrap_or_else(|e| panic!("decode rec{}: {e}", record.index));
        total_uploads += battle_data_pack::clut_uploads(&entry).len();
    }
    assert_eq!(
        total_uploads, 0,
        "clut_uploads should stay a no-op until the descriptor encoding is pinned"
    );
}
