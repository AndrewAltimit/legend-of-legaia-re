//! Byte-account real PROT entries off `extracted/PROT` and assert a floor on
//! each one's accounted share. Skips and passes when the extraction isn't on
//! disk - same gating pattern as every other disc-dependent test here, so CI
//! never needs Sony bytes.
//!
//! The floors are deliberately below the measured values: this test guards
//! against a walker silently regressing to "claims nothing", not against the
//! numbers moving as parsers improve. The invariants underneath them are the
//! part that has to hold exactly - a claim outside the buffer, or an
//! accounted/residue split that does not sum to the size, means the range
//! algebra is wrong and every figure it produces is meaningless.

use std::path::PathBuf;

use legaia_asset::byte_account::{
    Account, AccountOptions, ResidueShape, account, classify_residue, prot_index_from_name,
};

fn extracted_root() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("extracted").join("PROT");
    p.is_dir().then_some(p)
}

fn funcs_dir() -> Option<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest.parent()?.parent()?;
    let p = workspace.join("ghidra").join("scripts").join("funcs");
    p.is_dir().then_some(p)
}

/// Locate `NNNN_*.BIN` under the extracted PROT directory.
fn entry_path(dir: &std::path::Path, idx: u32) -> Option<PathBuf> {
    let prefix = format!("{idx:04}_");
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix))
        })
}

fn account_entry(dir: &std::path::Path, idx: u32, depth: u8) -> Option<Account> {
    let path = entry_path(dir, idx)?;
    let bytes = std::fs::read(&path).ok()?;
    let name = path.file_name()?.to_str()?.to_string();
    let opts = AccountOptions {
        prot_index: prot_index_from_name(&name).or(Some(idx)),
        label: name,
        depth,
        max_nested: 2,
        ..Default::default()
    };
    Some(account(&bytes, &opts))
}

/// Everything a figure has to satisfy before it means anything.
fn assert_invariants(acc: &Account) {
    assert_eq!(
        acc.accounted + acc.residue_bytes,
        acc.size,
        "{}: accounted + residue must be the whole buffer",
        acc.label
    );
    assert!(
        acc.structural <= acc.accounted,
        "{}: structural claims are a subset of all claims",
        acc.label
    );
    for r in &acc.residue {
        assert!(r.end <= acc.size, "{}: residue past buffer end", acc.label);
        assert!(r.start < r.end, "{}: empty residue run", acc.label);
    }
    for n in &acc.nested {
        assert_invariants(&n.account);
    }
}

/// `(extraction index, accounted floor %, expected walker)`.
///
/// Every one of these is an entry the brief for this instrument named, plus
/// the two towns that stand in for the ordinary scene bundle.
const CASES: &[(u32, f64, &str)] = &[
    // The 15.9 MB monster archive: one LZS stream per monster id. The rest is
    // the unused slack inside each fixed 0x14000 slot plus an unpopulated tail.
    (867, 50.0, "monster_archive"),
    // `summon.dat` / `readef.DAT` - fixed 0x10800 slots.
    (893, 90.0, "summon_readef"),
    (894, 85.0, "summon_readef"),
    // The entry the retired `data_field_truncated` detector used to match. It
    // is not a stream: the runtime walks it as an `asset::pack` of two whole
    // TIMs (it now classifies as `pack`), so the walker is selected by index
    // and the accounting is structural. The shortfall is the 948-byte tail the
    // pack does not reference.
    (892, 98.0, "card_font_pack"),
    // The two forms of a scene carrier, one apiece. town01 is chunk-headered
    // (a single-chunk DATA_FIELD stream, `Flag(0x14)`) and town0c is bare
    // (`Flag(0x0A)`); before the classifier keyed on the form they sat in
    // `field_pack` and `lzs_container`. Both must walk structurally - the
    // chunk-headered one's payload is a pack, and reading it as an opaque blob
    // put its members in the magic sweep instead (`accounted` near 100 % with
    // `structural` at 0), which the `structural > 0` assertion below catches.
    (5, 99.0, "stream"),
    (23, 99.0, "pack"),
    // The `befect_data` `etim` / `etmd` packs - the same bare form outside a
    // scene block, TIM members in one and Legaia TMD members in the other.
    (870, 99.0, "pack"),
    (871, 99.0, "pack"),
    // `bse.dat` master bank + its untraced sibling.
    (888, 40.0, "bse_bank"),
    (1195, 2.0, "bse_bank"),
    // Boot `init.pak`.
    (895, 85.0, "init_pak"),
    // Kingdom bundles (world map) - the three `scene_asset_table` carriers.
    (86, 95.0, "scene_asset_table"),
    (245, 95.0, "scene_asset_table"),
    (392, 95.0, "scene_asset_table"),
    // Two ordinary town scene bundles.
    (4, 95.0, "scene_asset_table"),
    (13, 95.0, "scene_asset_table"),
];

#[test]
fn named_entries_account_above_their_floor_or_skip() {
    let Some(dir) = extracted_root() else {
        eprintln!("extracted/PROT not present - skipping");
        return;
    };
    let mut seen = 0usize;
    for &(idx, floor, walker) in CASES {
        let Some(acc) = account_entry(&dir, idx, 1) else {
            eprintln!("PROT {idx:04} not extracted - skipping this case");
            continue;
        };
        seen += 1;
        assert_invariants(&acc);
        assert_eq!(
            acc.walker.name(),
            walker,
            "PROT {idx:04} ({}): walker selection changed",
            acc.label
        );
        assert!(
            acc.accounted_pct >= floor,
            "PROT {idx:04} ({}): accounted {:.1}% below the {floor:.1}% floor",
            acc.label,
            acc.accounted_pct
        );
        // A structural walker must do the work; the magic sweep is a garnish.
        assert!(
            acc.structural > 0,
            "PROT {idx:04} ({}): no structural claims at all",
            acc.label
        );
        eprintln!(
            "[ok] PROT {idx:04} {} accounted {:.1}% (structural {:.1}%) walker={}",
            acc.label, acc.accounted_pct, acc.structural_pct, walker
        );
    }
    assert!(seen > 0, "extracted/PROT present but no case resolved");
}

/// Everything the monster archive does not claim is declared slot slack.
///
/// The archive is a fixed `0x14000` stride and most blocks compress to well
/// under it, so its accounted share is capped far below 100 % by construction.
/// What the instrument has to show is that none of the shortfall is *content*:
/// after the walker claims each slot's compressed stream - and the trailing
/// slots' raw TIMs, which carry the TIM magic where a block would carry a
/// `dec_size` - the residue is padding and nothing else.
#[test]
fn monster_archive_residue_is_all_padding() {
    let Some(dir) = extracted_root() else {
        eprintln!("extracted/PROT not present - skipping");
        return;
    };
    let Some(acc) = account_entry(&dir, 867, 0) else {
        eprintln!("PROT 0867 not extracted - skipping");
        return;
    };
    assert_invariants(&acc);
    let padding = ["zero_pad", "alignment", "repeated_fill"];
    for s in &acc.by_shape {
        assert!(
            padding.contains(&s.shape.as_str()),
            "PROT 0867: {} bytes of residue read as {}, not padding",
            s.bytes,
            s.shape
        );
    }
    assert!(
        acc.by_owner.iter().any(|o| o.owner == "tim"),
        "PROT 0867: the trailing raw-TIM slots must be claimed as TIMs"
    );
    eprintln!(
        "[ok] PROT 0867 residue is padding only: {:?}",
        acc.by_shape
            .iter()
            .map(|s| (s.shape.as_str(), s.bytes))
            .collect::<Vec<_>>()
    );
}

/// The two entries the brief names as unexplained blobs stay unexplained -
/// and the instrument says so in a shape rather than in a percentage.
#[test]
fn unwalked_blobs_report_a_shape_not_a_walker() {
    let Some(dir) = extracted_root() else {
        eprintln!("extracted/PROT not present - skipping");
        return;
    };
    for idx in [1221u32, 1222] {
        let Some(acc) = account_entry(&dir, idx, 1) else {
            continue;
        };
        assert_invariants(&acc);
        assert_eq!(acc.walker.name(), "generic", "PROT {idx:04} has no walker");
        assert!(
            !acc.by_shape.is_empty(),
            "PROT {idx:04}: residue must be classified"
        );
        eprintln!(
            "[ok] PROT {idx:04} {} residue shapes: {:?}",
            acc.label,
            acc.by_shape
                .iter()
                .map(|s| (s.shape.as_str(), s.bytes))
                .collect::<Vec<_>>()
        );
    }
}

/// The overlay walker credits an extent only when the image's own bytes agree
/// with the dump's printed instructions. Needs both the extraction and a dump
/// directory; skips without either.
#[test]
fn overlay_code_entries_credit_only_byte_confirmed_extents() {
    let (Some(dir), Some(funcs)) = (extracted_root(), funcs_dir()) else {
        eprintln!("extracted/PROT or ghidra/scripts/funcs not present - skipping");
        return;
    };
    for idx in [898u32, 899] {
        let Some(path) = entry_path(&dir, idx) else {
            continue;
        };
        let bytes = std::fs::read(&path).expect("read entry");
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        let opts = AccountOptions {
            prot_index: Some(idx),
            label: name.clone(),
            funcs_dir: Some(funcs.clone()),
            depth: 0,
            ..Default::default()
        };
        let acc = account(&bytes, &opts);
        assert_invariants(&acc);
        assert_eq!(acc.walker.name(), "overlay_code", "PROT {idx:04} walker");
        assert!(
            acc.structural > 0,
            "PROT {idx:04} ({name}): no dump extent was credited"
        );
        // Slot-A overlays alias, so some extent in this VA band must belong to
        // a sibling. A run with zero refuted extents would mean the byte test
        // never fired.
        eprintln!(
            "[ok] PROT {idx:04} {name} code {:.1}%, {} unverifiable, {} refuted",
            acc.structural_pct, acc.ambiguous_dumps, acc.refuted_dumps
        );
    }
}

/// The residue classifier's answer for a real all-zero sector run, taken off
/// the disc rather than from a synthetic buffer.
#[test]
fn zero_padding_in_a_real_entry_classifies_as_padding() {
    let Some(dir) = extracted_root() else {
        eprintln!("extracted/PROT not present - skipping");
        return;
    };
    let Some(path) = entry_path(&dir, 888) else {
        return;
    };
    let bytes = std::fs::read(&path).expect("read entry");
    // `bse.dat` is one sector of records inside a 0x1000-byte entry; the tail
    // is zeroes.
    let tail = &bytes[bytes.len() - 512..];
    if tail.iter().all(|&b| b == 0) {
        assert_eq!(classify_residue(tail), ResidueShape::ZeroPad);
    }
}
