//! Decode the two real runtime SFX descriptor banks (`bse.dat` = extraction
//! entry 888, and the scene-prescript sibling at 1195) out of
//! `extracted/PROT.DAT` if present. Skips and passes when the archive isn't on
//! disk - the same gating pattern as the other disc-dependent tests, so CI
//! doesn't need Sony bytes.
//!
//! What this catches:
//! - The **column mapping** drifting. `+0..+4` are `p` / `t` / `l` / `n` / `id`
//!   (program / tone / level / flags / category), pinned because
//!   `FUN_80016B6C` decodes this bank and the static `DAT_8006F198` table with
//!   one block of code (`docs/formats/bse-dat.md`). The old shape reading typed
//!   `+4` as a `u32`; it is one category byte plus three bytes no reader
//!   touches.
//! - The **row index** ceasing to be `cue_id - 0x200`. Both tinted legs of the
//!   battle cue router `FUN_8004FE5C` write `record[ring_id - 0x200] + 4`, and
//!   the debug sound test enqueues cue `0x21B` for row 27. If the base drifts,
//!   the whole cue space points one bank-row off.
//! - Entry 888 shrinking below the cue span the router can produce
//!   (`0x200..0x2C8`), which would put a live cue past the terminator.
//! - The detector's zero-trailer gate loosening: `+5..+7` are zero in every row
//!   of both carriers, and that is the only thing separating this format from
//!   any other 8-byte-stride table with a `[u16][u16 4]` header.

use legaia_asset::bse_bank::{self, CUE_ID_BASE, HEADER_BYTES, RECORD_BYTES};
use std::path::PathBuf;

/// Extraction entry of `bse.dat` (raw TOC `0x37A` = 890, minus the +2 CDNAME
/// numbering skew - `docs/formats/cdname.md`).
const BSE_DAT_ENTRY: u32 = 888;
/// The scene-prescript sibling that carries the same format.
const PRESCRIPT_ENTRY: u32 = 1195;

/// Highest ring id `FUN_8004FE5C` can enqueue: its low leg maps
/// `id 0x1B..0x47` to `id + 0x281`.
const MAX_ROUTER_CUE_ID: u16 = 0x47 + 0x281;

fn prot_entry(index: u32) -> Option<Vec<u8>> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest
        .parent()?
        .parent()?
        .join("extracted")
        .join("PROT.DAT");
    path.is_file().then_some(())?;
    let mut archive = legaia_prot::archive::Archive::open(&path).ok()?;
    let entry = archive.entries.get(index as usize)?.clone();
    let mut out = Vec::new();
    archive.read_entry(&entry, &mut out).ok()?;
    Some(out)
}

#[test]
fn bse_dat_columns_decode_or_skips() {
    let Some(buf) = prot_entry(BSE_DAT_ENTRY) else {
        eprintln!("[skip] extracted/PROT.DAT not present");
        return;
    };
    let bank = bse_bank::detect(&buf).expect("entry 888 is a runtime SFX bank");
    assert_eq!(bank.head_word, 1, "tag word");
    assert_eq!(bank.body_offset, HEADER_BYTES, "record table starts at +4");
    assert_eq!(bank.records, 297, "record count");

    let rows = bse_bank::records(&buf);
    assert_eq!(rows.len(), bank.records);

    for (i, row) in rows.iter().enumerate() {
        // `n & 0x1F` is the voice count the drainer keys on; retail uses 1 or 2
        // and never sets the top two bits.
        assert!(
            (1..=2).contains(&row.voice_count()),
            "row {i}: voice count {}",
            row.voice_count()
        );
        assert_eq!(row.flags & 0xC0, 0, "row {i}: unused flag bits");
        // `l` is the note-level attribute: it clusters on 60 (0x3C) and tracks
        // the tone within a program.
        assert!(
            (60..=69).contains(&row.level),
            "row {i}: level {}",
            row.level
        );
        // The authored category is a default (the router rewrites it live).
        assert!(
            matches!(row.category, 0 | 2),
            "row {i}: category {}",
            row.category
        );
        // The three bytes past the category have no runtime reader.
        let raw = bse_bank::record(&buf, i).expect("raw row");
        assert_eq!(&raw[5..], &[0u8; 3], "row {i}: trailer");
        assert_eq!(raw.len(), RECORD_BYTES);
    }

    // Row index = cue id - 0x200.
    assert_eq!(bse_bank::record_for_cue(&buf, CUE_ID_BASE), Some(rows[0]));
    assert_eq!(
        bse_bank::record_for_cue(&buf, CUE_ID_BASE + 27),
        Some(rows[27]),
        "cue 0x21B is row 27 - the debug sound test's own pairing"
    );
    assert!(
        bse_bank::record_for_cue(&buf, CUE_ID_BASE - 1).is_none(),
        "a static-table id resolves to no runtime row"
    );

    // Every cue the battle router can enqueue lands inside the table.
    let highest_row = usize::from(MAX_ROUTER_CUE_ID - CUE_ID_BASE);
    assert!(
        highest_row < bank.records,
        "router cue 0x{MAX_ROUTER_CUE_ID:X} needs row {highest_row}, bank has {}",
        bank.records
    );
    assert!(bse_bank::record_for_cue(&buf, MAX_ROUTER_CUE_ID).is_some());
}

#[test]
fn prescript_sibling_is_the_same_format_or_skips() {
    let Some(buf) = prot_entry(PRESCRIPT_ENTRY) else {
        eprintln!("[skip] extracted/PROT.DAT not present");
        return;
    };
    let bank = bse_bank::detect(&buf).expect("entry 1195 is a runtime SFX bank");
    assert_eq!(bank.head_word, 1);
    assert_eq!(bank.body_offset, HEADER_BYTES);
    assert_eq!(bank.records, 7);

    let rows = bse_bank::records(&buf);
    // A per-scene bank keys one variable VAB slot, so its category column is
    // constant - unlike bse.dat, whose rows default to 0.
    assert!(
        rows.iter().all(|r| r.category == 2),
        "every prescript row keys the same category"
    );
    assert!(rows.iter().all(|r| (1..=2).contains(&r.voice_count())));
    // Nothing but the header, the rows and zero fill.
    let end = HEADER_BYTES + rows.len() * RECORD_BYTES;
    assert!(
        buf[end..].iter().all(|&b| b == 0),
        "the rest of the sector is zero fill"
    );
}
